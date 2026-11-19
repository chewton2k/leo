use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::routes::notes::{directory, save, NoteResponse};
use crate::AppState;

#[derive(serde::Serialize)]
pub(crate) struct TrashResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) directory: String,
    pub(crate) deleted_at: String,
    pub(crate) readable: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) pictures: usize,
    pub(crate) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<String>,
}

const TRASH_CHANGED: &str =
    "Something in the trash changed since this page showed it. Look at it again before deleting it for good.";

#[derive(Debug)]
pub(crate) struct TrashRefusal(pub(crate) StatusCode);

impl From<StatusCode> for TrashRefusal {
    fn from(status: StatusCode) -> Self {
        TrashRefusal(status)
    }
}

impl IntoResponse for TrashRefusal {
    fn into_response(self) -> Response {
        if self.0 == StatusCode::CONFLICT {
            crate::reply::error(self.0, TRASH_CHANGED)
        } else {
            self.0.into_response()
        }
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn named<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.strip_prefix(prefix)
        .filter(|name| leo_core::attachments::own_name(name))
}

fn picture_of(id: &str) -> Option<&str> {
    named(id, "picture:")
}

fn unused_of(id: &str) -> Option<&str> {
    named(id, "unused:")
}

fn held_of(id: &str) -> Option<&str> {
    id.strip_prefix("held:").filter(|id| {
        crate::jsonline::valid_id(id) || id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

pub(crate) async fn list_trash(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrashResponse>>, StatusCode> {
    let chats = state.chats.clone();
    state
        .read_store(move |store| {
            let counts = store.trashed_picture_counts();
            let counts = &counts;
            let row = |kind: &'static str| {
                move |t: leo_core::store::Trashed| TrashResponse {
                    pictures: counts.get(&t.id).copied().unwrap_or(0),
                    id: t.id,
                    title: t.title,
                    directory: t.directory,
                    deleted_at: t.deleted_at.to_rfc3339(),
                    readable: t.readable,
                    kind,
                    revision: t.revision.as_ref().map(leo_core::paths::digest_text),
                }
            };
            let mut rows: Vec<TrashResponse> =
                store.trashed().into_iter().map(row("note")).collect();
            rows.extend(store.picture_rows().into_iter().map(row("picture")));
            rows.extend(store.unused_rows().into_iter().map(row("unused")));
            rows.extend(
                crate::dependents::held_rows(&chats)
                    .into_iter()
                    .map(|(id, held)| TrashResponse {
                        id: format!("held:{id}"),
                        title: "Files of a removed note".to_string(),
                        directory: String::new(),
                        deleted_at: held.at.to_rfc3339(),
                        readable: true,
                        pictures: 0,
                        kind: "held",
                        revision: Some(leo_core::paths::digest_text(
                            &crate::dependents::held_revision(&held),
                        )),
                    }),
            );
            rows.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
            Ok(Json(rows))
        })
        .await
}

pub(crate) async fn restore_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            store.restore(&id).ok_or(StatusCode::NOT_FOUND)?;
            save(store)?;
            Ok(Json(NoteResponse::from_note(
                store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?,
            )))
        })
        .await
}

#[derive(Deserialize, Default)]
pub(crate) struct TrashChoice {
    #[serde(default)]
    pub(crate) ids: Vec<String>,
    #[serde(default)]
    pub(crate) all: bool,
    #[serde(default)]
    pub(crate) dirs: Vec<String>,
    #[serde(default)]
    pub(crate) revisions: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
pub(crate) struct TrashMove {
    #[serde(default)]
    pub(crate) notes: Vec<String>,
    #[serde(default)]
    pub(crate) dirs: Vec<String>,
}

pub(crate) async fn move_to_trash(
    State(state): State<AppState>,
    Json(body): Json<TrashMove>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            let mut folders = 0;
            let mut notes = 0;
            for dir in &body.dirs {
                let dir = dir.trim().trim_matches('/');
                if dir.is_empty() {
                    return Err(StatusCode::BAD_REQUEST);
                }
                directory(store, dir)?;
            }
            let under = |path: &str| {
                body.dirs.iter().any(|d| {
                    let d = d.trim().trim_matches('/');
                    path == d || path.starts_with(&format!("{d}/"))
                })
            };
            let mut gone_ids: Vec<String> = store
                .notes
                .iter()
                .filter(|n| under(&n.directory) || body.notes.contains(&n.id))
                .map(|n| n.id.clone())
                .collect();
            gone_ids.dedup();
            let gone_dirs: Vec<String> = store
                .directories
                .iter()
                .filter(|d| under(d))
                .cloned()
                .collect();
            for dir in &body.dirs {
                let (gone_notes, gone_dirs) =
                    store.delete_dir_recursive(dir.trim().trim_matches('/'));
                notes += gone_notes;
                folders += usize::from(gone_dirs > 0);
            }
            let ids: Vec<String> = body
                .notes
                .iter()
                .filter_map(|id| {
                    store
                        .notes
                        .iter()
                        .find(|n| &n.id == id)
                        .map(|n| n.id.clone())
                })
                .collect();
            notes += ids.len();
            if !ids.is_empty() {
                store.delete_notes(&ids);
            }
            save(store)?;
            for dir in &body.dirs {
                leo_core::store::remove_emptied(
                    &store.notes_dir.join(dir.trim().trim_matches('/')),
                );
            }
            Ok(Json(serde_json::json!({
                "notes": notes,
                "folders": folders,
                "ids": gone_ids,
                "dirs": gone_dirs,
            })))
        })
        .await
}

pub(crate) async fn delete_from_trash(
    State(state): State<AppState>,
    Json(choice): Json<TrashChoice>,
) -> Result<Json<serde_json::Value>, TrashRefusal> {
    let chats = state.chats.clone();
    let shown: std::collections::HashMap<String, leo_core::paths::FileDigest> = choice
        .revisions
        .iter()
        .filter_map(|(id, text)| Some((id.clone(), leo_core::paths::parse_digest(text)?)))
        .collect();
    let chosen = |id: &str| choice.all || choice.ids.iter().any(|picked| picked == id);
    let pictures = state.read_store(|store| Ok(store.picture_files())).await?;
    let checking = (
        state.chats.clone(),
        pictures
            .into_iter()
            .filter(|(id, _, seen)| chosen(id) && seen.is_some() && shown.contains_key(id))
            .collect::<Vec<_>>(),
        choice.ids.clone(),
        choice.all,
        shown.clone(),
    );
    let confirming = state.clone();
    let changed = tokio::task::spawn_blocking(move || {
        let (chats, pictures, ids, all, shown) = checking;
        let held_changed = crate::dependents::held_rows(&chats)
            .iter()
            .any(|(id, row)| {
                let key = format!("held:{id}");
                (all || ids.contains(&key))
                    && shown
                        .get(&key)
                        .map_or(!all, |seen| *seen != crate::dependents::held_revision(row))
            });
        let pictures: Vec<_> = pictures
            .into_iter()
            .filter(|(_, path, seen)| {
                seen.and_then(|seen| leo_core::attachments::same_bytes(path, &seen))
                    .is_none()
            })
            .map(|(id, path, _)| {
                let seen = leo_core::attachments::seen_now(&path, None);
                (id, seen)
            })
            .collect();
        let confirmed = if held_changed || !pictures.is_empty() {
            Default::default()
        } else {
            let ids = shown
                .keys()
                .filter(|id| leo_core::recording::valid_id(id) && (all || ids.contains(id)))
                .cloned()
                .collect();
            crate::dependents::confirm_removal(&confirming, ids)
        };
        (held_changed, pictures, confirmed)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (held_changed, pictures_changed, confirmed) = changed;
    let pictures_moved = !pictures_changed.is_empty();
    if pictures_moved {
        state
            .with_store(move |store| {
                for (id, seen) in &pictures_changed {
                    store.picture_changed(id, *seen);
                }
                Ok(())
            })
            .await?;
    }
    if held_changed || pictures_moved {
        return Err(TrashRefusal(StatusCode::CONFLICT));
    }
    let shown_after = shown.clone();
    let answer = state
        .with_store(move |store| {
            let chosen = |id: &str| choice.all || choice.ids.iter().any(|picked| picked == id);
            let changed = store
                .picture_rows()
                .into_iter()
                .chain(store.unused_rows())
                .filter(|row| chosen(&row.id) && row.revision.is_some())
                .any(|row| {
                    shown
                        .get(&row.id)
                        .map_or(!choice.all, |seen| Some(seen) != row.revision.as_ref())
                });
            if changed {
                return Ok(None);
            }
            let pick = |prefix: fn(&str) -> Option<&str>| -> Vec<String> {
                choice
                    .ids
                    .iter()
                    .filter_map(|id| prefix(id))
                    .map(str::to_string)
                    .collect()
            };
            let rows = |rows: Vec<leo_core::store::Trashed>| -> Vec<String> {
                rows.into_iter().map(|row| row.title).collect()
            };
            let (asked, held) = if choice.all {
                (
                    crate::dependents::Asked {
                        binned: rows(store.picture_rows()),
                        unused: rows(store.unused_rows()),
                        explicit: true,
                        shown: shown.clone(),
                    },
                    crate::dependents::held_rows(&chats).into_keys().collect(),
                )
            } else {
                (
                    crate::dependents::Asked {
                        binned: pick(picture_of),
                        unused: pick(unused_of),
                        explicit: true,
                        shown: shown.clone(),
                    },
                    pick(held_of),
                )
            };
            let emptied = store.delete_shown((!choice.all).then_some(&choice.ids[..]), &shown);
            if emptied.changed {
                return Ok(None);
            }
            Ok(Some((
                emptied.deleted,
                emptied.failed,
                emptied.forgotten,
                asked,
                held,
            )))
        })
        .await?;
    let Some((deleted, failed, forgotten, asked, held)) = answer else {
        return Err(TrashRefusal(StatusCode::CONFLICT));
    };
    let cleared = state.clone();
    let chosen = asked.binned.len() + asked.unused.len() + held.len();
    let left = tokio::task::spawn_blocking(move || {
        let known: std::collections::BTreeMap<String, crate::dependents::Held> =
            crate::dependents::held_rows(&cleared.chats);
        let held: Vec<String> = held
            .into_iter()
            .filter(|id| known.contains_key(id))
            .collect();
        let mut ids = forgotten;
        ids.extend(held.iter().cloned());
        crate::dependents::clear_shown(
            &cleared,
            ids,
            crate::dependents::Why::Asked,
            Some(&shown_after),
            &confirmed,
        );
        let still = crate::dependents::held_rows(&cleared.chats);
        let held_left = held.iter().filter(|id| still.contains_key(*id)).count();
        crate::dependents::pictures_asked(&cleared, chrono::Utc::now(), &asked) + held_left
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "deleted": deleted + chosen - left.min(chosen),
        "failed": failed + left,
    })))
}

pub(crate) async fn restore_many(
    State(state): State<AppState>,
    Json(choice): Json<TrashChoice>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let answer = state
        .with_store(move |store| {
            for dir in &choice.dirs {
                directory(store, dir)?;
            }
            for dir in &choice.dirs {
                store.create_dir(dir);
            }
            let mut kept = 0;
            let mut failed = 0;
            for name in choice.ids.iter().filter_map(|id| picture_of(id)) {
                if store.keep_picture(name) {
                    kept += 1;
                } else {
                    failed += 1;
                }
            }
            let restored = store.restore_many(&choice.ids);
            save(store)?;
            let held: Vec<String> = choice
                .ids
                .iter()
                .filter_map(|id| held_of(id))
                .map(str::to_string)
                .collect();
            Ok((
                restored.titles.len() + kept,
                restored.unreadable,
                failed,
                held,
            ))
        })
        .await?;
    let (restored, unreadable, failed, held) = answer;
    let chosen = held.len();
    let kept = state.clone();
    let files = tokio::task::spawn_blocking(move || {
        held.iter()
            .filter(|id| crate::dependents::keep_held(&kept, id))
            .count()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "restored": restored + files,
        "unreadable": unreadable,
        "files_failed": failed + chosen - files,
    })))
}
