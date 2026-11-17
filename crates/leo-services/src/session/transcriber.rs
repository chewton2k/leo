use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::wav;
use crate::ai::live;

pub type TranscribeFn = Arc<dyn Fn(&Path) -> Result<String, String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    Done {
        index: u32,
        text: String,
    },
    Retrying {
        index: u32,
        attempt: u32,
        error: String,
        wait: Duration,
    },
    Failed {
        index: u32,
        error: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub first_wait: Duration,
    pub most_wait: Duration,
    pub attempts_after_stop: u32,
    pub idle: Duration,
    pub workers: usize,
    pub finish_limit: Duration,
    pub keep_parts: u32,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            first_wait: Duration::from_secs(5),
            most_wait: Duration::from_secs(300),
            attempts_after_stop: 6,
            idle: Duration::from_millis(500),
            workers: 3,
            finish_limit: Duration::from_secs(120),
            keep_parts: 0,
        }
    }
}

impl Policy {
    pub fn wait(&self, attempt: u32) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1).min(16));
        self.first_wait.saturating_mul(factor).min(self.most_wait)
    }
}

pub struct Transcriber {
    cancel: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub fn rate_limited(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("429")
        || e.contains("rate limit")
        || e.contains("rate-limit")
        || e.contains("too many requests")
}

pub fn waiting_for_model(error: &str) -> bool {
    error.contains("Settings → Speech models")
        || error.contains(crate::speech::engine::STOPPED)
        || error.contains(crate::speech::engine::STOPPING)
}

pub fn waiting(dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<u32> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let digits = name.strip_prefix("seg-")?.strip_suffix(".wav")?;
            if digits.contains('.') {
                return None;
            }
            let index: u32 = digits.parse().ok()?;
            let settled = dir.join(format!("seg-{index:05}.txt")).exists()
                || dir.join(format!("seg-{index:05}.err")).exists();
            (!settled).then_some(index)
        })
        .collect();
    out.sort();
    out
}

fn recording_now(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".part.wav"))
    })
}

#[cfg(test)]
thread_local! { static BEFORE_WRITE: std::cell::RefCell<Option<Box<dyn Fn()>>> = Default::default(); }

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    #[cfg(test)]
    BEFORE_WRITE.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook();
        }
    });
    leo_core::paths::write_atomic(path, text).map_err(std::io::Error::other)
}

fn text_path(dir: &Path, index: u32) -> PathBuf {
    dir.join(format!("seg-{index:05}.txt"))
}

impl Transcriber {
    pub fn start(
        dir: &Path,
        transcribe: TranscribeFn,
        policy: Policy,
        recording: bool,
        events: Sender<Update>,
    ) -> Transcriber {
        let cancel = Arc::new(AtomicBool::new(false));
        let live_flag = Arc::new(AtomicBool::new(recording));
        let thread = {
            let dir = dir.to_path_buf();
            let (cancel, live_flag) = (Arc::clone(&cancel), Arc::clone(&live_flag));
            std::thread::Builder::new()
                .name("leo-transcriber".into())
                .spawn(move || run(&dir, transcribe, policy, &cancel, &live_flag, &events))
                .ok()
        };
        Transcriber {
            cancel,
            recording: live_flag,
            thread,
        }
    }

    pub fn recording_ended(&self) {
        self.recording.store(false, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn wait(mut self) {
        self.recording_ended();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Transcriber {
    fn drop(&mut self) {
        self.cancel();
        self.recording_ended();
        if let Some(t) = self.thread.take().filter(|t| t.is_finished()) {
            let _ = t.join();
        }
    }
}

#[derive(Default)]
struct Work {
    attempts: Mutex<HashMap<u32, (u32, Instant)>>,
    busy: Mutex<HashSet<u32>>,
    terminal: Mutex<HashSet<u32>>,
    stop_seen: AtomicBool,
    stopped_at: Mutex<Option<Instant>>,
}

impl Work {
    fn pending(&self, dir: &Path) -> Vec<u32> {
        let terminal = self
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        waiting(dir)
            .into_iter()
            .filter(|i| !terminal.contains(i))
            .collect()
    }
    fn claim(&self, dir: &Path) -> Option<u32> {
        let pending = self.pending(dir);
        let mut busy = self.busy.lock().ok()?;
        let attempts = self.attempts.lock().ok()?;
        let now = Instant::now();
        let index = pending
            .into_iter()
            .find(|i| !busy.contains(i) && attempts.get(i).is_none_or(|(_, next)| now >= *next))?;
        busy.insert(index);
        Some(index)
    }

    fn release(&self, index: u32) {
        if let Ok(mut busy) = self.busy.lock() {
            busy.remove(&index);
        }
    }

    fn idle(&self) -> bool {
        self.busy.lock().is_ok_and(|b| b.is_empty())
    }
}

fn run(
    dir: &Path,
    transcribe: TranscribeFn,
    policy: Policy,
    cancel: &AtomicBool,
    recording: &AtomicBool,
    events: &Sender<Update>,
) {
    let work = Work::default();
    std::thread::scope(|scope| {
        for _ in 0..policy.workers.max(1) {
            scope.spawn(|| worker(dir, &transcribe, policy, cancel, recording, events, &work));
        }
    });
}

fn worker(
    dir: &Path,
    transcribe: &TranscribeFn,
    policy: Policy,
    cancel: &AtomicBool,
    recording: &AtomicBool,
    events: &Sender<Update>,
    work: &Work,
) {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let still_recording = recording.load(Ordering::Relaxed);
        if !still_recording && !work.stop_seen.swap(true, Ordering::Relaxed) {
            if let Ok(mut when) = work.stopped_at.lock() {
                *when = Some(Instant::now());
            }
            if let Ok(mut attempts) = work.attempts.lock() {
                attempts.clear();
            }
        }
        let Some(index) = work.claim(dir) else {
            if work.pending(dir).is_empty()
                && !still_recording
                && !recording_now(dir)
                && work.idle()
            {
                return;
            }
            std::thread::sleep(policy.idle);
            continue;
        };
        settle(dir, index, transcribe, policy, recording, events, work);
        work.release(index);
    }
}

fn settle(
    dir: &Path,
    index: u32,
    transcribe: &TranscribeFn,
    policy: Policy,
    recording: &AtomicBool,
    events: &Sender<Update>,
    work: &Work,
) {
    let path = dir.join(wav::done_name(index));
    let result = match wav::read(&path) {
        Ok(samples) if live::is_silent(wav::peak(&samples)) => Ok(String::new()),
        Ok(_) => transcribe(&path).map(|t| {
            if live::is_silence_artifact(&t) {
                String::new()
            } else {
                t.trim().to_string()
            }
        }),
        Err(e) => Err(e.to_string()),
    };
    match result {
        Ok(text) => {
            if let Err(e) = write_atomic(&text_path(dir, index), &text) {
                if work
                    .stopped_at
                    .lock()
                    .ok()
                    .and_then(|v| *v)
                    .is_some_and(|at| at.elapsed() >= policy.finish_limit)
                {
                    if let Ok(mut terminal) = work.terminal.lock() {
                        terminal.insert(index);
                    }
                    let _ = events.send(Update::Failed {
                        index,
                        error: format!("Could not save transcript: {e}; audio retained"),
                    });
                    return;
                }
                work.attempts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(index, (0, Instant::now() + policy.idle));
                let _ = events.send(Update::Retrying {
                    index,
                    attempt: 0,
                    error: format!("could not save the transcript: {e}"),
                    wait: policy.idle,
                });
                return;
            }
            if index >= policy.keep_parts {
                let _ = std::fs::remove_file(&path);
            }
            work.attempts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&index);
            let _ = events.send(Update::Done { index, text });
        }
        Err(error) => {
            let attempt = work
                .attempts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&index)
                .map_or(1, |(n, _)| n + 1);
            let finishing = !recording.load(Ordering::Relaxed);
            let expired = work
                .stopped_at
                .lock()
                .ok()
                .and_then(|v| *v)
                .is_some_and(|at| at.elapsed() >= policy.finish_limit);
            if finishing
                && (expired
                    || (attempt >= policy.attempts_after_stop
                        && !rate_limited(&error)
                        && !waiting_for_model(&error)))
            {
                if waiting_for_model(&error)
                    || write_atomic(&dir.join(format!("seg-{index:05}.err")), &error).is_err()
                {
                    if let Ok(mut terminal) = work.terminal.lock() {
                        terminal.insert(index);
                    }
                }
                work.attempts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&index);
                let _ = events.send(Update::Failed { index, error });
            } else {
                let mut wait = if waiting_for_model(&error) {
                    policy.first_wait.min(Duration::from_secs(2))
                } else {
                    policy.wait(attempt)
                };
                if finishing {
                    if let Some(at) = work.stopped_at.lock().ok().and_then(|v| *v) {
                        wait = wait.min(policy.finish_limit.saturating_sub(at.elapsed()));
                    }
                }
                work.attempts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(index, (attempt, Instant::now() + wait));
                let _ = events.send(Update::Retrying {
                    index,
                    attempt,
                    error,
                    wait,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_no_model_message_keeps_the_part_waiting() {
        for message in [
            "Choose a model in Settings → Speech models. Your recorded audio is kept.",
            "Choose a speech model from Settings → Speech models.",
            "That speech model is not in the library. Choose one in Settings → Speech models.",
            "Download a model in Settings → Speech models. Your recorded audio is kept until one is ready.",
            "Choose and download a model in Settings → Speech models. Your audio is kept until one is ready.",
            crate::speech::engine::STOPPED,
            crate::speech::engine::STOPPING,
        ] {
            assert!(waiting_for_model(message), "{message}");
        }
        assert!(!waiting_for_model(
            "The speech engine is busy. Your recorded audio is kept; try again in a moment."
        ));
    }
    use std::sync::atomic::AtomicU32;
    use std::sync::mpsc;

    #[test]
    fn cancellation_does_not_wait_for_an_in_flight_provider() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let (started, start) = mpsc::channel();
        let (events, updates) = mpsc::channel();
        let worker = Transcriber::start(
            dir.path(),
            Arc::new(move |_| {
                let _ = started.send(());
                std::thread::sleep(Duration::from_millis(300));
                Ok("kept transcript".into())
            }),
            Policy {
                workers: 1,
                ..quick()
            },
            true,
            events,
        );
        start.recv_timeout(Duration::from_secs(2)).unwrap();
        let at = Instant::now();
        drop(worker);
        let elapsed = at.elapsed();
        assert!(matches!(
            updates.recv_timeout(Duration::from_secs(2)).unwrap(),
            Update::Done { .. }
        ));
        assert!(elapsed < Duration::from_millis(200), "{elapsed:?}");
        assert_eq!(
            std::fs::read_to_string(text_path(dir.path(), 0)).unwrap(),
            "kept transcript"
        );
    }

    #[test]
    fn persistent_rate_limits_end_after_the_finish_window_and_keep_audio() {
        let dir = tempfile::tempdir().unwrap();
        wav::write(
            &dir.path().join(wav::done_name(0)),
            &vec![1000; wav::RATE as usize],
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let policy = Policy {
            finish_limit: Duration::from_millis(40),
            ..quick()
        };
        let worker = Transcriber::start(
            dir.path(),
            Arc::new(|_| Err("429 too many requests".into())),
            policy,
            false,
            tx,
        );
        let start = Instant::now();
        worker.wait();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(rx.try_iter().any(|e| matches!(e, Update::Failed { .. })));
        assert!(dir.path().join(wav::done_name(0)).exists());
        assert!(dir.path().join("seg-00000.err").exists());
    }

    #[test]
    fn a_missing_model_ends_after_the_finish_window_and_leaves_the_segment_waiting() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let (tx, rx) = mpsc::channel();
        let worker = Transcriber::start(
            dir.path(),
            Arc::new(|_| Err("Download a model in Settings → Speech models.".into())),
            Policy {
                finish_limit: Duration::from_millis(40),
                ..quick()
            },
            false,
            tx,
        );
        let (ended, end) = mpsc::channel();
        std::thread::spawn(move || {
            worker.wait();
            let _ = ended.send(());
        });
        assert!(
            end.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the wait for a model never ended"
        );
        assert!(rx.try_iter().any(|e| matches!(e, Update::Failed { .. })));
        assert!(dir.path().join(wav::done_name(0)).exists());
        assert!(!dir.path().join("seg-00000.err").exists());
        assert_eq!(waiting(dir.path()), [0]);
    }

    fn quick() -> Policy {
        Policy {
            first_wait: Duration::from_millis(5),
            most_wait: Duration::from_millis(20),
            attempts_after_stop: 3,
            idle: Duration::from_millis(5),
            workers: 3,
            finish_limit: Duration::from_secs(120),
            keep_parts: 0,
        }
    }

    fn loud(dir: &Path, index: u32) {
        wav::write(&dir.join(wav::done_name(index)), &vec![3000i16; 1600]).unwrap();
    }

    fn named(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().to_string()
    }

    #[test]
    fn audio_kept_for_speaker_labels_stays_beside_its_text_up_to_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..3 {
            loud(dir.path(), i);
        }
        let (tx, _rx) = mpsc::channel();
        Transcriber::start(
            dir.path(),
            Arc::new(|p: &Path| Ok(named(p))),
            Policy {
                keep_parts: 2,
                ..quick()
            },
            false,
            tx,
        )
        .wait();
        for i in 0..3 {
            assert!(dir.path().join(format!("seg-{i:05}.txt")).exists());
            assert_eq!(dir.path().join(wav::done_name(i)).exists(), i < 2);
        }
        assert!(
            waiting(dir.path()).is_empty(),
            "kept audio is never transcribed again"
        );
    }

    #[test]
    fn every_segment_gets_its_text_and_its_audio_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            loud(dir.path(), i);
        }
        wav::write(&dir.path().join(wav::done_name(5)), &vec![0i16; 1600]).unwrap();
        let (tx, rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(|p: &Path| Ok(named(p))),
            quick(),
            false,
            tx,
        );
        t.wait();
        for i in 0..5 {
            let text = std::fs::read_to_string(dir.path().join(format!("seg-{i:05}.txt"))).unwrap();
            assert_eq!(text, format!("seg-{i:05}.wav"));
            assert!(!dir.path().join(wav::done_name(i)).exists());
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00005.txt")).unwrap(),
            ""
        );
        let updates: Vec<Update> = rx.try_iter().collect();
        assert_eq!(
            updates
                .iter()
                .filter(|u| matches!(u, Update::Done { .. }))
                .count(),
            6
        );
        assert!(!updates.iter().any(|u| matches!(u, Update::Failed { .. })));
    }

    #[test]
    fn a_flaky_provider_is_retried_until_it_answers() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let calls = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&calls);
        let (tx, rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 2 {
                    Err("429 rate limited".to_string())
                } else {
                    Ok("finally".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "finally"
        );
        let updates: Vec<Update> = rx.try_iter().collect();
        assert!(matches!(updates.last(), Some(Update::Done { .. })));
        assert_eq!(
            updates
                .iter()
                .filter(|u| matches!(u, Update::Retrying { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn a_segment_that_never_works_is_marked_and_keeps_its_audio() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        loud(dir.path(), 1);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(|p: &Path| {
                if named(p).contains("00000") {
                    Err("offline".to_string())
                } else {
                    Ok("fine".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.err")).unwrap(),
            "offline"
        );
        assert!(dir.path().join("seg-00000.wav").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00001.txt")).unwrap(),
            "fine"
        );
    }

    #[test]
    fn while_recording_it_waits_for_new_segments_and_never_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("seg-00001.part.wav"), "recording").unwrap();
        loud(dir.path(), 0);
        let fails = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&fails);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 10 {
                    Err("503".to_string())
                } else {
                    Ok("late".to_string())
                }
            }),
            quick(),
            true,
            tx,
        );
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !dir.path().join("seg-00000.err").exists(),
            "gave up while still recording"
        );
        std::fs::remove_file(dir.path().join("seg-00001.part.wav")).unwrap();
        loud(dir.path(), 1);
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "late"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00001.txt")).unwrap(),
            "late"
        );
    }

    #[test]
    fn a_rate_limit_is_waited_out_rather_than_given_up_on() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let calls = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&calls);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 12 {
                    Err("groq: 429 Too Many Requests".to_string())
                } else {
                    Ok("patient".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert!(calls.load(Ordering::Relaxed) > quick().attempts_after_stop);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "patient"
        );
        assert!(rate_limited("429"));
        assert!(rate_limited("Rate limit reached for model"));
        assert!(!rate_limited("401 unauthorized"));
    }

    #[test]
    fn several_segments_are_transcribed_at_once_and_stay_in_order() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            loud(dir.path(), i);
        }
        let running = Arc::new(AtomicU32::new(0));
        let most = Arc::new(AtomicU32::new(0));
        let (r, m) = (Arc::clone(&running), Arc::clone(&most));
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |p: &Path| {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                m.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(60));
                r.fetch_sub(1, Ordering::SeqCst);
                Ok(named(p))
            }),
            Policy {
                workers: 4,
                ..quick()
            },
            false,
            tx,
        );
        t.wait();
        let most = most.load(Ordering::SeqCst);
        assert!(most >= 2, "segments were not worked on in parallel");
        assert!(most <= 4, "more workers ran than asked for");
        for i in 0..12 {
            let text = std::fs::read_to_string(dir.path().join(format!("seg-{i:05}.txt"))).unwrap();
            assert_eq!(text, format!("seg-{i:05}.wav"));
        }
    }

    #[test]
    fn waits_grow_and_are_capped() {
        let p = Policy::default();
        assert_eq!(p.wait(1), Duration::from_secs(5));
        assert_eq!(p.wait(2), Duration::from_secs(10));
        assert_eq!(p.wait(4), Duration::from_secs(40));
        assert_eq!(p.wait(30), Duration::from_secs(300));
    }
}

#[cfg(test)]
mod model_wait_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_stopped_segment_waits_for_a_model_then_retries_without_an_error_file() {
        let dir = tempfile::tempdir().unwrap();
        wav::write(&dir.path().join(wav::done_name(0)), &vec![12000; 1600]).unwrap();
        let tries = Arc::new(AtomicUsize::new(0));
        let count = tries.clone();
        let (send, updates) = std::sync::mpsc::channel();
        let worker = Transcriber::start(
            dir.path(),
            Arc::new(move |_| {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err("Download a model in Settings → Speech models. Your recorded audio is kept until one is ready.".into())
                } else {
                    Ok("the lecture".into())
                }
            }),
            Policy {
                first_wait: Duration::from_millis(1),
                most_wait: Duration::from_millis(2),
                attempts_after_stop: 1,
                idle: Duration::from_millis(1),
                workers: 1,
                finish_limit: Duration::from_millis(50),
                keep_parts: 0,
            },
            false,
            send,
        );
        worker.wait();
        assert!(updates.try_iter().any(|u| matches!(u, Update::Done { .. })));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "the lecture"
        );
        assert!(!dir.path().join("seg-00000.err").exists());
    }
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    #[test]
    fn saving_a_transcript_does_not_hold_the_attempts_mutex() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BEFORE_WRITE.with(|hook| hook.borrow_mut().take());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        wav::write(&dir.path().join(wav::done_name(0)), &vec![12000; 1600]).unwrap();
        let work = Arc::new(Work::default());
        let observed = work.clone();
        BEFORE_WRITE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                assert!(
                    observed.attempts.try_lock().is_ok(),
                    "disk writes must happen outside the attempts mutex"
                );
            }))
        });
        let _reset = Reset;
        let (send, updates) = std::sync::mpsc::channel();
        let transcribe: TranscribeFn = Arc::new(|_| Ok("speech".into()));
        settle(
            dir.path(),
            0,
            &transcribe,
            Policy::default(),
            &AtomicBool::new(false),
            &send,
            &work,
        );
        assert!(matches!(updates.recv().unwrap(), Update::Done { .. }));
    }
}
