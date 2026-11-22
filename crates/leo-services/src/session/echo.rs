use std::path::{Path, PathBuf};

use super::{wav, Session, OVERLAP_SECS};

pub const BLOCK_MS: u64 = 10;
const BLOCK: usize = (wav::RATE as u64 * BLOCK_MS / 1000) as usize;
const WINDOW: i64 = 200;
const MOST_LAG: i64 = 200;
const FLOOR: u8 = 30;
const ACTIVE: usize = 50;
const CONFIDENT: f32 = 0.6;
const LEAST_SPREAD: f32 = 3.0;
const NEAR: i64 = 10;
const AGREE: usize = 2;
const SLACK: i64 = 2;
const LEAST_RATE: f32 = 0.2;
const LEAST_STEADY: usize = 3;
const LEAST_LINE: usize = 50;
const COPY_SHARE: f32 = 0.7;
const LINE_CORR: f32 = 0.5;
const ABOVE_LEAK: f32 = 6.0;

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
    steady: Vec<(i64, i64)>,
    gain: f32,
}

impl Leak {
    fn lag_near(&self, block: i64) -> i64 {
        let at = self
            .steady
            .partition_point(|(start, _)| start + WINDOW / 2 < block);
        [at.checked_sub(1), Some(at)]
            .into_iter()
            .flatten()
            .filter_map(|i| self.steady.get(i))
            .min_by_key(|(start, _)| (start + WINDOW / 2 - block).abs())
            .map_or(0, |(_, lag)| *lag)
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

fn active(levels: &[f32]) -> usize {
    levels.iter().filter(|&&l| l >= FLOOR as f32).count()
}

pub fn leak(
    mic: &mut dyn FnMut(i64) -> u8,
    far: &mut dyn FnMut(i64) -> u8,
    blocks: i64,
) -> Option<Leak> {
    let mut windows: Vec<(i64, i64, f32)> = Vec::new();
    let mut heard = 0usize;
    let mut start = 0;
    while start + WINDOW <= blocks {
        let near: Vec<f32> = (start..start + WINDOW).map(|t| mic(t) as f32).collect();
        let other: Vec<f32> = (start - MOST_LAG..start + WINDOW + MOST_LAG)
            .map(|t| far(t) as f32)
            .collect();
        let at = MOST_LAG as usize;
        if active(&near) >= ACTIVE && active(&other[at..at + WINDOW as usize]) >= ACTIVE {
            heard += 1;
            let (corr, lag) = (-MOST_LAG..=MOST_LAG)
                .map(|lag| {
                    let from = (MOST_LAG - lag) as usize;
                    (pearson(&near, &other[from..from + WINDOW as usize]), lag)
                })
                .fold(
                    (f32::MIN, 0),
                    |best, next| if next.0 > best.0 { next } else { best },
                );
            if corr >= CONFIDENT {
                let from = (MOST_LAG - lag) as usize;
                let both: Vec<f32> = near
                    .iter()
                    .zip(&other[from..from + WINDOW as usize])
                    .filter(|(m, f)| **m >= FLOOR as f32 && **f >= FLOOR as f32)
                    .map(|(m, f)| m - f)
                    .collect();
                if !both.is_empty() {
                    windows.push((start, lag, both.iter().sum::<f32>() / both.len() as f32));
                }
            }
        }
        start += WINDOW;
    }
    let steady: Vec<(i64, i64, f32)> = windows
        .iter()
        .filter(|(at, lag, _)| {
            windows
                .iter()
                .filter(|(other, other_lag, _)| {
                    other != at
                        && (other - at).abs() <= NEAR * WINDOW
                        && (other_lag - lag).abs() <= SLACK
                })
                .count()
                >= AGREE
        })
        .copied()
        .collect();
    if steady.len() < LEAST_STEADY || (steady.len() as f32) < LEAST_RATE * heard as f32 {
        return None;
    }
    let mut gains: Vec<f32> = steady.iter().map(|w| w.2).collect();
    gains.sort_by(f32::total_cmp);
    Some(Leak {
        steady: steady.iter().map(|w| (w.0, w.1)).collect(),
        gain: gains[gains.len() / 2],
    })
}

pub fn is_echo(
    leak: &Leak,
    mic: &mut dyn FnMut(i64) -> u8,
    far: &mut dyn FnMut(i64) -> u8,
    from: i64,
    to: i64,
) -> bool {
    let lag = leak.lag_near((from + to) / 2);
    let near: Vec<f32> = (from..to).map(|t| mic(t) as f32).collect();
    let other: Vec<f32> = (from..to).map(|t| far(t - lag) as f32).collect();
    let heard = active(&near);
    if heard < LEAST_LINE {
        return false;
    }
    let copies = near
        .iter()
        .zip(&other)
        .filter(|(m, f)| {
            **m >= FLOOR as f32 && **f >= FLOOR as f32 && **m <= **f + leak.gain + ABOVE_LEAK
        })
        .count();
    copies as f32 >= COPY_SHARE * heard as f32 && pearson(&near, &other) >= LINE_CORR
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

pub fn marks(you: &Session, others: &Session) -> Vec<bool> {
    let spans = you.spans_ms();
    let shift =
        (others.manifest.started - you.manifest.started).num_milliseconds() / BLOCK_MS as i64;
    let (mut mic, mut far) = (Track::of(you), Track::of(others));
    let blocks = mic.blocks();
    let mut near = |t: i64| mic.at(t);
    let mut other = |t: i64| far.at(t - shift);
    let Some(found) = leak(&mut near, &mut other, blocks) else {
        you.trace("echo", "no echo of the other side");
        return vec![false; spans.len()];
    };
    let marked: Vec<bool> = spans
        .iter()
        .map(|(from, to)| {
            is_echo(
                &found,
                &mut near,
                &mut other,
                (from / BLOCK_MS) as i64,
                to.div_ceil(BLOCK_MS) as i64,
            )
        })
        .collect();
    you.trace(
        "echo",
        &format!(
            "the other side reaches the microphone in {} windows, {:.0} dB below; {} of {} lines are its echo",
            found.steady.len(),
            -found.gain,
            marked.iter().filter(|m| **m).count(),
            marked.len()
        ),
    );
    marked
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn quiet(blocks: i64) -> Vec<u8> {
        (0..blocks).map(|t| 12 + (t * 7 % 5) as u8).collect()
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
        let talk = speech(1, LENGTH);
        let far: Vec<u8> = (0..LENGTH)
            .map(|t| if far_turn(t) { talk[t as usize] } else { 11 })
            .collect();
        let own = speech(2, LENGTH);
        let noise = quiet(LENGTH);
        let mic = (0..LENGTH)
            .map(|t| {
                if !far_turn(t) {
                    return own[t as usize];
                }
                let source = t - lag_at(t);
                let leaked = if headphones || source < 0 {
                    0
                } else {
                    far[source as usize].saturating_sub(25)
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
        assert!(leak.steady.iter().all(|(_, lag)| (lag - 18).abs() <= SLACK));
        assert!((leak.gain + 25.0).abs() < 3.0, "{}", leak.gain);
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
        assert_eq!(leak.lag_near(TURN), 12);
        assert_eq!(leak.lag_near(LENGTH - TURN), 47);
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

    #[test]
    fn levels_are_decibels_per_ten_milliseconds() {
        let mut samples = vec![0i16; BLOCK];
        samples.extend(vec![1000i16; BLOCK]);
        samples.extend(vec![i16::MAX; BLOCK / 2]);
        assert_eq!(levels(&samples), vec![0, 60, 90]);
    }
}
