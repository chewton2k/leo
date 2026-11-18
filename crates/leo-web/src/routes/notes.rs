use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use leo_core::store::{Store, MOST_EMPTY_FOLDERS};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct ListParams {
    pub(crate) limit: Option<usize>,
    pub(crate) dir: Option<String>,
    #[serde(default)]
    pub(crate) offset: usize,
    #[serde(default)]
    pub(crate) brief: bool,
    pub(crate) sort: Option<String>,
    pub(crate) show: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SearchParams {
    pub(crate) q: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: usize,
    #[serde(default)]
    pub(crate) brief: bool,
}

#[derive(Deserialize)]
pub(crate) struct CreateBody {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) directory: Option<String>,
    #[serde(default)]
    pub(crate) request: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateBody {
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) pinned: Option<bool>,
    pub(crate) base: Option<String>,
    #[serde(default)]
    pub(crate) written: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct CreateDirBody {
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub(crate) struct MoveDirBody {
    pub(crate) from: String,
    pub(crate) into: String,
}

#[derive(Deserialize)]
pub(crate) struct RenameDirBody {
    pub(crate) from: String,
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct MoveBody {
    pub(crate) directory: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct NoteResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) directory: String,
    pub(crate) pinned: bool,
    pub(crate) version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tasks: Option<[usize; 2]>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) recorded: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) writeup_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preview: Option<String>,
}

pub(crate) fn version_of(note: &leo_core::notes::Note) -> String {
    let parts = [
        note.title.as_str(),
        note.body.as_str(),
        &note.tags.join("\u{1f}"),
    ];
    let bytes = parts.iter().flat_map(|part| part.bytes().chain([0u8]));
    format!("{:016x}", crate::vectors::fnv64(bytes))
}

impl NoteResponse {
    pub(crate) fn from_note(n: &leo_core::notes::Note) -> Self {
        NoteResponse {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            created_at: n.created_at.to_rfc3339(),
            updated_at: n.updated_at.to_rfc3339(),
            directory: n.directory.clone(),
            pinned: n.pinned,
            version: version_of(n),
            tasks: None,
            recorded: false,
            writeup_pending: false,
            summary: None,
            preview: None,
        }
    }

    pub(crate) fn brief(n: &leo_core::notes::Note, words: &[String]) -> Self {
        NoteResponse {
            body: excerpt(&n.body, words),
            tasks: Some(tasks_in(&n.body)),
            ..NoteResponse::from_note(n)
        }
    }
}

pub(crate) const EXCERPT_CHARS: usize = 400;

pub(crate) fn tasks_in(body: &str) -> [usize; 2] {
    let mut done = 0;
    let mut all = 0;
    for line in body.lines() {
        let Some(rest) = line.trim_start().strip_prefix("- [") else {
            continue;
        };
        let mut chars = rest.chars();
        let (Some(mark), Some(']'), Some(' ')) = (chars.next(), chars.next(), chars.next()) else {
            continue;
        };
        match mark {
            ' ' => all += 1,
            'x' | 'X' => {
                all += 1;
                done += 1;
            }
            _ => {}
        }
    }
    [done, all]
}

pub(crate) fn excerpt(body: &str, words: &[String]) -> String {
    if words.is_empty() {
        return body.chars().take(EXCERPT_CHARS).collect();
    }
    let lower = body.to_lowercase();
    let at = words
        .iter()
        .filter_map(|w| lower.find(w.as_str()))
        .min()
        .and_then(|at| body.get(..at))
        .map(|before| before.chars().count().saturating_sub(120))
        .unwrap_or(0);
    let mut start = body.char_indices().nth(at).map_or(body.len(), |(i, _)| i);
    if at > 0 {
        if let Some((gap, c)) = body[start..]
            .char_indices()
            .take(30)
            .find(|(_, c)| c.is_whitespace())
        {
            start += gap + c.len_utf8();
        }
    }
    let mut out: String = body[start..].chars().take(EXCERPT_CHARS).collect();
    if at > 0 {
        out.insert(0, '…');
    }
    out
}

pub(crate) fn save(store: &Store) -> Result<(), StatusCode> {
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn directory(store: &Store, path: &str) -> Result<(), StatusCode> {
    store
        .validate_directory(path)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

pub(crate) async fn list_notes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Response, StatusCode> {
    let brief = params.brief;
    let (total, notes) = state
        .read_store(move |store| {
            let limit = params.limit.unwrap_or(100).min(1000);
            if let Some(ref dir) = params.dir {
                directory(store, dir)?;
            }
            let mut notes = store.list_notes(params.dir.as_deref());
            crate::browse::only(&mut notes, params.show.as_deref(), chrono::Utc::now())?;
            crate::browse::order(&mut notes, params.sort.as_deref())?;
            let total = notes.len();
            let page: Vec<leo_core::notes::Note> = notes
                .into_iter()
                .skip(params.offset)
                .take(limit)
                .cloned()
                .collect();
            Ok((total, page))
        })
        .await?;
    let graphs = std::sync::Arc::clone(&state.graphs);
    let page = tokio::task::spawn_blocking(move || {
        let summaries = if brief {
            graphs.note_summaries(&notes)
        } else {
            std::collections::BTreeMap::new()
        };
        notes
            .iter()
            .map(|note| {
                if brief {
                    NoteResponse {
                        summary: Some(
                            summaries
                                .get(&note.id)
                                .cloned()
                                .unwrap_or_else(|| crate::browse::summary(&note.body)),
                        ),
                        preview: Some(crate::browse::preview(&note.body)),
                        ..NoteResponse::brief(note, &[])
                    }
                } else {
                    NoteResponse::from_note(note)
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(([("x-total", total.to_string())], Json(page)).into_response())
}

pub(crate) async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let chats = state.chats.clone();
    state
        .read_store(move |store| {
            let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(NoteResponse {
                recorded: crate::tools::has_transcript(&store.notes_dir, &note.id),
                writeup_pending: crate::record::writeup_pending(&chats, &note.id),
                ..NoteResponse::from_note(note)
            }))
        })
        .await
}

pub(crate) fn make_note(
    store: &mut Store,
    title: &str,
    body: &str,
    dir: &str,
) -> Result<NoteResponse, StatusCode> {
    directory(store, dir)?;
    if !store.dir_exists(dir) && !store.create_dir(dir) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let note = store
        .create_note(title.to_string(), body.to_string(), Vec::new(), dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(NoteResponse::from_note(note))
}

pub(crate) async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .with_store(move |store| {
            let request = crate::routes::insert::requested(body.request.clone());
            if let Some(first) = request
                .as_deref()
                .and_then(|r| crate::routes::insert::remembered_creation("note", r))
            {
                return Ok((StatusCode::OK, Json(first)));
            }
            let resp = make_note(
                store,
                &body.title,
                body.body.as_deref().unwrap_or_default(),
                body.directory.as_deref().unwrap_or_default(),
            )?;
            save(store)?;
            let made = serde_json::json!(resp);
            if let Some(r) = &request {
                crate::routes::insert::remember_creation("note", r, &made);
            }
            Ok((StatusCode::CREATED, Json(made)))
        })
        .await
}

pub(crate) async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let margin = Arc::clone(&state.margin);
    let chats = state.chats.clone();
    state
        .with_store(move |store| {
            let recorded = crate::tools::has_transcript(&store.notes_dir, &id);
            let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
            let mut edited = body.title.is_some() || body.body.is_some() || body.tags.is_some();
            if edited
                && body
                    .base
                    .as_deref()
                    .is_some_and(|base| base != version_of(note))
            {
                let same = body.title.as_ref().is_none_or(|title| *title == note.title)
                    && body.body.as_ref().is_none_or(|text| *text == note.body)
                    && body.tags.as_ref().is_none_or(|tags| *tags == note.tags);
                if !same {
                    return Err(StatusCode::CONFLICT);
                }
                edited = false;
            }
            if let Some(title) = body.title {
                note.title = title;
            }
            if let Some(text) = body.body {
                note.body = text;
            }
            if let Some(tags) = body.tags {
                note.tags = tags;
            }
            if let Some(pinned) = body.pinned {
                note.pinned = pinned;
            }
            if edited {
                note.updated_at = chrono::Utc::now();
            }
            let resp = NoteResponse {
                recorded,
                ..NoteResponse::from_note(note)
            };
            save(store)?;
            if let Some(written) = body.written {
                crate::record::set_writeup_pending(&chats, &id, !written);
            }
            if edited {
                margin.edited(&id, &resp.version);
            }
            Ok(Json(resp))
        })
        .await
}

pub(crate) async fn delete_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    state
        .with_store(move |store| {
            let full = store
                .find_note(&id)
                .map(|n| n.id.clone())
                .ok_or(StatusCode::NOT_FOUND)?;
            store.delete_notes(&[full]);
            save(store)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
        .unwrap_or_else(|status| status)
}

pub(crate) async fn move_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.directory)?;
            if !store.dir_exists(&body.directory) {
                return Err(StatusCode::NOT_FOUND);
            }
            store
                .move_note(&id, &body.directory)
                .ok_or(StatusCode::NOT_FOUND)?;
            let resp = NoteResponse::from_note(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

pub(crate) const SEARCH_MOST: usize = 300;
pub(crate) const WORDS_UNDER_LOCK: usize = 16;

#[derive(serde::Serialize)]
pub(crate) struct SearchHit {
    #[serde(flatten)]
    pub(crate) note: NoteResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) why: Option<crate::search::Why>,
}

type Page = (
    usize,
    Vec<(leo_core::notes::Note, Option<crate::search::Why>)>,
);

fn kept_page(
    store: &Store,
    kept: &crate::Searched,
    now: (u64, u64, &Arc<crate::graph::Cache>),
    offset: usize,
    limit: usize,
) -> Option<Page> {
    let same = kept.stamps.0 == now.0
        && kept.stamps.1 == now.1
        && std::sync::Weak::ptr_eq(&kept.stamps.2, &Arc::downgrade(now.2));
    if !same {
        return None;
    }
    let page = kept
        .hits
        .iter()
        .skip(offset)
        .take(limit)
        .filter_map(|(id, at, why)| {
            store
                .notes
                .get(*at)
                .filter(|n| &n.id == id)
                .or_else(|| store.find_note(id))
                .map(|n| (n.clone(), why.clone()))
        })
        .collect();
    Some((kept.hits.len(), page))
}

fn settle(
    store: &Store,
    hits: Vec<crate::search::Hit<'_>>,
    query: String,
    stamps: (u64, u64, std::sync::Weak<crate::graph::Cache>),
    offset: usize,
    limit: usize,
) -> (crate::Searched, Page) {
    let at: std::collections::HashMap<&str, usize> = if hits.is_empty() {
        Default::default()
    } else {
        store
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect()
    };
    let page = hits
        .iter()
        .skip(offset)
        .take(limit)
        .map(|h| (h.note.clone(), h.why.clone()))
        .collect();
    let kept = hits
        .into_iter()
        .map(|h| (h.note.id.clone(), at[h.note.id.as_str()], h.why))
        .collect::<Vec<_>>();
    let total = kept.len();
    (
        crate::Searched {
            query,
            stamps,
            hits: Arc::new(kept),
        },
        (total, page),
    )
}

pub(crate) async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Response, StatusCode> {
    let q = params.q.clone().unwrap_or_default();
    let limit = params.limit.unwrap_or(SEARCH_MOST);
    let offset = params.offset;
    let kept = state
        .searched
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|s| s.query == q);
    let page = match kept {
        Some(kept) => {
            let graphs = Arc::clone(&state.graphs);
            let vectors = Arc::clone(&state.vectors);
            state
                .read_store(move |store| {
                    let cache = graphs.load();
                    Ok(kept_page(
                        store,
                        &kept,
                        (store.fingerprint(), vectors.version(), &cache),
                        offset,
                        limit,
                    ))
                })
                .await?
        }
        None => None,
    };
    let (total, notes) = match page {
        Some(page) => page,
        None => fresh_search(&state, q.clone(), offset, limit).await?,
    };
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = q
        .split_whitespace()
        .map(|w| w.trim_start_matches('#').to_lowercase())
        .filter(|w| !w.is_empty() && seen.insert(w.clone()))
        .collect();
    let brief = params.brief;
    let found = tokio::task::spawn_blocking(move || {
        notes
            .into_iter()
            .map(|(note, why)| SearchHit {
                note: if brief {
                    NoteResponse::brief(&note, &words)
                } else {
                    NoteResponse::from_note(&note)
                },
                why,
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(([("x-total", total.to_string())], Json(found)).into_response())
}

async fn fresh_search(
    state: &AppState,
    q: String,
    offset: usize,
    limit: usize,
) -> Result<Page, StatusCode> {
    let words = crate::search::words_of(&q);
    let version = state.vectors.version();
    let (close, cache, indexed) = {
        let meaning = state.meaning.clone();
        let vectors = Arc::clone(&state.vectors);
        let graphs = Arc::clone(&state.graphs);
        let index = Arc::clone(&state.search_index);
        let asked = q.clone();
        tokio::task::spawn_blocking(move || {
            (
                crate::vectors::close_to(meaning.as_ref(), &vectors, &asked, 20),
                graphs.load(),
                index.for_query(&asked).map(Arc::new),
            )
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    let graph = Arc::downgrade(&cache);
    let (kept, page) = if words.len() <= WORDS_UNDER_LOCK {
        state
            .read_store(move |store| {
                let usable = indexed.as_deref().filter(|c| c.usable(store));
                let hits = crate::search::with_meaning(
                    store,
                    crate::search::search_with(store, &cache, &q, usable),
                    &close,
                );
                let stamps = (store.fingerprint(), version, graph);
                Ok(settle(store, hits, q, stamps, offset, limit))
            })
            .await?
    } else {
        let found = indexed.clone();
        let (copy, taken_at, usable) = state
            .read_store(move |store| {
                let usable = found.as_deref().filter(|c| c.usable(store));
                let copy: Vec<(String, String, String, chrono::DateTime<chrono::Utc>)> = store
                    .notes
                    .iter()
                    .map(|n| {
                        let body = if usable.is_some_and(|c| c.holds_text_of(&n.id)) {
                            String::new()
                        } else {
                            n.body.clone()
                        };
                        (n.id.clone(), n.title.clone(), body, n.updated_at)
                    })
                    .collect();
                Ok((copy, store.fingerprint(), usable.is_some()))
            })
            .await?;
        let (ids, ranked) = tokio::task::spawn_blocking(move || {
            let ranked = crate::search::ranked(
                copy.iter()
                    .map(|(id, title, body, updated_at)| crate::search::Text {
                        id,
                        title,
                        body,
                        updated_at: *updated_at,
                    }),
                &cache,
                &words,
                indexed.as_deref().filter(|_| usable),
            );
            let ids: Vec<String> = copy.into_iter().map(|(id, ..)| id).collect();
            (ids, ranked)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        state
            .read_store(move |store| {
                let unmoved = store.fingerprint() == taken_at;
                let mut by_id: Option<std::collections::HashMap<&str, usize>> = None;
                let hits = ranked
                    .into_iter()
                    .filter_map(|(at, why)| {
                        let id = ids[at].as_str();
                        let here = if unmoved {
                            store.notes.get(at).filter(|n| n.id == id)
                        } else {
                            None
                        };
                        let note = here.or_else(|| {
                            let by_id = by_id.get_or_insert_with(|| {
                                store
                                    .notes
                                    .iter()
                                    .enumerate()
                                    .map(|(i, n)| (n.id.as_str(), i))
                                    .collect()
                            });
                            by_id.get(id).map(|i| &store.notes[*i])
                        })?;
                        Some(crate::search::Hit { note, why })
                    })
                    .collect();
                let hits = crate::search::with_meaning(
                    store,
                    crate::search::finish(store, &q, hits),
                    &close,
                );
                let stamps = (store.fingerprint(), version, graph);
                Ok(settle(store, hits, q, stamps, offset, limit))
            })
            .await?
    };
    *state.searched.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
    Ok(page)
}

#[derive(serde::Serialize)]
pub(crate) struct DirResponse {
    pub(crate) name: String,
    pub(crate) notes: usize,
}

fn note_counts(store: &Store) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for note in &store.notes {
        let directory = note.directory.as_str();
        if directory.is_empty() {
            continue;
        }
        for (at, _) in directory.match_indices('/') {
            *counts.entry(directory[..at].to_string()).or_default() += 1;
        }
        *counts.entry(directory.to_string()).or_default() += 1;
    }
    counts
}

pub(crate) async fn list_folders(
    State(state): State<AppState>,
) -> Result<Json<Vec<DirResponse>>, StatusCode> {
    state
        .read_store(|store| {
            let mut all = store.directories.clone();
            all.sort_by_key(|d| d.to_lowercase());
            let counts = note_counts(store);
            Ok(Json(
                all.into_iter()
                    .map(|name| DirResponse {
                        notes: counts.get(&name).copied().unwrap_or(0),
                        name,
                    })
                    .collect(),
            ))
        })
        .await
}

#[derive(Deserialize)]
pub(crate) struct RevParams {
    #[serde(default)]
    foreground: Option<String>,
}

pub(crate) async fn get_rev(
    State(state): State<AppState>,
    Query(params): Query<RevParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let foreground = matches!(params.foreground.as_deref(), Some("1" | "true"));
    if foreground {
        state
            .refresh_requested
            .store(true, std::sync::atomic::Ordering::Release);
    }
    let fingerprint = state.read_store(|store| Ok(store.fingerprint())).await?;
    let chats = state.chats.clone();
    let stamps = tokio::task::spawn_blocking(move || {
        if foreground {
            crate::boards::look_again(&chats);
        }
        [
            crate::tasks::generation(&chats),
            crate::boards::generation(&chats),
        ]
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bytes = std::iter::once(fingerprint)
        .chain(stamps)
        .flat_map(u64::to_le_bytes);
    Ok(Json(
        serde_json::json!({ "rev": format!("{:016x}", crate::vectors::fnv64(bytes)) }),
    ))
}

pub(crate) async fn move_dir(
    State(state): State<AppState>,
    Json(body): Json<MoveDirBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.from)?;
            directory(store, &body.into)?;
            if !store.dir_exists(&body.from) || !store.dir_exists(&body.into) {
                return Err(StatusCode::NOT_FOUND);
            }
            let to = store
                .move_dir(&body.from, &body.into)
                .ok_or(StatusCode::CONFLICT)?;
            save(store)?;
            carry_files(&store.notes_dir, body.from.trim_matches('/'), &to);
            leo_core::store::remove_emptied(&store.notes_dir.join(body.from.trim_matches('/')));
            Ok(Json(serde_json::json!({ "path": to })))
        })
        .await
}

pub(crate) async fn rename_dir(
    State(state): State<AppState>,
    Json(body): Json<RenameDirBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.from)?;
            directory(store, &body.name)?;
            if body.from.is_empty()
                || body.name.is_empty()
                || body.name.chars().count() > leo_core::store::MOST_FOLDER_NAME_CHARS
                || body.name.len() > leo_core::store::MOST_FOLDER_NAME_BYTES
                || body.name.contains(['/', '\\'])
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            if !store.dir_exists(&body.from) {
                return Err(StatusCode::NOT_FOUND);
            }
            let to = store
                .rename_dir(&body.from, &body.name)
                .ok_or(StatusCode::CONFLICT)?;
            save(store)?;
            carry_files(&store.notes_dir, &body.from, &to);
            leo_core::store::remove_emptied(&store.notes_dir.join(&body.from));
            Ok(Json(serde_json::json!({ "path": to })))
        })
        .await
}

fn carry_files(notes_dir: &std::path::Path, from: &str, to: &str) {
    let top = notes_dir.join(from);
    let dest = notes_dir.join(to);
    let mut found = vec![std::path::PathBuf::new()];
    let mut i = 0;
    while i < found.len() && i < MOST_EMPTY_FOLDERS {
        let here = found[i].clone();
        if let Ok(entries) = std::fs::read_dir(top.join(&here)) {
            for entry in entries.take(MOST_EMPTY_FOLDERS).flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let relative = here.join(entry.file_name());
                if kind.is_dir() {
                    if found.len() < MOST_EMPTY_FOLDERS {
                        found.push(relative);
                    }
                } else if kind.is_file() {
                    let target = dest.join(&relative);
                    if target.symlink_metadata().is_ok() {
                        continue;
                    }
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::rename(entry.path(), &target);
                }
            }
        }
        i += 1;
    }
}

pub(crate) async fn create_dir(
    State(state): State<AppState>,
    Json(body): Json<CreateDirBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.path)?;
            if !store.create_dir(&body.path) {
                return Err(StatusCode::CONFLICT);
            }
            save(store)?;
            Ok((
                StatusCode::CREATED,
                Json(serde_json::json!({ "path": body.path })),
            ))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excerpt_without_words_is_the_start_of_the_note() {
        let body = format!("{}🦊 tail", "é".repeat(EXCERPT_CHARS - 1));
        assert_eq!(
            excerpt(&body, &[]),
            body.chars().take(EXCERPT_CHARS).collect::<String>()
        );
        assert_eq!(excerpt("Short note", &[]), "Short note");
        assert_eq!(excerpt("Short note", &["absent".into()]), "Short note");
    }
}
