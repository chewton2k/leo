use std::collections::VecDeque;
use std::sync::Mutex;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::routes::felix::change_body;
use crate::routes::notes::save;
use crate::AppState;

const REMEMBERED: usize = 32;
pub(crate) const MOST_REQUEST: usize = 64;

type Done = Mutex<VecDeque<(String, String, serde_json::Value)>>;

static DONE: Done = Mutex::new(VecDeque::new());
static CREATED: Done = Mutex::new(VecDeque::new());

#[derive(Deserialize)]
pub(crate) struct InsertBody {
    #[serde(default)]
    text: String,
    #[serde(default)]
    job: Option<String>,
    #[serde(default)]
    line: Option<usize>,
    base: String,
    #[serde(default)]
    request: Option<String>,
}

pub(crate) fn inserted(body: &str, text: &str, line: Option<usize>) -> String {
    let text = text.trim_matches('\n');
    let Some(line) = line else {
        let base = body.trim_end();
        return if base.is_empty() {
            format!("{text}\n")
        } else {
            format!("{base}\n\n{text}\n")
        };
    };
    let lines: Vec<&str> = body.split('\n').collect();
    let at = line.min(lines.len());
    let head = lines[..at].join("\n");
    let tail = lines[at..].join("\n");
    let head = head.trim_end_matches('\n');
    let tail = tail.trim_start_matches('\n');
    match (head.is_empty(), tail.is_empty()) {
        (true, true) => format!("{text}\n"),
        (true, false) => format!("{text}\n\n{tail}"),
        (false, true) => format!("{head}\n\n{text}\n"),
        (false, false) => format!("{head}\n\n{text}\n\n{tail}"),
    }
}

pub(crate) fn requested(request: Option<String>) -> Option<String> {
    request.filter(|r| !r.is_empty() && r.len() <= MOST_REQUEST)
}

fn find(queue: &Done, note: &str, request: &str) -> Option<serde_json::Value> {
    let done = queue.lock().unwrap_or_else(|e| e.into_inner());
    done.iter()
        .find(|(n, r, _)| n == note && r == request)
        .map(|(_, _, v)| v.clone())
}

fn push(queue: &Done, note: &str, request: &str, value: &serde_json::Value) {
    let mut done = queue.lock().unwrap_or_else(|e| e.into_inner());
    done.push_back((note.to_string(), request.to_string(), value.clone()));
    while done.len() > REMEMBERED {
        done.pop_front();
    }
}

pub(crate) fn remembered(note: &str, request: &str) -> Option<serde_json::Value> {
    find(&DONE, note, request)
}

pub(crate) fn remember(note: &str, request: &str, value: &serde_json::Value) {
    push(&DONE, note, request, value);
}

pub(crate) fn remembered_creation(scope: &str, request: &str) -> Option<serde_json::Value> {
    find(&CREATED, scope, request)
}

pub(crate) fn remember_creation(scope: &str, request: &str, value: &serde_json::Value) {
    push(&CREATED, scope, request, value);
}

pub(crate) async fn insert_into_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut body): Json<InsertBody>,
) -> Response {
    if let Some(job) = body.job.as_deref() {
        let section = state
            .imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(job)
            .filter(|job| job.state == "done")
            .and_then(|job| job.section.clone());
        let Some(section) = section else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "That upload is not there any more, so leo has nothing to add. Upload the file again." })),
            )
                .into_response();
        };
        body.text = section;
    }
    if body.text.trim().is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let request = requested(body.request);
    let key = id.clone();
    if let Some(found) = request.as_deref().and_then(|r| remembered(&key, r)) {
        return Json(found).into_response();
    }
    let note_id = id.clone();
    let done = state
        .with_store(move |store| {
            let (applied, ()) = change_body(store, &note_id, Some(&body.base), |text| {
                Ok((inserted(text, &body.text, body.line), ()))
            })?;
            save(store)?;
            Ok(applied)
        })
        .await;
    match done {
        Ok(applied) => {
            let value = serde_json::json!(applied);
            if let Some(request) = request.as_deref() {
                remember(&key, request, &value);
            }
            Json(value).into_response()
        }
        Err(StatusCode::CONFLICT) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "The note changed since you chose where to add this. Look at it again and add it once more." })),
        )
            .into_response(),
        Err(code) => code.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_goes_at_the_end_or_after_the_given_line_as_its_own_block() {
        assert_eq!(inserted("", "New", None), "New\n");
        assert_eq!(inserted("A\n\n", "New", None), "A\n\nNew\n");
        assert_eq!(inserted("A\nB\nC", "New", Some(1)), "A\n\nNew\n\nB\nC");
        assert_eq!(inserted("A\nB", "New", Some(0)), "New\n\nA\nB");
        assert_eq!(inserted("A\nB", "New", Some(99)), "A\nB\n\nNew\n");
        assert_eq!(inserted("A\n\nB", "\n\nNew\n", Some(1)), "A\n\nNew\n\nB");
    }

    #[test]
    fn creations_and_margin_requests_never_evict_each_other() {
        let value = serde_json::json!(1);
        remember("kept-note", "kept-request", &value);
        for n in 0..REMEMBERED * 2 {
            remember_creation("card:board", &format!("made-{n}"), &value);
        }
        assert!(remembered("kept-note", "kept-request").is_some());
        remember_creation("task", "kept-task", &value);
        for n in 0..REMEMBERED * 2 {
            remember("note", &format!("reply-{n}"), &value);
        }
        assert!(remembered_creation("task", "kept-task").is_some());
        assert!(remembered_creation("card:board", "made-0").is_none());
    }
}
