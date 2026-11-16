use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::{wav, OVERLAP_SECS};
use crate::speech::Turn;

pub const FILE: &str = "speakers.json";
pub const MOST_SECS: u64 = 2 * 60 * 60;
const FRAME_MS: u64 = 10;
const LEAST_RUN_MS: u64 = 1_000;
const MOST_RUN_MS: u64 = 290_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spoken {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u8,
    pub text: String,
}

pub type Diarize<'a> = &'a dyn Fn(Vec<(PathBuf, u64)>) -> Result<Vec<Turn>, String>;
pub type Transcribe<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

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
    let mut order: Vec<u8> = Vec::new();
    let mut split = Vec::new();
    for (start, end, speaker) in merged {
        if !order.contains(&speaker) {
            order.push(speaker);
        }
        let named = order.iter().position(|&s| s == speaker).unwrap_or(0) as u8 + 1;
        let pieces = (end - start).div_ceil(MOST_RUN_MS).max(1);
        let size = (end - start).div_ceil(pieces);
        for piece in 0..pieces {
            let from = start + piece * size;
            split.push((from, (from + size).min(end), named));
        }
    }
    split
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
    diarize: Diarize,
    transcribe: Transcribe,
    stop: &AtomicBool,
) -> Result<Vec<Spoken>, String> {
    let lead = OVERLAP_SECS * wav::RATE as u64;
    let mut audio = Audio {
        parts: Vec::new(),
        loaded: None,
    };
    for index in 0..parts {
        let path = dir.join(wav::done_name(index));
        let bytes = std::fs::metadata(&path)
            .map_err(|_| "the audio of this recording is no longer kept".to_string())?
            .len();
        let samples = bytes.saturating_sub(wav::HEADER) / 2;
        let skip = if index == 0 { 0 } else { lead.min(samples) };
        audio.parts.push((path, skip, samples - skip));
    }
    let total_ms = audio.parts.iter().map(|p| p.2).sum::<u64>() * 1000 / wav::RATE as u64;
    if total_ms > MOST_SECS * 1000 {
        return Err("the recording is longer than two hours".into());
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
    let mut spoken: Vec<Spoken> = Vec::new();
    for (start_ms, end_ms, speaker) in runs {
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
            .map_err(|e| e.to_string())?;
        wav::write(file.path(), &samples).map_err(|e| e.to_string())?;
        let text = transcribe(file.path())?;
        let text = text.trim();
        let text = if crate::ai::live::is_silence_artifact(text) {
            ""
        } else {
            text
        };
        match spoken.last_mut() {
            _ if text.is_empty() => {}
            Some(last) if last.speaker == speaker => {
                last.end_ms = end_ms;
                last.text = format!("{} {text}", last.text);
            }
            _ => spoken.push(Spoken {
                start_ms,
                end_ms,
                speaker,
                text: text.to_string(),
            }),
        }
    }
    Ok(spoken)
}

pub fn read(dir: &Path) -> Option<Vec<Spoken>> {
    serde_json::from_slice::<Vec<Spoken>>(&std::fs::read(dir.join(FILE)).ok()?)
        .ok()
        .filter(|spoken| !spoken.is_empty())
}

pub fn write(dir: &Path, spoken: &[Spoken]) -> std::io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut file, spoken)?;
    file.as_file().sync_all()?;
    file.persist(dir.join(FILE)).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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
    fn runs_are_empty_without_turns_and_long_runs_are_split_for_the_speech_model() {
        assert!(runs(&[], 60_000).is_empty());
        assert!(runs(&[turn(0, 10, 1)], 0).is_empty());
        let long = runs(&[turn(0, 400_000, 1), turn(400_000, 410_000, 2)], 410_000);
        assert_eq!(
            long,
            vec![
                (0, 200_000, 1),
                (200_000, 400_000, 1),
                (400_000, 410_000, 2)
            ]
        );
        assert!(long.iter().all(|r| r.1 - r.0 <= MOST_RUN_MS));
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
            &diarize,
            &transcribe,
            &AtomicBool::new(false),
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
        assert!(label(dir.path(), 1, &one, &never, &AtomicBool::new(false))
            .unwrap()
            .is_empty());
        let two = |_: Vec<(PathBuf, u64)>| Ok(vec![turn(0, 1_000, 1), turn(1_000, 2_000, 2)]);
        let fine = |_: &Path| Ok("words".to_string());
        assert!(label(dir.path(), 1, &two, &fine, &AtomicBool::new(true)).is_err());
        assert!(label(dir.path(), 2, &two, &fine, &AtomicBool::new(false)).is_err());
        let broken = |_: Vec<(PathBuf, u64)>| Err("no model".to_string());
        assert_eq!(
            label(dir.path(), 1, &broken, &fine, &AtomicBool::new(false)),
            Err("no model".to_string())
        );
    }

    #[test]
    fn a_silent_run_between_one_speaker_s_runs_leaves_one_passage() {
        let dir = tempfile::tempdir().unwrap();
        part(dir.path(), 0, 9);
        let calls = RefCell::new(0);
        let three = |_: Vec<(PathBuf, u64)>| {
            Ok(vec![
                turn(0, 3_000, 1),
                turn(3_000, 6_000, 2),
                turn(6_000, 9_000, 1),
            ])
        };
        let transcribe = |_: &Path| {
            *calls.borrow_mut() += 1;
            Ok(["first", " ", "third"][*calls.borrow() - 1].to_string())
        };
        let spoken = label(dir.path(), 1, &three, &transcribe, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            spoken,
            vec![Spoken {
                start_ms: 0,
                end_ms: 9_000,
                speaker: 1,
                text: "first third".into()
            }]
        );
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
            &|p| engine.diarize(p).map_err(|e| e.to_string()),
            &transcribe,
            &AtomicBool::new(false),
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
        assert!(right >= 0.85, "speaker attribution {right:.3}");
        assert!(kept >= 0.95, "words kept {kept:.3}");
    }
}
