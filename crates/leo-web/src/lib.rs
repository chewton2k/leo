mod board_tools;
mod boards;
mod browse;
pub mod calc;
mod calendar;
mod store_refresh;
pub const CALENDAR_BYTES: u64 = 1_000_000_000;
pub mod app_settings;
pub mod captions;
pub mod chat;
pub mod chat_files;
pub mod chats;
mod combine;
pub mod db;
mod dependents;
pub mod dictation;
mod drawing;
pub mod export;
pub mod graph;
mod ics;
mod jsonline;
mod keeping;
pub mod margin;
pub mod ollama;
mod originals;
mod prefs;
mod progress;
mod read_cache;
pub mod record;
pub mod reply;
pub mod review;
mod routes;
mod routing;
pub mod search;
pub mod search_index;
pub mod sessions;
mod shutdown;
pub mod speech;
pub mod steer;
mod tasks;
#[cfg(test)]
mod tests;
mod titles;
mod token;
pub mod tools;
pub mod usage;
pub mod vectors;
mod warm;
mod work;

use std::future::IntoFuture;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use axum::http::StatusCode;
use leo_core::store::Store;

use crate::routes::auth::same_host;
use crate::routes::uploads::ImportJob;

pub use chat::{Conversation, Converser, Exchange, Instructions, Reply, Spent, Streamer, ToolSpec};
pub use graph::Writer;
pub use routes::assets::THEME;
pub use vectors::Meaning;

pub trait SettingsApi: Send + Sync {
    fn describe(&self, notes_dir: &std::path::Path) -> serde_json::Value;
    fn refresh_usage(&self, _claude: bool) -> Option<String> {
        None
    }
    fn writing(&self, _probe: bool) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn apply(&self, change: &serde_json::Value) -> Result<String>;
    fn test(&self, task: &str) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct UploadFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub path: Option<std::path::PathBuf>,
}

pub type Room = Arc<dyn Fn() -> usize + Send + Sync>;

pub struct Fetched {
    pub body: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub struct Streamed {
    pub written: bool,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

#[derive(Debug)]
pub struct TooBig;

impl std::fmt::Display for TooBig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("This calendar is larger than 1 GB, so leo did not read it.")
    }
}

impl std::error::Error for TooBig {}

pub trait CalendarAccess: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>>;
    fn fetch_into(
        &self,
        url: &str,
        etag: Option<&str>,
        modified: Option<&str>,
        sink: &mut dyn std::io::Write,
    ) -> Result<Streamed> {
        let got = self.fetch_since(url, etag, modified)?;
        if let Some(body) = &got.body {
            for piece in body.chunks(64 * 1024) {
                sink.write_all(piece)?;
            }
        }
        Ok(Streamed {
            written: got.body.is_some(),
            etag: got.etag,
            modified: got.modified,
        })
    }
    fn fetch_since(
        &self,
        url: &str,
        _etag: Option<&str>,
        _modified: Option<&str>,
    ) -> Result<Fetched> {
        Ok(Fetched {
            body: Some(self.fetch(url)?),
            etag: None,
            modified: None,
        })
    }
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, value: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

pub type Reader = Arc<dyn Fn(UploadFile, &mut dyn FnMut(&str)) -> Result<String> + Send + Sync>;

pub type ReaderTo = Arc<
    dyn Fn(
            &std::path::Path,
            &str,
            &str,
            &mut dyn FnMut(&str),
            &mut dyn std::io::Write,
        ) -> Result<()>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub place: String,
    pub path: std::path::PathBuf,
    pub photo: bool,
}

#[derive(Clone)]
pub struct Scratch(pub Arc<dyn std::any::Any + Send + Sync>);

impl std::fmt::Debug for Scratch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scratch")
    }
}

impl PartialEq for Scratch {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Made {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
    pub scratch: Option<Scratch>,
}

pub type Importer = Arc<
    dyn Fn(Vec<UploadFile>, &str, &mut dyn FnMut(&str, usize, usize)) -> Result<Made> + Send + Sync,
>;

pub type Lighter = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct Powers {
    pub writer: Option<Writer>,
    pub quick: Option<Writer>,
    pub toolless: Option<Writer>,
    pub housekeeping: Option<Writer>,
    pub usage: Arc<usage::Usage>,
    pub chat: Option<Streamer>,
    pub light_chat: Option<Streamer>,
    pub lighter: Option<Lighter>,
    pub toolless_chat: Option<Streamer>,
    pub settings: Option<Arc<dyn SettingsApi>>,
    pub speech: Option<Arc<dyn speech::SpeechApi>>,
    pub ollama: Option<Arc<dyn ollama::OllamaApi>>,
    pub dictation: Option<Arc<dyn dictation::DictationApi>>,
    pub app_settings: Option<Arc<dyn app_settings::AppApi>>,
    pub importer: Option<Importer>,
    pub listener: Option<record::Listener>,
    pub regenerator: Option<record::Regenerator>,
    pub calendar_access: Option<Arc<dyn CalendarAccess>>,
    pub housekeeper: Option<Arc<dyn record::Housekeeper>>,
    pub reader: Option<Reader>,
    pub reader_to: Option<ReaderTo>,
    pub room: Option<Room>,
    pub context: Option<Room>,
    pub converse: Option<chat::Converser>,
    pub seer: Option<captions::Seer>,
    pub meaning: Option<vectors::Meaning>,
}

pub(crate) fn stop_when_dropped() -> (leo_core::children::StopOnDrop, leo_core::children::Stop) {
    let stop = leo_core::children::Stop::default();
    (leo_core::children::StopOnDrop(stop.flag.clone()), stop)
}

pub(crate) fn no_writer() -> String {
    format!("{}no writing AI is chosen.", leo_core::NO_WRITING_AI)
}

#[derive(Clone)]
struct AppState {
    powers: Powers,
    visible: Arc<std::sync::atomic::AtomicBool>,
    shown: Arc<tokio::sync::Notify>,
    covered: Arc<std::sync::atomic::AtomicBool>,
    page_seen: Arc<std::sync::atomic::AtomicU64>,
    refresh_requested: Arc<std::sync::atomic::AtomicBool>,
    store: Arc<Mutex<Storage>>,
    gate: Arc<sessions::Gate>,
    graphs: Arc<graph::Graphs>,
    margin: Arc<margin::Reads>,
    progress: Arc<progress::Board>,
    warm: Arc<warm::Slot>,
    work: Arc<work::Work>,
    ollama_pulls: Arc<ollama::Pulls>,
    usage_refreshes: Arc<tokio::sync::Semaphore>,
    marking: Arc<tokio::sync::Semaphore>,
    regenerating: Arc<tokio::sync::Semaphore>,
    setting_tests: Arc<tokio::sync::Semaphore>,
    imports: Arc<Mutex<std::collections::HashMap<String, ImportJob>>>,
    import_slots: Arc<routes::uploads::ImportSlots>,
    zip_slots: Arc<tokio::sync::Semaphore>,
    combines: Arc<Mutex<std::collections::HashMap<String, routes::combine::CombineJob>>>,
    audio_work: Arc<Mutex<()>>,
    clearing: Arc<Mutex<()>>,
    picture_cleanup: Arc<Mutex<()>>,
    recording: record::Recordings,
    quitting: Arc<std::sync::atomic::AtomicBool>,
    capture_activity: leo_core::activity::Gate,
    discarded_recordings: Arc<Mutex<std::collections::VecDeque<String>>>,
    calendar: Arc<calendar::Calendar>,
    source_writing: Arc<tokio::sync::Mutex<()>>,
    chats: std::path::PathBuf,
    notes_dir: std::path::PathBuf,
    captions: Arc<captions::Captions>,
    originals: Arc<originals::Originals>,
    activity: Arc<Activity>,
    steering: steer::Steering,
    vectors: Arc<vectors::Vectors>,
    searched: Arc<Mutex<Option<Searched>>>,
    titles: Arc<titles::Titles>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    listed: Arc<tasks::ListedCache>,
    search_index: Arc<search_index::SearchIndex>,
}

#[derive(Clone)]
struct Searched {
    query: String,
    stamps: (u64, u64, std::sync::Weak<graph::Cache>),
    hits: Arc<Vec<(String, usize, Option<search::Why>)>>,
}

impl Powers {
    pub(crate) fn original_reader(&self) -> Option<ReaderTo> {
        self.reader_to
            .clone()
            .or_else(|| self.reader.clone().map(originals::through))
    }
}

impl std::ops::Deref for AppState {
    type Target = Powers;
    fn deref(&self) -> &Powers {
        &self.powers
    }
}

#[cfg(test)]
impl std::ops::DerefMut for AppState {
    fn deref_mut(&mut self) -> &mut Powers {
        &mut self.powers
    }
}

struct Activity {
    born: std::time::Instant,
    last: std::sync::atomic::AtomicU64,
}

impl Default for Activity {
    fn default() -> Activity {
        Activity {
            born: std::time::Instant::now(),
            last: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Activity {
    fn touch(&self) {
        let now = self.born.elapsed().as_millis() as u64;
        self.last.store(now, std::sync::atomic::Ordering::Relaxed);
    }

    fn idle(&self) -> std::time::Duration {
        let last = self.last.load(std::sync::atomic::Ordering::Relaxed);
        self.born
            .elapsed()
            .saturating_sub(std::time::Duration::from_millis(last))
    }
}

fn counts_as_use(method: &axum::http::Method, path: &str) -> bool {
    if !path.starts_with("/api/") || path == "/api/visible" {
        return false;
    }
    let polling = path == "/api/activity"
        || matches!(
            path,
            "/api/app"
                | "/api/dictation"
                | "/api/speech"
                | "/api/writing"
                | "/api/writing/ollama"
                | "/api/tasks"
                | "/api/rev"
                | "/api/processes"
        )
        || path.starts_with("/api/record/")
        || path.starts_with("/api/import/");
    !(method == axum::http::Method::GET && polling)
}

async fn note_activity(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if counts_as_use(request.method(), request.uri().path()) {
        state.activity.touch();
    }
    next.run(request).await
}

const GRAPH_CHECK: std::time::Duration = std::time::Duration::from_secs(30);
const PICTURES_PER_TICK: usize = 4;
const KEEPING_EVERY: std::time::Duration = std::time::Duration::from_secs(86_400);
const IDLE_CHECK: std::time::Duration = std::time::Duration::from_secs(30);
const GIVE_BACK_EVERY: std::time::Duration = std::time::Duration::from_secs(3_600);
const GIVE_BACK_IDLE: std::time::Duration = std::time::Duration::from_secs(600);

async fn give_back_when_idle(state: AppState) {
    let mut tick = tokio::time::interval_at(
        tokio::time::Instant::now() + GIVE_BACK_EVERY,
        GIVE_BACK_EVERY,
    );
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if state.activity.idle() < GIVE_BACK_IDLE
            || state.capture_activity.busy()
            || record::live(&state.recording)
        {
            continue;
        }
        let chats = state.chats.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _stop_on_drop = leo_core::children::StopOnDrop(Arc::clone(&stop));
        let asked = Arc::clone(&stop);
        let mut work = tokio::task::spawn_blocking(move || {
            let db = db::try_for_chats(&chats)?;
            let _ = db.rewrite(&asked);
            db.give_back()
        });
        loop {
            tokio::select! {
                _ = &mut work => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    if state.capture_activity.busy() || record::live(&state.recording) {
                        stop.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
    }
}

async fn keep_search_current(state: AppState) {
    let index = Arc::clone(&state.search_index);
    loop {
        while state.capture_activity.busy() || record::live(&state.recording) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let asked = Arc::clone(&index);
        let work = state
            .read_store(move |store| Ok(asked.stale(store, search_index::MOST_COPIED)))
            .await;
        let applied = match work {
            Ok(work) => {
                let busy = work.busy();
                let index = Arc::clone(&index);
                match tokio::task::spawn_blocking(move || index.apply(work)).await {
                    Ok(Ok(())) if busy => continue,
                    Ok(Ok(())) => true,
                    _ => false,
                }
            }
            Err(_) => false,
        };
        if applied {
            index.wake.notified().await;
        } else {
            tokio::time::sleep(search_index::RETRY).await;
        }
    }
}

const DEPENDENTS_IDLE: std::time::Duration = std::time::Duration::from_secs(60);

fn cleanup_idle(state: &AppState) -> bool {
    state.activity.idle() >= DEPENDENTS_IDLE
        && !state.capture_activity.busy()
        && !record::live(&state.recording)
        && !state.quitting.load(std::sync::atomic::Ordering::Acquire)
}

async fn keep_dependents_current(state: AppState) {
    let mut tick = tokio::time::interval(IDLE_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if !cleanup_idle(&state) {
            continue;
        }
        let cleanup = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if cleanup_idle(&cleanup) {
                keeping::expire_dependents(&cleanup);
            }
        })
        .await;
    }
}

async fn keep_graph_current(state: AppState) {
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + GRAPH_CHECK, GRAPH_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut read_at = None;
    let mut graphed_at = None;
    let mut stuck = None;
    let mut swept_at = None;
    let mut planned = None;
    let mut captions_at = None;
    let mut captions_seen = 0;
    let mut more_pictures = false;
    loop {
        tick.tick().await;
        if cleanup_idle(&state) && margin::sweep_due(swept_at) {
            let observed = margin::sweep_credit(&mut swept_at, std::time::Instant::now());
            if let Ok(Some(kept)) = state
                .read_store(|store| Ok(margin::sweep_kept(store)))
                .await
            {
                let swept = state.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let audio = swept
                        .housekeeper
                        .as_ref()
                        .map(|housekeeper| housekeeper.kept_for_notes())
                        .unwrap_or_default();
                    let data = swept.notes_dir.parent().unwrap_or(&swept.notes_dir);
                    let gone = margin::sweep_store_at(&swept.chats, kept, &audio, data, observed);
                    if !gone.is_empty() && cleanup_idle(&swept) {
                        dependents::clear(&swept, gone, dependents::Why::Swept);
                    }
                })
                .await;
            } else {
                let chats = state.chats.clone();
                let _ = tokio::task::spawn_blocking(move || margin::reset_missing(&chats)).await;
            }
        }
        if !state.visible.load(std::sync::atomic::Ordering::Acquire)
            || state.covered.load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = tokio::time::timeout(margin::SWEEP_EVERY, state.shown.notified()).await;
            continue;
        }
        if state.capture_activity.busy() {
            continue;
        }
        let Ok(seen) = state.read_store(|store| Ok(store.fingerprint())).await else {
            continue;
        };
        if read_at != Some(seen) {
            if let Some(meaning) = state.meaning.clone().filter(|m| m(&[], false).is_ok()) {
                let vectors = Arc::clone(&state.vectors);
                let listed = Arc::clone(&vectors);
                if let Ok((work, more)) = state
                    .read_store(move |store| {
                        let work = listed.stale(store, vectors::PIECES_PER_TICK);
                        listed.forget_gone(store);
                        Ok((work, listed.looking()))
                    })
                    .await
                {
                    if work.is_empty() {
                        if !more {
                            read_at = Some(seen);
                        }
                    } else {
                        let _ = tokio::task::spawn_blocking(move || {
                            vectors::read_in(&vectors, &meaning, &work)
                        })
                        .await;
                    }
                }
            }
        }
        let idle = state.activity.idle();
        if idle >= graph::UPDATE_WHEN_IDLE && state.vectors.near_pending() {
            let graphs = Arc::clone(&state.graphs);
            let _ = tokio::task::spawn_blocking(move || graphs.near()).await;
        }
        if idle < graph::UPDATE_WHEN_IDLE
            || graphed_at == Some(seen)
            || stuck.is_some_and(|(at, when): (_, std::time::Instant)| {
                at == seen && when.elapsed() < graph::RETRY_AUTO_AFTER
            })
        {
            continue;
        }
        let (mut settled, mut busy) = if state.graphs.writer().is_none() {
            (true, state.graphs.building().is_some())
        } else {
            let key = (seen, state.graphs.inputs());
            let plan = match planned
                .as_ref()
                .filter(|(at, _): &&(_, graph::Status)| *at == key)
            {
                Some((_, plan)) => plan.clone(),
                None => {
                    let Ok(sources) = state
                        .read_store(|store| Ok(graph::sources(&store.notes)))
                        .await
                    else {
                        continue;
                    };
                    let graphs = Arc::clone(&state.graphs);
                    let Ok(plan) =
                        tokio::task::spawn_blocking(move || graphs.status(&sources)).await
                    else {
                        continue;
                    };
                    planned = Some((key, plan.clone()));
                    plan
                }
            };
            let status = state.graphs.live(&plan);
            if state.graphs.due(&status, idle) {
                let Ok(sources) = state
                    .read_store(|store| Ok(graph::sources(&store.notes)))
                    .await
                else {
                    continue;
                };
                state.graphs.begin(sources);
                (false, true)
            } else {
                (
                    state.graphs.settled_by(&status),
                    state.graphs.building().is_some(),
                )
            }
        };
        if let Some(seer) = state.seer.clone() {
            let captions = Arc::clone(&state.captions);
            let at = (seen, captions.generation());
            let (paths, forget) = if captions_at != Some(at) || more_pictures {
                let listed = Arc::clone(&captions);
                let Ok((next, paths, forget)) = state
                    .read_store(move |store| {
                        let (next, changed) = store.changes_since(captions_seen);
                        Ok((
                            next,
                            captions::uncaptioned(store, &listed, PICTURES_PER_TICK, &changed),
                            captions::may_forget(store),
                        ))
                    })
                    .await
                else {
                    continue;
                };
                captions_seen = next;
                captions_at = Some(at);
                more_pictures = !paths.is_empty();
                (paths, forget)
            } else {
                (Vec::new(), false)
            };
            if forget {
                let stale = Arc::clone(&captions);
                let _ = tokio::task::spawn_blocking(move || stale.forget_stale()).await;
            }
            if !paths.is_empty() {
                settled = false;
                busy |= tokio::task::spawn_blocking(move || {
                    captions::caption_paths(&paths, &captions, &seer)
                })
                .await
                .is_ok_and(|done| done > 0);
            }
        }
        if settled && !state.captions.waiting() {
            graphed_at = Some(seen);
        } else if !busy {
            stuck = Some((seen, std::time::Instant::now()));
        }
    }
}

struct Storage {
    store: Store,
    reload: bool,
    changes: Option<store_refresh::Changes>,
}

impl std::ops::Deref for Storage {
    type Target = Store;
    fn deref(&self) -> &Store {
        &self.store
    }
}

impl std::ops::DerefMut for Storage {
    fn deref_mut(&mut self) -> &mut Store {
        &mut self.store
    }
}

fn lock_storage(store: &Mutex<Storage>) -> MutexGuard<'_, Storage> {
    store.lock().unwrap_or_else(|poisoned| {
        store.clear_poison();
        let mut storage = poisoned.into_inner();
        storage.reload = true;
        storage
    })
}

impl AppState {
    async fn with_store<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        self.access_store(true, work).await
    }

    async fn read_store<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        self.access_store(false, move |store| work(store)).await
    }

    async fn access_store<R: Send + 'static>(
        &self,
        writing: bool,
        work: impl FnOnce(&mut Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || store_access(&state, writing, work))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }

    #[cfg(test)]
    fn fresh(&self) -> MutexGuard<'_, Storage> {
        let mut store = lock_storage(&self.store);
        store.trash_may_have_changed();
        if store.reload || store.changed_on_disk() {
            if store.reload {
                store.forget_changes();
            }
            store.refresh().unwrap();
            store.reload = false;
        }
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        store
    }
}

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub port: u16,
    pub token: String,
}

pub(crate) struct Places {
    pub(crate) store: Store,
    pub(crate) sessions: std::path::PathBuf,
}

pub struct Running {
    port: u16,
    token: String,
    state: AppState,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    background: Background,
    grace: std::time::Duration,
}

struct Background {
    graph: tokio::task::JoinHandle<()>,
    idle: tokio::task::JoinHandle<()>,
    expiry: tokio::task::JoinHandle<()>,
    dependents: tokio::task::JoinHandle<()>,
    calendar: tokio::task::JoinHandle<()>,
    give_back: tokio::task::JoinHandle<()>,
    search: tokio::task::JoinHandle<()>,
}

impl Background {
    fn stop(&self) {
        self.graph.abort();
        self.idle.abort();
        self.expiry.abort();
        self.dependents.abort();
        self.calendar.abort();
        self.give_back.abort();
        self.search.abort();
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Running {
    pub fn set_visible(&self, visible: bool) -> bool {
        let changed = self
            .state
            .visible
            .swap(visible, std::sync::atomic::Ordering::AcqRel)
            != visible;
        if changed && visible {
            self.state
                .refresh_requested
                .store(true, std::sync::atomic::Ordering::Release);
            self.state
                .covered
                .store(false, std::sync::atomic::Ordering::Release);
            self.state.shown.notify_one();
        }
        changed
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn sign_in_url(&self) -> String {
        format!("http://127.0.0.1:{}/?token={}", self.port, self.token)
    }

    /// What Felix and leo's other AI work used since leo started, for the menu
    /// bar: "12 AI requests · 340k tokens (210k cached)".
    pub fn usage_line(&self) -> String {
        self.state.usage.line()
    }

    pub fn recording_live(&self) -> bool {
        record::live(&self.state.recording)
    }

    pub async fn stop(self) {
        let Running {
            state,
            stop,
            server,
            background,
            grace,
            ..
        } = self;
        let deadline = tokio::time::Instant::now() + grace;
        let worker =
            record::stop_for_quit(&state).map(|h| tokio::task::spawn_blocking(move || h.join()));
        background.stop();
        let _ = stop.send(());
        shutdown::drain(grace, server).await;
        if let Some(worker) = worker {
            let _ = tokio::time::timeout_at(deadline, worker).await;
        }
        drop(background);
        let warm = Arc::clone(&state.warm);
        drop(state);
        let cleanup = tokio::task::spawn_blocking(move || warm.clear());
        let _ = tokio::time::timeout(shutdown::CLEANUP, cleanup).await;
    }
}

pub fn fresh_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn saved_token() -> Result<String> {
    token::load_or_create(&leo_core::paths::config_dir()?.join("serve-token"), false)
}

pub async fn start(options: StartOptions, powers: Powers) -> Result<Running> {
    let places = Places {
        store: Store::load()?,
        sessions: leo_core::paths::config_dir()?.join("app-sessions.json"),
    };
    start_in(places, options, powers).await
}

pub(crate) async fn start_in(
    places: Places,
    options: StartOptions,
    powers: Powers,
) -> Result<Running> {
    let store = places.store;
    let notes = store.notes_dir.clone();
    let _db = db::try_at(
        &store
            .notes_dir
            .parent()
            .unwrap_or(&store.notes_dir)
            .join(db::FILE),
    )?;
    let swept = notes.clone();
    tokio::task::spawn_blocking(move || sweep_leftover_temps(&swept));
    let live: std::collections::HashSet<String> =
        store.notes.iter().map(|n| n.id.clone()).collect();
    let vectors = Arc::new(vectors::Vectors::for_live(&store.notes_dir, &live));
    let graphs = Arc::new(
        graph::Graphs::for_notes(&store.notes_dir, powers.writer.clone())
            .with_room(powers.room.clone())
            .with_vectors(Arc::clone(&vectors)),
    );
    let chats = chats::dir_for(&store.notes_dir);
    powers.usage.save_in(chats.clone());
    let captions = Arc::new(captions::Captions::for_notes(&store.notes_dir));
    let (listener, port) = bind(options.port).await?;
    let gate = Arc::new(sessions::Gate::new(
        options.token.clone(),
        sessions::Sessions::load(&places.sessions)?,
    ));
    let state = AppState {
        powers,
        margin: Default::default(),
        visible: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        shown: Default::default(),
        covered: Default::default(),
        page_seen: Default::default(),
        refresh_requested: Default::default(),
        store: Arc::new(Mutex::new(Storage {
            changes: Some(store_refresh::Changes::new(&notes)),
            store,
            reload: false,
        })),
        gate: Arc::clone(&gate),
        graphs,
        progress: Arc::new(progress::Board::kept_in(&chats)),
        warm: Default::default(),
        work: Default::default(),
        ollama_pulls: Default::default(),
        usage_refreshes: Arc::new(tokio::sync::Semaphore::new(1)),
        marking: Arc::new(tokio::sync::Semaphore::new(2)),
        regenerating: Arc::new(tokio::sync::Semaphore::new(2)),
        setting_tests: Arc::new(tokio::sync::Semaphore::new(1)),
        imports: Default::default(),
        import_slots: Default::default(),
        zip_slots: routes::downloads::zip_slots(),
        combines: Default::default(),
        audio_work: Default::default(),
        clearing: Default::default(),
        picture_cleanup: Default::default(),
        recording: Default::default(),
        quitting: Default::default(),
        capture_activity: leo_core::activity::global().clone(),
        discarded_recordings: Default::default(),
        calendar: Default::default(),
        source_writing: Default::default(),
        chats,
        notes_dir: notes.clone(),
        captions,
        originals: Default::default(),
        activity: Default::default(),
        steering: Default::default(),
        vectors,
        searched: Default::default(),
        titles: Default::default(),
        generation: Default::default(),
        listed: Default::default(),
        search_index: Arc::new(search_index::SearchIndex::for_notes(&notes)),
    };

    tokio::task::spawn_blocking({
        let notes = notes.clone();
        move || routes::uploads::sweep_incoming(&notes)
    });

    let _ = tokio::task::spawn_blocking({
        let state = state.clone();
        move || {
            dependents::recover_holds(&state);
            routes::combine::reconcile_retargets(&state, true);
        }
    })
    .await;

    let expiry = tokio::spawn({
        let state = state.clone();
        async move {
            let mut daily = tokio::time::interval(KEEPING_EVERY);
            loop {
                daily.tick().await;
                keeping::expire_choices(&state).await;
            }
        }
    });

    let background = Background {
        graph: tokio::spawn(keep_graph_current(state.clone())),
        idle: tokio::spawn(stop_idle_sessions(Arc::clone(&state.warm))),
        expiry,
        dependents: tokio::spawn(keep_dependents_current(state.clone())),
        calendar: tokio::spawn(calendar::offer_to_record(state.clone())),
        give_back: tokio::spawn(give_back_when_idle(state.clone())),
        search: tokio::spawn(keep_search_current(state.clone())),
    };
    let app = routes::router(state.clone()).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            same_host(port, request, next)
        },
    ));
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .into_future(),
    );
    Ok(Running {
        port,
        token: options.token,
        state,
        stop,
        server,
        background,
        grace: shutdown::GRACE,
    })
}

fn sweep_leftover_temps(notes: &std::path::Path) -> usize {
    use leo_core::paths::{own, sweep_stale_temps};
    let data = db::beside_of(notes, None);
    sweep_stale_temps(&notes.join(".leo"), 1)
        + sweep_stale_temps(data, 0)
        + sweep_stale_temps(&data.join(own::ORIGINALS), 2)
        + sweep_stale_temps(&data.join(own::SOURCES), 1)
        + sweep_stale_temps(&data.join(own::CALENDAR), 0)
}

#[cfg(test)]
mod leftover_temps {
    #[test]
    fn old_temporary_files_go_from_every_folder_leo_writes() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let old = [
            "notes/.leo/tasks/.tmpAbC123",
            "notes/.leo/boards/.tmpAbC123",
            "notes/.leo/cards/.tmpAbC123",
            ".tmpAbC123",
            "attachments/note/.tmpAbC123",
            "attachments/note/.read/.tmpAbC123",
            "recording-sources/note/.tmpAbC123",
            "calendar/.tmpAbC123",
        ];
        let kept = [
            "notes/.tmpAbC123",
            "notes/.leo/tasks/task-0001.json",
            "attachments/note/slides.pdf",
            "recording-sources/note/.tmpAbC12",
            "models/.tmpAbC123",
        ];
        for name in old.iter().chain(&kept) {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
                .unwrap();
        }
        assert_eq!(super::sweep_leftover_temps(&notes), old.len());
        for name in old {
            assert!(!dir.path().join(name).exists(), "{name}");
        }
        for name in kept {
            assert!(dir.path().join(name).exists(), "{name}");
        }
    }
}

async fn bind(wanted: u16) -> Result<(tokio::net::TcpListener, u16)> {
    let mut last = None;
    for port in wanted..wanted.saturating_add(20) {
        if port != 0 {
            let probe = tokio::net::TcpSocket::new_v4()?;
            if let Err(e) = probe.bind(std::net::SocketAddr::from(([0, 0, 0, 0], port))) {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    last = Some(e);
                    continue;
                }
            }
        }
        match tokio::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], port)))
            .await
        {
            Ok(listener) => {
                let port = listener.local_addr()?.port();
                return Ok((listener, port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "ports {wanted} to {} are all in use ({})",
        wanted.saturating_add(19),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

async fn stop_idle_sessions(warm: Arc<warm::Slot>) {
    let mut every = tokio::time::interval(IDLE_CHECK);
    loop {
        every.tick().await;
        if !warm.holding() {
            continue;
        }
        let warm = Arc::clone(&warm);
        let _ = tokio::task::spawn_blocking(move || warm.tidy()).await;
    }
}

fn store_access<R>(
    state: &AppState,
    writing: bool,
    work: impl FnOnce(&mut Store) -> Result<R, StatusCode>,
) -> Result<R, StatusCode> {
    let mut store = lock_storage(&state.store);
    let force = state
        .refresh_requested
        .swap(false, std::sync::atomic::Ordering::AcqRel);
    let check = force
        || store.changes.as_mut().is_none_or(|changes| changes.due())
        || (writing && !quiet_watcher_trusted());
    if store.reload || (check && store.changed_on_disk()) {
        if store.reload {
            store.forget_changes();
        }
        store
            .refresh()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.reload = false;
        state
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        state.search_index.wake.notify_one();
    }
    if store
        .changes
        .as_ref()
        .is_none_or(|changes| changes.trash_changed())
    {
        store.trash_may_have_changed();
    }
    let result = work(&mut store);
    if writing {
        state
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        state.search_index.wake.notify_one();
    }
    if writing
        && result
            .as_ref()
            .is_err_and(|status| !status.is_client_error())
    {
        store.reload = true;
    }
    result
}

fn quiet_watcher_trusted() -> bool {
    cfg!(not(windows))
}

fn store_now<R>(
    state: &AppState,
    work: impl FnOnce(&mut Store) -> Result<R, StatusCode>,
) -> Result<R, StatusCode> {
    state
        .refresh_requested
        .store(true, std::sync::atomic::Ordering::Release);
    store_access(state, true, work)
}

fn read_now<R>(
    state: &AppState,
    work: impl FnOnce(&Store) -> Result<R, StatusCode>,
) -> Result<R, StatusCode> {
    store_access(state, false, |store| work(store))
}

// ── Query params ──────────────────────────────────────────────────────────

// ── Request bodies ────────────────────────────────────────────────────────

// ── Response types ────────────────────────────────────────────────────────

// ── Handlers ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn polling_does_not_count_as_someone_using_leo() {
        use axum::http::Method;
        assert!(counts_as_use(&Method::PATCH, "/api/notes/abc"));
        assert!(counts_as_use(&Method::GET, "/api/notes"));
        assert!(counts_as_use(&Method::POST, "/api/record/r1/stop"));
        for polled in [
            "/api/app",
            "/api/dictation",
            "/api/speech",
            "/api/writing",
            "/api/writing/ollama",
            "/api/tasks",
            "/api/rev",
            "/api/processes",
            "/api/activity",
            "/api/record/r1",
            "/api/import/j1",
        ] {
            assert!(!counts_as_use(&Method::GET, polled), "{polled}");
        }
        assert!(!counts_as_use(&Method::GET, "/app.js"));
    }

    #[test]
    fn idle_time_restarts_whenever_leo_is_used() {
        let activity = Activity::default();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(activity.idle() >= std::time::Duration::from_millis(30));
        activity.touch();
        assert!(activity.idle() < std::time::Duration::from_millis(20));
    }
}

#[cfg(test)]
mod bind_tests {
    use super::*;

    #[tokio::test]
    async fn a_port_held_on_every_address_is_skipped() {
        let held = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let busy = held.local_addr().unwrap().port();
        let (listener, port) = bind(busy).await.unwrap();
        assert_ne!(port, busy);
        assert_eq!(
            listener.local_addr().unwrap().ip(),
            std::net::Ipv4Addr::LOCALHOST
        );
    }
}
