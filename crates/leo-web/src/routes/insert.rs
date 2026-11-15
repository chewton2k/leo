use std::collections::VecDeque;
use std::sync::Mutex;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::routes::felix::Applied;
use crate::routes::notes::{save, version_of, NoteResponse};
use crate::AppState;

pub(crate) const MOST_INSERT: usize = 200_000;
const REMEMBERED: usize = 32;
pub(crate) const MOST_REQUEST: usize = 64;

static DONE: Mutex<VecDeque<(String, String, serde_json::Value)>> = Mutex::new(VecDeque::new());

#[derive(Deserialize)]
pub(crate) struct InsertBody {
    text: String,
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

pub(crate) fn remembered(note: &str, request: &str) -> Option<serde_json::Value> {
    let done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    done.iter()
        .find(|(n, r, _)| n == note && r == request)
        .map(|(_, _, v)| v.clone())
}

pub(crate) fn remember(note: &str, request: &str, value: &serde_json::Value) {
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    done.push_back((note.to_string(), request.to_string(), value.clone()));
    while done.len() > REMEMBERED {
        done.pop_front();
    }
}

pub(crate) async fn insert_into_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InsertBody>,
) -> Response {
    if body.text.trim().is_empty() || body.text.chars().count() > MOST_INSERT {
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
            let note = store.find_note_mut(&note_id).ok_or(StatusCode::NOT_FOUND)?;
            if version_of(note) != body.base {
                return Err(StatusCode::CONFLICT);
            }
            let next = inserted(&note.body, &body.text, body.line);
            let before = std::mem::replace(&mut note.body, next);
            note.updated_at = chrono::Utc::now();
            let applied = Applied {
                note: NoteResponse::from_note(note),
                before,
            };
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
}
