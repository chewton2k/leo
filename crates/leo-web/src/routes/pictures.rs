use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use leo_core::attachments;
use serde::Deserialize;

use crate::AppState;

pub(crate) const PICTURE_BYTES: usize = attachments::MOST_BYTES / 3 * 4 + 64 * 1024;

#[derive(Deserialize)]
pub(crate) struct PictureAt {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) from: String,
}

pub(crate) async fn get_picture(
    State(state): State<AppState>,
    Query(at): Query<PictureAt>,
) -> Response {
    let notes_dir = state.notes_dir.clone();
    let found = tokio::task::spawn_blocking(move || {
        let path = attachments::resolve(&notes_dir, &at.from, &at.path)?;
        let bytes = std::fs::read(path).ok()?;
        let kind = attachments::kind_of(&bytes)?;
        Some((attachments::mime_of(kind)?, bytes))
    })
    .await;
    match found {
        Ok(Some((mime, bytes))) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct NewPicture {
    pub(crate) name: String,
    pub(crate) data: String,
}

pub(crate) async fn add_picture(
    State(state): State<AppState>,
    Json(body): Json<NewPicture>,
) -> Response {
    let data = body.data;
    let decoded = tokio::task::spawn_blocking(move || {
        base64::engine::general_purpose::STANDARD.decode(data.as_bytes())
    })
    .await;
    let Ok(Ok(bytes)) = decoded else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "That picture did not arrive whole." })),
        )
            .into_response();
    };
    if attachments::kind_of(&bytes).is_none() || bytes.len() > attachments::MOST_BYTES {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "leo can show PNG, JPEG, GIF and WebP pictures up to 20 MB." })),
        )
            .into_response();
    }
    let name = body.name;
    let notes_dir = state.notes_dir.clone();
    match tokio::task::spawn_blocking(move || attachments::save(&notes_dir, &name, &bytes)).await {
        Ok(Ok(path)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "path": path })),
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("leo could not save the picture: {e:#}") })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
