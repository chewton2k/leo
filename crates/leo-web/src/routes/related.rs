use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use leo_core::store::Store;
use serde::Deserialize;

use crate::AppState;

pub(crate) const MOST_RELATED: usize = 20;
const MOST_POSITIONS: i64 = 2_000;
const MOST_ANCHOR_BYTES: usize = 1_024;

#[derive(Deserialize)]
pub(crate) struct RelatedQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

fn linked_titles(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        let title = inner.split(['|', '#']).next().unwrap_or("").trim();
        if !title.is_empty() && !inner.contains('\n') {
            out.push(title.to_lowercase());
        }
        rest = &after[end + 2..];
    }
    out
}

pub(crate) fn related(
    store: &Store,
    cache: &crate::graph::Cache,
    titles: &crate::titles::Titles,
    id: &str,
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(id.to_string());
    let mut out = Vec::new();
    for (note, _kind, why) in crate::chat::connected(store, cache, id) {
        if !seen.insert(note.id.clone()) {
            continue;
        }
        out.push(serde_json::json!({ "id": note.id, "title": note.title, "folder": note.directory, "why": why }));
    }
    if let Some(this) = store.notes.iter().find(|n| n.id == id) {
        for title in linked_titles(&this.body) {
            if let Some(note) = titles.exact(store, &title) {
                if seen.insert(note.id.clone()) {
                    out.push(serde_json::json!({ "id": note.id, "title": note.title, "folder": note.directory, "why": "Linked from this note" }));
                }
            }
        }
    }
    out.truncate(limit);
    out
}

pub(crate) async fn related_notes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RelatedQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(4).clamp(1, MOST_RELATED);
    let graphs = Arc::clone(&state.graphs);
    let titles = Arc::clone(&state.titles);
    let found = state
        .read_store(move |store| {
            if !store.notes.iter().any(|n| n.id == id) {
                return Err(StatusCode::NOT_FOUND);
            }
            let cache = graphs.load();
            Ok(related(store, &cache, &titles, &id, limit + 1))
        })
        .await;
    match found {
        Ok(mut notes) => {
            let more = notes.len() > limit;
            notes.truncate(limit);
            Json(serde_json::json!({ "notes": notes, "more": more })).into_response()
        }
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn get_reading(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if !crate::chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let anchor = tokio::task::spawn_blocking(move || {
        crate::db::try_for_chats(&chats)?.with(|c| {
            c.query_row("SELECT anchor FROM reading WHERE note = ?1", [&id], |r| {
                r.get::<_, String>(0)
            })
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
        })
    })
    .await
    .ok()
    .and_then(Result::ok)
    .flatten()
    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    Json(serde_json::json!({ "anchor": anchor })).into_response()
}

#[derive(Deserialize)]
pub(crate) struct ReadingBody {
    anchor: serde_json::Value,
}

pub(crate) async fn put_reading(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadingBody>,
) -> Response {
    let text = body.anchor.to_string();
    if !crate::chats::valid_id(&id) || !body.anchor.is_object() || text.len() > MOST_ANCHOR_BYTES {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let now = chrono::Utc::now().to_rfc3339();
    let saved = tokio::task::spawn_blocking(move || {
        crate::db::try_for_chats(&chats)?.with(|c| {
        c.execute(
            "INSERT INTO reading (note, anchor, at) VALUES (?1, ?2, ?3) ON CONFLICT(note) DO UPDATE SET anchor = excluded.anchor, at = excluded.at",
            [&id, &text, &now],
        )?;
        c.execute(
            "DELETE FROM reading WHERE note NOT IN (SELECT note FROM reading ORDER BY at DESC LIMIT ?1)",
            [MOST_POSITIONS],
        )
        })
    })
    .await;
    match saved {
        Ok(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_are_read_without_labels_or_sections() {
        assert_eq!(
            linked_titles(
                "See [[Heaps]], [[Graphs|the graph note]] and [[Lecture#Part 2]]. [[broken"
            ),
            ["heaps", "graphs", "lecture"]
        );
    }
}
