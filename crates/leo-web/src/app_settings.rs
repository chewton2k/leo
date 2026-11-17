use crate::reply::error;
use crate::AppState;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub struct EventStarting {
    pub uid: String,
    pub title: String,
    pub start: chrono::DateTime<chrono::Utc>,
    pub online: bool,
}
pub trait AppApi: Send + Sync {
    fn view(&self) -> Result<Value>;
    fn change(&self, value: Value) -> Result<Value>;
    fn health(&self, notes: &Path) -> Result<Value>;
    fn event_starting(&self, _event: EventStarting) {}
}
async fn call(
    api: Option<Arc<dyn AppApi>>,
    change: Option<Value>,
    notes: Option<PathBuf>,
    margin: Option<String>,
) -> Response {
    let Some(api) = api else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "These settings are available in the Leo app. Reopen Leo and try again.",
        );
    };
    if change.as_ref().is_some_and(|change| {
        change["action"] == "sign_in"
            && !matches!(change["provider"].as_str(), Some("claude_code" | "codex"))
    }) {
        return error(
            StatusCode::BAD_REQUEST,
            "Choose Claude Code or Codex to sign in.",
        );
    }
    match tokio::task::spawn_blocking(move || {
        if let Some(notes) = notes {
            let mut view = api.health(&notes)?;
            if let (Some(line), Some(sections)) = (margin, view["sections"].as_array_mut()) {
                sections.push(json!({
                    "title": "Felix",
                    "checks": [{ "name": "Reading along", "state": "warn", "detail": line, "fix": "Open Felix in Settings to change how he reads.", "section": "felix" }],
                }));
            }
            Ok(view)
        } else {
            match change {
                Some(change) => api.change(change),
                None => api.view(),
            }
        }
    })
    .await
    {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "App settings could not be opened. Try again in a moment.",
        ),
    }
}
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    call(state.app_settings.clone(), None, None, None).await
}
pub(crate) async fn change(
    State(state): State<AppState>,
    value: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(value)) = value else {
        return error(
            StatusCode::BAD_REQUEST,
            "This app setting could not be read. Choose an option in Settings.",
        );
    };
    call(state.app_settings.clone(), Some(value), None, None).await
}
pub(crate) async fn health(State(state): State<AppState>) -> Response {
    let notes = match state.read_store(|s| Ok(s.notes_dir.clone())).await {
        Ok(notes) => notes,
        Err(status) => return status.into_response(),
    };
    call(
        state.app_settings.clone(),
        None,
        Some(notes),
        state.margin.problem(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake(
        std::sync::Mutex<Vec<serde_json::Value>>,
        std::sync::atomic::AtomicUsize,
    );
    impl AppApi for Fake {
        fn view(&self) -> anyhow::Result<serde_json::Value> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(serde_json::json!({"automatic_updates":true,"install_on_quit":true}))
        }
        fn change(&self, value: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            anyhow::ensure!(
                value["action"] == "check_updates" || value["action"] == "sign_in",
                "Choose an app setting."
            );
            self.0.lock().unwrap().push(value);
            self.view()
        }
        fn health(&self, _notes: &std::path::Path) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!({"sections":[]}))
        }
    }
    #[tokio::test]
    async fn a_margin_problem_shows_one_line_in_health() {
        let api = Some(std::sync::Arc::new(Fake::default()) as std::sync::Arc<dyn AppApi>);
        let response = call(
            api,
            None,
            Some(PathBuf::from("unused")),
            Some("Felix could not read.".into()),
        )
        .await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view["sections"][0]["title"], "Felix");
        assert_eq!(
            view["sections"][0]["checks"][0]["detail"],
            "Felix could not read."
        );
    }

    #[tokio::test]
    async fn app_changes_and_health_are_json_and_validate_actions() {
        let api = Some(std::sync::Arc::new(Fake::default()) as std::sync::Arc<dyn AppApi>);
        assert_eq!(
            call(api.clone(), None, None, None).await.status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            call(
                api.clone(),
                Some(serde_json::json!({"action":"check_updates"})),
                None,
                None
            )
            .await
            .status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            call(
                api.clone(),
                Some(serde_json::json!({"action":"invalid"})),
                None,
                None
            )
            .await
            .status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(api, None, Some(std::path::PathBuf::from("unused")), None)
                .await
                .status(),
            axum::http::StatusCode::OK
        );
    }

    #[tokio::test]
    async fn an_app_change_builds_the_view_once() {
        let fake = std::sync::Arc::new(Fake::default());
        let api = Some(fake.clone() as std::sync::Arc<dyn AppApi>);
        let asked = serde_json::json!({"action":"check_updates"});
        assert_eq!(
            call(api, Some(asked), None, None).await.status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(fake.1.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sign_in_reaches_the_app_only_for_claude_code_or_codex() {
        let fake = std::sync::Arc::new(Fake::default());
        let api = Some(fake.clone() as std::sync::Arc<dyn AppApi>);
        for provider in ["claude_code", "codex"] {
            let asked = serde_json::json!({"action":"sign_in","provider":provider});
            assert_eq!(
                call(api.clone(), Some(asked), None, None).await.status(),
                axum::http::StatusCode::OK
            );
        }
        for asked in [
            serde_json::json!({"action":"sign_in","provider":"ollama"}),
            serde_json::json!({"action":"sign_in","provider":"codex; rm -rf ~"}),
            serde_json::json!({"action":"sign_in"}),
        ] {
            assert_eq!(
                call(api.clone(), Some(asked), None, None).await.status(),
                axum::http::StatusCode::BAD_REQUEST
            );
        }
        let seen: Vec<_> = fake
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["provider"].clone())
            .collect();
        assert_eq!(
            seen,
            [serde_json::json!("claude_code"), serde_json::json!("codex")]
        );
    }
}
