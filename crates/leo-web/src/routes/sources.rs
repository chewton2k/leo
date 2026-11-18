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
    super::off(move || work(notes, note)).await?
}

#[derive(Default, Deserialize)]
pub(crate) struct SourceQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    passages: Option<String>,
    #[serde(default)]
    points: Option<String>,
}

fn cursor(value: Option<&str>) -> Result<Option<u64>, StatusCode> {
    match value {
        None => Ok(Some(0)),
        Some("end") => Ok(None),
        Some(n) => n.parse().map(Some).map_err(|_| StatusCode::BAD_REQUEST),
    }
}

fn joined<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) async fn sources(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SourceQuery>,
) -> Response {
    match outside(&state, Some(id.clone()), move |notes, _| {
        let failed = |_| StatusCode::INTERNAL_SERVER_ERROR;
        let heads = recording::list(&notes, &id).map_err(failed)?;
        let words = leo_core::notes::question_words(&query.q);
        let mut matches = Vec::new();
        let best = |matches: &mut Vec<(usize, serde_json::Value)>| {
            matches.sort_by_key(|m| std::cmp::Reverse(m.0));
            matches.truncate(10);
        };
        if !words.is_empty() {
            for head in &heads {
                recording::rows(
                    &notes,
                    &id,
                    head,
                    |seq, p| {
                        let lower = p.text.to_lowercase();
                        let score: usize =
                            words.iter().map(|w| lower.matches(w.as_str()).count()).sum();
                        if score > 0 {
                            matches.push((
                                score,
                                serde_json::json!({"source": head.id, "seq": seq, "score": score, "passage": p}),
                            ));
                            if matches.len() > 100 {
                                best(&mut matches);
                            }
                        }
                        Ok(true)
                    },
                    |_, _| Ok(false),
                )
                .map_err(failed)?;
            }
        }
        best(&mut matches);
        let page = match query.at.clone().or_else(|| heads.first().map(|h| h.id.clone())) {
            Some(at) => Some(
                recording::page(
                    &notes,
                    &id,
                    &at,
                    recording::Cursor {
                        passages: cursor(query.passages.as_deref())?,
                        points: cursor(query.points.as_deref())?,
                    },
                )
                .map_err(|_| StatusCode::NOT_FOUND)?,
            ),
            None => None,
        };
        let wants = joined(heads.iter().map(|h| h.wants.as_str()));
        let mut list = serde_json::to_value(&heads).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for head in list.as_array_mut().into_iter().flatten() {
            if let Some(head) = head.as_object_mut() {
                head.remove("trace");
            }
        }
        let matches: Vec<_> = matches.into_iter().map(|m| m.1).collect();
        Ok(serde_json::json!({"list": list, "wants": wants, "page": page, "matches": matches}))
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
            recording::Sources::of(&notes, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if sources.heads.is_empty() {
            return Err(StatusCode::NOT_FOUND);
        }
        let profile = recording::Profile {
            context: joined(sources.heads.iter().map(|h| h.context.as_str())),
            wants: body
                .wants
                .unwrap_or_else(|| joined(sources.heads.iter().map(|h| h.wants.as_str()))),
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
    let Ok(slot) = state.regenerating.clone().acquire_owned().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "leo is stopping.");
    };
    let (_guard, stop) = crate::stop_when_dropped();
    match tokio::task::spawn_blocking(move || {
        let _slot = slot;
        leo_core::children::with_stop(stop, || regenerate(sources, profile))
    })
    .await
    {
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
pub(crate) struct Fix {
    source: String,
    base: u64,
    changes: Vec<recording::Change>,
}

pub(crate) async fn edit_sources(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Fix>,
) -> Response {
    if state.source_writing.swap(true, Ordering::AcqRel) {
        return error(
            StatusCode::CONFLICT,
            "Another transcript correction is being saved. Try again in a moment.",
        );
    }
    let _editing = Editing(state.source_writing.clone());
    match outside(&state, Some(id.clone()), move |notes, _| {
        recording::edit(&notes, &id, &body.source, body.base, &body.changes).map_err(|e| {
            match e.downcast_ref::<recording::Refused>() {
                Some(recording::Refused::Stale) => StatusCode::CONFLICT,
                Some(recording::Refused::NoRow) => StatusCode::BAD_REQUEST,
                None if e
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    StatusCode::NOT_FOUND
                }
                None => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })
    })
    .await
    {
        Ok(head) => {
            Json(serde_json::json!({"saved": true, "revision": head.revision})).into_response()
        }
        Err(code) => error(code, "Could not save original notes"),
    }
}
