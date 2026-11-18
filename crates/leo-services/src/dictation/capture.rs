use anyhow::{bail, ensure, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError},
};
use std::time::{Duration, Instant};
pub const FIRST_AUDIO: Duration = Duration::from_secs(8);
const STALL: Duration = Duration::from_secs(5);
#[derive(Debug)]
pub struct Captured {
    pub samples: usize,
    pub stalled: bool,
}
fn append(
    count: &mut usize,
    samples: &[i16],
    heard: &mut impl FnMut(&[f32], f32, f64),
) -> Result<()> {
    ensure!(
        samples.len() <= 16_000 * 10,
        "The microphone sent too much audio. Choose another microphone and try again."
    );
    let chunk: Vec<f32> = samples.iter().map(|n| *n as f32 / 32768.0).collect();
    let rms = (chunk.iter().map(|n| n * n).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
    *count += chunk.len();
    heard(&chunk, rms, *count as f64 / 16_000.0);
    Ok(())
}
pub fn collect(
    rx: &Receiver<Vec<i16>>,
    stop: &AtomicBool,
    heard: impl FnMut(&[f32], f32, f64),
    close: impl FnOnce() + Send,
) -> Result<Captured> {
    collect_within(rx, stop, heard, close, FIRST_AUDIO, STALL)
}
fn collect_within(
    rx: &Receiver<Vec<i16>>,
    stop: &AtomicBool,
    mut heard: impl FnMut(&[f32], f32, f64),
    close: impl FnOnce() + Send,
    first_audio: Duration,
    stall: Duration,
) -> Result<Captured> {
    let started = Instant::now();
    let mut count = 0;
    let mut last_chunk = None;
    let mut stalled = false;
    let result = (|| -> Result<()> {
        while !stop.load(Ordering::Acquire) {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(samples) => {
                    last_chunk = Some(Instant::now());
                    append(&mut count, &samples, &mut heard)?
                }
                Err(RecvTimeoutError::Timeout)
                    if last_chunk.is_some_and(|at: Instant| at.elapsed() >= stall) =>
                {
                    stalled = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout)
                    if count == 0 && started.elapsed() >= first_audio =>
                {
                    bail!("The microphone is not sending audio. Choose another in Dictation settings and try again.")
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    })();
    let closed = std::thread::scope(|scope| -> Result<()> {
        let closing = scope.spawn(close);
        while !closing.is_finished() {
            if let Ok(samples) = rx.recv_timeout(Duration::from_millis(5)) {
                append(&mut count, &samples, &mut heard)?;
            }
        }
        Ok(())
    });
    result?;
    closed?;
    for samples in rx.try_iter().take(64) {
        append(&mut count, &samples, &mut heard)?;
    }
    Ok(Captured {
        samples: count,
        stalled,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_is_bounded_and_stops_without_waiting_for_audio() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(vec![1000; 16_000]).unwrap();
        drop(tx);
        let mut seconds = 0.0;
        assert_eq!(
            collect(&rx, &AtomicBool::new(false), |_, _, s| seconds = s, || {})
                .unwrap()
                .samples,
            16_000
        );
        assert_eq!(seconds, 1.0);
        let (_tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            collect(&rx, &AtomicBool::new(true), |_, _, _| panic!(), || {})
                .unwrap()
                .samples,
            0
        );
    }

    #[test]
    fn a_dictation_longer_than_five_minutes_keeps_all_its_audio() {
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..301 {
            tx.send(vec![1000; 16_000]).unwrap();
        }
        drop(tx);
        let mut heard = 0;
        let captured = collect(
            &rx,
            &AtomicBool::new(false),
            |chunk, _, _| heard += chunk.len(),
            || {},
        )
        .unwrap();
        assert_eq!(heard, 301 * 16_000);
        assert!(!captured.stalled);
    }

    #[test]
    fn one_message_over_ten_seconds_is_refused() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(vec![1000; 160_000]).unwrap();
        let captured = collect(&rx, &AtomicBool::new(true), |_, _, _| {}, || {}).unwrap();
        assert_eq!(captured.samples, 160_000);
        tx.send(vec![1000; 160_001]).unwrap();
        let error = collect(&rx, &AtomicBool::new(true), |_, _, _| {}, || {})
            .err()
            .unwrap();
        assert!(error.to_string().contains("too much audio"));
    }
}

#[cfg(test)]
mod tail_tests {
    use super::*;
    #[test]
    fn a_microphone_that_never_sends_audio_stops_the_dictation() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let error = collect_within(
            &rx,
            &AtomicBool::new(false),
            |_, _, _| {},
            || {},
            Duration::ZERO,
            STALL,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not sending audio"));
    }

    #[test]
    fn a_microphone_that_goes_quiet_ends_the_dictation_with_what_it_sent() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(vec![1000; 1600]).unwrap();
        let captured = collect_within(
            &rx,
            &AtomicBool::new(false),
            |_, _, _| {},
            || {},
            FIRST_AUDIO,
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(captured.stalled);
        assert_eq!(captured.samples, 1600);
        drop(tx);
    }

    #[test]
    fn audio_is_read_while_the_microphone_closes() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let pcm = collect(
            &rx,
            &AtomicBool::new(true),
            |_, _, _| {},
            || {
                for _ in 0..4 {
                    tx.send(vec![1000; 1600]).unwrap();
                }
            },
        );
        assert_eq!(pcm.unwrap().samples, 6400);
    }
    #[test]
    fn closing_the_microphone_precedes_draining_its_last_audio() {
        let (tx, rx) = std::sync::mpsc::channel();
        let closed = AtomicBool::new(false);
        let pcm = collect(
            &rx,
            &AtomicBool::new(true),
            |_, _, _| assert!(closed.load(Ordering::Acquire)),
            || {
                closed.store(true, Ordering::Release);
                tx.send(vec![1000; 1600]).unwrap();
            },
        );
        assert_eq!(pcm.unwrap().samples, 1600);
    }
}
