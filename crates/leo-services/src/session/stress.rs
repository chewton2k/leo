use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::capture::{synthetic, Capture, Source};
use super::transcriber::{Policy, TranscribeFn, Transcriber};
use super::{wav, Manifest, Session, OVERLAP_SECS};

fn quick() -> Policy {
    Policy {
        first_wait: Duration::from_millis(2),
        most_wait: Duration::from_millis(20),
        attempts_after_stop: 50,
        idle: Duration::from_millis(2),
        workers: 3,
        finish_limit: Duration::from_secs(30),
        keep_parts: 0,
        relist: Duration::from_secs(60),
    }
}

fn text_for(index: u32) -> String {
    format!("s{index}a s{index}b s{index}c")
}

fn flaky(calls: Arc<AtomicU64>) -> TranscribeFn {
    Arc::new(move |path: &Path| {
        let n = calls.fetch_add(1, Ordering::Relaxed);
        if n % 3 == 1 {
            return Err("429 too many requests".to_string());
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let index: u32 = name
            .trim_start_matches("seg-")
            .trim_end_matches(".wav")
            .parse()
            .unwrap();
        Ok(text_for(index))
    })
}

fn silent_segment(index: u32, segment_secs: u64) -> bool {
    let first = index as u64 * segment_secs;
    (first..first + segment_secs).all(|s| synthetic(s).iter().all(|x| *x == 0))
}

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

struct Run {
    segments: u32,
    peak_disk: u64,
    took: Duration,
}

fn record(secs: u64, speed: f64, segment_secs: u64, crash_after: Option<u32>) -> Run {
    let root = tempfile::tempdir().unwrap();
    let mut manifest = Manifest::new(None, None, "", false);
    manifest.segment_secs = segment_secs;
    let session = Session::create(root.path(), manifest).unwrap();
    let started = Instant::now();
    let calls = Arc::new(AtomicU64::new(0));
    let (tx, rx) = mpsc::channel();
    let capture = Capture::start(
        &session.dir,
        0,
        segment_secs,
        Source::Synthetic { secs, speed },
    )
    .unwrap();
    let transcriber =
        Transcriber::start(&session.dir, flaky(Arc::clone(&calls)), quick(), true, tx);

    let mut peak_disk = 0;
    let crash_copy = tempfile::tempdir().unwrap();
    let mut crashed = false;
    while !capture.ended() {
        peak_disk = peak_disk.max(dir_bytes(&session.dir));
        if let Some(after) = crash_after {
            if !crashed && session.segments().len() as u32 > after {
                for entry in std::fs::read_dir(&session.dir).unwrap().flatten() {
                    match std::fs::copy(entry.path(), crash_copy.path().join(entry.file_name())) {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => panic!("{e}"),
                    }
                }
                crashed = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    capture.stop().unwrap();
    transcriber.wait();
    drop(rx);

    let expected_segments = secs.div_ceil(segment_secs) as u32;
    check_complete(&session, expected_segments, segment_secs);

    if crashed {
        let resumed = Session::open(crash_copy.path()).unwrap();
        let cut_short = resumed
            .segments()
            .iter()
            .find(|s| s.state == super::SegmentState::Recording)
            .map(|s| s.index)
            .expect("the crash left a segment being recorded");
        resumed.recover_parts().unwrap();
        let (tx, _rx) = mpsc::channel();
        Transcriber::start(crash_copy.path(), flaky(calls), quick(), false, tx).wait();
        let assembled = resumed.assemble();
        assert!(assembled.failed.is_empty());
        assert_eq!(assembled.pending, 0);
        let text = assembled.text();
        for p in &assembled.parts {
            let barely_started = p.index == cut_short && p.text.is_empty();
            if !silent_segment(p.index, segment_secs) && !barely_started {
                assert!(
                    text.contains(&text_for(p.index)),
                    "segment {} lost after the crash",
                    p.index
                );
            }
        }
    }

    Run {
        segments: expected_segments,
        peak_disk,
        took: started.elapsed(),
    }
}

fn check_complete(session: &Session, expected: u32, segment_secs: u64) {
    let assembled = session.assemble();
    assert_eq!(assembled.pending, 0, "segments were left untranscribed");
    assert!(
        assembled.failed.is_empty(),
        "segments failed: {:?}",
        assembled.failed
    );
    assert_eq!(
        assembled.parts.len() as u32,
        expected,
        "a segment went missing"
    );
    let text = assembled.text();
    let mut last = 0;
    for part in &assembled.parts {
        let want = text_for(part.index);
        if silent_segment(part.index, segment_secs) {
            assert_eq!(part.text, "", "silence was sent for transcription");
            continue;
        }
        let at = text
            .find(&want)
            .unwrap_or_else(|| panic!("segment {} is missing", part.index));
        assert!(at >= last, "segment {} is out of order", part.index);
        assert_eq!(
            text.matches(&want).count(),
            1,
            "segment {} is doubled",
            part.index
        );
        last = at;
    }
    let leftover_audio = std::fs::read_dir(&session.dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".wav"))
        .count();
    assert_eq!(leftover_audio, 0, "audio was kept after its text was saved");
}

#[test]
fn a_three_hour_recording_with_a_flaky_provider_loses_nothing() {
    let run = record(3 * 3600, 3600.0, 300, None);
    assert_eq!(run.segments, 36);
    let segment_bytes = (300 + OVERLAP_SECS) * wav::RATE as u64 * 2;
    assert!(
        run.peak_disk < segment_bytes * 6,
        "disk grew to {} bytes for a recording that should keep only a few segments",
        run.peak_disk
    );
}

#[test]
fn a_crash_mid_recording_is_finished_from_what_was_on_disk() {
    record(2 * 3600, 2400.0, 300, Some(6));
}

#[test]
fn many_short_segments_roll_over_cleanly() {
    let run = record(1200, 2400.0, 7, None);
    assert_eq!(run.segments, 172);
}

#[test]
#[ignore = "simulates 24 hours of recording; run by hand"]
fn a_full_day_of_recording() {
    let run = record(24 * 3600, 20_000.0, 300, Some(100));
    println!(
        "24h: {} segments, peak disk {:.1} MB, took {:.1}s",
        run.segments,
        run.peak_disk as f64 / 1e6,
        run.took.as_secs_f64()
    );
    assert_eq!(run.segments, 288);
}
