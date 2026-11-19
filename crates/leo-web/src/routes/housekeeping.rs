use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{keeping, AppState};

fn keep_page(keep: leo_core::keep::Keep) -> serde_json::Value {
    let choices = leo_core::keep::CHOICES
        .iter()
        .map(|d| serde_json::json!({ "days": d, "label": leo_core::keep::describe(*d) }))
        .collect::<Vec<_>>();
    serde_json::json!({
        "trash_days": keep.trash_days,
        "chat_days": keep.chat_days,
        "upload_days": keep.upload_days,
        "transcript_days": keep.transcript_days,
        "audio_days": keep.audio_days,
        "choices": choices,
    })
}

pub(crate) async fn get_keep(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .read_store(|store| Ok(Json(keep_page(leo_core::keep::load(&store.notes_dir)))))
        .await
}

#[derive(serde::Deserialize)]
pub(crate) struct Shorter {
    field: String,
    days: Option<u32>,
}

pub(crate) async fn preview_keep(
    State(state): State<AppState>,
    Json(body): Json<Shorter>,
) -> Response {
    if !leo_core::keep::CHOICES.contains(&body.days) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let count = keeping::would_delete(&state, &body.field, body.days).await;
    Json(serde_json::json!({ "count": count })).into_response()
}

pub(crate) async fn set_keep(
    State(state): State<AppState>,
    Json(keep): Json<leo_core::keep::Keep>,
) -> Response {
    let days = [
        keep.trash_days,
        keep.chat_days,
        keep.upload_days,
        keep.transcript_days,
        keep.audio_days,
    ];
    if !days.iter().all(|d| leo_core::keep::CHOICES.contains(d)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Pick one of the choices offered." })),
        )
            .into_response();
    }
    let done = state
        .with_store(move |store| {
            Ok(leo_core::keep::save(&store.notes_dir, &keep)
                .map(|()| keep_page(keep))
                .map_err(|e| format!("{e:#}")))
        })
        .await;
    match done {
        Ok(Ok(page)) => {
            tokio::task::spawn_blocking(move || keeping::apply_choices(&state));
            Json(page).into_response()
        }
        Ok(Err(cause)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("leo could not save that: {cause}") })),
        )
            .into_response(),
        Err(code) => code.into_response(),
    }
}
