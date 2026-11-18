use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::AppState;

fn no_settings() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "Settings are not available from this server." })),
    )
        .into_response()
}

pub(crate) async fn get_settings(State(state): State<AppState>) -> Response {
    settings_page(state, None).await
}

async fn settings_page(state: AppState, note: Option<String>) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = state.notes_dir.clone();
    let usage = std::sync::Arc::clone(&state.usage);
    let described = tokio::task::spawn_blocking(move || {
        let mut page = settings.describe(&notes_dir);
        page["felix_days"] = serde_json::json!(usage.days());
        page["felix_recent"] = serde_json::json!(usage.recent());
        page
    });
    match described.await {
        Ok(mut page) => {
            if let Some(note) = note {
                page["note"] = serde_json::json!(note);
            }
            Json(page).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn writing(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let probe = query.get("probe").is_some_and(|asked| asked == "signin");
    match tokio::task::spawn_blocking(move || settings.writing(probe)).await {
        Ok(view) => Json(view).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn refresh_usage(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Response {
    let claude = serde_json::from_slice::<serde_json::Value>(&body)
        .is_ok_and(|asked| asked["claude"] == true);
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let Ok(permit) = state.usage_refreshes.clone().try_acquire_owned() else {
        return (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "Usage is already refreshing. Wait a moment and try again." }))).into_response();
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        settings.refresh_usage(claude)
    })
    .await
    {
        Ok(note) => settings_page(state, note).await,
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Could not refresh usage. Try again in a moment." })),
        )
            .into_response(),
    }
}

pub(crate) async fn change_setting(
    State(state): State<AppState>,
    Json(change): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = state.notes_dir.clone();
    let warm = std::sync::Arc::clone(&state.warm);
    let usage = std::sync::Arc::clone(&state.usage);
    let done = tokio::task::spawn_blocking(move || {
        settings.apply(&change).map(|message| {
            warm.clear();
            let mut page = settings.describe(&notes_dir);
            page["felix_days"] = serde_json::json!(usage.days());
            page["felix_recent"] = serde_json::json!(usage.recent());
            (message, page)
        })
    })
    .await;
    match done {
        Ok(Ok((message, page))) => {
            Json(serde_json::json!({ "message": message, "settings": page })).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn test_setting(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let task = body
        .get("task")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let Ok(slot) = state.setting_tests.clone().acquire_owned().await else {
        return crate::reply::error(StatusCode::SERVICE_UNAVAILABLE, "leo is stopping.");
    };
    let (_guard, stop) = crate::stop_when_dropped();
    match tokio::task::spawn_blocking(move || {
        let _slot = slot;
        leo_core::children::with_stop(stop, || settings.test(&task))
    })
    .await
    {
        Ok(Ok(message)) => Json(serde_json::json!({ "message": message })).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
