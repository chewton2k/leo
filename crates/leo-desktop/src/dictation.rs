use anyhow::{ensure, Context, Result};
use leo_services::dictation::{
    activation::{Action, Activation},
    generation::Generation,
    pieces::{Live, Piece, Pieces, Pipeline},
    settings::{Mode, Popup, Settings, Writing},
    visualizer::Visualizer,
};
use leo_services::speech::{
    chunking::join_transcripts,
    engine::{engine, DictationStream, Priority},
    library::{library, Selected},
    supervisor::EngineError,
    vad::{
        CaptureVad, SileroVad, SILERO_VAD_THRESHOLD, VAD_OFFLINE_HANGOVER_MS,
        VAD_STREAMING_HANGOVER_MS,
    },
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter};

const CANCELLED: &str = "Dictation cancelled.";
const STOPPED_UNEXPECTEDLY: &str = "Dictation stopped unexpectedly. Try again.";
const NOT_TRANSCRIBED: &str =
    "Dictation could not be transcribed. Try again or choose a smaller model in Settings.";
const MICROPHONE: &str = "Microphone";
const MIC_STOPPED: &str = "The microphone stopped. Leo pasted what it heard.";
const MIC_STOPPED_EARLY: &str = "The microphone stopped before anything was heard.";
const SILENCE: &str = "The microphone sent only silence. Check that it isn't muted, or choose another in Dictation settings.";
const NOTHING_HEARD: &str = "Nothing heard";
const NOTHING: &str = "Nothing was heard. Check the microphone in Dictation settings.";
const CHECK_PASTE: &str = "Check the paste";
const WITHOUT_AI: &str = "Pasted without AI writing";
const TEST_FOLDER: &str = "The test output needs a folder.";
const FINISH_TO_SAVE: &str = "Finish dictating before changing its settings.";
const UNREADABLE: &str =
    "These dictation settings could not be read. Check the fields and try again.";
const FINISH_TO_RECORD: &str = "Finish dictating before recording a shortcut.";
const SHORTCUTS_OFF: &str = "Shortcuts are not running in this session. Reopen leo and try again.";
const CHOOSE_RECENT: &str = "Choose a recent dictation.";
const CHOOSE_THEME: &str = "Choose Light, Dark or Match.";
const CHOOSE_ACTION: &str = "Choose an action in Dictation settings.";

const FEED_SAMPLES: usize = 512;
const SHOWN_CHARS: usize = 25_000;

const NOTICE_FOR: std::time::Duration = std::time::Duration::from_secs(4);
#[derive(Debug, PartialEq)]
enum Finish {
    Done,
    Problem(String),
    Notice(&'static str, String),
}
#[derive(Default)]
struct Queue {
    activation: Activation,
    busy: Option<u64>,
    queued: Activation,
    queued_wants: bool,
}
impl Queue {
    fn input(
        &mut self,
        pressed: bool,
        now: Instant,
        mode: Mode,
        capturing: bool,
        stop: &AtomicBool,
    ) -> Action {
        if self.busy.is_some() && !capturing && stop.load(Ordering::Acquire) {
            match self.queued.input(pressed, now, mode) {
                Action::Start => self.queued_wants = true,
                Action::Stop => self.queued_wants = false,
                Action::None => {}
            }
            return Action::None;
        }
        match self.activation.input(pressed, now, mode) {
            Action::Start if self.busy.is_none() => Action::Start,
            Action::Stop => {
                stop.store(true, Ordering::Release);
                self.activation.reset();
                if self.busy.is_some() && !capturing {
                    Action::None
                } else {
                    Action::Stop
                }
            }
            _ => Action::None,
        }
    }
    fn complete(&mut self, id: u64, current: bool) -> bool {
        if self.busy != Some(id) {
            return false;
        }
        self.busy = None;
        let start = self.queued_wants && current;
        self.activation = if start {
            std::mem::take(&mut self.queued)
        } else {
            Activation::default()
        };
        self.queued.reset();
        self.queued_wants = false;
        start
    }
    fn cancel(&mut self) {
        self.queued.reset();
        self.queued_wants = false;
        self.activation.reset();
    }
}
enum Event {
    Input(bool, bool, Instant),
    Complete(u64),
    Cancel,
    Shutdown(mpsc::SyncSender<()>),
}
pub struct Dictation {
    app: AppHandle,
    settings: Mutex<Settings>,
    path: PathBuf,
    state: Mutex<Value>,
    theme: Mutex<String>,
    problem: Mutex<Option<String>>,
    generation: Generation,
    revision: AtomicU64,
    capturing: Arc<AtomicBool>,
    escape: Arc<AtomicBool>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
    tx: mpsc::SyncSender<Event>,
    shortcut: OnceLock<crate::shortcut::Shortcuts>,
    pub usage: OnceLock<Arc<leo_web::usage::Usage>>,
}
impl Dictation {
    pub fn new(app: AppHandle) -> Result<Arc<Self>> {
        let path = leo_core::paths::config_dir()?.join(leo_core::paths::own::DICTATION);
        let (settings, problem) = Settings::recover(&path);
        let mut in_use = settings.clone();
        in_use.shortcut = crate::shortcut::loaded(settings.shortcut.clone()).0;
        let (tx, rx) = mpsc::sync_channel(32);
        let this = Arc::new(Self {
            app: app.clone(),
            settings: Mutex::new(in_use),
            path,
            state: Mutex::new(json!({"state":"idle"})),
            theme: Mutex::new("system".into()),
            problem: Mutex::new(problem),
            generation: Generation::default(),
            revision: AtomicU64::new(0),
            capturing: Arc::new(AtomicBool::new(false)),
            escape: Arc::new(AtomicBool::new(false)),
            stop: Mutex::new(None),
            tx,
            shortcut: OnceLock::new(),
            usage: OnceLock::new(),
        });
        let input = this.tx.clone();
        if !Self::test_mode() {
            leo_services::session::mic::warm_up();
            std::thread::spawn(|| {
                let _ = SileroVad::new(SILERO_VAD_THRESHOLD);
                let _ = library().and_then(|models| models.selected());
                if let Ok(config) = leo_core::paths::config_dir() {
                    engine().prepare_after_update(&config, env!("CARGO_PKG_VERSION"));
                }
            });
            let keys = crate::shortcut::Shortcuts::start(
                app,
                settings.shortcut,
                this.escape.clone(),
                Arc::new(move |pressed, cancel, at| {
                    if input.try_send(Event::Input(pressed, cancel, at)).is_err() {
                        log::warn!("Dictation dropped a shortcut input: its queue is full.");
                    }
                }),
            );
            let _ = this.shortcut.set(keys);
        }
        let worker = this.clone();
        std::thread::spawn(move || worker.run(rx));
        Ok(this)
    }
    fn test_mode() -> bool {
        cfg!(debug_assertions) && std::env::var_os("LEO_DICTATION_TEST").is_some()
    }
    fn preferences(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn shortcut_health(&self) -> (Value, String) {
        (
            self.shortcut.get().map(|s| s.view()).unwrap_or(Value::Null),
            self.preferences().shortcut,
        )
    }
    pub fn popup(&self) -> Value {
        let mut value = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.decorate(&mut value);
        value
    }
    fn decorate(&self, value: &mut Value) {
        value["theme"] = json!(self.theme.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let settings = self.preferences();
        value["minimal"] = json!(settings.popup != Popup::Live);
        value["top"] = json!(settings.top);
    }
    fn publish(&self, mut view: Value) {
        if self.preferences().popup == Popup::Off
            && !matches!(view["state"].as_str(), Some("error" | "notice"))
        {
            return;
        }
        self.decorate(&mut view);
        let _ = self.app.emit_to("dictation", "dictation", view);
    }
    fn stamp(&self, state: &mut Value) -> Value {
        state["rev"] = json!(self.revision.fetch_add(1, Ordering::AcqRel) + 1);
        state.clone()
    }
    fn change(&self, id: u64, apply: impl FnOnce(&mut Value)) -> bool {
        let view = self.generation.apply(id, &self.state, |state| {
            apply(state);
            self.stamp(state)
        });
        let changed = view.is_some();
        if let Some(view) = view {
            self.publish(view);
        }
        changed
    }
    fn update(&self, id: u64, value: Value) -> bool {
        self.change(id, |state| *state = value)
    }
    fn set_escape(&self, active: bool) {
        self.escape.store(active, Ordering::Release);
        if let Some(keys) = self.shortcut.get() {
            keys.wake();
        }
    }
    fn phase(&self, id: u64, phase: &str) {
        self.change(id, |state| state["state"] = json!(phase));
    }
    fn notice(self: &Arc<Self>, id: u64, settings: &Settings, title: &str, message: &str) {
        if !self.update(
            id,
            json!({"state":"notice","title":title,"message":message}),
        ) {
            return;
        }
        self.show(id, settings);
        let this = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(NOTICE_FOR);
            if this.update(id, json!({"state":"idle"})) {
                crate::overlay::hide(&this.app);
            }
        });
    }
    fn idle(&self) {
        let view = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            *state = json!({"state":"idle"});
            self.stamp(&mut state)
        };
        self.publish(view);
        crate::overlay::hide(&self.app);
    }
    fn show(self: &Arc<Self>, id: u64, settings: &Settings) {
        if settings.popup == Popup::Off {
            crate::overlay::hide(&self.app);
            return;
        }
        let this = self.clone();
        crate::overlay::show(
            &self.app,
            settings,
            Arc::new(move || this.generation.current(id)),
        );
    }
    pub fn cancel(&self) {
        self.generation.cancel();
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.store(true, Ordering::Release);
        }
        self.capturing.store(false, Ordering::Release);
        self.set_escape(false);
        engine().cancel();
        self.idle();
        let _ = self.tx.try_send(Event::Cancel);
    }
    pub fn shutdown(&self) {
        self.generation.cancel();
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.store(true, Ordering::Release);
        }
        engine().cancel();
        if let Some(keys) = self.shortcut.get() {
            keys.stop();
        }
        let (done, finished) = mpsc::sync_channel(1);
        let _ = self.tx.try_send(Event::Shutdown(done));
        let _ = finished.recv_timeout(std::time::Duration::from_secs(5));
    }
    fn run(self: Arc<Self>, rx: mpsc::Receiver<Event>) {
        let mut queue = Queue::default();
        while let Ok(event) = rx.recv() {
            match event {
                Event::Input(pressed, cancel, at) => {
                    if cancel {
                        self.cancel();
                        continue;
                    }
                    let stop = self
                        .stop
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                        .unwrap_or_default();
                    match queue.input(
                        pressed,
                        at,
                        self.preferences().mode,
                        self.capturing.load(Ordering::Acquire),
                        &stop,
                    ) {
                        Action::Start => queue.busy = Some(self.begin()),
                        Action::Stop => {
                            self.capturing.store(false, Ordering::Release);
                            if let Some(id) = queue.busy {
                                self.phase(
                                    id,
                                    if engine().loaded() {
                                        "transcribing"
                                    } else {
                                        "loading"
                                    },
                                );
                            }
                        }
                        Action::None => {}
                    }
                }
                Event::Cancel => {
                    self.generation.cancel();
                    queue.cancel();
                    if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                    {
                        stop.store(true, Ordering::Release);
                    }
                    self.capturing.store(false, Ordering::Release);
                    self.set_escape(false);
                    self.idle();
                }
                Event::Complete(id) => {
                    if queue.busy == Some(id) {
                        self.capturing.store(false, Ordering::Release);
                        if queue.complete(id, self.generation.current(id)) {
                            queue.busy = Some(self.begin());
                        }
                    }
                }
                Event::Shutdown(done) => {
                    let _ = done.send(());
                    break;
                }
            }
        }
    }
    fn begin(self: &Arc<Self>) -> u64 {
        let activity = leo_core::activity::global().begin();
        let id = self.generation.next();
        let settings = self.preferences();
        let stop = Arc::new(AtomicBool::new(false));
        *self.stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop.clone());
        self.capturing.store(true, Ordering::Release);
        self.set_escape(true);
        self.update(
            id,
            json!({"state":"starting","seconds":0,"levels":[],"committed":"","tentative":""}),
        );
        self.show(id, &settings);
        let this = self.clone();
        std::thread::spawn(move || {
            let result =
                guarded(|| activity.and_then(|_activity| this.capture(id, &settings, &stop)));
            if this.generation.current(id) {
                this.set_escape(false);
                let mut shown = settings.clone();
                if shown.popup == Popup::Off {
                    shown.popup = Popup::Minimal;
                }
                match result {
                    Ok(Finish::Problem(message)) => {
                        this.update(id, json!({"state":"error","message":message}));
                        this.show(id, &shown);
                    }
                    Ok(Finish::Notice(title, message)) => this.notice(id, &shown, title, &message),
                    Ok(Finish::Done) => {
                        this.update(id, json!({"state":"idle"}));
                        crate::overlay::hide(&this.app);
                    }
                    Err(e) => {
                        this.update(id, json!({"state":"error","message":e.to_string()}));
                        this.show(id, &shown);
                    }
                }
            }
            let _ = this.tx.send(Event::Complete(id));
        });
        id
    }
    fn capture(
        self: &Arc<Self>,
        id: u64,
        settings: &Settings,
        stop: &AtomicBool,
    ) -> Result<Finish> {
        let _awake = leo_services::session::awake::Awake::recording();
        let _reservation = engine().dictation();
        let app = (settings.writing != Writing::Off)
            .then(crate::permissions::frontmost_app)
            .flatten();
        let selected = library()?.selected()?;
        let live = selected.streaming && selected.live;
        if !live {
            engine().warm(selected.clone());
        }
        ensure!(self.generation.current(id), CANCELLED);
        let named = settings.microphone.clone();
        if stop.load(Ordering::Acquire) {
            return Ok(Finish::Done);
        }
        let (mut mic, rx) = if let Some(path) = std::env::var("LEO_FAKE_AUDIO")
            .ok()
            .filter(|_| Self::test_mode())
        {
            let pcm = leo_services::session::wav::read(std::path::Path::new(&path))?;
            let (tx, rx) = mpsc::sync_channel(64);
            let flag = self.clone();
            std::thread::spawn(move || {
                for part in pcm.chunks(1600) {
                    if !flag.generation.current(id) || tx.send(part.to_vec()).is_err() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
            (None, rx)
        } else {
            let (mic, rx) = leo_services::session::mic::Mic::open_named(named)?;
            (Some(mic), rx)
        };
        let open = |this: &Arc<Self>| {
            let this = this.clone();
            live.then(|| {
                engine()
                    .stream(
                        selected.clone(),
                        settings.custom_words.clone(),
                        move |committed, tentative| {
                            this.change(id, |state| {
                                state["committed"] = json!(newest(&committed));
                                state["tentative"] = json!(newest(&tentative));
                            });
                        },
                    )
                    .ok()
            })
            .flatten()
        };
        let mut stream = open(self);
        let mut pending = Vec::new();
        let mut kept = Vec::new();
        let mut raw = Vec::new();
        let mut pieces = Pieces::default();
        let mut pipeline: Option<Pipeline> = None;
        let mut cut = 0;
        let mut broken = None;
        let hangover = if selected.streaming {
            VAD_STREAMING_HANGOVER_MS
        } else {
            VAD_OFFLINE_HANGOVER_MS
        };
        let mut speech = match SileroVad::new(SILERO_VAD_THRESHOLD) {
            Ok(silero) => Some(CaptureVad::new(Box::new(silero), hangover)),
            Err(error) => {
                log::warn!("Dictation is not filtered, Silero VAD did not load: {error:#}");
                None
            }
        };
        let mut ready = false;
        let mut visualizer = Visualizer::new();
        let pcm = leo_services::dictation::capture::collect(
            &rx,
            stop,
            |chunk, _, seconds| {
                if !ready {
                    ready = true;
                    let mut began = false;
                    self.change(id, |state| {
                        if state["state"] == "starting" {
                            state["state"] = json!("listening");
                            began = true;
                        }
                    });
                    if began {
                        let this = self.clone();
                        chime(settings.sounds, true, move || this.generation.current(id));
                    }
                }
                if let Some(levels) = visualizer.feed(chunk) {
                    self.change(id, |state| {
                        state["seconds"] = json!(seconds);
                        state["levels"] = json!(levels);
                    });
                }
                let heard = speech
                    .as_mut()
                    .map_or_else(|| chunk.to_vec(), |vad| vad.push(chunk));
                if broken.is_none() && pieces.cut(chunk.len(), !heard.is_empty()) {
                    if pipeline.is_none() {
                        match self.pipeline(&selected, &settings.custom_words) {
                            Ok(made) => pipeline = Some(made),
                            Err(error) => broken = Some(error),
                        }
                    }
                    if let Some(pipeline) = &pipeline {
                        let piece = Piece {
                            index: cut,
                            live: live_text(stream.take(), std::mem::take(&mut pending)),
                            kept: std::mem::take(&mut kept),
                            raw: std::mem::take(&mut raw),
                        };
                        cut += 1;
                        if let Err(error) = pipeline.push(piece) {
                            broken = Some(error);
                        }
                        stream = open(self);
                    }
                }
                kept.extend_from_slice(&heard);
                if selected.streaming {
                    raw.extend_from_slice(chunk);
                }
                if let Some(live) = stream.as_ref() {
                    pending.extend_from_slice(&heard);
                    if pending.len() >= FEED_SAMPLES {
                        if !feed(live, &pending) {
                            stream = None;
                        }
                        pending.clear();
                    }
                }
            },
            || {
                if let Some(mic) = mic.as_mut() {
                    mic.close();
                }
            },
        );
        self.capturing.store(false, Ordering::Release);
        if settings.writing == Writing::Off {
            self.set_escape(false);
        }
        let problem = mic
            .as_ref()
            .and_then(|mic| mic.problem.lock().ok().and_then(|p| p.clone()));
        let released = Instant::now();
        let captured = pcm.map_err(|e| problem.clone().map_or(e, anyhow::Error::msg))?;
        if let Some(error) = broken {
            log::warn!("Dictation could not be cut into pieces: {error:#}");
            anyhow::bail!(NOT_TRANSCRIBED);
        }
        if let Some(problem) = &problem {
            log::warn!("Dictation's microphone stopped: {problem}");
        }
        let stopped = problem.is_some() || captured.stalled;
        let signal = mic.as_ref().is_none_or(|mic| mic.heard_signal());
        ensure!(self.generation.current(id), CANCELLED);
        self.phase(
            id,
            if engine().loaded() {
                "transcribing"
            } else {
                "loading"
            },
        );
        {
            let this = self.clone();
            chime(settings.sounds, false, move || this.generation.current(id));
        }
        let tail = speech.map(CaptureVad::finish).unwrap_or_default();
        kept.extend_from_slice(&tail);
        pending.extend_from_slice(&tail);
        let text = match pipeline {
            None => {
                match self.piece_text(
                    id,
                    live_text(stream, pending),
                    kept,
                    raw,
                    &selected,
                    &settings.custom_words,
                )? {
                    Some(text) => text,
                    None => return Ok(Finish::Done),
                }
            }
            Some(pipeline) => {
                pipeline.push(Piece {
                    index: cut,
                    live: live_text(stream, pending),
                    kept,
                    raw,
                })?;
                cut += 1;
                match pipeline.finish() {
                    Ok(texts) => join_transcripts(&texts),
                    Err(e) if cancelled(&e) => return Ok(Finish::Done),
                    Err(e) => {
                        log::warn!("Dictation could not be transcribed: {e:#}");
                        anyhow::bail!(NOT_TRANSCRIBED);
                    }
                }
            }
        };
        let pieces = cut.max(1);
        let timing = move || {
            eprintln!(
                "  dictation: {} s, {pieces} piece(s), pasted {} ms after release",
                captured.samples / 16_000,
                released.elapsed().as_millis()
            )
        };
        ensure!(self.generation.current(id), CANCELLED);
        let language = if selected.language != "auto" {
            Some(selected.language.as_str())
        } else {
            leo_services::speech::catalog::catalog()
                .get(&selected.id)
                .ok()
                .filter(|m| m.languages.len() == 1)
                .and_then(|m| m.languages.first().map(String::as_str))
        };
        let text = std::panic::catch_unwind(|| {
            leo_services::dictation::text::clean(&text, settings, language)
        })
        .unwrap_or_else(|_| {
            log::warn!("Dictation's text rules failed, so the transcript is pasted as heard.");
            text.clone()
        });
        if !leo_services::dictation::text::meaningful(&text) {
            return Ok(outcome(None, None, stopped, signal));
        }
        let mut fallback = None;
        let text = if settings.writing == Writing::Off {
            text
        } else {
            self.phase(id, "writing");
            let this = self.clone();
            match leo_host::polish(
                self.usage.get().cloned().unwrap_or_default(),
                &text,
                settings.writing,
                language,
                app.as_deref(),
                &settings.custom_words,
                Arc::new(move || !this.generation.current(id)),
            ) {
                Ok(polished) => polished,
                Err(reason) => {
                    ensure!(self.generation.current(id), CANCELLED);
                    fallback = Some(reason);
                    text
                }
            }
        };
        self.set_escape(false);
        ensure!(self.generation.current(id), CANCELLED);
        if let Err(e) =
            leo_core::paths::data_dir().and_then(|data| leo_web::dictation::remember(&data, &text))
        {
            log::warn!("Dictation could not be kept in Recent dictations: {e:#}");
        }
        let output = text;
        ensure!(self.generation.current(id), CANCELLED);
        if Self::test_mode() {
            if let Some(path) = std::env::var_os("LEO_DICTATION_OUTPUT") {
                use std::io::Write;
                let path = PathBuf::from(path);
                let mut file =
                    tempfile::NamedTempFile::new_in(path.parent().context(TEST_FOLDER)?)?;
                file.write_all(output.as_bytes())?;
                file.persist(path).map_err(|e| e.error)?;
                timing();
                return Ok(Finish::Done);
            }
        }
        let pasted = crate::clipboard::paste(&self.app, &output, {
            let this = self.clone();
            Arc::new(move || this.generation.current(id))
        })
        .map_err(anyhow::Error::msg)?;
        timing();
        Ok(outcome(Some(pasted), fallback, stopped, signal))
    }
}
impl Dictation {
    fn pipeline(&self, selected: &Selected, words: &[String]) -> Result<Pipeline> {
        let spool = tempfile::Builder::new()
            .prefix(leo_services::ai::DICTATION_ROOM)
            .tempdir()?
            .keep();
        let (selected, words) = (selected.clone(), words.to_vec());
        Pipeline::new(spool, move |audio| {
            engine().transcribe(selected.clone(), audio, Priority::Dictation, words.clone())
        })
    }
    fn piece_text(
        &self,
        id: u64,
        live: Option<Live>,
        kept: Vec<f32>,
        raw: Vec<f32>,
        selected: &Selected,
        words: &[String],
    ) -> Result<Option<String>> {
        let transcript = live
            .and_then(|live| live())
            .filter(|text| !text.trim().is_empty());
        ensure!(self.generation.current(id), CANCELLED);
        Ok(Some(match transcript {
            Some(text) => text,
            None => match engine().transcribe(
                selected.clone(),
                batch_audio(kept, raw, selected.streaming),
                Priority::Dictation,
                words.to_vec(),
            ) {
                Ok(text) => text,
                Err(e) if cancelled(&e) => return Ok(None),
                Err(e) => {
                    log::warn!("Dictation could not be transcribed: {e:#}");
                    anyhow::bail!(NOT_TRANSCRIBED);
                }
            },
        }))
    }
}
impl leo_web::dictation::DictationApi for Dictation {
    fn view(&self) -> Result<Value> {
        let settings = self.preferences();
        let runtime = self.popup();
        let shortcut = self.shortcut.get().map(|s| s.view());
        let problem = self
            .problem
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let microphones = leo_services::session::mic::microphones();
        Ok(
            json!({"settings":settings,"runtime":runtime,"shortcut":shortcut,"microphones":microphones,"worker_loaded":engine().loaded(),"problem":problem,"writing_ready":leo_services::ai::writing_ready()}),
        )
    }
    fn change(&self, value: Value) -> Result<Value> {
        match value["action"].as_str() {
            Some("test_press") if Self::test_mode() => {
                self.tx
                    .try_send(Event::Input(true, false, Instant::now()))?;
            }
            Some("test_release") if Self::test_mode() => {
                self.tx
                    .try_send(Event::Input(false, false, Instant::now()))?;
            }
            Some("save") => {
                ensure!(!self.capturing.load(Ordering::Acquire), FINISH_TO_SAVE);
                let settings: Settings =
                    serde_json::from_value(value["settings"].clone()).context(UNREADABLE)?;
                settings.validate()?;
                crate::shortcut::validate(&settings.shortcut).map_err(anyhow::Error::msg)?;
                settings.commit(&self.path, || {
                    let previous = self.preferences();
                    if let Some(shortcut) = self.shortcut.get() {
                        if let Err(error) = shortcut.configure(&settings.shortcut) {
                            let _ = shortcut.configure(&previous.shortcut);
                            return Err(anyhow::anyhow!(error));
                        }
                    }
                    *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
                    Ok(())
                })?;
                *self.problem.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Some("record_shortcut") => {
                ensure!(!self.capturing.load(Ordering::Acquire), FINISH_TO_RECORD);
                self.shortcut
                    .get()
                    .context(SHORTCUTS_OFF)?
                    .record()
                    .map_err(anyhow::Error::msg)?;
            }
            Some("resume_shortcut") => self.shortcut.get().context(SHORTCUTS_OFF)?.resume(),
            Some("cancel") => self.cancel(),
            Some("copy") => {
                let text = value["text"].as_str().context(CHOOSE_RECENT)?;
                crate::clipboard::copy(&self.app, text).map_err(anyhow::Error::msg)?;
            }
            Some("theme") => {
                let choice = value["theme"].as_str().unwrap_or("system");
                ensure!(["light", "dark", "system"].contains(&choice), CHOOSE_THEME);
                *self.theme.lock().unwrap_or_else(|e| e.into_inner()) = choice.into();
                let view = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
                self.publish(view);
            }
            _ => anyhow::bail!(CHOOSE_ACTION),
        }
        Ok(json!({"ok":true}))
    }
}
fn chime(enabled: bool, start: bool, current: impl Fn() -> bool + Send + 'static) {
    if enabled {
        std::thread::spawn(move || {
            static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
            let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            if current() {
                crate::audio_feedback::play(start);
            }
        });
    }
}
fn guarded(work: impl FnOnce() -> Result<Finish>) -> Result<Finish> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(anyhow::anyhow!(STOPPED_UNEXPECTEDLY)))
}
fn batch_audio(kept: Vec<f32>, raw: Vec<f32>, streaming: bool) -> Vec<f32> {
    if kept.is_empty() && streaming {
        raw
    } else {
        kept
    }
}
fn outcome(
    pasted: Option<crate::clipboard::Pasted>,
    fallback: Option<&'static str>,
    stopped: bool,
    signal: bool,
) -> Finish {
    use crate::clipboard::Pasted;
    match (pasted, fallback) {
        (None, _) if stopped => Finish::Problem(MIC_STOPPED_EARLY.into()),
        (None, _) if !signal => Finish::Notice(MICROPHONE, SILENCE.into()),
        (None, _) => Finish::Notice(NOTHING_HEARD, NOTHING.into()),
        (Some(Pasted::Manual(message)), _) => Finish::Problem(message),
        (Some(Pasted::Unsure(message)), _) => Finish::Notice(CHECK_PASTE, message),
        (Some(Pasted::Done), Some(reason)) if stopped => {
            Finish::Notice(MICROPHONE, format!("{MIC_STOPPED} {reason}"))
        }
        (Some(Pasted::Done), None) if stopped => Finish::Notice(MICROPHONE, MIC_STOPPED.into()),
        (Some(Pasted::Done), Some(reason)) => Finish::Notice(WITHOUT_AI, reason.into()),
        (Some(Pasted::Done), None) => Finish::Done,
    }
}
fn cancelled(error: &anyhow::Error) -> bool {
    matches!(error.downcast_ref(), Some(EngineError::Cancelled))
}
fn live_text(stream: Option<DictationStream>, pending: Vec<f32>) -> Option<Live> {
    stream.map(|stream| -> Live {
        Box::new(move || {
            feed(&stream, &pending);
            stream.finish().ok()
        })
    })
}
fn feed(stream: &DictationStream, pcm: &[f32]) -> bool {
    pcm.chunks(16_000).all(|part| stream.feed(part.to_vec()))
}
fn newest(text: &str) -> String {
    let count = text.chars().count();
    text.chars()
        .skip(count.saturating_sub(SHOWN_CHARS))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::Pasted;

    fn tap(queue: &mut Queue, capturing: bool, stop: &AtomicBool) -> Action {
        let now = Instant::now();
        let action = queue.input(true, now, Mode::Tap, capturing, stop);
        assert_eq!(
            queue.input(false, now, Mode::Tap, capturing, stop),
            Action::None
        );
        action
    }

    #[test]
    fn a_press_after_cancel_is_dropped_and_never_starts_a_later_dictation() {
        let mut queue = Queue::default();
        let stop = AtomicBool::new(false);
        assert_eq!(tap(&mut queue, false, &stop), Action::Start);
        queue.busy = Some(1);
        queue.cancel();
        stop.store(true, Ordering::Release);
        assert_eq!(tap(&mut queue, false, &stop), Action::None);
        assert!(queue.queued_wants);
        assert!(!queue.complete(1, false));
        assert!(!queue.queued_wants);
        let stop = AtomicBool::new(false);
        assert_eq!(tap(&mut queue, false, &stop), Action::Start);
        queue.busy = Some(2);
        assert_eq!(tap(&mut queue, true, &stop), Action::Stop);
        assert!(stop.load(Ordering::Acquire));
        assert!(!queue.complete(2, true));
    }

    #[test]
    fn a_stop_press_after_capture_ended_on_its_own_is_swallowed() {
        let mut queue = Queue::default();
        let stop = AtomicBool::new(false);
        assert_eq!(tap(&mut queue, false, &stop), Action::Start);
        queue.busy = Some(1);
        assert_eq!(tap(&mut queue, false, &stop), Action::None);
        assert!(!queue.queued_wants);
        assert!(stop.load(Ordering::Acquire));
        assert!(!queue.complete(1, true));
    }

    #[test]
    fn a_press_while_decoding_starts_the_next_dictation() {
        let mut queue = Queue::default();
        let stop = AtomicBool::new(false);
        assert_eq!(tap(&mut queue, false, &stop), Action::Start);
        queue.busy = Some(1);
        assert_eq!(tap(&mut queue, true, &stop), Action::Stop);
        assert_eq!(tap(&mut queue, false, &stop), Action::None);
        assert!(queue.queued_wants);
        assert!(queue.complete(1, true));
        queue.busy = Some(2);
        let stop = AtomicBool::new(false);
        assert_eq!(tap(&mut queue, true, &stop), Action::Stop);
    }

    #[test]
    fn a_panic_becomes_an_error_and_audio_falls_back_to_raw_only_for_streaming() {
        let error = guarded(|| panic!("boom")).unwrap_err();
        assert_eq!(error.to_string(), STOPPED_UNEXPECTEDLY);
        assert_eq!(guarded(|| Ok(Finish::Done)).unwrap(), Finish::Done);
        assert_eq!(batch_audio(vec![1.0], vec![2.0], true), vec![1.0]);
        assert_eq!(batch_audio(Vec::new(), vec![2.0], true), vec![2.0]);
        assert!(batch_audio(Vec::new(), vec![2.0], false).is_empty());
    }

    #[test]
    fn every_ending_has_its_message() {
        assert_eq!(
            outcome(None, None, true, true),
            Finish::Problem(MIC_STOPPED_EARLY.into())
        );
        assert_eq!(
            outcome(None, None, false, false),
            Finish::Notice(MICROPHONE, SILENCE.into())
        );
        assert_eq!(
            outcome(None, Some("x"), false, true),
            Finish::Notice(NOTHING_HEARD, NOTHING.into())
        );
        assert_eq!(
            outcome(Some(Pasted::Manual("m".into())), None, true, true),
            Finish::Problem("m".into())
        );
        assert_eq!(
            outcome(Some(Pasted::Unsure("u".into())), Some("x"), true, true),
            Finish::Notice(CHECK_PASTE, "u".into())
        );
        assert_eq!(
            outcome(Some(Pasted::Done), Some("x"), true, true),
            Finish::Notice(MICROPHONE, format!("{MIC_STOPPED} x"))
        );
        assert_eq!(
            outcome(Some(Pasted::Done), None, true, true),
            Finish::Notice(MICROPHONE, MIC_STOPPED.into())
        );
        assert_eq!(
            outcome(Some(Pasted::Done), Some("x"), false, false),
            Finish::Notice(WITHOUT_AI, "x".into())
        );
        assert_eq!(outcome(Some(Pasted::Done), None, false, true), Finish::Done);
    }
}
