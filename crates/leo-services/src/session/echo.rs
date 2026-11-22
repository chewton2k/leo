use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{wav, Segment, SegmentState, Session, OVERLAP_SECS};

pub const BLOCK_MS: u64 = 10;
const BLOCK: usize = (wav::RATE as u64 * BLOCK_MS / 1000) as usize;
const WINDOW: i64 = 200;
const MOST_LAG: i64 = 200;
const FLOOR: u8 = 30;
const ABOVE_IDLE: f32 = 6.0;
const IDLE_SHARE: usize = 10;
const ACTIVE: usize = 50;
const CONFIDENT: f32 = 0.6;
const LEAST_SPREAD: f32 = 3.0;
const NEAR: i64 = 10;
const AGREE: usize = 2;
const SLACK: i64 = 2;
const LEAST_RATE: f32 = 0.2;
const LEAST_STEADY: usize = 3;
const LEAST_LINE: usize = 50;
const OWN_SHARE: f32 = 0.05;
const OWN_RUN: usize = 20;
const LINE_CORR: f32 = 0.5;
const ABOVE_LEAK: f32 = 6.0;
const SHORT_BY: u64 = 100;

pub fn levels_name(index: u32) -> String {
    format!("seg-{index:05}.levels")
}

pub fn levels(samples: &[i16]) -> Vec<u8> {
    samples
        .chunks(BLOCK)
        .map(|chunk| {
            let power = chunk.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / chunk.len() as f64;
            let db = 10.0 * power.log10();
            if db.is_finite() {
                db.clamp(0.0, 90.0).round() as u8
            } else {
                0
            }
        })
        .collect()
}

pub fn write_levels(audio: &Path, to: &Path) -> anyhow::Result<()> {
    leo_core::paths::write_atomic(to, levels(&wav::read(audio)?))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leak {
    steady: Vec<(i64, i64, f32)>,
    floors: (f32, f32),
}

impl Leak {
    fn near(&self, block: i64) -> (i64, f32) {
        let centre = |w: &(i64, i64, f32)| w.0 + WINDOW / 2;
        let at = self.steady.partition_point(|w| centre(w) < block);
        let Some(nearest) = [at.checked_sub(1), Some(at)]
            .into_iter()
            .flatten()
            .filter_map(|i| self.steady.get(i))
            .min_by_key(|w| (centre(w) - block).abs())
        else {
            return (0, 0.0);
        };
        let reach = NEAR * WINDOW;
        let from = self.steady.partition_point(|w| centre(w) < block - reach);
        let to = self.steady.partition_point(|w| centre(w) <= block + reach);
        let mut gains: Vec<f32> = self.steady[from..to].iter().map(|w| w.2).collect();
        if gains.is_empty() {
            gains.push(nearest.2);
        }
        gains.sort_by(f32::total_cmp);
        (nearest.1, gains[gains.len() / 2])
    }
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len()) as f32;
    if n < 2.0 {
        return 0.0;
    }
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (x - ma, y - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if (aa / n).sqrt() < LEAST_SPREAD || (bb / n).sqrt() < LEAST_SPREAD {
        return 0.0;
    }
    ab / (aa * bb).sqrt()
}

fn active(levels: &[f32], floor: f32) -> usize {
    levels.iter().filter(|&&l| l >= floor).count()
}

fn floor_of(levels: &mut dyn FnMut(i64) -> u8, blocks: i64) -> f32 {
    let mut counts = [0usize; 256];
    for t in 0..blocks {
        counts[levels(t) as usize] += 1;
    }
    let total: usize = counts[1..].iter().sum();
    let mut seen = 0;
    for (level, n) in counts.iter().enumerate().skip(1) {
        seen += n;
        if seen * IDLE_SHARE >= total {
            return (level as f32 + ABOVE_IDLE).max(FLOOR as f32);
        }
    }
    FLOOR as f32
}

fn best_lag(near: &[f32], other: &[f32]) -> (f32, i64) {
    let n = near.len() as f64;
    let mean = near.iter().map(|&x| x as f64).sum::<f64>() / n;
    let centred: Vec<f64> = near.iter().map(|&x| x as f64 - mean).collect();
    let aa: f64 = centred.iter().map(|x| x * x).sum();
    if (aa / n).sqrt() < LEAST_SPREAD as f64 {
        return (0.0, 0);
    }
    (-MOST_LAG..=MOST_LAG)
        .map(|lag| {
            let from = (MOST_LAG - lag) as usize;
            let (mut sum, mut square, mut product) = (0.0, 0.0, 0.0);
            for (a, &x) in centred.iter().zip(&other[from..from + near.len()]) {
                let x = x as f64;
                sum += x;
                square += x * x;
                product += a * x;
            }
            let spread = (square - sum * sum / n).max(0.0);
            if (spread / n).sqrt() < LEAST_SPREAD as f64 {
                return (0.0, lag);
            }
            ((product / (aa * spread).sqrt()) as f32, lag)
        })
        .fold(
            (f32::MIN, 0),
            |best, next| if next.0 > best.0 { next } else { best },
        )
}

fn steady_of(windows: &[(i64, i64, f32)]) -> Vec<(i64, i64, f32)> {
    let reach = NEAR * WINDOW;
    windows
        .iter()
        .enumerate()
        .filter(|(i, (at, lag, _))| {
            let from = windows.partition_point(|w| w.0 < at - reach);
            let to = windows.partition_point(|w| w.0 <= at + reach);
            windows[from..to]
                .iter()
                .enumerate()
                .filter(|(j, w)| from + j != *i && (w.1 - lag).abs() <= SLACK)
                .count()
                >= AGREE
        })
        .map(|(_, w)| *w)
        .collect()
}

pub fn leak(
    mic: &mut dyn FnMut(i64) -> u8,
    far: &mut dyn FnMut(i64) -> u8,
    blocks: i64,
    halt: &AtomicBool,
) -> Option<Leak> {
    if halt.load(Ordering::Relaxed) {
        return None;
    }
    let floors = (floor_of(mic, blocks), floor_of(far, blocks));
    let mut windows: Vec<(i64, i64, f32)> = Vec::new();
    let mut heard = 0usize;
    let mut start = 0;
    while start + WINDOW <= blocks {
        if halt.load(Ordering::Relaxed) {
            return None;
        }
        let near: Vec<f32> = (start..start + WINDOW).map(|t| mic(t) as f32).collect();
        let other: Vec<f32> = (start - MOST_LAG..start + WINDOW + MOST_LAG)
            .map(|t| far(t) as f32)
            .collect();
        let at = MOST_LAG as usize;
        if active(&near, floors.0) >= ACTIVE
            && active(&other[at..at + WINDOW as usize], floors.1) >= ACTIVE
        {
            heard += 1;
            let (corr, lag) = best_lag(&near, &other);
            if corr >= CONFIDENT {
                let from = (MOST_LAG - lag) as usize;
                let both: Vec<f32> = near
                    .iter()
                    .zip(&other[from..from + WINDOW as usize])
                    .filter(|(m, f)| **m >= floors.0 && **f >= floors.1)
                    .map(|(m, f)| m - f)
                    .collect();
                if !both.is_empty() {
                    windows.push((start, lag, both.iter().sum::<f32>() / both.len() as f32));
                }
            }
        }
        start += WINDOW;
    }
    let steady = steady_of(&windows);
    if steady.len() < LEAST_STEADY || (steady.len() as f32) < LEAST_RATE * heard as f32 {
        return None;
    }
    Some(Leak { steady, floors })
}

pub fn is_echo(
    leak: &Leak,
    mic: &mut dyn FnMut(i64) -> u8,
    far: &mut dyn FnMut(i64) -> u8,
    from: i64,
    to: i64,
) -> bool {
    if to <= from {
        return false;
    }
    let (lag, gain) = leak.near((from + to) / 2);
    let (mic_floor, far_floor) = leak.floors;
    let near: Vec<f32> = (from..to).map(|t| mic(t) as f32).collect();
    let wide: Vec<f32> = (from - SLACK..to + SLACK)
        .map(|t| far(t - lag) as f32)
        .collect();
    let other = &wide[SLACK as usize..wide.len() - SLACK as usize];
    let (mut heard, mut own, mut run, mut longest) = (0usize, 0usize, 0usize, 0usize);
    for (m, f) in near.iter().zip(
        wide.windows(2 * SLACK as usize + 1)
            .map(|w| w.iter().copied().fold(0.0, f32::max)),
    ) {
        if *m < mic_floor {
            continue;
        }
        heard += 1;
        if f >= far_floor && *m <= f + gain + ABOVE_LEAK {
            run = 0;
        } else {
            own += 1;
            run += 1;
            longest = longest.max(run);
        }
    }
    heard >= LEAST_LINE
        && longest < OWN_RUN
        && own as f32 <= OWN_SHARE * heard as f32
        && pearson(&near, other) >= LINE_CORR
}

struct Track {
    dir: PathBuf,
    per_part: i64,
    lead: i64,
    parts: i64,
    loaded: Vec<(i64, Vec<u8>)>,
}

impl Track {
    fn of(session: &Session) -> Track {
        Track {
            dir: session.dir.clone(),
            per_part: (session.manifest.segment_secs * 1000 / BLOCK_MS) as i64,
            lead: (OVERLAP_SECS * 1000 / BLOCK_MS) as i64,
            parts: session.segments().last().map_or(0, |s| s.index as i64 + 1),
            loaded: Vec::new(),
        }
    }

    fn blocks(&self) -> i64 {
        self.parts * self.per_part
    }

    fn unmeasured(&self, segments: &[Segment]) -> Vec<u32> {
        let last = segments.last().map(|s| s.index);
        segments
            .iter()
            .filter(|s| matches!(&s.state, SegmentState::Done(text) if !text.trim().is_empty()))
            .filter(|s| {
                let kept =
                    std::fs::metadata(self.dir.join(levels_name(s.index))).map_or(0, |m| m.len());
                let lead = if s.index == 0 { 0 } else { self.lead as u64 };
                kept == 0
                    || (Some(s.index) != last && kept + SHORT_BY < self.per_part as u64 + lead)
            })
            .map(|s| s.index)
            .collect()
    }

    fn at(&mut self, block: i64) -> u8 {
        if block < 0 || self.per_part == 0 {
            return 0;
        }
        let part = block / self.per_part;
        if part >= self.parts {
            return 0;
        }
        if !self.loaded.iter().any(|(p, _)| *p == part) {
            let levels = std::fs::read(self.dir.join(levels_name(part as u32))).unwrap_or_default();
            if self.loaded.len() >= 2 {
                self.loaded.remove(0);
            }
            self.loaded.push((part, levels));
        }
        let skip = if part == 0 { 0 } else { self.lead };
        self.loaded
            .iter()
            .find(|(p, _)| *p == part)
            .and_then(|(_, levels)| levels.get((block % self.per_part + skip) as usize))
            .copied()
            .unwrap_or(0)
    }
}

pub fn marks(you: &Session, others: &Session, halt: &AtomicBool) -> Vec<bool> {
    let spans = you.spans_ms();
    let shift =
        (others.manifest.started - you.manifest.started).num_milliseconds() / BLOCK_MS as i64;
    let (mut mic, mut far) = (Track::of(you), Track::of(others));
    for (name, session, track) in [("You", you, &mic), ("Others", others, &far)] {
        let gaps = track.unmeasured(&session.segments());
        if !gaps.is_empty() {
            let parts: Vec<String> = gaps.iter().map(|p| (p + 1).to_string()).collect();
            you.trace(
                "echo",
                &format!(
                    "{name} track: sound levels missing or cut short for part {}; lines there are never marked as echo",
                    parts.join(", ")
                ),
            );
        }
    }
    let blocks = mic.blocks();
    let mut near = |t: i64| mic.at(t);
    let mut other = |t: i64| far.at(t - shift);
    let Some(found) = leak(&mut near, &mut other, blocks, halt) else {
        let why = if halt.load(Ordering::Relaxed) {
            "finishing now, echo not judged"
        } else {
            "no echo of the other side"
        };
        you.trace("echo", why);
        return vec![false; spans.len()];
    };
    let mut marked = Vec::with_capacity(spans.len());
    for (from, to) in &spans {
        if halt.load(Ordering::Relaxed) {
            you.trace("echo", "finishing now, echo not judged");
            return vec![false; spans.len()];
        }
        marked.push(is_echo(
            &found,
            &mut near,
            &mut other,
            (from / BLOCK_MS) as i64,
            to.div_ceil(BLOCK_MS) as i64,
        ));
    }
    you.trace(
        "echo",
        &format!(
            "the other side reaches the microphone in {} windows; {} of {} lines are its echo",
            found.steady.len(),
            marked.iter().filter(|m| **m).count(),
            marked.len()
        ),
    );
    marked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{turns_name, Manifest};

    const LENGTH: i64 = 60_000;
    const TURN: i64 = 2_000;

    fn random(seed: &mut u64) -> u64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *seed >> 33
    }

    fn speech(seed: u64, blocks: i64) -> Vec<u8> {
        let mut seed = seed;
        let mut out = Vec::new();
        while (out.len() as i64) < blocks {
            let talk = 15 + random(&mut seed) % 30;
            let loud = 55 + random(&mut seed) % 12;
            for _ in 0..talk {
                out.push((loud + random(&mut seed) % 6) as u8);
            }
            for _ in 0..5 + random(&mut seed) % 25 {
                out.push((10 + random(&mut seed) % 6) as u8);
            }
        }
        out.truncate(blocks as usize);
        out
    }

    fn quiet(blocks: i64, idle: u8) -> Vec<u8> {
        (0..blocks).map(|t| idle + (t * 7 % 6) as u8).collect()
    }

    fn far_turn(t: i64) -> bool {
        (t / TURN) % 2 == 0
    }

    struct Call {
        mic: Vec<u8>,
        far: Vec<u8>,
    }

    impl Call {
        fn leak(&self) -> Option<Leak> {
            leak(
                &mut |t| self.mic.get(t as usize).copied().unwrap_or(0),
                &mut |t| self.far.get(t as usize).copied().unwrap_or(0),
                self.mic.len() as i64,
                &AtomicBool::new(false),
            )
        }

        fn echo(&self, leak: &Leak, from: i64, to: i64) -> bool {
            is_echo(
                leak,
                &mut |t| self.mic.get(t as usize).copied().unwrap_or(0),
                &mut |t| self.far.get(t as usize).copied().unwrap_or(0),
                from,
                to,
            )
        }
    }

    fn turns(lag_at: impl Fn(i64) -> i64, headphones: bool) -> Call {
        room(lag_at, headphones, 12, |_| 25)
    }

    fn room(
        lag_at: impl Fn(i64) -> i64,
        headphones: bool,
        idle: u8,
        drop_at: impl Fn(i64) -> u8,
    ) -> Call {
        let talk = speech(1, LENGTH);
        let far: Vec<u8> = (0..LENGTH)
            .map(|t| if far_turn(t) { talk[t as usize] } else { 11 })
            .collect();
        let own = speech(2, LENGTH);
        let noise = quiet(LENGTH, idle);
        let mic = (0..LENGTH)
            .map(|t| {
                if !far_turn(t) {
                    return own[t as usize].max(noise[t as usize]);
                }
                let source = t - lag_at(t);
                let leaked = if headphones || source < 0 {
                    0
                } else {
                    far[source as usize].saturating_sub(drop_at(t))
                };
                leaked.max(noise[t as usize])
            })
            .collect();
        Call { mic, far }
    }

    #[test]
    fn an_echo_at_a_steady_lag_marks_the_far_side_lines_and_never_your_own() {
        let call = turns(|_| 18, false);
        let leak = call.leak().expect("the far side reaches the microphone");
        assert!(leak
            .steady
            .iter()
            .all(|(_, lag, gain)| (lag - 18).abs() <= SLACK && (gain + 25.0).abs() < 3.0));
        for turn in 0..LENGTH / TURN {
            let (from, to) = (turn * TURN + 18, (turn + 1) * TURN);
            assert_eq!(
                call.echo(&leak, from, to),
                far_turn(turn * TURN),
                "turn {turn}"
            );
        }
    }

    #[test]
    fn a_noisy_microphone_still_tells_its_echo_from_your_own_lines() {
        let call = room(|_| 18, false, 33, |_| 18);
        let leak = call.leak().expect("the far side reaches the microphone");
        for turn in 0..LENGTH / TURN {
            let (from, to) = (turn * TURN + 18, (turn + 1) * TURN);
            assert_eq!(
                call.echo(&leak, from, to),
                far_turn(turn * TURN),
                "turn {turn}"
            );
        }
        assert_eq!(room(|_| 18, true, 33, |_| 18).leak(), None);
    }

    #[test]
    fn talking_over_the_far_side_stays_yours_even_with_an_echo() {
        let mut call = turns(|_| 18, false);
        let leak = call.leak().unwrap();
        let own = speech(3, TURN);
        let at = 4 * TURN;
        for (i, level) in own.iter().enumerate() {
            let t = at as usize + i;
            call.mic[t] = call.mic[t].max(level.saturating_add(4));
        }
        assert!(!call.echo(&leak, at, at + TURN));
    }

    #[test]
    fn crosstalk_without_echo_is_never_marked() {
        let call = Call {
            mic: speech(5, LENGTH),
            far: speech(6, LENGTH),
        };
        assert_eq!(call.leak(), None);
    }

    #[test]
    fn headphones_or_a_silent_side_give_no_evidence() {
        assert_eq!(turns(|_| 18, true).leak(), None);
        let silent = Call {
            mic: speech(7, LENGTH),
            far: vec![0; LENGTH as usize],
        };
        assert_eq!(silent.leak(), None);
        let deaf = Call {
            mic: vec![0; LENGTH as usize],
            far: speech(8, LENGTH),
        };
        assert_eq!(deaf.leak(), None);
    }

    #[test]
    fn a_lag_that_changes_once_is_followed() {
        let change = LENGTH / 2;
        let lag_at = |t: i64| if t < change { 12 } else { 47 };
        let call = turns(lag_at, false);
        let leak = call.leak().unwrap();
        assert_eq!(leak.near(TURN).0, 12);
        assert_eq!(leak.near(LENGTH - TURN).0, 47);
        for turn in (0..LENGTH / TURN).filter(|n| far_turn(n * TURN)) {
            let from = turn * TURN + lag_at(turn * TURN);
            assert!(call.echo(&leak, from, (turn + 1) * TURN), "turn {turn}");
        }
    }

    #[test]
    fn very_short_lines_are_left_alone() {
        let call = turns(|_| 18, false);
        let leak = call.leak().unwrap();
        assert!(!call.echo(&leak, 18, 18 + 30));
        assert!(!call.echo(&leak, 18, 18));
    }

    fn pad(call: &Call, from: i64, own_to: i64) -> Call {
        let mut mic = call.mic.clone();
        let own = speech(9, own_to - from);
        for (i, level) in own.iter().enumerate() {
            let t = (from + i as i64) as usize;
            mic[t] = mic[t].max(level.saturating_add(4));
        }
        Call {
            mic,
            far: call.far.clone(),
        }
    }

    #[test]
    fn a_short_line_of_yours_before_a_long_echo_in_one_span_is_not_echo() {
        let call = turns(|_| 18, false);
        let leak = call.leak().unwrap();
        let onset = 4 * TURN;
        assert!(call.echo(&leak, onset + 18, onset + TURN));
        let mixed = pad(&call, onset, onset + 100);
        assert!(!mixed.echo(&leak, onset, onset + 900));
        assert!(!mixed.echo(&leak, onset, onset + TURN));
    }

    #[test]
    fn your_speech_with_echo_over_its_tail_is_not_echo() {
        let call = turns(|_| 18, false);
        let leak = call.leak().unwrap();
        let onset = 6 * TURN;
        for own in [100, 300, 600, 1000] {
            let mixed = pad(&call, onset, onset + own);
            assert!(!mixed.echo(&leak, onset, onset + TURN), "{own}");
        }
    }

    #[test]
    fn spans_made_from_one_onset_to_the_next_are_judged_by_their_own_sound() {
        let call = turns(|_| 18, false);
        let leak = call.leak().unwrap();
        for turn in (0..LENGTH / TURN - 3).filter(|n| far_turn(n * TURN)) {
            let onset = turn * TURN + 18;
            assert!(call.echo(&leak, onset, (turn + 1) * TURN), "echo {turn}");
            assert!(
                !call.echo(&leak, onset, (turn + 2) * TURN),
                "echo then your silence-less turn {turn}"
            );
            assert!(
                !call.echo(&leak, (turn + 1) * TURN, (turn + 3) * TURN),
                "your line then echo {turn}"
            );
        }
    }

    #[test]
    fn a_louder_far_side_later_in_the_call_is_still_echo_and_crosstalk_is_not() {
        let step = LENGTH / 2;
        let call = room(|_| 18, false, 12, |t| if t < step { 25 } else { 10 });
        let leak = call.leak().unwrap();
        for turn in (step / TURN + 8..LENGTH / TURN).filter(|n| far_turn(n * TURN)) {
            assert!(
                call.echo(&leak, turn * TURN + 18, (turn + 1) * TURN),
                "echo {turn}"
            );
            let talking = pad(&call, turn * TURN, (turn + 1) * TURN);
            assert!(
                !talking.echo(&leak, turn * TURN + 18, (turn + 1) * TURN),
                "crosstalk {turn}"
            );
        }
        for turn in (0..step / TURN - 8).filter(|n| far_turn(n * TURN)) {
            assert!(call.echo(&leak, turn * TURN + 18, (turn + 1) * TURN));
        }
    }

    #[test]
    fn the_steady_filter_scales_to_a_day_of_windows() {
        let day = 24 * 3600 * 100 / WINDOW;
        let windows: Vec<(i64, i64, f32)> =
            (0..day).map(|i| (i * WINDOW, 18 + i % 2, -25.0)).collect();
        let started = std::time::Instant::now();
        let steady = steady_of(&windows);
        assert_eq!(steady.len(), windows.len());
        assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
        let lone: Vec<(i64, i64, f32)> = (0..day).map(|i| (i * WINDOW * 100, 18, -25.0)).collect();
        assert!(steady_of(&lone).is_empty());
    }

    #[test]
    fn finishing_now_stops_the_search_before_it_reads_anything() {
        let reads = std::cell::Cell::new(0u64);
        let mut mic = |_: i64| {
            reads.set(reads.get() + 1);
            60u8
        };
        let mut far = |_: i64| 60u8;
        let found = leak(&mut mic, &mut far, 24 * 3600 * 100, &AtomicBool::new(true));
        assert_eq!(found, None);
        assert_eq!(reads.get(), 0);
    }

    type Damage<'a> = (&'a str, (i64, i64), &'a dyn Fn(&Sides));

    struct Sides {
        _root: tempfile::TempDir,
        you: Session,
        others: Session,
    }

    const PER_PART: usize = 30_000;
    const LEAD: usize = 100;

    fn lay(
        root: &Path,
        started: chrono::DateTime<chrono::Utc>,
        screen: bool,
        whole: &[u8],
        onsets: &[(i64, String)],
    ) -> Session {
        let mut manifest = Manifest::new(None, None, "", screen);
        manifest.started = started;
        manifest.turns = true;
        let session = Session::create(root, manifest).unwrap();
        for part in 0..whole.len().div_ceil(PER_PART) {
            let lo = if part == 0 { 0 } else { part * PER_PART - LEAD };
            let hi = ((part + 1) * PER_PART).min(whole.len());
            std::fs::write(session.dir.join(levels_name(part as u32)), &whole[lo..hi]).unwrap();
            let entries: Vec<(u64, String)> = onsets
                .iter()
                .filter(|(t, _)| *t as usize / PER_PART == part)
                .map(|(t, text)| ((*t as usize - lo) as u64 * BLOCK_MS, text.clone()))
                .collect();
            std::fs::write(
                session.dir.join(format!("seg-{part:05}.txt")),
                format!("said {part}"),
            )
            .unwrap();
            std::fs::write(
                session.dir.join(turns_name(part as u32)),
                serde_json::to_string(&entries).unwrap(),
            )
            .unwrap();
        }
        session
    }

    fn sides(call: &Call, shift: i64) -> Sides {
        let root = tempfile::tempdir().unwrap();
        let started = chrono::Utc::now();
        let others_started = started + chrono::Duration::milliseconds(shift * BLOCK_MS as i64);
        let far: Vec<u8> = (0..call.far.len() as i64)
            .map(|k| call.far.get((k + shift) as usize).copied().unwrap_or(0))
            .collect();
        let onsets: Vec<(i64, String)> = line_starts()
            .into_iter()
            .map(|(at, echo)| (at, format!("{} {at}", if echo { "echo" } else { "own" })))
            .collect();
        let you = lay(&root.path().join("you"), started, false, &call.mic, &onsets);
        let others = lay(&root.path().join("others"), others_started, true, &far, &[]);
        Sides {
            _root: root,
            you,
            others,
        }
    }

    fn line_starts() -> Vec<(i64, bool)> {
        (0..LENGTH / TURN)
            .map(|turn| {
                let echo = far_turn(turn * TURN);
                (turn * TURN + if echo { 18 } else { 0 }, echo)
            })
            .collect()
    }

    #[test]
    fn tracks_started_at_different_times_and_split_in_parts_are_lined_up() {
        let call = turns(|_| 18, false);
        for shift in [300, -200, 0] {
            let expected: Vec<bool> = line_starts()
                .into_iter()
                .map(|(at, echo)| echo && at >= shift)
                .collect();
            let sides = sides(&call, shift);
            let marked = marks(&sides.you, &sides.others, &AtomicBool::new(false));
            assert_eq!(marked, expected, "shift {shift}");
            let traced = sides.you.traces();
            assert!(
                traced.iter().all(|t| !t.detail.contains("cut short")),
                "{traced:?}"
            );
        }
    }

    #[test]
    fn missing_or_cut_short_levels_mark_nothing_there_and_say_so() {
        let call = turns(|_| 18, false);
        let cases: [Damage; 4] = [
            ("You", (0, 30_000), &|s: &Sides| {
                std::fs::remove_file(s.you.dir.join(levels_name(0))).unwrap()
            }),
            ("Others", (30_000, 60_000), &|s: &Sides| {
                std::fs::remove_file(s.others.dir.join(levels_name(1))).unwrap()
            }),
            ("You", (6_000, 30_000), &|s: &Sides| {
                std::fs::write(s.you.dir.join(levels_name(0)), &call.mic[..6_000]).unwrap()
            }),
            ("Others", (0, 30_000), &|s: &Sides| {
                std::fs::write(s.others.dir.join(levels_name(0)), []).unwrap()
            }),
        ];
        for (side, (lo, hi), damage) in cases {
            let sides = sides(&call, 0);
            damage(&sides);
            let marked = marks(&sides.you, &sides.others, &AtomicBool::new(false));
            let expected: Vec<bool> = line_starts()
                .into_iter()
                .map(|(at, echo)| echo && !(lo..hi).contains(&at))
                .collect();
            assert_eq!(marked, expected, "{side} {lo}");
            let said = sides.you.traces();
            assert!(
                said.iter()
                    .any(|t| t.detail.starts_with(&format!("{side} track"))
                        && t.detail.contains("never marked")),
                "{side}: {said:?}"
            );
        }
    }

    #[test]
    fn finishing_now_marks_nothing_and_says_so() {
        let call = turns(|_| 18, false);
        let sides = sides(&call, 0);
        let marked = marks(&sides.you, &sides.others, &AtomicBool::new(true));
        assert!(marked.iter().all(|m| !m));
        assert!(sides
            .you
            .traces()
            .iter()
            .any(|t| t.detail.contains("finishing now")));
    }

    #[test]
    fn levels_are_decibels_per_ten_milliseconds() {
        let mut samples = vec![0i16; BLOCK];
        samples.extend(vec![1000i16; BLOCK]);
        samples.extend(vec![i16::MAX; BLOCK / 2]);
        assert_eq!(levels(&samples), vec![0, 60, 90]);
    }
}
