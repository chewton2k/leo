use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::AppState;

const FILE: &str = "ui-prefs.json";
const CONTENT: [&str; 2] = ["leo-chat-v1", "leo-search"];
const MOST_VALUE: usize = 16 * 1024;
const MOST_TOTAL: usize = 128 * 1024;
pub(crate) const BODY_BYTES: usize = MOST_TOTAL + 64 * 1024;
const SCRIPT: &str = include_str!("web/prefs.js");

static WRITING: Mutex<()> = Mutex::new(());

type Prefs = BTreeMap<String, String>;

#[derive(Deserialize)]
pub(crate) struct Change {
    #[serde(default)]
    set: Prefs,
    #[serde(default)]
    remove: Vec<String>,
}

fn key_ok(key: &str) -> bool {
    key.strip_prefix("leo-").is_some_and(|rest| {
        (1..=48).contains(&rest.len())
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) && !key.starts_with("leo-draft")
        && !CONTENT.contains(&key)
}

fn size_of(prefs: &Prefs) -> usize {
    prefs
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum()
}

fn load(dir: &Path) -> Prefs {
    use std::io::Read;
    let Ok(file) = leo_core::paths::open_regular(&dir.join(FILE)) else {
        return Prefs::new();
    };
    let mut text = String::new();
    if file
        .take(MOST_TOTAL as u64 * 2)
        .read_to_string(&mut text)
        .is_err()
    {
        return Prefs::new();
    }
    let mut prefs: Prefs = serde_json::from_str(&text).unwrap_or_default();
    prefs.retain(|key, value| key_ok(key) && value.len() <= MOST_VALUE);
    while size_of(&prefs) > MOST_TOTAL {
        prefs.pop_last();
    }
    prefs
}

fn merge(mut prefs: Prefs, change: Change) -> Option<Prefs> {
    for key in &change.remove {
        if !key_ok(key) {
            return None;
        }
        prefs.remove(key);
    }
    for (key, value) in change.set {
        if !key_ok(&key) {
            return None;
        }
        if value.len() > MOST_VALUE {
            prefs.remove(&key);
        } else {
            prefs.insert(key, value);
        }
    }
    (size_of(&prefs) <= MOST_TOTAL).then_some(prefs)
}

fn apply(dir: &Path, change: Change) -> Result<(), StatusCode> {
    let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let merged = merge(load(dir), change).ok_or(StatusCode::BAD_REQUEST)?;
    let text = serde_json::to_string(&merged).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    leo_core::paths::write_atomic(&dir.join(FILE), text)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn dir_of(state: &AppState) -> PathBuf {
    state.chats.parent().unwrap_or(&state.chats).to_path_buf()
}

fn off() -> bool {
    std::env::var_os("LEO_NO_UI_PREFS").is_some()
}

pub(crate) async fn script(State(state): State<AppState>) -> Response {
    if off() {
        return (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            "",
        )
            .into_response();
    }
    let dir = dir_of(&state);
    let Ok(prefs) = tokio::task::spawn_blocking(move || load(&dir)).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let seed = serde_json::to_string(&prefs).unwrap_or_else(|_| "{}".into());
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        format!("window.__LEO_PREFS__={seed};\n{SCRIPT}"),
    )
        .into_response()
}

pub(crate) async fn change(State(state): State<AppState>, Json(change): Json<Change>) -> Response {
    if off() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dir = dir_of(&state);
    match tokio::task::spawn_blocking(move || apply(&dir, change)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(code)) => code.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(set: &[(&str, &str)], remove: &[&str]) -> Change {
        Change {
            set: set
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            remove: remove.iter().map(|k| k.to_string()).collect(),
        }
    }

    #[test]
    fn only_leo_keys_of_the_right_shape_are_kept() {
        assert!(key_ok("leo-side-order"));
        assert!(key_ok(&format!("leo-{}", "a".repeat(48))));
        for bad in [
            "leo-",
            "side-order",
            "leo-Side",
            "leo-a:b",
            "leo_a",
            "leo-draft-v1",
            "leo-draft",
            "leo-chat-v1",
            "leo-search",
            &format!("leo-{}", "a".repeat(49)),
        ] {
            assert!(!key_ok(bad), "{bad}");
        }
    }

    #[test]
    fn changes_merge_per_key_and_bad_ones_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), change(&[("leo-a", "1"), ("leo-b", "2")], &[])).unwrap();
        apply(dir.path(), change(&[("leo-a", "3")], &["leo-b"])).unwrap();
        apply(dir.path(), change(&[("leo-c", "4")], &[])).unwrap();
        let kept = load(dir.path());
        assert_eq!(kept.len(), 2);
        assert_eq!(kept["leo-a"], "3");
        assert_eq!(kept["leo-c"], "4");

        let big = "x".repeat(MOST_VALUE + 1);
        apply(dir.path(), change(&[("leo-a", &big)], &[])).unwrap();
        let kept = load(dir.path());
        assert_eq!(kept.keys().collect::<Vec<_>>(), ["leo-c"]);
        for refused in [
            change(&[("leo-draft-v1", "1")], &[]),
            change(&[("leo-ok", "1"), ("Bad", "1")], &[]),
            change(&[], &["../x"]),
        ] {
            assert_eq!(apply(dir.path(), refused), Err(StatusCode::BAD_REQUEST));
        }
        assert_eq!(load(dir.path()), kept);
    }

    #[test]
    fn the_total_is_capped_and_a_damaged_file_reads_as_empty() {
        let value = "x".repeat(MOST_VALUE);
        let over = Change {
            set: (0..9)
                .map(|i| (format!("leo-k{i}"), value.clone()))
                .collect(),
            remove: vec![],
        };
        assert!(merge(Prefs::new(), over).is_none());

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{not json").unwrap();
        assert!(load(dir.path()).is_empty());
        std::fs::write(
            dir.path().join(FILE),
            r#"{"leo-ok":"1","bad":"2","leo-draft-v1":"3"}"#,
        )
        .unwrap();
        assert_eq!(load(dir.path()).keys().collect::<Vec<_>>(), ["leo-ok"]);
    }
}
