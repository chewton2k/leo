//! The worker thread.
//!
//! The event loop must never block on the network, a subprocess, or the
//! microphone, so anything slow runs here and reports back over a channel. The
//! worker never touches the `Store`: it emits a final transcript and the App
//! saves, which keeps all persistence on one thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::ai::live;
use crate::listen::Recorder;

/// Token budget for one condense pass. The answer is only 2-4 bullets, but the
/// budget has to cover a reasoning model's hidden thinking too — too small and
/// free models on OpenRouter burn the whole allowance before emitting any
/// content.
const CONDENSE_MAX_TOKENS: u32 = 1200;
/// How often the worker wakes to check the clock and the stop flag.
const POLL: Duration = Duration::from_millis(250);

/// Progress from a background job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    Started { label: String },
    Progress { label: String },
    /// The full raw transcript so far.
    Transcript(String),
    /// The full condensed bullet stream so far.
    LiveNote(String),
    /// A provider degraded mid-job; shown once in the status line.
    ProviderFallback { from: String, to: String },
    /// The job finished and produced this transcript for the App to save.
    Finished { transcript: String },
    Failed(String),
}

/// A running background job.
pub struct Job {
    rx: Receiver<TaskEvent>,
    stop: Arc<AtomicBool>,
    done: bool,
}

impl Job {
    /// Ask the job to wind down. It still reports a final result, so a stop is
    /// not a cancel: audio already recorded is transcribed and saved.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// True once a terminal event has been observed.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Take everything the worker has sent since the last call. Never blocks.
    pub fn drain(&mut self) -> Vec<TaskEvent> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if matches!(event, TaskEvent::Finished { .. } | TaskEvent::Failed(_)) {
                        self.done = true;
                    }
                    out.push(event);
                }
                Err(TryRecvError::Empty) => break,
                // The worker thread ended without a terminal event.
                Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
        out
    }
}

/// Read a WAV's duration in whole seconds via sox.
fn wav_secs(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("sox")
        .args(["--i", "-D", path.to_str()?])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .map(|d| d as u64)
        .filter(|&d| d > 0)
}

/// Copy the growing recording and repair the copy's header, so a slice can be
/// cut from it. The live file's DataSize is still zero — `rec` only writes it on
/// exit — and `sox trim` on such a file yields nothing.
fn snapshot(source: &Path) -> Option<PathBuf> {
    let dest = std::env::temp_dir().join(format!("leo-live-snapshot-{}.wav", std::process::id()));
    std::fs::copy(source, &dest).ok()?;
    crate::listen::repair_wav_header(&dest);
    Some(dest)
}

/// Cut one slice out of a snapshot for transcription.
fn cut(source: &Path, slice: live::Slice) -> Option<PathBuf> {
    let dest = std::env::temp_dir().join(format!("leo-live-slice-{}.wav", std::process::id()));
    let ok = std::process::Command::new("sox")
        .arg(source)
        .arg(&dest)
        .arg("trim")
        .arg(slice.start_secs.to_string())
        .arg(slice.duration_secs.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok && dest.exists() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) > 100 {
        Some(dest)
    } else {
        let _ = std::fs::remove_file(&dest);
        None
    }
}

/// State the two loops share across iterations.
struct Live {
    transcript: String,
    condensed: String,
    /// Where the rolling loop has transcribed up to, in seconds.
    cursor: u64,
    /// Raw words not yet condensed.
    pending_words: usize,
    last_condense: Instant,
}

/// Start recording with rolling transcription and a condense loop.
pub fn start_listen(screen: bool) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);

    thread::spawn(move || {
        let recorder = match Recorder::start(screen) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
                return;
            }
        };
        let _ = tx.send(TaskEvent::Started { label: "Recording".to_string() });

        let mut state = Live {
            transcript: String::new(),
            condensed: String::new(),
            cursor: 0,
            pending_words: 0,
            last_condense: Instant::now(),
        };
        let mut last_roll = Instant::now();

        while !worker_stop.load(Ordering::Relaxed) {
            thread::sleep(POLL);

            let secs = recorder.elapsed().as_secs();
            let _ = tx.send(TaskEvent::Progress {
                label: format!("Recording {:02}:{:02}", secs / 60, secs % 60),
            });

            if last_roll.elapsed() < live::ROLL_INTERVAL {
                continue;
            }
            last_roll = Instant::now();
            roll_once(recorder.path(), &mut state, &tx);
            condense_if_due(&mut state, &tx);
        }

        // Stopping is not cancelling: finish the recording and transcribe it
        // whole. The rolling transcript is lower quality by construction —
        // slices are stitched across boundaries — so the saved note uses a
        // single pass over the finished file, matching non-live behavior. If
        // that fails, fall back to the rolling text rather than losing the
        // recording entirely.
        let _ = tx.send(TaskEvent::Progress { label: "Transcribing".to_string() });

        let final_transcript = match recorder.stop() {
            Ok(path) => {
                let result = crate::ai::transcribe_outcome(&path);
                let _ = std::fs::remove_file(&path);
                match result {
                    Ok(outcome) => {
                        for f in &outcome.fallbacks {
                            let _ = tx.send(TaskEvent::ProviderFallback {
                                from: f.from.clone(),
                                to: f.to.clone(),
                            });
                        }
                        outcome.value
                    }
                    Err(e) if !state.transcript.trim().is_empty() => {
                        let _ = tx.send(TaskEvent::ProviderFallback {
                            from: format!("final transcription failed ({e})"),
                            to: "the live transcript".to_string(),
                        });
                        state.transcript.clone()
                    }
                    Err(e) => {
                        let _ = tx.send(TaskEvent::Failed(e.to_string()));
                        return;
                    }
                }
            }
            Err(e) => {
                // No audio at all: only worth reporting if the live loop never
                // heard anything either.
                if state.transcript.trim().is_empty() {
                    let _ = tx.send(TaskEvent::Failed(e.to_string()));
                    return;
                }
                state.transcript.clone()
            }
        };

        let _ = tx.send(TaskEvent::Finished { transcript: final_transcript });
    });

    Job { rx, stop, done: false }
}

/// One rolling pass: cut the new tail, transcribe it, stitch it on.
fn roll_once(source: &Path, state: &mut Live, tx: &mpsc::Sender<TaskEvent>) {
    let Some(snap) = snapshot(source) else {
        return;
    };
    let recorded = wav_secs(&snap).unwrap_or(0);

    let Some(slice) = live::next_slice(state.cursor, recorded) else {
        let _ = std::fs::remove_file(&snap);
        return;
    };
    let Some(slice_path) = cut(&snap, slice) else {
        let _ = std::fs::remove_file(&snap);
        return;
    };
    let _ = std::fs::remove_file(&snap);

    let result = crate::ai::transcribe_outcome(&slice_path);
    let _ = std::fs::remove_file(&slice_path);

    match result {
        Ok(outcome) => {
            for f in &outcome.fallbacks {
                let _ = tx.send(TaskEvent::ProviderFallback {
                    from: f.from.clone(),
                    to: f.to.clone(),
                });
            }
            let before = state.transcript.split_whitespace().count();
            state.transcript = live::stitch(&state.transcript, &outcome.value);
            let after = state.transcript.split_whitespace().count();
            state.pending_words += after.saturating_sub(before);
            state.cursor = recorded;
            let _ = tx.send(TaskEvent::Transcript(state.transcript.clone()));
        }
        // A failed slice is not fatal: the cursor stays put so the next pass
        // covers the same audio again.
        Err(e) => {
            let _ = tx.send(TaskEvent::Progress {
                label: format!("Transcription retrying ({e})"),
            });
        }
    }
}

/// Condense the un-summarized tail into bullets, when enough has piled up.
fn condense_if_due(state: &mut Live, tx: &mpsc::Sender<TaskEvent>) {
    if !live::should_condense(state.pending_words, state.last_condense.elapsed()) {
        return;
    }

    // Only the words not yet summarized go into the prompt.
    let words: Vec<&str> = state.transcript.split_whitespace().collect();
    let start = words.len().saturating_sub(state.pending_words);
    let new_material = words[start..].join(" ");
    if new_material.trim().is_empty() {
        state.pending_words = 0;
        return;
    }

    let prompt = live::condense_prompt(&new_material, &live::context_tail(&state.condensed));
    match crate::ai::chat_outcome(prompt, CONDENSE_MAX_TOKENS) {
        Ok(outcome) => {
            for f in &outcome.fallbacks {
                let _ = tx.send(TaskEvent::ProviderFallback {
                    from: f.from.clone(),
                    to: f.to.clone(),
                });
            }
            let bullets = live::clean_bullets(&outcome.value);
            if !bullets.is_empty() {
                if !state.condensed.is_empty() {
                    state.condensed.push('\n');
                }
                state.condensed.push_str(&bullets.join("\n"));
                let _ = tx.send(TaskEvent::LiveNote(state.condensed.clone()));
            }
            // Consumed either way: a model that returned prose will not do
            // better on a retry with the same input, and the raw text is
            // still kept for the saved note.
            state.pending_words = 0;
            state.last_condense = Instant::now();
        }
        Err(e) => {
            // Keep the words pending and try again next interval.
            state.last_condense = Instant::now();
            let _ = tx.send(TaskEvent::Progress {
                label: format!("Condensing retrying ({e})"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job whose worker never starts still drains cleanly and reports done,
    /// rather than blocking the event loop forever.
    #[test]
    fn a_dropped_worker_marks_the_job_done() {
        let (tx, rx) = mpsc::channel::<TaskEvent>();
        let mut job = Job { rx, stop: Arc::new(AtomicBool::new(false)), done: false };
        drop(tx);
        assert!(job.drain().is_empty());
        assert!(job.is_done());
    }

    #[test]
    fn draining_returns_events_in_order_and_notices_the_terminal_one() {
        let (tx, rx) = mpsc::channel();
        let mut job = Job { rx, stop: Arc::new(AtomicBool::new(false)), done: false };

        tx.send(TaskEvent::Started { label: "Recording".to_string() }).unwrap();
        tx.send(TaskEvent::Transcript("hello".to_string())).unwrap();
        assert_eq!(job.drain().len(), 2);
        assert!(!job.is_done());

        tx.send(TaskEvent::Finished { transcript: "hello".to_string() }).unwrap();
        let events = job.drain();
        assert_eq!(events.len(), 1);
        assert!(job.is_done());
    }

    #[test]
    fn requesting_stop_is_visible_to_the_worker() {
        let (_tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let job = Job { rx, stop: Arc::clone(&stop), done: false };
        assert!(!job.stop_requested());
        job.request_stop();
        assert!(stop.load(Ordering::Relaxed));
        assert!(job.stop_requested());
    }

    #[test]
    fn a_failure_event_also_ends_the_job() {
        let (tx, rx) = mpsc::channel();
        let mut job = Job { rx, stop: Arc::new(AtomicBool::new(false)), done: false };
        tx.send(TaskEvent::Failed("no microphone".to_string())).unwrap();
        job.drain();
        assert!(job.is_done());
    }
}
