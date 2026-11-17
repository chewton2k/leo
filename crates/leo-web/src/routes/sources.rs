use crate::reply::error;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use leo_core::recording;
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct Editing(Arc<AtomicBool>);
impl Drop for Editing {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn outside<R: Send + 'static>(
    state: &AppState,
    id: Option<String>,
    work: impl FnOnce(std::path::PathBuf, Option<leo_core::notes::Note>) -> Result<R, StatusCode>
        + Send
        + 'static,
) -> Result<R, StatusCode> {
    let (notes, note) = state
        .read_store(move |store| {
            let note = match id {
                Some(id) => Some(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?.clone()),
                None => None,
            };
            Ok((store.notes_dir.clone(), note))
        })
        .await?;
    tokio::task::spawn_blocking(move || work(notes, note))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

#[derive(Default, Deserialize)]
pub(crate) struct SourceQuery {
    #[serde(default)]
    q: String,
}

pub(crate) async fn sources(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SourceQuery>,
) -> Response {
    match outside(&state, Some(id.clone()), move |notes, _| {
        let sources = recording::load(&notes, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let words = leo_core::notes::question_words(&query.q);
        let mut matches = Vec::new();
        for source in &sources {
            for (index, p) in source.passages.iter().enumerate() {
                let lower = p.text.to_lowercase();
                let score: usize = words.iter().map(|w| lower.matches(w.as_str()).count()).sum();
                if score > 0 { matches.push(serde_json::json!({"source": source.id, "index": index, "score": score, "passage": p})); }
            }
        }
        matches.sort_by_key(|m| std::cmp::Reverse(m["score"].as_u64().unwrap_or(0)));
        matches.truncate(10);
        let versions = sources.iter().map(|s| (s.id.clone(), source_version(s))).collect::<std::collections::BTreeMap<_,_>>();
        Ok(serde_json::json!({"sources": sources, "matches": matches, "versions":versions}))
    }).await {
        Ok(value) => Json(value).into_response(), Err(code) => error(code, "Could not read this recording"),
    }
}

#[derive(Deserialize)]
pub(crate) struct Regenerate {
    #[serde(default)]
    wants: Option<String>,
}

pub(crate) async fn regenerate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Regenerate>,
) -> Response {
    let Some(regenerate) = state.regenerator.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Note generation is not available",
        );
    };
    let input = outside(&state, Some(id.clone()), move |notes, note| {
        let note = note.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let sources =
            recording::load(&notes, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if sources.is_empty() {
            return Err(StatusCode::NOT_FOUND);
        }
        let joined = |part: fn(&recording::Archive) -> &str| {
            sources
                .iter()
                .map(part)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let profile = recording::Profile {
            context: joined(|s| s.context.as_str()),
            wants: body.wants.unwrap_or_else(|| joined(|s| s.wants.as_str())),
            ..Default::default()
        }
        .checked()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        Ok((sources, profile, super::notes::version_of(note)))
    })
    .await;
    let (sources, profile, base) = match input {
        Ok(v) => v,
        Err(code) => {
            return error(
                code,
                "This note needs a kept recording transcript, and what you want must be under 4,000 characters",
            )
        }
    };
    match tokio::task::spawn_blocking(move || regenerate(sources, profile)).await {
        Ok(Ok((title, body))) => {
            Json(serde_json::json!({"title": title, "body": body, "base": base})).into_response()
        }
        Ok(Err(e)) => error(
            StatusCode::BAD_GATEWAY,
            leo_core::writing_said(&e.to_string()),
        ),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not regenerate this note",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct RawSource {
    base: String,
    source: String,
    points: Vec<recording::Point>,
    passages: Vec<recording::Passage>,
}

pub(crate) async fn edit_sources(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RawSource>,
) -> Response {
    if body.passages.len() > 2000
        || body.points.len() > 500
        || body
            .passages
            .iter()
            .any(|p| p.text.len() > 100_000 || p.start_secs > p.end_secs)
        || body.points.iter().any(|p| p.text.len() > 8000)
    {
        return error(
            StatusCode::BAD_REQUEST,
            "Recording source is too large or has invalid times",
        );
    }
    if state.source_writing.swap(true, Ordering::AcqRel) {
        return error(
            StatusCode::CONFLICT,
            "Another transcript correction is being saved. Try again in a moment.",
        );
    }
    let _editing = Editing(state.source_writing.clone());
    match outside(&state, Some(id.clone()), move |notes, _| {
        let mut sources =
            recording::load(&notes, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let source = sources
            .iter_mut()
            .find(|s| s.id == body.source)
            .ok_or(StatusCode::NOT_FOUND)?;
        if source_version(source) != body.base {
            return Err(StatusCode::CONFLICT);
        }
        let backup = leo_core::paths::contained_path(
            &recording::root(&notes),
            &std::path::Path::new(&id).join(format!("{}.original", source.id)),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        if !backup.exists() {
            recording::write_json(&backup, source)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        source.points = body.points;
        source.passages = body.passages;
        recording::save(&notes, &id, source).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    {
        Ok(()) => Json(serde_json::json!({"saved": true})).into_response(),
        Err(code) => error(code, "Could not save original notes"),
    }
}

fn source_version(source: &recording::Archive) -> String {
    let text = serde_json::to_vec(source).unwrap_or_default();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in text {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}
