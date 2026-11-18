use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::reply::error;
use crate::routes::notes::{directory as valid_directory, save};
use crate::{store_now, AppState};

pub const MOST_TRANSCRIPT_CHARS: usize = 200_000;
pub const LEVELS_KEPT: usize = 48;
const DISCARDS_KEPT: usize = 1000;
const WAIT_FOR_WRITES: Duration = Duration::from_secs(5);
const MOST_WARNINGS: usize = 40;
const WARNING_CHARS: usize = 2_000;
const SLIDE_TEXT_CHARS: usize = 6500;
const NOTE_GONE: &str = "The note this recording belongs to is not in your notes; restore it from the trash, then recover again.";
pub const RECORD_PAGE: usize = 50;
pub const VIEW_POINTS: usize = 50;
const STILL_SAVING: &str =
    "The point is kept in this recording and is still being saved; it shows in the list once it is.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Nativecall,
    Microphone,
    Screen,
}

impl Source {
    pub fn is_sound(self) -> bool {
        self == Source::Screen
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: String,
    pub title: Option<String>,
    pub directory: String,
    pub source: Source,
    pub profile: leo_core::recording::Profile,
    #[serde(default)]
    pub started: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Checks {
    pub speech: bool,
    pub writing: bool,
    pub headset: bool,
}

impl Default for Checks {
    fn default() -> Self {
        Checks {
            speech: true,
            writing: true,
            headset: false,
        }
    }
}

pub trait Housekeeper: Send + Sync {
    fn checks(&self) -> Checks {
        Checks::default()
    }
    fn pending_recordings(&self) -> anyhow::Result<Vec<Pending>> {
        Ok(Vec::new())
    }
    fn pending_page(
        &self,
        _after: Option<&str>,
        _limit: usize,
    ) -> anyhow::Result<(Vec<Pending>, Option<String>)> {
        Ok((self.pending_recordings()?, None))
    }
    fn pending_recording(&self, id: &str) -> anyhow::Result<Option<Pending>> {
        Ok(self.pending_recordings()?.into_iter().find(|p| p.id == id))
    }
    fn old_recordings(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<Pending>> {
        Ok(self
            .pending_recordings()?
            .into_iter()
            .filter(|p| p.started.is_some_and(|started| started < before))
            .collect())
    }
    fn discard_recording(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn discard_recordings(&self, ids: &[String]) -> usize {
        ids.iter()
            .filter(|id| self.discard_recording(id).is_ok())
            .count()
    }
    fn kept_audio_count(&self, _before: chrono::DateTime<chrono::Utc>) -> usize {
        0
    }
    fn expire_kept_audio(&self, _before: chrono::DateTime<chrono::Utc>) -> usize {
        0
    }
    fn discard_kept_for_notes(&self, _notes: &[String]) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn kept_for_notes(&self) -> Vec<String> {
        Vec::new()
    }
    fn retarget_kept(&self, _from: &str, _target: &str) -> anyhow::Result<Vec<Moved>> {
        Ok(Vec::new())
    }
    fn restore_kept(&self, _moved: &[Moved]) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub session: String,
    pub previous: Option<String>,
    pub target: String,
}

pub struct Listening {
    pub preview: Arc<AtomicBool>,
    pub recover: Option<String>,
    pub id: String,
    pub profile: leo_core::recording::Profile,
    pub call: bool,
    pub screen: bool,
    pub directory: String,
    pub title: Option<String>,
    pub stop: Arc<AtomicBool>,
    pub finish_now: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<(u64, String)>>>,
    pub saved_points: Arc<AtomicUsize>,
    pub wants: Arc<Mutex<Option<String>>>,
}

pub enum Heard {
    Clock {
        secs: u64,
        paused: bool,
        level: f32,
    },
    Step {
        label: String,
        steps: Option<(usize, usize)>,
    },
    Transcript(String),
    Warning(String),
}

pub type Commit = Arc<dyn Fn(&std::path::Path, &str) -> Result<()> + Send + Sync>;
pub type Regenerator = Arc<
    dyn Fn(
            Vec<leo_core::recording::Archive>,
            leo_core::recording::Profile,
        ) -> Result<(String, String)>
        + Send
        + Sync,
>;

#[derive(Default)]
pub struct Recorded {
    pub title: String,
    pub body: String,
    pub source: Option<leo_core::recording::Archive>,
    pub commit: Option<Commit>,
    pub writer_failed: bool,
    pub into: Option<String>,
}

pub type Listener = Arc<dyn Fn(Listening, &mut dyn FnMut(Heard)) -> Result<Recorded> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct RecordView {
    pub id: String,
    pub session: String,
    pub source: Source,
    pub state: &'static str,
    pub secs: u64,
    pub step: String,
    pub steps: Option<(usize, usize)>,
    pub transcript: String,
    pub warnings: Vec<String>,
    pub points: Vec<(u64, String)>,
    pub points_total: usize,
    pub wants: String,
    pub levels: Vec<f32>,
    pub levels_start: u64,
    pub note: Option<String>,
    pub error: Option<String>,
    pub writer_failed: bool,
}

impl RecordView {
    fn warn(&mut self, text: String) {
        let text: String = text.chars().take(WARNING_CHARS).collect();
        if self.warnings.last() == Some(&text) {
            return;
        }
        if self.warnings.len() >= MOST_WARNINGS {
            self.warnings.remove(0);
        }
        self.warnings.push(text);
    }

    fn push_level(&mut self, level: f32) {
        self.levels
            .push((level.clamp(0.0, 1.0) * 1000.0).round() / 1000.0);
        let extra = self.levels.len().saturating_sub(LEVELS_KEPT);
        self.levels.drain(..extra);
        self.levels_start += extra as u64;
    }
}

pub(crate) struct RecordJob {
    view: RecordView,
    writing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    finish_now: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    points: Arc<Mutex<Vec<(u64, String)>>>,
    saved_points: Arc<AtomicUsize>,
    wants: Arc<Mutex<Option<String>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

struct Writing(Arc<AtomicBool>);
impl Writing {
    fn take(job: &RecordJob) -> anyhow::Result<Self> {
        if job.writing.swap(true, Ordering::AcqRel) {
            anyhow::bail!("Audio or a point is being saved. Try again in a moment.");
        }
        Ok(Self(job.writing.clone()))
    }
}
impl Drop for Writing {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl RecordJob {
    pub(crate) fn view(&self) -> RecordView {
        self.view.clone()
    }

    pub(crate) fn session_id(&self) -> String {
        self.view.session.clone()
    }

    #[cfg(test)]
    pub(crate) fn writing(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.writing)
    }

    fn live(&self) -> bool {
        matches!(self.view.state, "starting" | "recording" | "paused")
    }

    pub(crate) fn busy(&self) -> bool {
        self.live() || self.view.state == "writing"
    }

    fn end(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.live() {
            self.view.state = "writing";
            self.view.step = "Transcribing the recording".into();
        }
    }
}

pub(crate) type Recordings = Arc<Mutex<Option<RecordJob>>>;

pub(crate) fn stop_for_quit(state: &AppState) -> Option<std::thread::JoinHandle<()>> {
    let mut held = state.recording.lock().unwrap_or_else(|e| e.into_inner());
    state.quitting.store(true, Ordering::Release);
    let job = held.as_mut()?;
    job.stop.store(true, Ordering::Relaxed);
    job.worker.take()
}

pub(crate) fn discard_unless_in_use(
    recordings: &Recordings,
    housekeeper: &dyn Housekeeper,
    session: &str,
) -> Option<anyhow::Result<()>> {
    let held = recordings.lock().unwrap_or_else(|e| e.into_inner());
    if held.as_ref().is_some_and(|job| job.view.session == session) {
        return None;
    }
    Some(housekeeper.discard_recording(session))
}

pub(crate) fn discard_all_unless_in_use(
    recordings: &Recordings,
    housekeeper: &dyn Housekeeper,
    mut ids: Vec<String>,
) -> usize {
    let held = recordings.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(job) = held.as_ref() {
        ids.retain(|id| *id != job.view.session);
    }
    housekeeper.discard_recordings(&ids)
}

pub(crate) fn live(recordings: &Recordings) -> bool {
    recordings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(RecordJob::live)
}

async fn writes_done(state: &AppState, id: &str) {
    let until = Instant::now() + WAIT_FOR_WRITES;
    while Instant::now() < until
        && with_job(state, id, |job| job.writing.load(Ordering::Acquire)).unwrap_or(false)
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn with_job<R>(
    state: &AppState,
    id: &str,
    work: impl FnOnce(&mut RecordJob) -> R,
) -> Result<R, (StatusCode, &'static str)> {
    let mut held = state.recording.lock().unwrap_or_else(|e| e.into_inner());
    match held.as_mut() {
        Some(job) if job.view.id == id => Ok(work(job)),
        _ => Err((StatusCode::NOT_FOUND, "That recording is over.")),
    }
}

fn change(recordings: &Recordings, id: &str, work: impl FnOnce(&mut RecordJob)) {
    if let Some(job) = recordings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .filter(|j| j.view.id == id)
    {
        work(job);
    }
}

fn keep_tail(text: String) -> String {
    let count = text.chars().count();
    if count <= MOST_TRANSCRIPT_CHARS {
        return text;
    }
    text.chars().skip(count - MOST_TRANSCRIPT_CHARS).collect()
}

#[derive(Default, Deserialize)]
pub(crate) struct Page {
    pub(crate) after: Option<String>,
}

fn folder_name(name: &str) -> bool {
    let (stamp, rest) = name.split_at_checked(15).unwrap_or((name, "x"));
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").is_ok()
        && (rest.is_empty()
            || rest
                .strip_prefix('-')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

pub(crate) async fn overview(State(state): State<AppState>, Query(page): Query<Page>) -> Response {
    if page
        .after
        .as_deref()
        .is_some_and(|after| !folder_name(after))
    {
        return error(
            StatusCode::BAD_REQUEST,
            "That page of recordings does not exist.",
        );
    }
    let mut pending = Vec::new();
    let mut after = None;
    let mut checks = Checks::default();
    if let Some(housekeeper) = state.housekeeper.clone() {
        if let Ok((saved, ready)) = tokio::task::spawn_blocking(move || {
            (
                housekeeper.pending_page(page.after.as_deref(), RECORD_PAGE),
                housekeeper.checks(),
            )
        })
        .await
        {
            checks = ready;
            let (saved, next) = saved.unwrap_or_default();
            after = next;
            let owners: Vec<String> = saved.iter().filter_map(|p| p.note.clone()).collect();
            let live = if owners.is_empty() {
                None
            } else {
                state
                    .read_store(move |store| {
                        Ok(owners
                            .into_iter()
                            .filter(|id| store.find_note(id).is_some())
                            .collect::<std::collections::HashSet<_>>())
                    })
                    .await
                    .ok()
            };
            pending.extend(
                saved
                    .into_iter()
                    .filter(|p| {
                        p.note
                            .as_ref()
                            .is_none_or(|note| live.as_ref().is_none_or(|live| live.contains(note)))
                    })
                    .map(|p| serde_json::to_value(p).unwrap_or_default()),
            );
        }
    }
    let job = state
        .recording
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|j| j.view.clone());
    Json(serde_json::json!({
        "available": state.listener.is_some(),
        "local": true,
        "job": job,
        "pending": pending,
        "more": after.is_some(),
        "after": after,
        "checks": checks,
    }))
    .into_response()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StartBody {
    #[serde(default)]
    recover: Option<String>,
    #[serde(default)]
    directory: String,
    #[serde(default)]
    title: Option<String>,
    source: Source,
    #[serde(default)]
    profile: Option<leo_core::recording::Profile>,
}

pub(crate) async fn start(State(state): State<AppState>, Json(body): Json<StartBody>) -> Response {
    let Some(listener) = state.listener.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Recording is not available in this copy of leo.",
        );
    };
    let directory = body.directory.trim().trim_matches('/').to_string();
    let checked = state
        .with_store({
            let directory = directory.clone();
            move |store| valid_directory(store, &directory)
        })
        .await;
    if let Err(code) = checked {
        return error(code, "Check the folder before recording.");
    }
    let mut profile = match body.profile.clone().unwrap_or_default().checked() {
        Ok(profile) => profile,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()),
    };
    profile.speakers &= body.source != Source::Nativecall;
    let title = body
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let activity = match state.capture_activity.begin() {
        Ok(lease) => lease,
        Err(e) => return error(StatusCode::CONFLICT, e.to_string()),
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let finish_now = Arc::new(AtomicBool::new(false));
    let points: Arc<Mutex<Vec<(u64, String)>>> = Default::default();
    let saved_points: Arc<AtomicUsize> = Default::default();
    let wants: Arc<Mutex<Option<String>>> = Default::default();
    {
        let mut held = state.recording.lock().unwrap_or_else(|e| e.into_inner());
        if state.quitting.load(Ordering::Acquire) {
            drop(held);
            return error(StatusCode::SERVICE_UNAVAILABLE, "leo is quitting.");
        }
        if held
            .as_ref()
            .is_some_and(|j| j.busy() || j.writing.load(Ordering::Acquire))
        {
            drop(held);
            return error(
                StatusCode::CONFLICT,
                "A recording is already going. Stop it first.",
            );
        }
        *held = Some(RecordJob {
            writing: Default::default(),
            view: RecordView {
                id: id.clone(),
                session: body.recover.clone().unwrap_or_else(|| id.clone()),
                source: body.source,
                state: if body.recover.is_some() {
                    "writing"
                } else {
                    "starting"
                },
                secs: 0,
                step: if body.recover.is_some() {
                    "Recovering saved audio"
                } else {
                    "Starting"
                }
                .into(),
                steps: None,
                transcript: String::new(),
                warnings: Vec::new(),
                points: Vec::new(),
                points_total: 0,
                wants: profile.wants.clone(),
                levels: Vec::new(),
                levels_start: 0,
                note: None,
                error: None,
                writer_failed: false,
            },
            stop: Arc::clone(&stop),
            finish_now: finish_now.clone(),
            pause: Arc::clone(&pause),
            points: Arc::clone(&points),
            saved_points: Arc::clone(&saved_points),
            wants: Arc::clone(&wants),
            worker: None,
        });
    }
    let listening = Listening {
        preview: state.visible.clone(),
        recover: body.recover,
        id: id.clone(),
        profile,
        call: body.source == Source::Nativecall,
        screen: body.source.is_sound(),
        directory: directory.clone(),
        title: title.clone(),
        stop,
        finish_now,
        pause,
        points,
        saved_points,
        wants,
    };
    let worker = state.clone();
    let job = id.clone();
    let handle = std::thread::spawn(move || {
        let _activity = activity;
        run(worker, job, listener, listening, directory, title)
    });
    change(&state.recording, &id, |job| job.worker = Some(handle));
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response()
}

fn run(
    state: AppState,
    id: String,
    listener: Listener,
    listening: Listening,
    directory: String,
    title: Option<String>,
) {
    struct RunGuard {
        recordings: Recordings,
        id: String,
    }
    impl Drop for RunGuard {
        fn drop(&mut self) {
            change(&self.recordings, &self.id, |job| {
                if job.view.state != "done" && job.view.state != "failed" {
                    job.stop.store(true, Ordering::Relaxed);
                    job.view.state = "failed";
                    job.view.error = Some(
                        "Recording worker stopped unexpectedly; its audio is available for recovery."
                            .into(),
                    );
                }
            });
        }
    }
    let recovering = listening.recover.is_some();
    let recordings = Arc::clone(&state.recording);
    let _guard = RunGuard {
        recordings: Arc::clone(&recordings),
        id: id.clone(),
    };
    let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        listener(listening, &mut |heard| {
            change(&recordings, &id, |job| match heard {
                Heard::Clock {
                    secs,
                    paused,
                    level,
                } => {
                    job.view.secs = secs;
                    job.view.push_level(level);
                    if job.live() {
                        job.view.state = if paused { "paused" } else { "recording" };
                    }
                }
                Heard::Step { label, steps } => {
                    if !job.live() {
                        job.view.step = label;
                        job.view.steps = steps;
                    }
                }
                Heard::Transcript(text) => job.view.transcript = keep_tail(text),
                Heard::Warning(text) => {
                    job.view.warn(text);
                }
            })
        })
    }))
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "Recording worker stopped unexpectedly; its audio is available for recovery."
        ))
    });
    let recorded = match written {
        Ok(found) => found,
        Err(e) => {
            change(&recordings, &id, |job| {
                job.stop.store(true, Ordering::Relaxed);
                job.view.state = "failed";
                job.view.error = Some(e.to_string());
            });
            return;
        }
    };
    change(&recordings, &id, |job| {
        job.view.state = "writing";
        job.view.step = "Saving the note".into();
    });
    let title = title.unwrap_or(recorded.title);
    let body = recorded.body;
    let into = recorded.into;
    let writer_failed = recorded.writer_failed && into.is_none();
    let source = recorded.source;
    let commit = recorded.commit;
    let gone = || {
        change(&recordings, &id, |job| {
            job.view.state = "failed";
            job.view.error = Some(NOTE_GONE.into());
        })
    };
    if let Some(into) = into.clone() {
        if !store_now(&state, move |store| Ok(store.find_note(&into).is_some())).unwrap_or(false) {
            return gone();
        }
    }
    if let Some(source) = &source {
        let save = if recovering {
            leo_core::recording::save_merged
        } else {
            leo_core::recording::save
        };
        if save(
            &state.notes_dir,
            into.as_deref().unwrap_or(&source.id),
            source,
        )
        .is_err()
        {
            change(&recordings, &id, |job| {
                job.view.state = "failed";
                job.view.error = Some("The recording sources could not be saved. Check free disk space and recover this recording.".into());
            });
            return;
        }
    }
    let joined = into.clone();
    let made = store_now(&state, move |store| {
        if let Some(into) = joined {
            return match store.find_note(&into) {
                Some(_) => Ok((into, store.notes_dir.clone())),
                None => Err(StatusCode::NOT_FOUND),
            };
        }
        if !directory.is_empty() && !store.dir_exists(&directory) {
            store.create_dir(&directory);
        }
        let stable_id = source.as_ref().map(|s| s.id.clone());
        if let Some(id) = &stable_id {
            if store.find_note(id).is_some() {
                return Ok((id.clone(), store.notes_dir.clone()));
            }
        }
        let mut note = store
            .create_note(title, body.trim().to_string(), vec![], &directory)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .id
            .clone();
        if let Some(id) = stable_id {
            if let Some(created) = store.find_note_mut(&note) {
                created.id = id.clone();
                note = id;
            }
            store.find_note_mut(&note);
        }
        save(store)?;
        Ok((note, store.notes_dir.clone()))
    });
    let mut warning = None;
    if let Ok((note, notes)) = &made {
        if writer_failed {
            set_writeup_pending(&state.chats, note, true);
        }
        if let Some(commit) = &commit {
            if let Err(e) = commit(notes, note) {
                warning = Some(format!(
                    "The note is saved; recovery cleanup will retry: {e}"
                ));
            }
        }
    }
    change(&recordings, &id, |job| match made {
        Ok((note, _)) => {
            if let Some(warning) = warning {
                job.view.warn(warning);
            }
            job.view.state = "done";
            job.view.note = Some(note);
            job.view.writer_failed = writer_failed;
        }
        Err(StatusCode::NOT_FOUND) if into.is_some() => {
            job.view.state = "failed";
            job.view.error = Some(NOTE_GONE.into());
        }
        Err(_) => {
            job.view.state = "failed";
            job.view.error = Some(
                "The note could not be saved. Recover the recording after checking disk space."
                    .into(),
            );
        }
    });
}

pub(crate) fn writeup_pending(chats: &std::path::Path, note: &str) -> bool {
    crate::db::for_chats(chats)
        .with(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM writeup_pending WHERE note = ?1)",
                [note],
                |r| r.get(0),
            )
        })
        .unwrap_or(false)
}

pub(crate) fn set_writeup_pending(chats: &std::path::Path, note: &str, pending: bool) {
    let sql = if pending {
        "INSERT OR IGNORE INTO writeup_pending (note) VALUES (?1)"
    } else {
        "DELETE FROM writeup_pending WHERE note = ?1"
    };
    let _ = crate::db::for_chats(chats).with(|c| c.execute(sql, [note]));
}

pub(crate) async fn status(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match with_job(&state, &id, |job| job.view.clone()) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub(crate) async fn recover(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Some(housekeeper) = state.housekeeper.clone() {
        let pending = tokio::task::spawn_blocking(move || housekeeper.pending_recording(&id)).await;
        if let Ok(Ok(Some(saved))) = pending {
            return start(
                State(state),
                Json(StartBody {
                    recover: Some(saved.id),
                    directory: saved.directory,
                    title: saved.title,
                    source: saved.source,
                    profile: Some(saved.profile),
                }),
            )
            .await;
        }
    }
    error(StatusCode::NOT_FOUND, "That recording cannot be recovered.")
}

#[derive(Deserialize)]
pub(crate) struct PauseBody {
    paused: bool,
}

pub(crate) async fn pause(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PauseBody>,
) -> Response {
    match with_job(&state, &id, |job| {
        if job.live() {
            job.pause.store(body.paused, Ordering::Relaxed);
            job.view.state = if body.paused { "paused" } else { "recording" };
        }
        job.view.clone()
    }) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct PointBody {
    text: String,
}

pub(crate) async fn point(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PointBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Write the point first.");
    }
    if text.len() > 8000 {
        return error(StatusCode::BAD_REQUEST, "Keep a point under 8,000 bytes.");
    }
    append_point(&state, &id, text, None, None).await
}

async fn append_point(
    state: &AppState,
    id: &str,
    text: String,
    at: Option<u64>,
    warning: Option<String>,
) -> Response {
    writes_done(state, id).await;
    let input = match with_job(state, id, |job| -> anyhow::Result<_> {
        if !job.live() {
            anyhow::bail!("This recording is not taking points.");
        }
        let lease = Writing::take(job)?;
        let at = at.unwrap_or(job.view.secs).min(job.view.secs);
        Ok((lease, at))
    }) {
        Ok(Ok(input)) => input,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, e.to_string()),
        Err((code, message)) => return error(code, message),
    };
    let (lease, at) = input;
    let worker = state.clone();
    let id = id.to_string();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let _lease = lease;
        persist_point(&worker, &id, text, at, warning)
    })
    .await;
    match result {
        Ok(Ok(kept)) => acknowledged(kept).await,
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The point could not be saved. Check free disk space and retry.",
        ),
    }
}

type Kept = (RecordView, usize, Arc<AtomicUsize>);

async fn acknowledged((view, position, saved): Kept) -> Response {
    let until = Instant::now() + WAIT_FOR_WRITES;
    while saved.load(Ordering::Acquire) < position {
        if Instant::now() >= until {
            return error(StatusCode::SERVICE_UNAVAILABLE, STILL_SAVING);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Json(view).into_response()
}

#[derive(Deserialize)]
pub(crate) struct Earlier {
    before: Option<String>,
}

pub(crate) async fn earlier_points(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Earlier>,
) -> Response {
    let Some(before) = page.before.as_deref().and_then(|b| b.parse::<usize>().ok()) else {
        return error(
            StatusCode::BAD_REQUEST,
            "That page of points does not exist.",
        );
    };
    match with_job(&state, &id, |job| {
        let points = job.points.lock().unwrap_or_else(|e| e.into_inner());
        let end = before.min(points.len());
        let from = end.saturating_sub(VIEW_POINTS);
        serde_json::json!({ "from": from, "points": points.get(from..end).unwrap_or_default() })
    }) {
        Ok(page) => Json(page).into_response(),
        Err((status, message)) => error(status, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct WantsBody {
    text: String,
}

fn persist_point(
    state: &AppState,
    id: &str,
    text: String,
    at: u64,
    warning: Option<String>,
) -> Result<Kept> {
    with_job(state, id, |job| {
        let position = {
            let mut points = job.points.lock().unwrap_or_else(|e| e.into_inner());
            points.push((at, text.clone()));
            points.len()
        };
        job.view.points.push((at, text));
        let extra = job.view.points.len().saturating_sub(VIEW_POINTS);
        job.view.points.drain(..extra);
        job.view.points_total = position;
        if let Some(warning) = warning {
            job.view.warn(warning);
        }
        (job.view.clone(), position, Arc::clone(&job.saved_points))
    })
    .map_err(|(_, message)| anyhow::anyhow!(message))
}

pub(crate) async fn wants(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WantsBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.chars().count() > leo_core::recording::MOST_WANTS_CHARS {
        return error(
            StatusCode::BAD_REQUEST,
            "Keep what you want from the notes under 4,000 characters.",
        );
    }
    writes_done(&state, &id).await;
    let lease = match with_job(&state, &id, |job| -> anyhow::Result<_> {
        if !job.live() {
            anyhow::bail!(
                "The notes are already being written; change this before you stop next time."
            );
        }
        Writing::take(job)
    }) {
        Ok(Ok(lease)) => lease,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, e.to_string()),
        Err((code, message)) => return error(code, message),
    };
    let result = with_job(&state, &id, |job| {
        *job.wants.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
        job.view.wants = text;
        job.view.clone()
    });
    drop(lease);
    match result {
        Ok(view) => Json(view).into_response(),
        Err((code, message)) => error(code, message),
    }
}

pub(crate) async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    writes_done(&state, &id).await;
    match with_job(&state, &id, |job| {
        if job.writing.load(Ordering::Acquire) {
            return None;
        }
        job.end();
        Some(job.view.clone())
    }) {
        Ok(Some(view)) => Json(view).into_response(),
        Ok(None) => error(
            StatusCode::CONFLICT,
            "Audio or a point is still being saved. Try Stop again in a moment.",
        ),
        Err((status, message)) => error(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_recordings_keep_only_a_bounded_tail_of_actionable_warnings() {
        let mut view = view();
        for index in 0..MOST_WARNINGS + 5 {
            view.warn(format!("{index} {}", "文字".repeat(WARNING_CHARS)));
        }
        assert_eq!(view.warnings.len(), MOST_WARNINGS);
        assert!(view
            .warnings
            .iter()
            .all(|text| text.chars().count() <= WARNING_CHARS));
        let last = view.warnings.last().unwrap().clone();
        view.warn(last);
        assert_eq!(view.warnings.len(), MOST_WARNINGS);
        assert!(view.warnings[0].starts_with("5 "));
    }

    fn view() -> RecordView {
        RecordView {
            id: "r".into(),
            session: "r".into(),
            source: Source::Microphone,
            state: "recording",
            secs: 0,
            step: String::new(),
            steps: None,
            transcript: String::new(),
            warnings: Vec::new(),
            points: Vec::new(),
            points_total: 0,
            wants: String::new(),
            levels: Vec::new(),
            levels_start: 0,
            note: None,
            error: None,
            writer_failed: false,
        }
    }

    #[test]
    fn levels_keep_the_last_few_seconds_and_count_what_scrolled_away() {
        let mut v = view();
        for i in 0..(LEVELS_KEPT + 5) {
            v.push_level(i as f32 / 1000.0);
        }
        assert_eq!(v.levels.len(), LEVELS_KEPT);
        assert_eq!(v.levels_start, 5);
        assert_eq!(v.levels[0], 0.005, "the first kept level is the sixth sent");
        v.push_level(7.0);
        assert_eq!(*v.levels.last().unwrap(), 1.0, "levels are clamped");
        assert_eq!(v.levels_start, 6);
    }
}

#[derive(Deserialize)]
pub(crate) struct Snapshot {
    data: String,
    at: u64,
}
pub(crate) async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Snapshot>,
) -> Response {
    use base64::Engine;
    writes_done(&state, &id).await;
    let notes = state.notes_dir.clone();
    let input = with_job(&state, &id, |job| -> anyhow::Result<_> {
        anyhow::ensure!(job.live(), "Start a recording before pasting a screenshot.");
        Ok((Writing::take(job)?, body.at.min(job.view.secs)))
    });
    let (lease, at) = match input {
        Ok(Ok(input)) => input,
        Ok(Err(problem)) => return error(StatusCode::CONFLICT, problem.to_string()),
        Err((status, problem)) => return error(status, problem),
    };
    let saved = tokio::task::spawn_blocking(move || -> Result<Kept> {
        let _lease = lease;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&body.data)
            .map_err(|_| {
                anyhow::anyhow!("The screenshot did not arrive whole. Try pasting it again.")
            })?;
        let kind = leo_core::attachments::kind_of(&bytes)
            .ok_or_else(|| anyhow::anyhow!("Paste a PNG, JPEG, GIF or WebP screenshot."))?;
        let mime = leo_core::attachments::mime_of(kind).unwrap_or("image/png");
        let path = leo_core::attachments::save(&notes, "slide.png", &bytes)?;
        let mut point = format!("Slide at {at}\n![Captured slide]({path})");
        let mut warning = None;
        if let Some(reader) = state.reader.clone() {
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reader(
                    crate::UploadFile {
                        name: format!("slide.{kind}"),
                        mime: mime.into(),
                        bytes,
                    },
                    &mut |_| {},
                )
            }));
            match read {
                Ok(Ok(text)) => {
                    point.push('\n');
                    point.extend(text.chars().take(SLIDE_TEXT_CHARS));
                }
                _ => warning = Some("The slide image is kept; its text could not be read.".into()),
            }
        }
        persist_point(&state, &id, point, at, warning)
    })
    .await;
    match saved {
        Ok(Ok(kept)) => acknowledged(kept).await,
        Ok(Err(problem)) => error(StatusCode::BAD_REQUEST, problem.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The screenshot could not be kept. Try pasting it again.",
        ),
    }
}

pub(crate) async fn finish_available(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match with_job(&state, &id, |job| {
        if job.view.state != "writing" {
            return false;
        }
        job.finish_now.store(true, Ordering::Relaxed);
        job.view.step = "Finishing with the available transcript".into();
        true
    }) {
        Ok(true) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"finishing":true})),
        )
            .into_response(),
        _ => error(
            StatusCode::CONFLICT,
            "Stop recording before finishing with the available transcript.",
        ),
    }
}

async fn discard_pending(state: AppState, id: String) -> Response {
    let Some(housekeeper) = state.housekeeper.clone() else {
        return error(StatusCode::NOT_FOUND, "That recording is over.");
    };
    let recordings = state.recording.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<bool>> {
        if housekeeper.pending_recording(&id)?.is_none() {
            return Ok(Some(false));
        }
        discard_unless_in_use(&recordings, &*housekeeper, &id)
            .transpose()
            .map(|done| done.map(|()| true))
    })
    .await;
    match result {
        Ok(Ok(None)) => error(
            StatusCode::CONFLICT,
            "This recording is being recovered. Wait for it to finish first.",
        ),
        Ok(Ok(Some(true))) => Json(serde_json::json!({ "deleted": true })).into_response(),
        Ok(Ok(Some(false))) => error(StatusCode::NOT_FOUND, "That recording is over."),
        _ => error(
            StatusCode::CONFLICT,
            "The recording could not be deleted. Try again.",
        ),
    }
}

pub(crate) async fn discard(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state
        .discarded_recordings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&id)
    {
        return Json(serde_json::json!({ "deleted": true })).into_response();
    }
    let (lease, session) = match with_job(&state, &id, |job| {
        anyhow::ensure!(
            job.view.state == "failed",
            "Stop the recording and wait for it to finish first."
        );
        Ok((Writing::take(job)?, job.session_id()))
    }) {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, e.to_string()),
        Err((StatusCode::NOT_FOUND, _)) => return discard_pending(state, id).await,
        Err((code, message)) => return error(code, message),
    };
    let housekeeper = state.housekeeper.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let _lease = lease;
        if let Some(housekeeper) = housekeeper {
            housekeeper.discard_recording(&session)?;
        }
        let mut held = state.recording.lock().unwrap_or_else(|e| e.into_inner());
        if held.as_ref().is_some_and(|job| job.view.id == id) {
            let mut gone = state
                .discarded_recordings
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            gone.push_back(id);
            if gone.len() > DISCARDS_KEPT {
                gone.pop_front();
            }
            *held = None;
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "deleted": true })).into_response(),
        _ => error(
            StatusCode::CONFLICT,
            "The recording could not be deleted. Try again.",
        ),
    }
}
