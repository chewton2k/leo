use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::{wav, write_atomic, OVERLAP_SECS};
use crate::speech::Turn;

pub const FILE: &str = "speakers.json";
pub const MOST_SECS: u64 = 2 * 60 * 60;
const FRAME_MS: u64 = 10;
const LEAST_RUN_MS: u64 = 1_000;
const CUT_SEARCH_MS: u64 = 2_000;
const CUT_WINDOW_MS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spoken {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u8,
    pub text: String,
}

pub type Diarize<'a> = &'a dyn Fn(Vec<(PathBuf, u64)>) -> Result<Vec<Turn>, String>;
pub type Transcribe<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

pub fn part_of(spoken: &Spoken, segment_ms: u64) -> u64 {
    (spoken.start_ms + spoken.end_ms) / 2 / segment_ms.max(1)
}

pub fn runs(turns: &[Turn], total_ms: u64) -> Vec<(u64, u64, u8)> {
    let frames = total_ms.div_ceil(FRAME_MS) as usize;
    let mut label = vec![0u8; frames];
    let mut sorted = turns.to_vec();
    sorted.sort_by_key(|t| (t.start_ms, t.end_ms));
    for turn in &sorted {
        let from = (turn.start_ms / FRAME_MS) as usize;
        let to = (turn.end_ms.div_ceil(FRAME_MS) as usize).min(frames);
        for frame in label.iter_mut().take(to).skip(from) {
            if *frame == 0 {
                *frame = turn.speaker;
            }
        }
    }
    let Some(first) = label.iter().position(|&s| s != 0) else {
        return Vec::new();
    };
    let lead = label[first];
    label[..first].fill(lead);
    let mut at = first;
    while at < frames {
        if label[at] != 0 {
            at += 1;
            continue;
        }
        let end = label[at..]
            .iter()
            .position(|&s| s != 0)
            .map_or(frames, |n| at + n);
        let (left, right) = (
            label[at - 1],
            label.get(end).copied().unwrap_or(label[at - 1]),
        );
        let middle = at + (end - at) / 2;
        label[at..middle].fill(left);
        label[middle..end].fill(right);
        at = end;
    }
    let mut out: Vec<(u64, u64, u8)> = Vec::new();
    for (frame, &speaker) in label.iter().enumerate() {
        let start = frame as u64 * FRAME_MS;
        let end = (start + FRAME_MS).min(total_ms);
        match out.last_mut() {
            Some(last) if last.2 == speaker => last.1 = end,
            _ => out.push((start, end, speaker)),
        }
    }
    let mut merged: Vec<(u64, u64, u8)> = Vec::new();
    for run in out {
        match merged.last_mut() {
            Some(last) if last.2 == run.2 || run.1 - run.0 < LEAST_RUN_MS => last.1 = run.1,
            _ => merged.push(run),
        }
    }
    if merged.len() > 1 && merged[0].1 - merged[0].0 < LEAST_RUN_MS {
        merged[1].0 = merged[0].0;
        merged.remove(0);
    }
    renumbered(merged)
}

fn renumbered(runs: Vec<(u64, u64, u8)>) -> Vec<(u64, u64, u8)> {
    let mut order: Vec<u8> = Vec::new();
    runs.into_iter()
        .map(|(start, end, speaker)| {
            if !order.contains(&speaker) {
                order.push(speaker);
            }
            let named = order.iter().position(|&s| s == speaker).unwrap_or(0) as u8 + 1;
            (start, end, named)
        })
        .collect()
}

fn quietest_ms(samples: &[i16], from_ms: u64) -> u64 {
    let window = (CUT_WINDOW_MS * wav::RATE as u64 / 1000) as usize;
    let best = samples
        .chunks(window)
        .enumerate()
        .min_by_key(|(_, chunk)| chunk.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>())
        .map_or(0, |(at, _)| at as u64);
    from_ms + best * CUT_WINDOW_MS + CUT_WINDOW_MS / 2
}

struct Audio {
    parts: Vec<(PathBuf, u64, u64)>,
    loaded: Option<(usize, Vec<i16>)>,
}

impl Audio {
    fn samples(&mut self, start_ms: u64, end_ms: u64) -> Result<Vec<i16>, String> {
        let per_ms = wav::RATE as u64 / 1000;
        let mut out = Vec::new();
        let mut offset = 0;
        for (index, (path, skip, len)) in self.parts.iter().enumerate() {
            let (from, to) = (offset, offset + len);
            offset = to;
            if to <= start_ms * per_ms || from >= end_ms * per_ms {
                continue;
            }
            if self.loaded.as_ref().is_none_or(|(at, _)| *at != index) {
                let samples = wav::read(path).map_err(|e| e.to_string())?;
                self.loaded = Some((index, samples));
            }
            let samples = &self.loaded.as_ref().expect("just loaded").1;
            let a = (start_ms * per_ms).max(from) - from + skip;
            let b = (end_ms * per_ms).min(to) - from + skip;
            out.extend_from_slice(
                samples
                    .get(a as usize..(b as usize).min(samples.len()))
                    .unwrap_or(&[]),
            );
        }
        Ok(out)
    }
}

pub fn label(
    dir: &Path,
    parts: u32,
    segment_ms: u64,
    diarize: Diarize,
    transcribe: Transcribe,
    stop: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<Vec<Spoken>, String> {
    let lead = OVERLAP_SECS * wav::RATE as u64;
    let mut audio = Audio {
        parts: Vec::new(),
        loaded: None,
    };
    let too_long = || "the recording is longer than two hours".to_string();
    for index in 0..parts {
        let path = dir.join(wav::done_name(index));
        let bytes = std::fs::metadata(&path)
            .map_err(|_| {
                if index as u64 * segment_ms > MOST_SECS * 1000 {
                    too_long()
                } else {
                    "the audio of this recording is no longer kept".to_string()
                }
            })?
            .len();
        let samples = bytes.saturating_sub(wav::HEADER) / 2;
        let skip = if index == 0 { 0 } else { lead.min(samples) };
        audio.parts.push((path, skip, samples - skip));
    }
    let total_ms = audio.parts.iter().map(|p| p.2).sum::<u64>() * 1000 / wav::RATE as u64;
    if total_ms > MOST_SECS * 1000 {
        return Err(too_long());
    }
    if stop.load(Ordering::Relaxed) {
        return Err("Finish now was pressed".into());
    }
    let turns = diarize(
        audio
            .parts
            .iter()
            .map(|(path, skip, _)| (path.clone(), skip * 1000 / wav::RATE as u64))
            .collect(),
    )?;
    let runs = runs(&turns, total_ms);
    if runs.iter().all(|r| r.2 == 1) {
        return Ok(Vec::new());
    }
    let mut pieces = Vec::new();
    for (mut start, end, speaker) in runs {
        let mut boundary = (start / segment_ms.max(1) + 1) * segment_ms.max(1);
        while boundary + LEAST_RUN_MS < end && start + LEAST_RUN_MS < boundary {
            let from = boundary
                .saturating_sub(CUT_SEARCH_MS)
                .max(start + LEAST_RUN_MS);
            let to = (boundary + CUT_SEARCH_MS).min(end - LEAST_RUN_MS);
            let cut = quietest_ms(&audio.samples(from, to)?, from).clamp(from, to);
            pieces.push((start, cut, speaker));
            start = cut;
            boundary += segment_ms.max(1);
        }
        pieces.push((start, end, speaker));
    }
    let mut spoken: Vec<Spoken> = Vec::new();
    let total = pieces.len();
    for (done, (start_ms, end_ms, speaker)) in pieces.into_iter().enumerate() {
        progress(done, total);
        if stop.load(Ordering::Relaxed) {
            return Err("Finish now was pressed".into());
        }
        let samples = audio.samples(start_ms, end_ms)?;
        if crate::ai::live::is_silent(wav::peak(&samples)) {
            continue;
        }
        let file = tempfile::Builder::new()
            .prefix(".speaker-")
            .suffix(".wav")
            .tempfile_in(dir)
            .map_err(|e| e.to_string())?
            .into_temp_path();
        wav::write(&file, &samples).map_err(|e| e.to_string())?;
        let text = transcribe(&file).or_else(|_| transcribe(&file))?;
        let text = text.trim();
        if text.is_empty() || crate::ai::live::is_silence_artifact(text) {
            continue;
        }
        let next = Spoken {
            start_ms,
            end_ms,
            speaker,
            text: text.to_string(),
        };
        match spoken.last_mut() {
            Some(last)
                if last.speaker == speaker
                    && part_of(last, segment_ms) == part_of(&next, segment_ms) =>
            {
                last.end_ms = end_ms;
                last.text = format!("{} {text}", last.text);
            }
            _ => spoken.push(next),
        }
    }
    let named = renumbered(
        spoken
            .iter()
            .map(|s| (s.start_ms, s.end_ms, s.speaker))
            .collect(),
    );
    if named.iter().all(|r| r.2 == 1) {
        return Ok(Vec::new());
    }
    for (spoken, (_, _, speaker)) in spoken.iter_mut().zip(named) {
        spoken.speaker = speaker;
    }
    Ok(spoken)
}

pub fn read(dir: &Path) -> Option<Vec<Spoken>> {
    serde_json::from_slice::<Vec<Spoken>>(&std::fs::read(dir.join(FILE)).ok()?)
        .ok()
        .filter(|spoken| !spoken.is_empty())
}

pub fn write(dir: &Path, spoken: &[Spoken]) -> anyhow::Result<()> {
    write_atomic(&dir.join(FILE), &serde_json::to_string(spoken)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const SEGMENT: u64 = 300_000;

    fn turn(start_ms: u64, end_ms: u64, speaker: u8) -> Turn {
        Turn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    #[test]
    fn runs_cover_the_whole_recording_and_split_gaps_between_neighbours() {
        let tiled = runs(&[turn(500, 4_000, 2), turn(6_000, 9_000, 1)], 10_000);
        assert_eq!(tiled, vec![(0, 5_000, 1), (5_000, 10_000, 2)]);
    }

    #[test]
    fn runs_merge_short_turns_and_keep_the_earlier_speaker_where_they_overlap() {
        let merged = runs(
            &[
                turn(0, 5_000, 1),
                turn(4_000, 8_000, 2),
                turn(8_000, 8_400, 1),
                turn(8_400, 12_000, 2),
            ],
            12_000,
        );
        assert_eq!(merged, vec![(0, 5_000, 1), (5_000, 12_000, 2)]);
        assert_eq!(
            runs(&[turn(0, 600, 2), turn(600, 6_000, 1)], 6_000),
            vec![(0, 6_000, 1)]
        );
    }

    #[test]
    fn runs_are_empty_without_turns_and_numbered_by_first_appearance() {
        assert!(runs(&[], 60_000).is_empty());
        assert!(runs(&[turn(0, 10, 1)], 0).is_empty());
        assert_eq!(
            runs(&[turn(0, 4_000, 3), turn(4_000, 9_000, 1)], 9_000),
            vec![(0, 4_000, 1), (4_000, 9_000, 2)]
        );
    }

    fn part(dir: &Path, index: u32, secs: u64) {
        wav::write(
            &dir.join(wav::done_name(index)),
            &vec![1000i16; (secs * wav::RATE as u64) as usize],
        )
        .unwrap();
    }

    #[test]
    fn labelling_skips_each_later_part_s_lead_and_transcribes_every_run() {
        let dir = tempfile::tempdir().unwrap();
        part(dir.path(), 0, 4);
        part(dir.path(), 1, 3);
        let asked = RefCell::new(Vec::new());
        let lengths = RefCell::new(Vec::new());
        let diarize = |parts: Vec<(PathBuf, u64)>| {
            *asked.borrow_mut() = parts.iter().map(|p| p.1).collect();
            Ok(vec![turn(0, 3_000, 1), turn(3_000, 6_000, 2)])
        };
        let transcribe = |path: &Path| {
            let samples = wav::read(path).unwrap().len();
            lengths.borrow_mut().push(samples);
            Ok(format!("{samples} samples"))
        };
        let spoken = label(
            dir.path(),
            2,
            4_000,
            &diarize,
            &transcribe,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(*asked.borrow(), vec![0, 1_000]);
        assert_eq!(*lengths.borrow(), vec![48_000, 48_000]);
        assert_eq!(
            spoken
                .iter()
                .map(|s| (s.start_ms, s.end_ms, s.speaker))
                .collect::<Vec<_>>(),
            vec![(0, 3_000, 1), (3_000, 6_000, 2)]
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".speaker-")));
    }

    #[test]
    fn labelling_gives_nothing_for_one_speaker_and_fails_without_audio_or_when_stopped() {
        let dir = tempfile::tempdir().unwrap();
        part(dir.path(), 0, 2);
        let never =
            |_: &Path| -> Result<String, String> { panic!("one speaker needs no second pass") };
        let one = |_: Vec<(PathBuf, u64)>| Ok(vec![turn(0, 2_000, 3)]);
        assert!(label(
            dir.path(),
            1,
            SEGMENT,
            &one,
            &never,
            &AtomicBool::new(false),
            &|_, _| {}
        )
        .unwrap()
        .is_empty());
        let two = |_: Vec<(PathBuf, u64)>| Ok(vec![turn(0, 1_000, 1), turn(1_000, 2_000, 2)]);
        let fine = |_: &Path| Ok("words".to_string());
        assert!(label(
            dir.path(),
            1,
            SEGMENT,
            &two,
            &fine,
            &AtomicBool::new(true),
            &|_, _| {}
        )
        .is_err());
        let missing = |segment_ms| {
            label(
                dir.path(),
                2,
                segment_ms,
                &two,
                &fine,
                &AtomicBool::new(false),
                &|_, _| {},
            )
            .unwrap_err()
        };
        assert_eq!(
            missing(SEGMENT),
            "the audio of this recording is no longer kept"
        );
        assert_eq!(
            missing(MOST_SECS * 1000 + 1),
            "the recording is longer than two hours"
        );
        let broken = |_: Vec<(PathBuf, u64)>| Err("no model".to_string());
        assert_eq!(
            label(
                dir.path(),
                1,
                SEGMENT,
                &broken,
                &fine,
                &AtomicBool::new(false),
                &|_, _| {}
            ),
            Err("no model".to_string())
        );
    }

    #[test]
    fn a_silent_run_between_one_speaker_s_runs_leaves_one_passage() {
        let dir = tempfile::tempdir().unwrap();
        part(dir.path(), 0, 12);
        let calls = RefCell::new(0);
        let four = |_: Vec<(PathBuf, u64)>| {
            Ok(vec![
                turn(0, 3_000, 1),
                turn(3_000, 6_000, 2),
                turn(6_000, 9_000, 1),
                turn(9_000, 12_000, 2),
            ])
        };
        let transcribe = |_: &Path| {
            *calls.borrow_mut() += 1;
            Ok(["first", " ", "third", "fourth"][*calls.borrow() - 1].to_string())
        };
        let spoken = label(
            dir.path(),
            1,
            SEGMENT,
            &four,
            &transcribe,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(
            spoken
                .iter()
                .map(|s| (s.start_ms, s.end_ms, s.speaker, s.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, 9_000, 1, "first third"), (9_000, 12_000, 2, "fourth")]
        );
    }

    #[test]
    fn a_run_across_a_part_boundary_is_cut_at_its_quietest_moment_and_kept_per_part() {
        let dir = tempfile::tempdir().unwrap();
        let mut samples = vec![1000i16; 10 * wav::RATE as usize];
        samples[3_500 * 16..3_600 * 16].fill(0);
        wav::write(&dir.path().join(wav::done_name(0)), &samples).unwrap();
        let two = |_: Vec<(PathBuf, u64)>| Ok(vec![turn(0, 6_000, 1), turn(6_000, 10_000, 2)]);
        let steps = RefCell::new(Vec::new());
        let spoken = label(
            dir.path(),
            1,
            4_000,
            &two,
            &|_: &Path| Ok("words".to_string()),
            &AtomicBool::new(false),
            &|done, total| steps.borrow_mut().push((done, total)),
        )
        .unwrap();
        assert_eq!(
            spoken
                .iter()
                .map(|s| (s.start_ms, s.end_ms, s.speaker))
                .collect::<Vec<_>>(),
            vec![
                (0, 3_510, 1),
                (3_510, 6_000, 1),
                (6_000, 7_010, 2),
                (7_010, 10_000, 2)
            ]
        );
        assert_eq!(
            spoken.iter().map(|s| part_of(s, 4_000)).collect::<Vec<_>>(),
            vec![0, 1, 1, 2]
        );
        assert_eq!(*steps.borrow(), vec![(0, 4), (1, 4), (2, 4), (3, 4)]);
    }

    #[test]
    fn a_failed_run_is_tried_again_and_silence_is_never_sent_to_the_speech_model() {
        let dir = tempfile::tempdir().unwrap();
        let mut samples = vec![1000i16; 6 * wav::RATE as usize];
        samples[..2 * wav::RATE as usize].fill(0);
        wav::write(&dir.path().join(wav::done_name(0)), &samples).unwrap();
        let calls = RefCell::new(0);
        let three = |_: Vec<(PathBuf, u64)>| {
            Ok(vec![
                turn(0, 2_000, 1),
                turn(2_000, 4_000, 2),
                turn(4_000, 6_000, 3),
            ])
        };
        let flaky = |_: &Path| {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 {
                Err("busy".to_string())
            } else {
                Ok("words".to_string())
            }
        };
        let spoken = label(
            dir.path(),
            1,
            SEGMENT,
            &three,
            &flaky,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(
            *calls.borrow(),
            3,
            "one retry, and the silent first run is skipped"
        );
        assert_eq!(
            spoken.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let never = |_: Vec<(PathBuf, u64)>| -> Result<Vec<Turn>, String> {
            panic!("Finish now skips the pass")
        };
        assert!(label(
            dir.path(),
            1,
            SEGMENT,
            &never,
            &flaky,
            &AtomicBool::new(true),
            &|_, _| {}
        )
        .is_err());
    }

    #[test]
    fn saved_labels_round_trip_and_an_empty_or_broken_file_means_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_none());
        write(dir.path(), &[]).unwrap();
        assert!(read(dir.path()).is_none());
        std::fs::write(dir.path().join(FILE), "{broken").unwrap();
        assert!(read(dir.path()).is_none());
        let spoken = vec![Spoken {
            start_ms: 0,
            end_ms: 900,
            speaker: 1,
            text: "hi".into(),
        }];
        write(dir.path(), &spoken).unwrap();
        assert_eq!(read(dir.path()), Some(spoken));
    }

    fn best_match(spans: &[(String, u8, u64)]) -> u64 {
        let refs: Vec<String> = spans
            .iter()
            .map(|s| s.0.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let hyps: Vec<u8> = spans
            .iter()
            .map(|s| s.1)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        fn search(
            at: usize,
            hyps: &[u8],
            refs: &[String],
            taken: &mut Vec<Option<usize>>,
            spans: &[(String, u8, u64)],
        ) -> u64 {
            if at == hyps.len() {
                return spans
                    .iter()
                    .filter(|(r, h, _)| {
                        let i = hyps.iter().position(|x| x == h).unwrap();
                        taken[i].is_some_and(|t| &refs[t] == r)
                    })
                    .map(|s| s.2)
                    .sum();
            }
            let mut best = 0;
            for choice in (0..refs.len()).map(Some).chain([None]) {
                if choice.is_some() && taken.contains(&choice) {
                    continue;
                }
                taken.push(choice);
                best = best.max(search(at + 1, hyps, refs, taken, spans));
                taken.pop();
            }
            best
        }
        search(0, &hyps, &refs, &mut Vec::new(), spans)
    }

    #[test]
    #[ignore = "needs LEO_HOME with a chosen speech model, the speaker model and a meeting recording"]
    fn speakers_across_models() {
        let audio =
            std::env::var("LEO_SPEAKER_AUDIO").expect("LEO_SPEAKER_AUDIO: a 16 kHz mono WAV");
        let truth = std::env::var("LEO_SPEAKER_TRUTH").expect("LEO_SPEAKER_TRUTH: its RTTM");
        let selected = crate::speech::library::library()
            .unwrap()
            .selected()
            .unwrap();
        let samples = wav::read(Path::new(&audio)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (segment, lead) = (
            300 * wav::RATE as usize,
            (OVERLAP_SECS * wav::RATE as u64) as usize,
        );
        let mut parts = 0;
        for start in (0..samples.len()).step_by(segment) {
            let from = if start == 0 { 0 } else { start - lead };
            wav::write(
                &dir.path().join(wav::done_name(parts)),
                &samples[from..(start + segment).min(samples.len())],
            )
            .unwrap();
            parts += 1;
        }
        let engine = crate::speech::engine::engine();
        let transcribe = |path: &Path| {
            let pcm = wav::read(path)
                .unwrap()
                .iter()
                .map(|&s| s as f32 / 32768.0)
                .collect();
            engine
                .transcribe(
                    selected.clone(),
                    pcm,
                    crate::speech::engine::Priority::Segment,
                    Vec::new(),
                )
                .map_err(|e| e.to_string())
        };
        let started = std::time::Instant::now();
        let spoken = label(
            dir.path(),
            parts,
            300_000,
            &|p| {
                engine
                    .diarize(p, &AtomicBool::new(false))
                    .map_err(|e| e.to_string())
            },
            &transcribe,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        let took = started.elapsed().as_secs_f64();
        let plain: usize = (0..parts)
            .map(|i| {
                transcribe(&dir.path().join(wav::done_name(i)))
                    .unwrap()
                    .split_whitespace()
                    .count()
            })
            .sum();
        let words: usize = spoken
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum();
        let mut spans = Vec::new();
        let mut total = 0;
        for line in std::fs::read_to_string(&truth).unwrap().lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            let a = (f[3].parse::<f64>().unwrap() * 1000.0) as u64;
            let b = a + (f[4].parse::<f64>().unwrap() * 1000.0) as u64;
            total += b - a;
            for s in &spoken {
                let overlap = b.min(s.end_ms).saturating_sub(a.max(s.start_ms));
                if overlap > 0 {
                    spans.push((f[7].to_string(), s.speaker, overlap));
                }
            }
        }
        let covered: u64 = spans.iter().map(|s| s.2).sum();
        let right = best_match(&spans) as f64 / covered.max(1) as f64;
        let kept = words as f64 / plain.max(1) as f64;
        println!(
            "speakers_across_models model={} minutes={:.1} speakers={} right={:.1}% heard={:.1}% words={} plain_words={} kept={:.1}% labelling_s={:.1}",
            selected.id,
            samples.len() as f64 / wav::RATE as f64 / 60.0,
            spoken.iter().map(|s| s.speaker).max().unwrap_or(0),
            right * 100.0,
            covered as f64 * 100.0 / total.max(1) as f64,
            words,
            plain,
            kept * 100.0,
            took
        );
        assert!(right >= 0.80, "speaker attribution {right:.3}");
        let heard = covered as f64 / total.max(1) as f64;
        assert!(heard >= 0.75, "speech with text {heard:.3}");
        assert!(kept >= 0.95, "words kept {kept:.3}");
    }
}
