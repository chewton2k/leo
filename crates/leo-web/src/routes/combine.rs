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
    #[serde(skip)]
    pub(crate) started: Option<Instant>,
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
    if into.body.chars().count() + from.body.chars().count() > combine::MOST_CHARS {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "These notes exceed the combine limit. Combine fewer sections at a time.",
        ));
    }
    Ok((into, from, notes_dir))
}

fn combined(
    writer: &crate::graph::Writer,
    into: &Note,
    from: &Note,
    notes_dir: &std::path::Path,
    request_chars: usize,
) -> Result<serde_json::Value, &'static str> {
    let made = combine::write(writer, notes_dir, into, from, request_chars)?;
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
    let id = uuid::Uuid::new_v4().to_string();
    let job = CombineJob {
        id: id.clone(),
        into: into.id.clone(),
        with: from.id.clone(),
        into_title: into.title.clone(),
        with_title: from.title.clone(),
        state: "working",
        result: None,
        error: String::new(),
        started: Some(Instant::now()),
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
            std::sync::Arc::new(move |system: &str, user: &str, most: u32| {
                let started = Instant::now();
                let reply = stream(
                    system,
                    user,
                    most,
                    &mut |piece| meter.add(piece),
                    &mut || meter.restart(),
                    &|| false,
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
    let request_chars = state.graphs.scale().batch_chars;
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
            let done = combined(&writer, &into, &from, &notes_dir, request_chars);
            if let Some(job) = jobs.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&id) {
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
    if jobs.get(&job).is_some_and(|j| j.state != "working") {
        jobs.remove(&job);
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
    if body.body.trim().is_empty() || body.body.chars().count() > combine::MOST_CHARS * 2 {
        return error(
            StatusCode::BAD_REQUEST,
            "The combined note is empty or too long.",
        );
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
            let copied = leo_core::recording::copy_to(&notes_dir, &from, &target, &from_title)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            match share_originals(&notes_dir, &from, &target) {
                Ok(originals) => Ok((copied, originals)),
                Err(_) => {
                    let _ = leo_core::recording::remove(&notes_dir, &target, &copied);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        })
        .await
    };
    let (copied, originals) = match copying {
        Ok(Ok(copies)) => copies,
        Ok(Err(code)) => return error(code, "The combined note could not be saved."),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The combined note could not be saved.",
            )
        }
    };
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
    let done = state
        .with_store(move |store| {
            let undo = |store: &leo_core::store::Store| {
                let _ = leo_core::recording::remove(&store.notes_dir, &target, &copied);
                let _ = remove_originals(&store.notes_dir, &target, &originals);
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
            store.delete_notes(std::slice::from_ref(&from));
            if let Err(code) = save(store) {
                undo(store);
                return Err(code);
            }
            let record = Combined {
                from: from.clone(),
                originals,
                at: chrono::Utc::now(),
            };
            Ok((
                target,
                record,
                serde_json::json!({ "note": response, "before": before, "removed": from }),
            ))
        })
        .await;
    match done {
        Ok((target, record, value)) => {
            let chats = state.chats.clone();
            let _ =
                tokio::task::spawn_blocking(move || set_combined(&chats, &target, &record)).await;
            Json(value).into_response()
        }
        Err(StatusCode::CONFLICT) => changed_while_combining(),
        Err(code) => error(code, "The combined note could not be saved."),
    }
}

fn changed_while_combining() -> Response {
    error(
        StatusCode::CONFLICT,
        "One of the notes changed while they were being combined. Combine them again.",
    )
}

#[cfg(test)]
pub(crate) type AfterCopy = Box<dyn FnOnce() + Send>;

#[cfg(test)]
pub(crate) static AFTER_COPY: std::sync::Mutex<Vec<(String, AfterCopy)>> =
    std::sync::Mutex::new(Vec::new());

#[derive(Deserialize)]
pub(crate) struct Uncombine {
    from: String,
}

#[derive(serde::Serialize, Deserialize)]
pub(crate) struct Combined {
    from: String,
    originals: Vec<String>,
    at: chrono::DateTime<chrono::Utc>,
}

const COMBINED: &str = "combined:";

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
    for (key, data) in rows {
        if serde_json::from_str::<Combined>(&data).map_or(true, |record| record.at < before) {
            let _ = db.with(|c| c.execute("DELETE FROM margin_state WHERE key = ?1", [&key]));
        }
    }
}

pub(crate) async fn uncombine(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Uncombine>,
) -> Response {
    let chats = state.chats.clone();
    let done = state
        .with_store(move |store| {
            store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
            let from = body.from;
            let kept = store.find_note(&from).is_some()
                || leo_core::store::trashed_ids(&store.notes_dir).contains(&from);
            if !kept {
                return Err(StatusCode::CONFLICT);
            }
            let copies: Vec<String> = leo_core::recording::list(&store.notes_dir, &into)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .into_iter()
                .filter(|h| h.copied_from == from)
                .map(|h| h.id)
                .collect();
            leo_core::recording::remove(&store.notes_dir, &into, &copies)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let Some(record) = combined_of(&chats, &into).filter(|r| r.from == from) else {
                return Ok(());
            };
            remove_originals(&store.notes_dir, &into, &record.originals)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
