use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use leo_core::notes::Note;
use serde::Deserialize;

use super::notes::{save, version_of, NoteResponse};
use super::uploads::{remove_originals, share_originals};
use crate::reply::error;
use crate::{combine, progress, AppState};

const MOST_JOBS: usize = 20;
const MOST_WORKING: usize = 2;
const KEPT_FOR: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CombineJob {
    pub(crate) id: String,
    pub(crate) into: String,
    pub(crate) with: String,
    pub(crate) into_title: String,
    pub(crate) with_title: String,
    pub(crate) state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
    pub(crate) parts: usize,
    #[serde(skip)]
    pub(crate) started: Option<Instant>,
    #[serde(skip)]
    pub(crate) run: Option<combine::Run>,
}

#[derive(Deserialize)]
pub(crate) struct Start {
    with: String,
}

type Failure = (StatusCode, &'static str);

async fn notes_for(
    state: &AppState,
    into: String,
    with: String,
) -> Result<(Note, Note, std::path::PathBuf), Failure> {
    let found = state
        .read_store(move |store| {
            let a = store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?.clone();
            let b = store.find_note(&with).ok_or(StatusCode::NOT_FOUND)?.clone();
            if a.id == b.id {
                return Err(StatusCode::BAD_REQUEST);
            }
            Ok((a, b, store.notes_dir.clone()))
        })
        .await;
    let (into, from, notes_dir) = match found {
        Ok(found) => found,
        Err(StatusCode::BAD_REQUEST) => {
            return Err((StatusCode::BAD_REQUEST, "Drop a note on a different note."))
        }
        Err(code) => return Err((code, "One of those notes is gone. Refresh and try again.")),
    };
    Ok((into, from, notes_dir))
}

fn combined(
    writer: &crate::graph::Writer,
    into: &Note,
    from: &Note,
    notes_dir: &std::path::Path,
    request_chars: usize,
    run: &combine::Run,
) -> Result<serde_json::Value, &'static str> {
    let made = combine::write(writer, notes_dir, into, from, request_chars, run)?;
    Ok(serde_json::json!({
        "title": into.title,
        "with_title": from.title,
        "body": made.body,
        "added": made.added,
        "kept": made.kept,
        "kept_enough": made.kept >= combine::KEPT_ENOUGH,
        "base": version_of(into),
        "with_base": version_of(from),
    }))
}

pub(crate) async fn start(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Start>,
) -> Response {
    let Some(writer) = state.graphs.writer() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, crate::no_writer());
    };
    let (into, from, notes_dir) = match notes_for(&state, into, body.with).await {
        Ok(found) => found,
        Err((code, why)) => return error(code, why),
    };
    let request_chars = state.graphs.scale().batch_chars;
    let (into, from, parts) = {
        let (a, b) = (into, from);
        match tokio::task::spawn_blocking(move || {
            let parts = combine::parts_needed(&a, &b, request_chars);
            (a, b, parts)
        })
        .await
        {
            Ok(found) => found,
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "The notes could not be combined.",
                )
            }
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let run = run_for(&into.id);
    let job = CombineJob {
        id: id.clone(),
        into: into.id.clone(),
        with: from.id.clone(),
        into_title: into.title.clone(),
        with_title: from.title.clone(),
        state: "working",
        result: None,
        error: String::new(),
        parts,
        started: Some(Instant::now()),
        run: Some(run.clone()),
    };
    {
        let mut jobs = state.combines.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|_, j| j.started.is_some_and(|t| t.elapsed() < KEPT_FOR));
        if jobs
            .values()
            .any(|j| j.state == "working" && j.into == job.into && j.with == job.with)
        {
            return error(
                StatusCode::CONFLICT,
                "These notes are already being combined.",
            );
        }
        if jobs.values().filter(|job| job.state == "working").count() >= MOST_WORKING {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "Two combines are already running. Let one finish, then try again.",
            );
        }
        if jobs.len() >= MOST_JOBS {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "Review or discard some combined notes first.",
            );
        }
        jobs.insert(id.clone(), job.clone());
    }
    let expected = (into.body.chars().count() + from.body.chars().count()) as f64;
    let ticket = state.progress.start_sized(
        format!("combine:{id}"),
        progress::COMBINE,
        Some(expected.max(500.0)),
    );
    let writer: crate::Writer = match (&ticket, state.chat.clone()) {
        (Some(t), Some(stream)) => {
            let meter = std::sync::Arc::clone(&t.meter);
            let usage = std::sync::Arc::clone(&state.usage);
            let stop = std::sync::Arc::clone(&run.stopped);
            std::sync::Arc::new(move |system: &str, user: &str, most: u32| {
                let started = Instant::now();
                let reply = stream(
                    system,
                    user,
                    most,
                    &mut |piece| meter.add(piece),
                    &mut || meter.restart(),
                    &|| stop.load(std::sync::atomic::Ordering::SeqCst),
                );
                usage.record(
                    crate::usage::Purpose::OtherWriting,
                    reply.as_ref().ok().and_then(|r| r.spent.as_ref()),
                    started.elapsed(),
                    reply.is_err(),
                );
                Ok(reply?.text)
            })
        }
        (Some(t), None) => progress::metered(writer, std::sync::Arc::clone(&t.meter)),
        (None, _) => writer,
    };
    let jobs = std::sync::Arc::clone(&state.combines);
    let spawned = std::thread::Builder::new()
        .name("leo-combine".into())
        .spawn(move || {
            struct CombineGuard {
                jobs:
                    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, CombineJob>>>,
                id: String,
                done: bool,
            }
            impl Drop for CombineGuard {
                fn drop(&mut self) {
                    if !self.done {
                        if let Some(job) = self
                            .jobs
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get_mut(&self.id)
                        {
                            job.state = "failed";
                            job.error = "The notes could not be combined.".into();
                        }
                    }
                }
            }
            let _held = ticket;
            let mut guard = CombineGuard {
                jobs: std::sync::Arc::clone(&jobs),
                id: id.clone(),
                done: false,
            };
            let done = combined(&writer, &into, &from, &notes_dir, request_chars, &run);
            let mut jobs = jobs.lock().unwrap_or_else(|e| e.into_inner());
            if run.stopped.load(std::sync::atomic::Ordering::SeqCst) {
                jobs.remove(&id);
            } else if let Some(job) = jobs.get_mut(&id) {
                match done {
                    Ok(view) => {
                        job.state = "ready";
                        job.result = Some(view);
                    }
                    Err(why) => {
                        job.state = "failed";
                        job.error = why.into();
                    }
                }
            }
            drop(jobs);
            guard.done = true;
        });
    if spawned.is_err() {
        state
            .combines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.id);
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The notes could not be combined.",
        );
    }
    Json(job).into_response()
}

pub(crate) async fn job(State(state): State<AppState>, Path(job): Path<String>) -> Response {
    let found = state
        .combines
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&job)
        .cloned();
    match found {
        Some(found) => {
            let mut view = serde_json::json!(found);
            if found.state == "working" {
                view["progress"] = state
                    .progress
                    .view(&format!("combine:{job}"))
                    .unwrap_or(serde_json::Value::Null);
                view["done"] = found
                    .run
                    .as_ref()
                    .map_or(0, |r| r.done.load(std::sync::atomic::Ordering::SeqCst))
                    .into();
            }
            Json(view).into_response()
        }
        None => error(
            StatusCode::NOT_FOUND,
            "That combined note is no longer here. Drop the notes on each other again.",
        ),
    }
}

pub(crate) async fn forget(State(state): State<AppState>, Path(job): Path<String>) -> Response {
    let mut jobs = state.combines.lock().unwrap_or_else(|e| e.into_inner());
    match jobs.get(&job) {
        Some(found) if found.state == "working" => {
            if let Some(run) = &found.run {
                run.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Some(_) => {
            jobs.remove(&job);
        }
        None => {}
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub(crate) struct Keep {
    with: String,
    body: String,
    base: String,
    with_base: String,
}

pub(crate) async fn keep(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Keep>,
) -> Response {
    if body.body.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "The combined note is empty.");
    }
    let (from, from_title, target, notes_dir) = {
        let (with, base, with_base, into) = (
            body.with.clone(),
            body.base.clone(),
            body.with_base.clone(),
            into.clone(),
        );
        let seen = state
            .read_store(move |store| {
                let (from, from_title) = {
                    let from = store.find_note(&with).ok_or(StatusCode::NOT_FOUND)?;
                    if version_of(from) != with_base {
                        return Err(StatusCode::CONFLICT);
                    }
                    (from.id.clone(), from.title.clone())
                };
                let note = store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
                if note.id == from {
                    return Err(StatusCode::BAD_REQUEST);
                }
                if version_of(note) != base {
                    return Err(StatusCode::CONFLICT);
                }
                Ok((from, from_title, note.id.clone(), store.notes_dir.clone()))
            })
            .await;
        match seen {
            Ok(seen) => seen,
            Err(StatusCode::CONFLICT) => return changed_while_combining(),
            Err(code) => return error(code, "The combined note could not be saved."),
        }
    };
    let copying = {
        let (notes_dir, from, target, from_title) = (
            notes_dir.clone(),
            from.clone(),
            target.clone(),
            from_title.clone(),
        );
        tokio::task::spawn_blocking(move || {
            let (mut created, mut checked) =
                copy_transcripts(&notes_dir, &from, &target, &from_title)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            #[cfg(test)]
            after_publishing(&target);
            let sources: Vec<leo_core::paths::FileDigest> =
                crate::originals::files(&notes_dir, &from)
                    .into_iter()
                    .filter_map(|(name, _)| {
                        let path = crate::routes::uploads::original_path(&notes_dir, &from, &name)?;
                        leo_core::paths::digest_stamped(&path, u64::MAX)
                            .ok()
                            .map(|(digest, _)| digest)
                    })
                    .collect();
            match share_originals(&notes_dir, &from, &target) {
                Ok(originals) => {
                    for name in originals {
                        if let Some((path, stamp, digest)) =
                            original_checked(&notes_dir, &target, &name)
                                .filter(|(_, _, digest)| sources.contains(digest))
                        {
                            created.originals.insert(name.clone(), digest);
                            checked.originals.push((name, path, stamp));
                        }
                    }
                    Ok((created, checked))
                }
                Err(_) => {
                    let _ = remove_checked(&notes_dir, &target, &checked);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        })
        .await
    };
    let (created, checked) = match copying {
        Ok(Ok(copies)) => copies,
        Ok(Err(code)) => return error(code, "The combined note could not be saved."),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The combined note could not be saved.",
            )
        }
    };
    let intent = (state.clone(), from.clone(), target.clone());
    let noting = |wanted: bool| {
        let (state, from, target) = intent.clone();
        tokio::task::spawn_blocking(move || {
            let _audio = audio(&state);
            note_retarget(&state.chats, &from, &target, wanted)
        })
    };
    if !noting(true).await.unwrap_or(false) {
        let _ = remove_checked(&state.notes_dir, &target, &checked);
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "leo could not record the combine. Try again.",
        );
    }
    #[cfg(test)]
    {
        let hook = {
            let mut hooks = AFTER_COPY.lock().unwrap_or_else(|e| e.into_inner());
            hooks
                .iter()
                .position(|(id, _)| *id == target)
                .map(|at| hooks.remove(at).1)
        };
        if let Some(hook) = hook {
            hook();
        }
    }
    let chats = state.chats.clone();
    let saving = state.clone();
    let done = state
        .with_store(move |store| {
            let undo = |store: &leo_core::store::Store| {
                let _ = remove_checked(&store.notes_dir, &target, &checked);
            };
            let still = store
                .find_note(&from)
                .is_some_and(|n| version_of(n) == body.with_base)
                && store
                    .find_note(&target)
                    .is_some_and(|n| version_of(n) == body.base);
            if !still {
                undo(store);
                return Err(StatusCode::CONFLICT);
            }
            let Some(note) = store.find_note_mut(&target) else {
                undo(store);
                return Err(StatusCode::NOT_FOUND);
            };
            let before = std::mem::replace(&mut note.body, body.body);
            note.updated_at = chrono::Utc::now();
            let response = NoteResponse::from_note(note);
            let version = version_of(note);
            let record = Combined {
                from: from.clone(),
                at: chrono::Utc::now(),
                kept: Vec::new(),
                created: created.clone(),
            };
            let _audio = audio(&saving);
            if !change_retarget(&chats, &from, &target, |entry| {
                entry.record = Some(record.clone());
                entry.version = Some(version);
            }) {
                undo(store);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            store.delete_notes(std::slice::from_ref(&from));
            if let Err(code) = save(store) {
                undo(store);
                return Err(code);
            }
            change_retarget(&chats, &from, &target, |entry| entry.committed = true);
            Ok((
                target,
                record,
                serde_json::json!({ "note": response, "before": before, "removed": from }),
            ))
        })
        .await;
    match done {
        Ok((target, record, value)) => {
            let state = state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _audio = audio(&state);
                change_retarget(&state.chats, &record.from, &target, |r| r.committed = true);
                move_kept(&state, &record.from, &target);
            })
            .await;
            Json(value).into_response()
        }
        Err(code) => {
            let _ = noting(false).await;
            if code == StatusCode::CONFLICT {
                changed_while_combining()
            } else {
                error(code, "The combined note could not be saved.")
            }
        }
    }
}

fn changed_while_combining() -> Response {
    error(
        StatusCode::CONFLICT,
        "One of the notes changed while they were being combined. Combine them again.",
    )
}

#[cfg(test)]
fn after_publishing(target: &str) {
    let hook = {
        let mut hooks = AFTER_COPY.lock().unwrap_or_else(|e| e.into_inner());
        let key = format!("published:{target}");
        hooks
            .iter()
            .position(|(id, _)| *id == key)
            .map(|at| hooks.remove(at).1)
    };
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
pub(crate) type AfterCopy = Box<dyn FnOnce() + Send>;

#[cfg(test)]
pub(crate) static AFTER_COPY: std::sync::Mutex<Vec<(String, AfterCopy)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(not(test))]
fn run_for(_: &str) -> combine::Run {
    combine::Run::new()
}

#[cfg(test)]
fn run_for(into: &str) -> combine::Run {
    let mut run = combine::Run::new();
    let mut clocks = CLOCKS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = clocks.iter().position(|(note, _)| note == into) {
        run.now = clocks.remove(at).1;
    }
    run
}

#[cfg(test)]
pub(crate) type Clock = std::sync::Arc<dyn Fn() -> Duration + Send + Sync>;

#[cfg(test)]
pub(crate) static CLOCKS: std::sync::Mutex<Vec<(String, Clock)>> =
    std::sync::Mutex::new(Vec::new());

#[derive(Deserialize)]
pub(crate) struct Uncombine {
    from: String,
}

#[derive(Clone, serde::Serialize, Deserialize)]
pub(crate) struct Combined {
    from: String,
    at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    kept: Vec<crate::record::Moved>,
    #[serde(default)]
    created: Created,
}

#[derive(Clone, Default, serde::Serialize, Deserialize)]
struct Created {
    transcripts: std::collections::BTreeMap<String, leo_core::paths::FileDigest>,
    originals: std::collections::BTreeMap<String, leo_core::paths::FileDigest>,
}

type Stamped = Vec<(std::path::PathBuf, leo_core::paths::EntryStamp)>;

#[derive(Default)]
struct Checked {
    transcripts: Vec<(String, Stamped)>,
    originals: Vec<(String, std::path::PathBuf, leo_core::paths::EntryStamp)>,
}

fn stamp_of(path: &std::path::Path) -> Option<leo_core::paths::EntryStamp> {
    let meta = std::fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_file())?;
    Some(leo_core::paths::entry_stamp(&meta))
}

fn transcript_files(
    notes: &std::path::Path,
    note: &str,
    ids: &std::collections::BTreeSet<&str>,
) -> std::collections::BTreeMap<String, Option<Stamped>> {
    let mut files: std::collections::BTreeMap<String, Option<Stamped>> = Default::default();
    let entries = leo_core::recording::folder(notes, note)
        .ok()
        .and_then(|folder| std::fs::read_dir(folder).ok());
    for entry in entries.into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((id, _)) = name.split_once('.').filter(|(id, _)| ids.contains(id)) else {
            continue;
        };
        let path = entry.path();
        let found = files
            .entry(id.to_string())
            .or_insert_with(|| Some(Vec::new()));
        match (found.as_mut(), stamp_of(&path)) {
            (Some(list), Some(stamp)) => list.push((path, stamp)),
            _ => *found = None,
        }
    }
    for list in files.values_mut().flatten() {
        list.sort();
    }
    files
}

fn transcript_digest(files: &Stamped) -> Option<leo_core::paths::FileDigest> {
    let mut hashed = Vec::new();
    for (path, stamp) in files {
        let digest = leo_core::paths::digest_file(path, stamp.1).ok()?;
        (stamp_of(path) == Some(*stamp)).then_some(())?;
        hashed.push((path.file_name()?.to_string_lossy().to_string(), digest));
    }
    let bytes = serde_json::to_vec(&hashed).ok()?;
    (!hashed.is_empty()).then(|| leo_core::paths::digest(&bytes))
}

fn original_checked(
    notes: &std::path::Path,
    note: &str,
    name: &str,
) -> Option<(
    std::path::PathBuf,
    leo_core::paths::EntryStamp,
    leo_core::paths::FileDigest,
)> {
    let path = crate::routes::uploads::original_path(notes, note, name)?;
    let stamp = stamp_of(&path)?;
    let digest = leo_core::paths::digest_file(&path, stamp.1).ok()?;
    (stamp_of(&path) == Some(stamp)).then_some((path, stamp, digest))
}

fn copy_transcripts(
    notes: &std::path::Path,
    from: &str,
    target: &str,
    label: &str,
) -> anyhow::Result<(Created, Checked)> {
    let staging_id = format!("combine-{}", uuid::Uuid::new_v4().simple());
    let staging = leo_core::recording::folder(notes, &staging_id)?;
    let published = (|| {
        let copied = leo_core::recording::copy_to(notes, from, &staging_id, label)?;
        let into = leo_core::recording::folder(notes, target)?;
        std::fs::create_dir_all(&into)?;
        let ids = copied.iter().map(String::as_str).collect();
        let mut created = Created::default();
        let mut checked = Checked::default();
        for (id, files) in transcript_files(notes, &staging_id, &ids) {
            let Some(files) = files else { continue };
            let Some(digest) = transcript_digest(&files) else {
                continue;
            };
            let head = format!("{id}.json");
            if std::fs::symlink_metadata(into.join(&head)).is_ok() {
                continue;
            }
            let (heads, lines): (Vec<_>, Vec<_>) = files
                .iter()
                .partition(|(path, _)| path.file_name().is_some_and(|name| *name == *head));
            let moved = lines.iter().chain(&heads).all(|(path, _)| {
                path.file_name()
                    .is_some_and(|name| leo_core::paths::move_new(path, &into.join(name)).is_ok())
            });
            if !moved {
                continue;
            }
            let one = std::collections::BTreeSet::from([id.as_str()]);
            if let Some(Some(now)) = transcript_files(notes, target, &one).remove(&id) {
                if transcript_digest(&now) == Some(digest) {
                    created.transcripts.insert(id.clone(), digest);
                    checked.transcripts.push((id, now));
                }
            }
        }
        Ok((created, checked))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    published
}

fn check_created(notes: &std::path::Path, target: &str, created: &Created) -> Checked {
    let ids = created.transcripts.keys().map(String::as_str).collect();
    let transcripts = transcript_files(notes, target, &ids)
        .into_iter()
        .filter_map(|(id, files)| {
            let files = files?;
            (transcript_digest(&files).as_ref() == created.transcripts.get(&id))
                .then_some((id, files))
        })
        .collect();
    let originals = created
        .originals
        .iter()
        .filter_map(|(name, digest)| {
            let (path, stamp, found) = original_checked(notes, target, name)?;
            (found == *digest).then(|| (name.clone(), path, stamp))
        })
        .collect();
    Checked {
        transcripts,
        originals,
    }
}

fn remove_checked(notes: &std::path::Path, target: &str, checked: &Checked) -> anyhow::Result<()> {
    let ids = checked
        .transcripts
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    let now = transcript_files(notes, target, &ids);
    let transcripts: Vec<String> = checked
        .transcripts
        .iter()
        .filter(|(id, files)| {
            now.get(id)
                .is_some_and(|found| found.as_ref() == Some(files))
        })
        .map(|(id, _)| id.clone())
        .collect();
    leo_core::recording::remove(notes, target, &transcripts)?;
    let originals: Vec<String> = checked
        .originals
        .iter()
        .filter(|(_, path, stamp)| stamp_of(path) == Some(*stamp))
        .map(|(name, _, _)| name.clone())
        .collect();
    remove_originals(notes, target, &originals)?;
    Ok(())
}

const COMBINED: &str = "combined:";
const RETARGET: &str = "retarget";

pub(crate) fn audio(state: &AppState) -> std::sync::MutexGuard<'_, ()> {
    state.audio_work.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn try_audio(state: &AppState) -> Option<std::sync::MutexGuard<'_, ()>> {
    match state.audio_work.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

#[derive(Clone, serde::Serialize, Deserialize)]
struct Retarget {
    from: String,
    target: String,
    committed: bool,
    #[serde(default)]
    moved: Option<Vec<crate::record::Moved>>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    record: Option<Combined>,
}

fn retargets(chats: &std::path::Path) -> Vec<Retarget> {
    crate::margin::state_of(chats, RETARGET).unwrap_or_default()
}

fn note_retarget(chats: &std::path::Path, from: &str, target: &str, wanted: bool) -> bool {
    let mut all = retargets(chats);
    all.retain(|r| !(r.from == from && r.target == target));
    if wanted {
        all.push(Retarget {
            from: from.to_string(),
            target: target.to_string(),
            committed: false,
            moved: None,
            version: None,
            record: None,
        });
    }
    crate::margin::set_state(chats, RETARGET, &all)
}

fn change_retarget(
    chats: &std::path::Path,
    from: &str,
    target: &str,
    change: impl FnOnce(&mut Retarget),
) -> bool {
    let mut all = retargets(chats);
    if let Some(entry) = all
        .iter_mut()
        .find(|r| r.from == from && r.target == target)
    {
        change(entry);
        return crate::margin::set_state(chats, RETARGET, &all);
    }
    false
}

pub(crate) fn pending_retargets(
    chats: &std::path::Path,
) -> std::collections::BTreeSet<(String, String)> {
    retargets(chats)
        .into_iter()
        .map(|r| (r.from, r.target))
        .collect()
}

fn move_kept(state: &AppState, from: &str, target: &str) {
    let chats = &state.chats;
    let Some(entry) = retargets(chats)
        .into_iter()
        .find(|r| r.from == from && r.target == target && r.committed)
    else {
        return;
    };
    let Some(mut record) = combined_of(chats, target)
        .filter(|r| r.from == from)
        .or(entry.record)
    else {
        return;
    };
    if !set_combined(chats, target, &record) {
        return;
    }
    if let Some(housekeeper) = &state.housekeeper {
        let moved = if let Some(moved) = entry.moved {
            moved
        } else {
            let Ok(moved) = housekeeper.plan_retarget(from, target) else {
                return;
            };
            if !change_retarget(chats, from, target, |r| r.moved = Some(moved.clone())) {
                return;
            }
            moved
        };
        if housekeeper.apply_retarget(from, target, &moved).is_err() {
            return;
        }
        record.kept = moved;
        if !set_combined(chats, target, &record) {
            return;
        }
    }
    note_retarget(chats, from, target, false);
}

pub(crate) fn retry_retargets(state: &AppState) {
    reconcile_retargets(state, false);
}

pub(crate) fn reconcile_retargets(state: &AppState, startup: bool) {
    let chats = &state.chats;
    let entries = retargets(chats);
    for entry in entries {
        let saved = if entry.committed {
            Some(true)
        } else {
            crate::store_now(state, |store| {
                if !store.unreadable.is_empty() || !store.trash_complete() {
                    return Ok(None);
                }
                let archived = combined_of(chats, &entry.target)
                    .is_some_and(|record| record.from == entry.from);
                if store.find_note(&entry.from).is_some() {
                    return Ok((entry.record.is_none() && entry.moved.is_none()).then_some(false));
                }
                let saved = store.trashed_ids().contains(&entry.from)
                    && store.find_note(&entry.target).is_some_and(|note| {
                        archived || entry.version.as_deref() == Some(&version_of(note))
                    });
                Ok(saved.then_some(true))
            })
            .unwrap_or(None)
        };
        let _audio = audio(state);
        if saved == Some(true)
            && change_retarget(chats, &entry.from, &entry.target, |r| r.committed = true)
        {
            move_kept(state, &entry.from, &entry.target);
        } else if startup && saved == Some(false) {
            note_retarget(chats, &entry.from, &entry.target, false);
        }
    }
}

fn set_combined(chats: &std::path::Path, target: &str, record: &Combined) -> bool {
    crate::margin::set_state(chats, &format!("{COMBINED}{target}"), record)
}

fn combined_of(chats: &std::path::Path, target: &str) -> Option<Combined> {
    crate::margin::state_of(chats, &format!("{COMBINED}{target}"))
}

fn forget_combined(chats: &std::path::Path, target: &str) -> bool {
    crate::db::for_chats(chats)
        .with(|c| {
            c.execute(
                "DELETE FROM margin_state WHERE key = ?1",
                [format!("{COMBINED}{target}")],
            )
        })
        .is_ok()
}

pub(crate) fn forget_old_combines(chats: &std::path::Path, before: chrono::DateTime<chrono::Utc>) {
    let db = crate::db::for_chats(chats);
    let Ok(rows) = db.with(|c| {
        c.prepare("SELECT key, data FROM margin_state WHERE key LIKE 'combined:%'")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()
    }) else {
        return;
    };
    #[cfg(test)]
    {
        let hook = {
            let mut hooks = AFTER_COPY.lock().unwrap_or_else(|e| e.into_inner());
            let key = format!("forget:{}", chats.display());
            hooks
                .iter()
                .position(|(id, _)| *id == key)
                .map(|at| hooks.remove(at).1)
        };
        if let Some(hook) = hook {
            hook();
        }
    }
    for (key, data) in rows {
        if serde_json::from_str::<Combined>(&data).map_or(true, |record| record.at < before) {
            let _ = db.with(|c| {
                c.execute(
                    "DELETE FROM margin_state WHERE key = ?1 AND data = ?2",
                    [&key, &data],
                )
            });
        }
    }
}

pub(crate) async fn uncombine(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Uncombine>,
) -> Response {
    let chats = state.chats.clone();
    let housekeeper = state.housekeeper.clone();
    let undoing = state.clone();
    let looked = (
        chats.clone(),
        state.notes_dir.clone(),
        into.clone(),
        body.from.clone(),
    );
    let checked = tokio::task::spawn_blocking(move || {
        let (chats, notes, into, from) = looked;
        combined_of(&chats, &into)
            .filter(|record| record.from == from)
            .map(|record| (check_created(&notes, &into, &record.created), record.at))
    })
    .await
    .unwrap_or(None);
    let done = state
        .with_store(move |store| {
            store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
            let from = body.from;
            let kept = store.find_note(&from).is_some() || store.trashed_ids().contains(&from);
            if !kept {
                return Err(StatusCode::CONFLICT);
            }
            let record = combined_of(&chats, &into).filter(|r| r.from == from);
            if let (Some(record), Some((checked, at))) = (&record, &checked) {
                if record.at == *at {
                    remove_checked(&store.notes_dir, &into, checked)
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                }
            }
            let _audio = audio(&undoing);
            let noted = retargets(&chats)
                .into_iter()
                .find(|r| r.from == from && r.target == into);
            if let (Some(housekeeper), Some(moved)) = (
                housekeeper.as_ref(),
                noted.as_ref().and_then(|entry| entry.moved.as_deref()),
            ) {
                housekeeper
                    .restore_kept(moved)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            note_retarget(&chats, &from, &into, false);
            let Some(record) = record else {
                return Ok(());
            };
            if let Some(housekeeper) = housekeeper.as_ref().filter(|_| !record.kept.is_empty()) {
                housekeeper
                    .restore_kept(&record.kept)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            forget_combined(&chats, &into);
            Ok(())
        })
        .await;
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(StatusCode::CONFLICT) => error(
            StatusCode::CONFLICT,
            "The note that was combined is gone, so this cannot be undone.",
        ),
        Err(code) => error(
            code,
            "The recordings and original files could not be moved back.",
        ),
    }
}
