use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ai::chat::Jotted;
use crate::ai::live;
use crate::ai::long::{draft_key, draft_prompt, Drafted};
use crate::session::capture::{Capture, Source};
use crate::session::transcriber::{Policy, TranscribeFn, Transcriber, Update};
use crate::session::{self, wav, Manifest, Point, Session};

const POLL: Duration = Duration::from_millis(250);

pub const SILENT_RECORDING: &str = "No sound was recorded. macOS may not be letting \
     leo use the microphone: System Settings > Privacy & Security > \
     Microphone, then quit and reopen leo.";

pub const SILENT_COMPUTER_AUDIO: &str = "No sound was recorded from the computer. Check that \
     something was playing, and on a Mac that leo may record computer audio in System \
     Settings > Privacy & Security, then try again.";

const LIVE_MOST_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started(String),
    Progress {
        label: String,
        steps: Option<(usize, usize)>,
    },
    Clock {
        secs: u64,
        paused: bool,
        level: f32,
    },
    Transcript(String),
    Fallback {
        from: String,
        to: String,
    },
    Warning(String),
    Failed(String),
    Finished {
        transcript: String,
        session: PathBuf,
    },
}

#[derive(Clone, Default)]
pub struct Controls {
    pub finish_now: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<Jotted>>>,
    pub saved_points: Arc<AtomicUsize>,
    pub wants: Arc<Mutex<Option<String>>>,
    pub preview: Option<Arc<AtomicBool>>,
}

impl Controls {
    fn preview_enabled(&self) -> bool {
        self.preview
            .as_ref()
            .is_none_or(|visible| visible.load(Ordering::Acquire))
    }
}

pub struct Request {
    pub id: Option<String>,
    pub profile: Option<leo_core::recording::Profile>,
    pub title: Option<String>,
    pub append_to: Option<String>,
    pub dir: String,
    pub screen: bool,
    pub input: Source,
    pub draft: Option<DraftFn>,
    pub turns: bool,
}

pub fn transcribe_segment() -> TranscribeFn {
    Arc::new(|path: &Path| {
        let session = Session::open(path.parent().unwrap_or(Path::new("."))).ok();
        let start = Instant::now();
        let turns = session
            .as_ref()
            .filter(|s| s.manifest.turns)
            .zip(path.file_name().and_then(|name| name.to_str()))
            .and_then(|(s, name)| match session::index_of(name) {
                Some((index, "wav")) => Some(s.dir.join(session::turns_name(index))),
                _ => None,
            });
        let outcome = match turns {
            Some(file) => crate::ai::transcribe_turns(path)
                .and_then(|timed| {
                    leo_core::paths::write_atomic(&file, &serde_json::to_string(&timed)?)?;
                    let texts: Vec<&str> = timed.iter().map(|(_, text)| text.as_str()).collect();
                    Ok(crate::ai::chain::ChainOutcome {
                        value: crate::speech::chunking::join_transcripts(&texts),
                        provider: "local speech with times".into(),
                        fallbacks: Vec::new(),
                        spent: None,
                    })
                })
                .map_err(|e| e.to_string()),
            None => crate::ai::transcribe_outcome(path).map_err(|e| e.to_string()),
        };
        if let Some(session) = session {
            let detail = match &outcome {
                Ok(o) => format!(
                    "{} via {}; {:.1}s; {} fallback(s)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    o.provider,
                    start.elapsed().as_secs_f64(),
                    o.fallbacks.len()
                ),
                Err(_) => format!(
                    "{} failed after {:.1}s",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    start.elapsed().as_secs_f64()
                ),
            };
            session.trace("transcription", &detail);
        }
        outcome.map(|o| o.value)
    })
}

fn caught<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".to_string())
    })
}

pub type DraftFn = Arc<
    dyn Fn(
            crate::ai::chat::Prompt,
            u32,
            &dyn Fn() -> bool,
        ) -> anyhow::Result<crate::ai::chain::ChainOutcome<String>>
        + Send
        + Sync,
>;

const DRAFTS: &str = "drafts.json";
const DRAFT_EVERY: Duration = Duration::from_secs(2);
const MOST_DRAFT_FAILURES: usize = 2;

pub struct Drafter {
    chat: DraftFn,
    budget: crate::ai::budget::Budget,
    done: Option<Drafted>,
    profile: leo_core::recording::Profile,
    running: Option<std::thread::JoinHandle<Option<Drafted>>>,
    cancel: Arc<AtomicBool>,
    failures: usize,
    ended: bool,
    read: std::collections::BTreeMap<u32, session::Segment>,
}

impl Drafter {
    pub fn new(chat: DraftFn, budget: crate::ai::budget::Budget) -> Drafter {
        Drafter {
            chat,
            budget,
            done: None,
            profile: Default::default(),
            running: None,
            cancel: Default::default(),
            failures: 0,
            ended: false,
            read: Default::default(),
        }
    }

    pub fn tick(&mut self, session: &Session, backlog: usize) {
        if self.running.as_ref().is_some_and(|run| run.is_finished()) {
            match self.running.take().map(|run| run.join()) {
                Some(Ok(Some(drafted))) => self.done = Some(drafted),
                _ => {
                    self.failures += 1;
                    self.ended = self.failures >= MOST_DRAFT_FAILURES;
                }
            }
        }
        if self.ended || self.running.is_some() || backlog > 0 {
            return;
        }
        if session.manifest.profile != self.profile {
            self.done = None;
            self.profile = session.manifest.profile.clone();
        }
        let upto = self.done.as_ref().map_or(0, |d| d.upto);
        for index in session.settled().0 {
            if self.read.contains_key(&index) {
                continue;
            }
            if let Some(segment) = session.segment(index).filter(|s| {
                matches!(
                    s.state,
                    session::SegmentState::Done(_) | session::SegmentState::Failed(_)
                )
            }) {
                self.read.insert(index, segment);
            }
        }
        let segments: Vec<session::Segment> =
            self.read.range(upto..).map(|(_, s)| s.clone()).collect();
        let run: Vec<session::Segment> = segments
            .iter()
            .enumerate()
            .take_while(|(i, s)| {
                s.index == upto + *i as u32 && matches!(s.state, session::SegmentState::Done(_))
            })
            .map(|(_, s)| s.clone())
            .collect();
        if run.is_empty() {
            self.ended = segments.first().is_some_and(|s| {
                s.index == upto && matches!(s.state, session::SegmentState::Failed(_))
            });
            if self.ended {
                session.trace(
                    "draft-ended",
                    "a part of the recording could not be transcribed",
                );
            }
            return;
        }
        let fresh = session::assemble(&run, session.manifest.segment_secs).parts;
        let read: Vec<session::Segment> = self.read.values().cloned().collect();
        let all = session::assemble(&read, session.manifest.segment_secs).parts;
        let points = session.jotted();
        let profile = self.profile.clone();
        let dir = session.dir.clone();
        let (start, end) = (fresh[0].start_secs, fresh[fresh.len() - 1].end_secs);
        let typed = points.iter().any(|p| p.at_secs >= start && p.at_secs < end);
        if !typed && fresh.iter().all(|p| p.text.trim().is_empty()) {
            if let Some(done) = self.done.take() {
                let upto = upto + run.len() as u32;
                let next = Drafted {
                    upto,
                    key: draft_key(&all, &points, &profile, upto),
                    ..done
                };
                keep_draft(&dir, &next);
                self.done = Some(next);
            }
            return;
        }
        let (notes, commitments) = self.done.as_ref().map_or_else(Default::default, |d| {
            (d.notes.clone(), d.commitments.clone())
        });
        let room = crate::ai::long::room_after_profile(self.budget, &profile);
        let Some((prompt, n)) = draft_prompt(&fresh, &notes, &points, room) else {
            self.ended = true;
            session.trace(
                "draft-ended",
                "the next stretch is too long for the writing AI",
            );
            return;
        };
        let Ok(lease) = leo_core::activity::global().begin() else {
            return;
        };
        let upto = fresh[n - 1].index + 1;
        let key = draft_key(&all, &points, &profile, upto);
        let chat = self.chat.clone();
        let cancel = self.cancel.clone();
        let prompt = crate::ai::long::with_profile(prompt, &profile);
        self.running = Some(std::thread::spawn(move || {
            let _lease = lease;
            let stopped = || cancel.load(Ordering::Relaxed);
            let reply = match caught(|| chat(prompt, crate::ai::STRUCTURE_MAX_TOKENS, &stopped)) {
                Ok(Ok(outcome)) => {
                    draft_trace(
                        &dir,
                        "draft-provider",
                        &format!(
                            "{}; {} fallback(s)",
                            outcome.provider,
                            outcome.fallbacks.len()
                        ),
                    );
                    outcome.value
                }
                Ok(Err(e)) => {
                    draft_trace(&dir, "draft-failed", &crate::ai::writing_error(&e));
                    return None;
                }
                Err(panic) => {
                    draft_trace(&dir, "draft-failed", &panic);
                    return None;
                }
            };
            let (more, found) =
                crate::ai::long::split_drafted(&crate::ai::chat::clean_reply(&reply));
            if more.is_empty() && found.is_empty() {
                draft_trace(&dir, "draft-failed", "the AI returned an empty draft");
                return None;
            }
            let drafted = Drafted {
                upto,
                key,
                notes: crate::ai::long::joined(&notes, &more),
                commitments: crate::ai::long::joined(&commitments, &found).replace("\n\n", "\n"),
            };
            keep_draft(&dir, &drafted);
            Some(drafted)
        }));
    }

    pub fn finish(mut self, finish_now: &AtomicBool, emit: &dyn Fn(Event)) {
        let Some(run) = self.running.take() else {
            return;
        };
        emit(Event::Progress {
            label: "Writing the notes so far".to_string(),
            steps: None,
        });
        while !run.is_finished() {
            if finish_now.load(Ordering::Relaxed) {
                self.cancel.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(POLL);
        }
        let _ = run.join();
    }
}

fn draft_trace(dir: &Path, kind: &str, message: &str) {
    if let Ok(session) = Session::open(dir) {
        session.trace(kind, message);
    }
}

fn keep_draft(dir: &Path, drafted: &Drafted) {
    if !dir.join("session.json").exists() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(drafted) {
        let _ = leo_core::paths::write_atomic(&dir.join(DRAFTS), bytes);
    }
}

pub fn drafts_for(
    session: &Session,
    parts: &[session::Part],
    existing: Option<&str>,
) -> Option<Drafted> {
    if existing.is_some() {
        return None;
    }
    let drafted: Drafted =
        serde_json::from_slice(&std::fs::read(session.dir.join(DRAFTS)).ok()?).ok()?;
    let key = draft_key(
        parts,
        &session.jotted(),
        &session.manifest.profile,
        drafted.upto,
    );
    (drafted.key == key).then_some(drafted)
}

fn points_of(jotted: &[Jotted]) -> Vec<Point> {
    jotted
        .iter()
        .map(|p| Point {
            at_secs: p.at_secs,
            text: p.text.clone(),
        })
        .collect()
}

fn clock_label(word: &str, secs: u64) -> String {
    format!("{word} {}", crate::ai::chat::clock(secs))
}

pub struct Live {
    pace: Duration,
    interval: Duration,
    silent_slices: usize,
    warned_silent: bool,
    finals: std::collections::BTreeMap<u32, String>,
    settled: u32,
    base: u64,
    settled_at: u64,
    committed: String,
    tail: String,
    shown: String,
}

impl Live {
    pub fn new(pace: Duration) -> Live {
        Live {
            pace,
            interval: pace,
            silent_slices: 0,
            warned_silent: false,
            finals: Default::default(),
            settled: 0,
            base: 0,
            settled_at: 0,
            committed: String::new(),
            tail: String::new(),
            shown: String::new(),
        }
    }

    fn display(&self) -> String {
        let mut out = String::new();
        for text in self.finals.values() {
            out = live::stitch(&out, text);
        }
        live::stitch(&out, &live::stitch(&self.committed, &self.tail))
    }

    fn apply(&mut self, update: Update, emit: &dyn Fn(Event), segment_samples: u64) {
        match update {
            Update::Done { index, text } => {
                self.finals.insert(index, text);
            }
            Update::Failed { index, error } => {
                self.finals.insert(index, String::new());
                emit(Event::Warning(format!(
                    "part {} of the recording could not be transcribed ({error}); its audio is kept",
                    index + 1
                )));
            }
            Update::Retrying {
                attempt,
                error,
                wait,
                ..
            } => {
                if attempt >= 2 {
                    emit(Event::Warning(format!(
                        "transcription is retrying ({error}); trying again in {}s, and nothing is lost",
                        wait.as_secs()
                    )));
                }
            }
        }
        while self.finals.contains_key(&self.settled) {
            self.settled += 1;
        }
        let base = self.settled as u64 * segment_samples;
        if base > self.base {
            self.base = base;
            self.committed.clear();
            self.tail.clear();
            self.settled_at = self.settled_at.max(base);
        }
    }

    fn show(&mut self, emit: &dyn Fn(Event)) {
        let shown = self.display();
        if shown != self.shown {
            self.shown = shown.clone();
            emit(Event::Transcript(shown));
        }
    }
}

fn live_model_ready() -> bool {
    !crate::speech::engine::engine().dictating()
        && crate::speech::library::library()
            .and_then(|l| l.selected())
            .is_ok_and(|m| m.live)
}

fn hear(samples: &[i16]) -> Option<String> {
    let selected = crate::speech::library::library().ok()?.selected().ok()?;
    let pcm = samples.iter().map(|s| *s as f32 / 32768.0).collect();
    let text = crate::speech::engine::engine()
        .transcribe(
            selected,
            pcm,
            crate::speech::engine::Priority::Live,
            Vec::new(),
        )
        .ok()?;
    Some(if live::is_silence_artifact(&text) {
        String::new()
    } else {
        text
    })
}

fn silent_for(session: &Session) -> &'static str {
    if session.manifest.screen {
        SILENT_COMPUTER_AUDIO
    } else {
        SILENT_RECORDING
    }
}

fn roll(capture: &Capture, state: &mut Live, screen: bool, emit: &dyn Fn(Event)) {
    let rate = wav::RATE as u64;
    let from = state.settled_at.max(state.base);
    let (start, samples) = capture.tail(from, (LIVE_MOST_SECS * rate) as usize);
    if start > state.settled_at {
        state.settled_at = start;
    }
    if (samples.len() as u64) < rate {
        return;
    }
    let end = start + samples.len() as u64;
    if live::is_silent(wav::peak(&samples)) {
        if samples.len() as u64 >= live::SETTLE_AFTER_SECS * rate {
            state.settled_at = end;
            state.tail.clear();
        }
        state.silent_slices += 1;
        if state.silent_slices >= 4 && !state.warned_silent {
            state.warned_silent = true;
            emit(if screen {
                Event::Fallback {
                    from: "no sound from the computer".to_string(),
                    to: "check that something is playing".to_string(),
                }
            } else {
                Event::Fallback {
                    from: "no sound from the microphone".to_string(),
                    to: "check System Settings > Privacy & Security > Microphone".to_string(),
                }
            });
        }
        return;
    }
    state.silent_slices = 0;
    let mut open = &samples[..];
    if samples.len() as u64 >= live::SETTLE_AFTER_SECS * rate {
        let cut = live::quietest(
            &samples,
            (live::SETTLE_EARLIEST_SECS * rate) as usize,
            samples.len() - (live::SETTLE_KEEP_SECS * rate) as usize,
            (rate / 10) as usize,
        );
        let Some(settled) = hear(&samples[..cut]) else {
            state.interval = live::backoff(state.interval);
            return;
        };
        state.committed = live::stitch(&state.committed, &settled);
        state.settled_at = start + cut as u64;
        state.tail.clear();
        open = &samples[cut..];
    }
    match hear(open) {
        Some(text) => {
            state.interval = state.pace;
            state.tail = text;
        }
        None => state.interval = live::backoff(state.interval),
    }
}

pub fn finish(
    mut session: Session,
    transcriber: Transcriber,
    updates: &Receiver<Update>,
    state: &mut Live,
    emit: &dyn Fn(Event),
    finish_now: &AtomicBool,
) {
    session.manifest.stopped = true;
    if let Err(e) = session.save() {
        session.trace(
            "save-failed",
            &format!("could not mark the recording stopped: {e}"),
        );
    }
    transcriber.recording_ended();
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    let (mut settled, total) = session.settled();
    let mut no_model = false;
    let mut take =
        |update: Update, state: &mut Live, settled: &mut std::collections::BTreeSet<u32>| {
            match &update {
                Update::Done { index, .. } => {
                    settled.insert(*index);
                }
                Update::Failed { index, error } => {
                    settled.insert(*index);
                    no_model |= session::transcriber::waiting_for_model(error);
                }
                _ => {}
            }
            state.apply(update, emit, segment_samples);
        };
    while !transcriber.is_finished() {
        if finish_now.load(Ordering::Relaxed) {
            transcriber.cancel();
        }
        for update in updates.try_iter() {
            take(update, state, &mut settled);
        }
        emit(Event::Progress {
            label: "Transcribing the recording".to_string(),
            steps: (total > 1).then_some((settled.len().min(total), total)),
        });
        state.show(emit);
        std::thread::sleep(POLL);
    }
    transcriber.wait();
    if finish_now.load(Ordering::Relaxed) {
        for index in session::transcriber::waiting(&session.dir) {
            let path = session.dir.join(format!("seg-{index:05}.err"));
            let _ = std::fs::write(
                path,
                "Finished with available transcript; audio kept for retry",
            );
        }
        session.trace(
            "finish-available",
            "Queued transcription skipped by the user; audio kept for retry",
        );
    }
    for update in updates.try_iter() {
        take(update, state, &mut settled);
    }
    let assembled = session.assemble();
    if assembled.pending > 0 {
        emit(Event::Failed(if no_model {
            "No speech model was ready, so this recording is not transcribed yet. Its audio is kept: download a model in Settings → Speech models, then recover the recording.".into()
        } else {
            "Some transcript files could not be saved. Audio is kept; free disk space and recover the session.".into()
        }));
        return;
    }
    if assembled.is_silent() && session.points().is_empty() {
        let _ = std::fs::remove_dir_all(&session.dir);
        emit(Event::Failed(silent_for(&session).to_string()));
        return;
    }
    label_speakers(
        &session,
        emit,
        finish_now,
        &|parts| {
            let shown = std::cell::Cell::new(None);
            crate::speech::engine::engine()
                .diarize(parts, finish_now, &|done, total| {
                    let steps = Some(((done >> 20) as usize, (total >> 20) as usize));
                    if shown.replace(steps) != steps {
                        emit(if done >= total {
                            Event::Progress {
                                label: "Telling speakers apart".to_string(),
                                steps: None,
                            }
                        } else {
                            Event::Progress {
                                label: "Downloading the speaker model (MiB)".to_string(),
                                steps,
                            }
                        })
                    }
                })
                .map_err(|e| e.to_string())
        },
        &|path| transcribe_segment()(path),
    );
    let assembled = session.assemble();
    emit(Event::Finished {
        transcript: assembled.text(),
        session: session.dir.clone(),
    });
}

fn policy_for(session: &Session) -> Policy {
    Policy {
        keep_parts: kept_parts(session),
        ..Policy::default()
    }
}

fn kept_parts(session: &Session) -> u32 {
    if session.manifest.profile.speakers {
        (session::speakers::MOST_SECS / session.manifest.segment_secs.max(1)) as u32 + 1
    } else {
        0
    }
}

fn label_speakers(
    session: &Session,
    emit: &dyn Fn(Event),
    finish_now: &AtomicBool,
    diarize: session::speakers::Diarize,
    transcribe: session::speakers::Transcribe,
) {
    if !session.manifest.profile.speakers || session.dir.join(session::speakers::FILE).exists() {
        return;
    }
    let skipped = |problem: &str| {
        session.trace("speakers-skipped", problem);
        emit(Event::Warning(format!(
            "Speakers are not labelled: {}. The transcript is kept as it is.",
            problem.trim_end_matches('.')
        )));
    };
    if !session.assemble().failed.is_empty() {
        return skipped("part of the recording could not be transcribed");
    }
    emit(Event::Progress {
        label: "Telling speakers apart".to_string(),
        steps: None,
    });
    let parts = session.segments().len() as u32;
    let started = Instant::now();
    let labelled = session::speakers::label(
        &session.dir,
        parts,
        session.manifest.segment_secs * 1000,
        diarize,
        transcribe,
        finish_now,
        &|done, total| {
            emit(Event::Progress {
                label: "Telling speakers apart".to_string(),
                steps: Some((done, total)),
            })
        },
    );
    let spoken = match labelled {
        Ok(spoken) => spoken,
        Err(problem) if crate::speech::engine::engine().stopping() => {
            return skipped(&format!("{problem}; recover the recording to try again"));
        }
        Err(problem) => {
            skipped(&problem);
            Vec::new()
        }
    };
    session.trace(
        "speakers",
        &format!(
            "{} labelled passages in {:.1}s",
            spoken.len(),
            started.elapsed().as_secs_f64()
        ),
    );
    if let Err(e) = session::speakers::write(&session.dir, &spoken) {
        skipped(&format!("the labels could not be saved ({e})"));
    }
    for index in 0..parts {
        if session.dir.join(format!("seg-{index:05}.txt")).exists() {
            let _ = std::fs::remove_file(session.dir.join(wav::done_name(index)));
        }
    }
}

pub fn resume(dir: &Path, emit: &dyn Fn(Event)) {
    resume_with_controls(dir, &Controls::default(), emit)
}

pub fn resume_with_controls(dir: &Path, controls: &Controls, emit: &dyn Fn(Event)) {
    let _model = crate::speech::engine::engine().pin();
    let session = match Session::open(dir) {
        Ok(s) => s,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _lock = match session.lock() {
        Ok(l) => l,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    emit(Event::Started(
        "Finishing an interrupted recording".to_string(),
    ));
    if let Err(e) = session.recover_parts() {
        emit(Event::Warning(format!(
            "some audio could not be recovered ({e})"
        )));
    }
    crate::ai::warm_credentials();
    let (updates_tx, updates) = mpsc::channel();
    let transcriber = Transcriber::start(
        &session.dir,
        transcribe_segment(),
        policy_for(&session),
        false,
        updates_tx,
    );
    let mut state = Live::new(live::ROLL_INTERVAL);
    finish(
        session,
        transcriber,
        &updates,
        &mut state,
        emit,
        &controls.finish_now,
    );
}

fn take_wants(session: &mut Session, controls: &Controls) -> bool {
    let Some(wants) = controls.wants.lock().ok().and_then(|w| w.clone()) else {
        return false;
    };
    if session.manifest.profile.wants == wants {
        return false;
    }
    session.manifest.profile.wants = wants;
    true
}

fn lock_or_remove(session: &Session) -> anyhow::Result<session::Lock> {
    session.lock().inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&session.dir);
    })
}

pub fn record(request: Request, controls: &Controls, emit: &dyn Fn(Event)) {
    let _activity = match leo_core::activity::global().begin() {
        Ok(lease) => lease,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _audio = match crate::dictation::audio_gate::gate().recording() {
        Ok(guard) => guard,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _awake = super::awake::Awake::recording();
    let _model = crate::speech::engine::engine().pin();
    let Request {
        id,
        profile,
        title,
        append_to,
        dir,
        screen,
        input,
        draft,
        turns,
    } = request;
    let labelled = profile.as_ref().is_some_and(|p| p.speakers);
    let drafting = draft.is_some() && append_to.is_none() && !labelled;
    let mut drafter = draft
        .filter(|_| append_to.is_none() && !labelled)
        .map(|chat| Drafter::new(chat, crate::ai::writing_budget()));
    let started = session::root().and_then(|root| {
        let mut manifest = Manifest::new(title, append_to, &dir, screen);
        if let Some(id) = id {
            manifest.id = id;
        }
        if let Some(profile) = profile {
            manifest.profile = profile;
        }
        manifest.turns = turns;
        let session = Session::create(&root, manifest)?;
        session.trace("capture", "Recording started");
        session.trace("drafting", if drafting { "on" } else { "off" });
        let lock = lock_or_remove(&session)?;
        let opened = Capture::start(&session.dir, 0, session.manifest.segment_secs, input);
        match opened {
            Ok(capture) => Ok((session, lock, capture)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&session.dir);
                Err(e)
            }
        }
    });
    let (mut session, _lock, capture) = match started {
        Ok(parts) => parts,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    emit(Event::Started("Recording".to_string()));
    crate::ai::warm_credentials();

    let (updates_tx, updates) = mpsc::channel();
    let transcriber = Transcriber::start(
        &session.dir,
        transcribe_segment(),
        policy_for(&session),
        true,
        updates_tx,
    );
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    let mut state = Live::new(live::ROLL_INTERVAL);
    let mut last_roll = Instant::now() - live::ROLL_INTERVAL;
    let mut last_draft = Instant::now();
    let mut save_failed = false;
    let mut heard_up_to = 0;
    let mut lost_up_to = 0;

    while !controls.stop.load(Ordering::Relaxed) && !capture.ended() {
        std::thread::sleep(POLL);
        capture.set_paused(controls.pause.load(Ordering::Relaxed));

        match save_points(&session, controls) {
            Ok(()) => save_failed = false,
            Err(e) if !std::mem::replace(&mut save_failed, true) => {
                session.trace("save-failed", &format!("could not save typed points: {e}"));
            }
            Err(_) => {}
        }
        if take_wants(&mut session, controls) {
            let _ = session.save();
        }
        for update in updates.try_iter() {
            state.apply(update, emit, segment_samples);
        }

        let lost = capture.dropped_samples();
        if lost > lost_up_to {
            let warning = format!(
                "An audio gap of {:.2}s occurred near {}",
                (lost - lost_up_to) as f64 / wav::RATE as f64,
                crate::ai::chat::clock(capture.recorded_secs() as u64)
            );
            session.trace("audio-gap", &warning);
            emit(Event::Warning(warning));
            lost_up_to = lost;
        }
        let secs = capture.recorded_secs() as u64;
        let backlog = transcriber.backlog();
        if let Some(drafter) = drafter
            .as_mut()
            .filter(|_| last_draft.elapsed() >= DRAFT_EVERY)
        {
            last_draft = Instant::now();
            drafter.tick(&session, backlog);
        }
        let word = if capture.paused() {
            "Paused"
        } else {
            "Recording"
        };
        let mut label = clock_label(word, secs);
        if backlog > 1 {
            label.push_str(&format!(" · {backlog} parts waiting to be transcribed"));
        }
        let recorded = capture.recorded_samples();
        let level = if capture.paused() {
            0.0
        } else {
            capture.level_since(heard_up_to)
        };
        heard_up_to = recorded;
        emit(Event::Clock {
            secs,
            paused: capture.paused(),
            level,
        });
        emit(Event::Progress { label, steps: None });

        if !capture.paused()
            && controls.preview_enabled()
            && backlog == 0
            && last_roll.elapsed() >= state.interval
            && live_model_ready()
        {
            last_roll = Instant::now();
            if let Err(message) =
                caught(|| roll(&capture, &mut state, session.manifest.screen, emit))
            {
                state.interval = live::backoff(state.interval);
                leo_core::diag::warn(format!(
                    "live transcription hit a problem ({message}); the recording continues"
                ));
            }
        }
        state.show(emit);
    }

    let lost = capture.dropped_samples();
    if lost > lost_up_to {
        let warning = format!(
            "An audio gap of {:.2}s occurred at the end of capture",
            (lost - lost_up_to) as f64 / wav::RATE as f64
        );
        session.trace("audio-gap", &warning);
        emit(Event::Warning(warning));
    }
    if let Some(problem) = capture.problem() {
        emit(Event::Warning(format!(
            "{problem} What was recorded is being saved."
        )));
    }
    final_points(&mut session, controls, emit);
    match capture.stop_with_stats() {
        Ok((samples, _)) => {
            session.manifest.duration_secs = Some(samples.div_ceil(wav::RATE as u64))
        }
        Err(e) => emit(Event::Warning(e.to_string())),
    }
    finish(
        session,
        transcriber,
        &updates,
        &mut state,
        emit,
        &controls.finish_now,
    );
    if let Some(drafter) = drafter {
        drafter.finish(&controls.finish_now, emit);
    }
}

fn final_points(session: &mut Session, controls: &Controls, emit: &dyn Fn(Event)) {
    emit(Event::Progress {
        label: "Transcribing the recording".to_string(),
        steps: None,
    });
    if let Err(e) = save_points(session, controls) {
        session.trace("save-failed", &format!("could not save typed points: {e}"));
        emit(Event::Warning(
            "Some typed points could not be saved; check free disk space.".into(),
        ));
    }
    take_wants(session, controls);
}

fn save_points(session: &Session, controls: &Controls) -> anyhow::Result<()> {
    let saved = controls.saved_points.load(Ordering::Acquire);
    let fresh = match controls.points.lock() {
        Ok(points) => points_of(points.get(saved..).unwrap_or_default()),
        Err(_) => return Ok(()),
    };
    if fresh.is_empty() {
        return Ok(());
    }
    let added = session.add_points(&fresh)?;
    controls
        .saved_points
        .store(saved + added as usize, Ordering::Release);
    Ok(())
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Written {
    pub title: String,
    pub body: String,
    pub problems: Vec<String>,
    #[serde(default)]
    pub writer_failed: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Prepared {
    key: String,
    written: Written,
}

fn reusable(bytes: &[u8], key: &str) -> Option<Written> {
    serde_json::from_slice::<Prepared>(bytes)
        .ok()
        .filter(|prepared| prepared.key == key && !prepared.written.writer_failed)
        .map(|prepared| prepared.written)
}

pub fn write_up(
    dir: &Path,
    existing: Option<&str>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> anyhow::Result<Written> {
    let session = Session::open(dir)?;
    session.trace(
        "generation",
        "Writing notes from transcript and typed points",
    );
    let assembled = session.assemble();
    let points = session.jotted();
    use sha2::{Digest, Sha256};
    let key = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        Sha256::digest(serde_json::to_vec(&(
            assembled.text(),
            &assembled.failed,
            &session.points(),
            &session.manifest.profile,
            existing,
        ))?),
    );
    let cached = dir.join("prepared.json");
    if let Some(written) = std::fs::read(&cached)
        .ok()
        .and_then(|bytes| reusable(&bytes, &key))
    {
        return Ok(written);
    }

    let fallback = session
        .manifest
        .started
        .with_timezone(&chrono::Local)
        .format("Recording, %b %-d %-I:%M %p")
        .to_string();
    let structured = crate::ai::long::structure_recording(
        &assembled.parts,
        &points,
        existing,
        drafts_for(&session, &assembled.parts, existing).as_ref(),
        &fallback,
        &|prompt, max| {
            crate::ai::chat_outcome(
                crate::ai::long::with_profile(prompt, &session.manifest.profile),
                max,
            )
            .map(|o| {
                session.trace(
                    "generation-provider",
                    &format!("{}; {} fallback(s)", o.provider, o.fallbacks.len()),
                );
                o.value
            })
        },
        progress,
        crate::ai::long::room_after_profile(crate::ai::writing_budget(), &session.manifest.profile),
    );
    let notice = session::failure_notice(&assembled.failed, dir);
    let body = if notice.is_empty() {
        structured.body
    } else {
        format!("{notice}\n\n{}", structured.body)
    };
    let label = session
        .manifest
        .profile
        .context
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Calendar event:"))
        .and_then(|event| {
            leo_core::filename::tracking_label(event)
                .or_else(|| leo_core::filename::series_label(event))
        });
    let written = Written {
        title: leo_core::filename::with_label(&structured.title, label.as_deref()),
        body,
        problems: structured.problems,
        writer_failed: structured.writer_failed,
    };
    leo_core::recording::write_json(
        &cached,
        &Prepared {
            key,
            written: written.clone(),
        },
    )?;
    session.trace(
        "prepared",
        "Generated result saved; waiting for note persistence",
    );
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled_session(root: &Path) -> Session {
        let mut manifest = Manifest::new(None, None, "", false);
        manifest.profile.speakers = true;
        let session = Session::create(root, manifest).unwrap();
        for index in 0..2 {
            std::fs::write(session.dir.join(format!("seg-{index:05}.txt")), "plain").unwrap();
            wav::write(
                &session.dir.join(wav::done_name(index)),
                &vec![1000i16; 16_000 * 4],
            )
            .unwrap();
        }
        session
    }

    #[test]
    fn labelling_at_finish_saves_the_labels_and_drops_the_kept_audio() {
        let root = tempfile::tempdir().unwrap();
        let session = labelled_session(root.path());
        let events = Mutex::new(Vec::new());
        label_speakers(
            &session,
            &|e| events.lock().unwrap().push(e),
            &AtomicBool::new(false),
            &|_| {
                Ok(vec![
                    crate::speech::Turn {
                        start_ms: 0,
                        end_ms: 3_000,
                        speaker: 1,
                    },
                    crate::speech::Turn {
                        start_ms: 3_000,
                        end_ms: 7_000,
                        speaker: 2,
                    },
                ])
            },
            &|_| Ok("words".to_string()),
        );
        assert_eq!(session.spoken().map(|s| s.len()), Some(2));
        assert!(!session.dir.join(wav::done_name(0)).exists());
        assert!(!session.dir.join(wav::done_name(1)).exists());
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Progress {
                steps: Some((1, 2)),
                ..
            }
        )));
        assert!(!events.iter().any(|e| matches!(e, Event::Warning(_))));
    }

    #[test]
    fn labelling_is_skipped_with_a_warning_for_a_failed_part_or_a_failed_pass() {
        let root = tempfile::tempdir().unwrap();
        let session = labelled_session(root.path());
        std::fs::write(session.dir.join("seg-00002.err"), "failed").unwrap();
        let warned = Mutex::new(Vec::new());
        let emit = |e| {
            if let Event::Warning(w) = e {
                warned.lock().unwrap().push(w)
            }
        };
        let never =
            |_: Vec<(std::path::PathBuf, u64)>| -> Result<Vec<crate::speech::Turn>, String> {
                panic!("no pass with a failed part")
            };
        label_speakers(&session, &emit, &AtomicBool::new(false), &never, &|_| {
            Ok(String::new())
        });
        assert!(!session.dir.join(session::speakers::FILE).exists());
        std::fs::remove_file(session.dir.join("seg-00002.err")).unwrap();
        label_speakers(
            &session,
            &emit,
            &AtomicBool::new(false),
            &|_| Err("no model".into()),
            &|_| Ok(String::new()),
        );
        assert!(session.dir.join(session::speakers::FILE).exists());
        assert!(session.spoken().is_none());
        let warned = warned.into_inner().unwrap();
        assert_eq!(warned.len(), 2);
        assert!(warned[1].contains("no model"));
    }

    #[test]
    fn a_cached_write_up_is_reused_only_when_the_writer_succeeded() {
        let cached = |failed| {
            serde_json::to_vec(&Prepared {
                key: "k".into(),
                written: Written {
                    title: "T".into(),
                    body: "B".into(),
                    problems: Vec::new(),
                    writer_failed: failed,
                },
            })
            .unwrap()
        };
        assert_eq!(reusable(&cached(false), "k").unwrap().body, "B");
        assert!(reusable(&cached(false), "other").is_none());
        assert!(reusable(&cached(true), "k").is_none());
    }

    #[test]
    #[ignore = "benchmark; run by hand"]
    fn benchmark_preview_audio_preparation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.wav");
        let samples = vec![3000i16; 20 * wav::RATE as usize];
        for trial in 0..5 {
            let start = Instant::now();
            for _ in 0..8 {
                wav::write(&path, &samples).unwrap();
                std::hint::black_box(
                    samples
                        .iter()
                        .map(|s| *s as f32 / 32768.0)
                        .collect::<Vec<_>>(),
                );
                std::fs::remove_file(&path).unwrap();
            }
            let before = start.elapsed();
            let start = Instant::now();
            for _ in 0..8 {
                std::hint::black_box(
                    samples
                        .iter()
                        .map(|s| *s as f32 / 32768.0)
                        .collect::<Vec<_>>(),
                );
            }
            println!("trial={trial} previews=8 audio_secs=20 before_us={} after_us={} before_syncs=8 after_syncs=0", before.as_micros(), start.elapsed().as_micros());
        }
    }
    #[test]
    fn a_recording_without_a_model_uses_one_worker_and_the_local_pace() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", false)).unwrap();
        assert_eq!(policy_for(&session).workers, 1);
        assert_eq!(
            Live::new(live::ROLL_INTERVAL).interval,
            Duration::from_secs(3)
        );
    }

    #[test]
    fn hiding_only_disables_preview_and_resume_keeps_capture_controls_intact() {
        let mut controls = Controls::default();
        assert!(controls.preview_enabled());
        let visible = Arc::new(AtomicBool::new(true));
        controls.preview = Some(visible.clone());
        for shown in [true, false, true, false] {
            visible.store(shown, Ordering::Release);
            assert_eq!(controls.preview_enabled(), shown);
            assert!(!controls.stop.load(Ordering::Acquire));
            assert!(!controls.pause.load(Ordering::Acquire));
            assert!(!controls.finish_now.load(Ordering::Acquire));
        }
    }

    #[test]
    fn a_start_that_cannot_lock_leaves_no_folder() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", true)).unwrap();
        std::fs::create_dir(session.dir.join("lock")).unwrap();
        assert!(lock_or_remove(&session).is_err());
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
        assert!(Session::unfinished(root.path()).is_empty());
    }

    fn ending_of_a_silent_recording(screen: bool) -> Option<Event> {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", screen)).unwrap();
        wav::write(&session.dir.join(wav::done_name(0)), &vec![0i16; 16_000]).unwrap();
        let (tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &session.dir,
            std::sync::Arc::new(|_| Ok("unused".into())),
            Policy::default(),
            false,
            tx,
        );
        let events = std::sync::Mutex::new(Vec::new());
        finish(
            session,
            transcriber,
            &updates,
            &mut Live::new(live::ROLL_INTERVAL),
            &|event| events.lock().unwrap().push(event),
            &AtomicBool::new(false),
        );
        events.into_inner().unwrap().pop()
    }

    #[test]
    fn a_silent_computer_audio_recording_is_told_about_computer_audio() {
        assert_eq!(
            ending_of_a_silent_recording(true),
            Some(Event::Failed(SILENT_COMPUTER_AUDIO.to_string()))
        );
    }

    #[test]
    fn a_silent_microphone_recording_keeps_the_microphone_sentence() {
        assert_eq!(
            ending_of_a_silent_recording(false),
            Some(Event::Failed(SILENT_RECORDING.to_string()))
        );
    }

    #[test]
    fn progress_after_stop_counts_parts_settled_before_and_after() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", false)).unwrap();
        let dir = session.dir.clone();
        for index in 0..3 {
            std::fs::write(dir.join(format!("seg-{index:05}.txt")), "said").unwrap();
        }
        for index in 3..5 {
            wav::write(&dir.join(wav::done_name(index)), &vec![3000i16; 16_000]).unwrap();
        }
        let (tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &dir,
            std::sync::Arc::new(|_| {
                std::thread::sleep(Duration::from_millis(400));
                Ok("said".into())
            }),
            Policy {
                workers: 1,
                ..Policy::default()
            },
            false,
            tx,
        );
        let events = std::sync::Mutex::new(Vec::new());
        finish(
            session,
            transcriber,
            &updates,
            &mut Live::new(live::ROLL_INTERVAL),
            &|event| events.lock().unwrap().push(event),
            &AtomicBool::new(false),
        );
        let steps: Vec<(usize, usize)> = events
            .into_inner()
            .unwrap()
            .into_iter()
            .filter_map(|event| match event {
                Event::Progress { steps, .. } => steps,
                _ => None,
            })
            .collect();
        assert!(steps.len() >= 2, "{steps:?}");
        assert!(steps.iter().all(|&(_, total)| total == 5), "{steps:?}");
        assert!(steps[0].0 >= 3, "{steps:?}");
        assert!(steps.windows(2).all(|w| w[0].0 <= w[1].0), "{steps:?}");
        assert!(steps.last().unwrap().0 >= 4, "{steps:?}");
    }

    #[test]
    fn a_recording_stopped_without_a_model_ends_unsaved_and_stays_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", false)).unwrap();
        let dir = session.dir.clone();
        wav::write(&dir.join(wav::done_name(0)), &vec![3000i16; 1600]).unwrap();
        let (tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &dir,
            std::sync::Arc::new(|_| Err("Download a model in Settings → Speech models.".into())),
            Policy {
                first_wait: Duration::from_millis(5),
                idle: Duration::from_millis(5),
                finish_limit: Duration::from_millis(40),
                ..Policy::default()
            },
            false,
            tx,
        );
        let events = std::sync::Mutex::new(Vec::new());
        finish(
            session,
            transcriber,
            &updates,
            &mut Live::new(live::ROLL_INTERVAL),
            &|event| events.lock().unwrap().push(event),
            &AtomicBool::new(false),
        );
        let events = events.into_inner().unwrap();
        assert!(
            matches!(events.last(), Some(Event::Failed(why)) if why.contains("speech model")),
            "{:?}",
            events.last()
        );
        assert!(dir.join(wav::done_name(0)).exists());
        let found: Vec<PathBuf> = Session::unfinished(root.path())
            .into_iter()
            .map(|s| s.dir)
            .collect();
        assert_eq!(found, [dir]);
    }

    #[test]
    fn points_arriving_with_stop_are_mirrored_before_the_final_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut session =
            Session::create(dir.path(), Manifest::new(None, None, "", false)).unwrap();
        let controls = Controls::default();
        final_points(&mut session, &controls, &|event| {
            if matches!(event, Event::Progress { .. }) {
                controls.points.lock().unwrap().push(Jotted {
                    at_secs: 4,
                    text: "The pasted slide".into(),
                });
            }
        });
        assert_eq!(session.points().len(), 1);
        assert_eq!(session.points()[0].text, "The pasted slide");
        assert_eq!(controls.saved_points.load(Ordering::Acquire), 1);
    }

    fn drafting_session(dir: &Path, files: &[(&str, &str)]) -> Session {
        let session = Session::create(dir, Manifest::new(None, None, "", false)).unwrap();
        for (name, text) in files {
            std::fs::write(session.dir.join(name), text).unwrap();
        }
        session
    }

    fn answer(text: &str) -> anyhow::Result<crate::ai::chain::ChainOutcome<String>> {
        Ok(crate::ai::chain::ChainOutcome {
            value: text.to_string(),
            provider: "fake".into(),
            fallbacks: Vec::new(),
            spent: None,
        })
    }

    fn counting(calls: &Arc<std::sync::atomic::AtomicUsize>, reply: &'static str) -> DraftFn {
        let calls = calls.clone();
        Arc::new(move |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            if reply.is_empty() {
                anyhow::bail!("busy")
            }
            answer(reply)
        })
    }

    fn gated(release: &Arc<AtomicBool>, started: &Arc<AtomicBool>, reply: &'static str) -> DraftFn {
        let (release, started) = (release.clone(), started.clone());
        Arc::new(move |_, _, stopped| {
            started.store(true, Ordering::SeqCst);
            while !release.load(Ordering::SeqCst) {
                if stopped() {
                    release.store(true, Ordering::SeqCst);
                    anyhow::bail!("stopped")
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            answer(reply)
        })
    }

    fn wait_for(flag: &AtomicBool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !flag.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(flag.load(Ordering::SeqCst));
    }

    fn settle(drafter: &mut Drafter, session: &Session) {
        let until = Instant::now() + Duration::from_secs(10);
        while drafter.running.is_some() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
            drafter.tick(session, 1);
        }
        assert!(drafter.running.is_none());
    }

    fn budget() -> crate::ai::budget::Budget {
        crate::ai::budget::Budget::AGENT
    }

    #[test]
    fn the_drafter_reads_each_transcript_once() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "first stretch")]);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut drafter = Drafter::new(counting(&calls, "## First\n- one"), budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        std::fs::write(session.dir.join("seg-00001.txt"), "second stretch").unwrap();
        let all = session.assemble().parts;
        std::fs::remove_file(session.dir.join("seg-00000.txt")).unwrap();
        std::fs::create_dir(session.dir.join("seg-00000.txt")).unwrap();
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let done = drafter.done.clone().unwrap();
        assert_eq!(done.upto, 2);
        let points = session.jotted();
        assert_eq!(
            done.key,
            draft_key(&all, &points, &session.manifest.profile, 2)
        );
    }

    #[test]
    fn drafts_cover_only_the_finished_start_of_the_recording() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(
            root.path(),
            &[
                ("seg-00000.txt", "disks have platters"),
                ("seg-00001.wav", ""),
                ("seg-00002.txt", "caching later"),
            ],
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut drafter = Drafter::new(
            counting(&calls, "## Platters\n- spin\n\n## Action items\n- [ ] HW 4 due Friday\n\n## Check yourself\n> q"),
            budget(),
        );
        drafter.tick(&session, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "speech still behind");
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let done = drafter.done.clone().unwrap();
        assert_eq!((done.upto, done.notes.as_str()), (1, "## Platters\n- spin"));
        assert_eq!(done.commitments, "- [ ] HW 4 due Friday");
        let parts = session.assemble().parts;
        assert_eq!(drafts_for(&session, &parts, None), Some(done));
        assert_eq!(drafts_for(&session, &parts, Some("old")), None);
        let mut changed = Session::open(&session.dir).unwrap();
        changed.manifest.profile.wants = "proofs only".into();
        assert_eq!(drafts_for(&changed, &parts, None), None);
        std::fs::write(session.dir.join(DRAFTS), b"{not json").unwrap();
        assert_eq!(drafts_for(&session, &parts, None), None);
        let trace = std::fs::read_to_string(session.dir.join("trace.jsonl")).unwrap();
        assert!(trace.contains("draft-provider") && trace.contains("fake"));
    }

    #[test]
    fn one_draft_runs_at_a_time_and_the_next_continues_the_notes() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "first stretch")]);
        let (release, started) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (gate, count, seen) = (release.clone(), calls.clone(), started.clone());
        let chat: DraftFn = Arc::new(move |prompt: crate::ai::chat::Prompt, _, _| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            seen.store(true, Ordering::SeqCst);
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            if n == 1 {
                assert!(prompt.user.contains("## One"));
            }
            answer(&format!("## {}\n- x", ["One", "Two"][n]))
        });
        let mut drafter = Drafter::new(chat, budget());
        drafter.tick(&session, 0);
        wait_for(&started);
        std::fs::write(session.dir.join("seg-00001.txt"), "second stretch").unwrap();
        drafter.tick(&session, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.store(true, Ordering::SeqCst);
        settle(&mut drafter, &session);
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let done = drafter.done.clone().unwrap();
        assert_eq!(
            (done.upto, done.notes.as_str()),
            (2, "## One\n- x\n\n## Two\n- x")
        );
    }

    #[test]
    fn a_changed_wish_starts_the_drafts_over() {
        let root = tempfile::tempdir().unwrap();
        let mut session = drafting_session(root.path(), &[("seg-00000.txt", "first stretch")]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let prompts = seen.clone();
        let chat: DraftFn = Arc::new(move |prompt: crate::ai::chat::Prompt, _, _| {
            prompts.lock().unwrap().push(prompt.user.clone());
            answer("## Fresh\n- y")
        });
        let mut drafter = Drafter::new(chat, budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        session.manifest.profile.wants = "proofs only".into();
        std::fs::write(session.dir.join("seg-00001.txt"), "second stretch").unwrap();
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].contains("first stretch") && seen[1].contains("proofs only"));
        assert!(!seen[1].contains("## Fresh"));
        assert_eq!(drafter.done.as_ref().unwrap().notes, "## Fresh\n- y");
    }

    #[test]
    fn a_failed_segment_two_failed_drafts_or_a_stretch_too_big_end_drafting() {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = drafting_session(root.path(), &[("seg-00000.err", "x")]);
        let mut drafter = Drafter::new(counting(&calls, "## A"), budget());
        drafter.tick(&failed, 0);
        assert!(drafter.ended);
        let trace = std::fs::read_to_string(failed.dir.join("trace.jsonl")).unwrap();
        assert!(trace.contains("draft-ended"));
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let mut drafter = Drafter::new(counting(&calls, ""), budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert!(!drafter.ended, "one failure is retried");
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert!(drafter.ended);
        drafter.tick(&session, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!session.dir.join(DRAFTS).exists());
        let trace = std::fs::read_to_string(session.dir.join("trace.jsonl")).unwrap();
        assert!(trace.contains("draft-failed"));
        let tiny = crate::ai::budget::Budget {
            words: 10,
            at_once: 1,
        };
        let mut drafter = Drafter::new(counting(&calls, "## A"), tiny);
        drafter.tick(&session, 0);
        assert!(drafter.ended && drafter.running.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_silent_stretch_moves_on_without_a_call_unless_points_were_typed_in_it() {
        let root = tempfile::tempdir().unwrap();
        let mut session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut drafter = Drafter::new(counting(&calls, "## A\n- a"), budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        std::fs::write(session.dir.join("seg-00001.txt"), "  ").unwrap();
        drafter.tick(&session, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(drafter.done.as_ref().unwrap().upto, 2);
        let parts = session.assemble().parts;
        assert!(drafts_for(&session, &parts, None).is_some_and(|d| d.upto == 2));
        session.manifest.points = vec![Point {
            at_secs: 700,
            text: "board work matters".into(),
        }];
        std::fs::write(session.dir.join("seg-00002.txt"), "").unwrap();
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(drafter.done.as_ref().unwrap().upto, 3);
    }

    #[test]
    fn a_draft_that_lands_after_the_session_is_gone_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let (release, started) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut drafter = Drafter::new(gated(&release, &started, "## A"), budget());
        drafter.tick(&session, 0);
        wait_for(&started);
        std::fs::remove_file(session.dir.join("session.json")).unwrap();
        release.store(true, Ordering::SeqCst);
        drafter.finish(&AtomicBool::new(false), &|_| {});
        assert!(!session.dir.join(DRAFTS).exists());
    }

    #[test]
    fn finish_now_stops_a_running_draft_and_waits_for_it() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let (release, started) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut drafter = Drafter::new(gated(&release, &started, "## A"), budget());
        drafter.tick(&session, 0);
        wait_for(&started);
        let said = Mutex::new(Vec::new());
        drafter.finish(&AtomicBool::new(true), &|event| {
            said.lock().unwrap().push(event)
        });
        assert!(
            release.load(Ordering::SeqCst),
            "the draft saw the stop before finish returned"
        );
        assert!(!session.dir.join(DRAFTS).exists());
        assert!(matches!(
            &said.lock().unwrap()[..],
            [Event::Progress { .. }]
        ));
    }
}
