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
    let done = state
        .with_store(move |store| {
            let from = store.find_note(&body.with).ok_or(StatusCode::NOT_FOUND)?;
            if version_of(from) != body.with_base {
                return Err(StatusCode::CONFLICT);
            }
            let (from, from_title) = (from.id.clone(), from.title.clone());
            let note = store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
            if note.id == from {
                return Err(StatusCode::BAD_REQUEST);
            }
            if version_of(note) != body.base {
                return Err(StatusCode::CONFLICT);
            }
            let target = note.id.clone();
            let notes_dir = store.notes_dir.clone();
            let copied = leo_core::recording::copy_to(&notes_dir, &from, &target, &from_title)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let note = store.find_note_mut(&target).ok_or(StatusCode::NOT_FOUND)?;
            let before = std::mem::replace(&mut note.body, body.body);
            note.updated_at = chrono::Utc::now();
            let response = NoteResponse::from_note(note);
            store.delete_notes(std::slice::from_ref(&from));
            if let Err(code) = save(store) {
                let _ = leo_core::recording::remove(&notes_dir, &target, &copied);
                return Err(code);
            }
            Ok(serde_json::json!({ "note": response, "before": before, "removed": from, "sources": copied }))
        })
        .await;
    match done {
        Ok(value) => Json(value).into_response(),
        Err(StatusCode::CONFLICT) => error(
            StatusCode::CONFLICT,
            "One of the notes changed while they were being combined. Combine them again.",
        ),
        Err(code) => error(code, "The combined note could not be saved."),
    }
}

#[derive(Deserialize)]
pub(crate) struct Uncombine {
    #[serde(default)]
    sources: Vec<String>,
}

pub(crate) async fn uncombine(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Uncombine>,
) -> Response {
    if body.sources.len() > 1000 {
        return error(StatusCode::BAD_REQUEST, "Too many recordings.");
    }
    let done = state
        .with_store(move |store| {
            store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
            leo_core::recording::remove(&store.notes_dir, &into, &body.sources)
                .map_err(|_| StatusCode::BAD_REQUEST)
        })
        .await;
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => error(code, "The recordings could not be moved back."),
    }
}
