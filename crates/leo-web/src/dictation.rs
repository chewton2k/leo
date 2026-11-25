use crate::reply::error;
use crate::AppState;
use anyhow::{ensure, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

pub trait DictationApi: Send + Sync {
    fn view(&self) -> Result<Value>;
    fn change(&self, change: Value) -> Result<Value>;
}

pub const PREVIEW_CHARS: i64 = 2_000;
pub const MOST_BODY: usize = 64 * 1024 * 1024;
const GONE: &str = "This dictation is no longer in Recent dictations.";
const NOTHING_TO_KEEP: &str = "This dictation has no text to keep in Recent dictations.";

pub fn remember(data: &Path, text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), NOTHING_TO_KEEP);
    crate::db::try_at(&data.join(crate::db::FILE))?.with(|db| {
        let tx = db.transaction()?;
        tx.execute("INSERT INTO dictations(at,text) VALUES (?1,?2)", rusqlite::params![chrono::Utc::now().to_rfc3339(), text])?;
        tx.execute("DELETE FROM dictations WHERE id NOT IN (SELECT id FROM dictations ORDER BY id DESC LIMIT 20)", [])?;
        tx.commit()
    })
}
pub fn recent(data: &Path) -> Result<Vec<Value>> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.prepare("SELECT id,at,substr(text,1,?1),edited,substr(original,1,?1),length(text)>?1,COALESCE(length(original),0)>?1 FROM dictations ORDER BY id DESC LIMIT 20")?.query_map([PREVIEW_CHARS], |r| Ok(json!({"id":r.get::<_,i64>(0)?, "at":r.get::<_,String>(1)?, "text":r.get::<_,String>(2)?, "edited":r.get::<_,i64>(3)? != 0, "original":r.get::<_,Option<String>>(4)?, "cut":r.get::<_,bool>(5)?, "original_cut":r.get::<_,bool>(6)?})))?.collect()
    })
}
pub fn text_of(data: &Path, id: i64) -> Result<Option<(String, Option<String>)>> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.query_row(
            "SELECT text,original FROM dictations WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })
}
pub fn edit(data: &Path, id: i64, text: &str) -> Result<()> {
    let text = text.trim();
    ensure!(!text.is_empty(), NOTHING_TO_KEEP);
    let changed = crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.execute(
            "UPDATE dictations SET original=COALESCE(original,text), text=?1, edited=1 WHERE id=?2",
            rusqlite::params![text, id],
        )
    })?;
    ensure!(changed == 1, GONE);
    Ok(())
}
pub fn clear(data: &Path) -> Result<()> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.execute("DELETE FROM dictations", [])?;
        Ok(())
    })
}
async fn call(
    api: Option<Arc<dyn DictationApi>>,
    data: std::path::PathBuf,
    change: Option<Value>,
) -> Response {
    let Some(api) = api else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Dictation is available in the Leo app. Reopen Leo and try again.",
        );
    };
    let work = tokio::task::spawn_blocking(move || -> Result<Value> {
        let mut full = None;
        if let Some(mut change) = change {
            match change["action"].as_str() {
                Some("clear") => clear(&data)?,
                Some("edit") => edit(
                    &data,
                    change["id"].as_i64().unwrap_or(0),
                    change["text"].as_str().unwrap_or(""),
                )?,
                Some("copy") => {
                    let id = change["id"].as_i64().unwrap_or(0);
                    let Some((text, _)) = text_of(&data, id)? else {
                        anyhow::bail!(GONE);
                    };
                    change["text"] = json!(text);
                    api.change(change)?;
                }
                Some("full") => {
                    let id = change["id"].as_i64().unwrap_or(0);
                    let Some((text, original)) = text_of(&data, id)? else {
                        anyhow::bail!(GONE);
                    };
                    full = Some(json!({"id": id, "text": text, "original": original}));
                }
                _ => {
                    api.change(change)?;
                }
            }
        }
        let mut view = api.view()?;
        view["recent"] = json!(recent(&data)?);
        if let Some(full) = full {
            view["full"] = full;
        }
        Ok(view)
    })
    .await;
    match work {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Dictation settings could not be opened. Try again.",
        ),
    }
}
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let data = crate::db::data_dir(&state.chats).to_path_buf();
    call(state.dictation.clone(), data, None).await
}
pub(crate) async fn change(
    State(state): State<AppState>,
    change: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(change)) = change else {
        return error(
            StatusCode::BAD_REQUEST,
            "This dictation change could not be read. Try again in Settings.",
        );
    };
    let data = crate::db::data_dir(&state.chats).to_path_buf();
    call(state.dictation.clone(), data, Some(change)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_dictations_are_local_bounded_and_clearable() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..23 {
            remember(dir.path(), &format!("text {n}")).unwrap();
        }
        let all = recent(dir.path()).unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!(all[0]["text"], "text 22");
        assert_eq!(all[19]["text"], "text 3");
        assert!(!dir.path().join("notes").exists());
        clear(dir.path()).unwrap();
        assert!(recent(dir.path()).unwrap().is_empty());
    }
    #[test]
    fn editing_a_recent_dictation_keeps_the_row_and_refuses_unknown_ids() {
        let dir = tempfile::tempdir().unwrap();
        remember(dir.path(), "ship it to leo seal I users").unwrap();
        let id = recent(dir.path()).unwrap()[0]["id"].as_i64().unwrap();
        assert_eq!(recent(dir.path()).unwrap()[0]["edited"], false);
        assert!(recent(dir.path()).unwrap()[0]["original"].is_null());
        edit(dir.path(), id, "  Ship it to Leo CLI users  ").unwrap();
        let all = recent(dir.path()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0]["text"], "Ship it to Leo CLI users");
        assert_eq!(all[0]["edited"], true);
        assert_eq!(all[0]["original"], "ship it to leo seal I users");
        edit(dir.path(), id, "Ship it to Leo CLI users now").unwrap();
        assert_eq!(
            recent(dir.path()).unwrap()[0]["original"],
            "ship it to leo seal I users"
        );
        edit(dir.path(), id, "Ship it to Leo CLI users").unwrap();
        assert!(edit(dir.path(), id + 1, "x").is_err());
        assert!(edit(dir.path(), id, "   ").is_err());
        assert_eq!(
            recent(dir.path()).unwrap()[0]["text"],
            "Ship it to Leo CLI users"
        );
    }
    fn long_text(chars: usize, seed: char) -> String {
        ['a', seed, 'é', '😀', ' ']
            .iter()
            .cycle()
            .take(chars)
            .collect()
    }
    #[test]
    fn a_long_dictation_is_kept_whole_and_listed_by_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let text = long_text(300_000, 'b');
        remember(dir.path(), &text).unwrap();
        let row = recent(dir.path()).unwrap()[0].clone();
        let start: String = text.chars().take(2_000).collect();
        assert_eq!(row["text"], start);
        assert_eq!(row["cut"], true);
        assert_eq!(row["original_cut"], false);
        let id = row["id"].as_i64().unwrap();
        assert_eq!(text_of(dir.path(), id).unwrap().unwrap().0, text);
        let changed = long_text(299_999, 'c');
        edit(dir.path(), id, &changed).unwrap();
        let row = recent(dir.path()).unwrap()[0].clone();
        assert_eq!(row["original"], start);
        assert_eq!(row["original_cut"], true);
        assert_eq!(
            text_of(dir.path(), id).unwrap().unwrap(),
            (changed.clone(), Some(text.clone()))
        );
        for _ in 0..20 {
            remember(dir.path(), &text).unwrap();
        }
        let poll = serde_json::to_string(&recent(dir.path()).unwrap())
            .unwrap()
            .chars()
            .count();
        println!("one poll of twenty 300,000-character rows: {poll} characters");
        assert!(poll < 20 * 2 * 2_000 + 20 * 200);
    }
}
