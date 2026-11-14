mod board_tools;
mod boards;
mod browse;
pub mod calc;
mod calendar;
mod store_refresh;
pub const CALENDAR_BYTES: u64 = calendar::MOST_BYTES;
pub mod app_settings;
pub mod captions;
pub mod chat;
pub mod chat_files;
pub mod chats;
mod combine;
pub mod db;
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
pub mod sessions;
mod shutdown;
pub mod speech;
pub mod steer;
mod tasks;
#[cfg(test)]
mod tests;
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
}

pub type Room = Arc<dyn Fn() -> usize + Send + Sync>;

pub struct Fetched {
    pub body: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub trait CalendarAccess: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>>;
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

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub place: String,
    pub bytes: Vec<u8>,
    pub photo: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Made {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
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
    pub room: Option<Room>,
    pub context: Option<Room>,
    pub converse: Option<chat::Converser>,
    pub seer: Option<captions::Seer>,
    pub meaning: Option<vectors::Meaning>,
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
    imports: Arc<Mutex<std::collections::HashMap<String, ImportJob>>>,
    import_slots: Arc<routes::uploads::ImportSlots>,
    zip_slots: Arc<tokio::sync::Semaphore>,
    combines: Arc<Mutex<std::collections::HashMap<String, routes::combine::CombineJob>>>,
    recording: record::Recordings,
    capture_activity: leo_core::activity::Gate,
    discarded_recordings: Arc<Mutex<std::collections::VecDeque<String>>>,
    calendar: Arc<calendar::Calendar>,
    source_writing: Arc<std::sync::atomic::AtomicBool>,
    chats: std::path::PathBuf,
    captions: Arc<captions::Captions>,
    originals: Arc<originals::Originals>,
    activity: Arc<Activity>,
    steering: steer::Steering,
    vectors: Arc<vectors::Vectors>,
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

async fn keep_graph_current(state: AppState) {
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + GRAPH_CHECK, GRAPH_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut read_at = None;
    let mut graphed_at = None;
    let mut stuck = None;
    let mut swept_at = None;
    loop {
        tick.tick().await;
        if margin::sweep_due(swept_at) {
            swept_at = Some(std::time::Instant::now());
            let chats = state.chats.clone();
            let _ = state
                .read_store(move |store| Ok(margin::sweep_store(&chats, store)))
                .await;
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
                if let Ok(work) = state
                    .read_store(move |store| {
                        let work = listed.stale(store, vectors::PIECES_PER_TICK);
                        listed.forget_gone(store);
                        Ok(work)
                    })
                    .await
                {
                    if work.is_empty() {
                        read_at = Some(seen);
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
        if idle < graph::UPDATE_WHEN_IDLE
            || graphed_at == Some(seen)
            || stuck.is_some_and(|(at, when): (_, std::time::Instant)| {
                at == seen && when.elapsed() < graph::RETRY_AUTO_AFTER
            })
        {
            continue;
        }
        let Ok(sources) = state
            .read_store(|store| Ok(graph::sources(&store.notes)))
            .await
        else {
            continue;
        };
        let graphs = Arc::clone(&state.graphs);
        let Ok((mut settled, mut busy)) = tokio::task::spawn_blocking(move || {
            let started = graphs.update_if_due(sources.clone(), idle);
            (
                !started && graphs.settled(&sources),
                started || graphs.building().is_some(),
            )
        })
        .await
        else {
            continue;
        };
        if let Some(seer) = state.seer.clone() {
            let captions = Arc::clone(&state.captions);
            let listed = Arc::clone(&captions);
            let Ok((paths, forget)) = state
                .read_store(move |store| {
                    Ok((
                        captions::uncaptioned(store, &listed, PICTURES_PER_TICK),
                        captions::may_forget(store),
                    ))
                })
                .await
            else {
                continue;
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
        tokio::task::spawn_blocking(move || {
            let mut store = lock_storage(&state.store);
            let force = state
                .refresh_requested
                .swap(false, std::sync::atomic::Ordering::AcqRel);
            let check =
                writing || force || store.changes.as_mut().is_none_or(|changes| changes.due());
            if store.reload || (check && store.changed_on_disk()) {
                store
                    .refresh()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store.reload = false;
            }
            let result = work(&mut store);
            if writing
                && result
                    .as_ref()
                    .is_err_and(|status| !status.is_client_error())
            {
                store.reload = true;
            }
            result
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }

    #[cfg(test)]
    fn fresh(&self) -> MutexGuard<'_, Storage> {
        let mut store = lock_storage(&self.store);
        if store.reload || store.changed_on_disk() {
            store.refresh().unwrap();
            store.reload = false;
        }
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
    calendar: tokio::task::JoinHandle<()>,
}

impl Background {
    fn stop(&self) {
        self.graph.abort();
        self.idle.abort();
        self.expiry.abort();
        self.calendar.abort();
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
        background.stop();
        let _ = stop.send(());
        shutdown::drain(grace, server).await;
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
    let vectors = Arc::new(vectors::Vectors::for_notes(&store.notes_dir));
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
        imports: Default::default(),
        import_slots: Default::default(),
        zip_slots: routes::downloads::zip_slots(),
        combines: Default::default(),
        recording: Default::default(),
        capture_activity: leo_core::activity::global().clone(),
        discarded_recordings: Default::default(),
        calendar: Default::default(),
        source_writing: Default::default(),
        chats,
        captions,
        originals: Default::default(),
        activity: Default::default(),
        steering: Default::default(),
        vectors,
    };

    let expiry = tokio::spawn({
        let state = state.clone();
        async move {
            let mut daily = tokio::time::interval(KEEPING_EVERY);
            loop {
                daily.tick().await;
                keeping::expire_now(&state).await;
            }
        }
    });

    let background = Background {
        graph: tokio::spawn(keep_graph_current(state.clone())),
        idle: tokio::spawn(stop_idle_sessions(Arc::clone(&state.warm))),
        expiry,
        calendar: tokio::spawn(calendar::offer_to_record(state.clone())),
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

fn store_now<R>(
    state: &AppState,
    work: impl FnOnce(&mut Store) -> Result<R, StatusCode>,
) -> Result<R, StatusCode> {
    let mut store = lock_storage(&state.store);
    if store.reload || store.changed_on_disk() {
        store
            .refresh()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.reload = false;
    }
    let result = work(&mut store);
    if result
        .as_ref()
        .is_err_and(|status| !status.is_client_error())
    {
        store.reload = true;
    }
    result
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
