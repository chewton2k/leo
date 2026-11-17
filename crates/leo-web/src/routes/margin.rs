use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::margin::{self, Prefs, Refused, Thread};
use crate::routes::felix::Applied;
use crate::routes::insert;
use crate::routes::notes::{save, version_of, NoteResponse};
use crate::usage::Purpose;
use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct ReadBody {
    version: String,
    blocks: Vec<String>,
    #[serde(default)]
    most: Option<usize>,
    #[serde(default)]
    gone: Vec<String>,
    #[serde(default, deserialize_with = "listed")]
    keep: Vec<String>,
    #[serde(default)]
    changed: Vec<Changed>,
}

fn listed<'de, D: serde::Deserializer<'de>>(to: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Keep {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<Keep>::deserialize(to)? {
        Some(Keep::One(id)) => vec![id],
        Some(Keep::Many(ids)) => ids.into_iter().take(2 * margin::MOST_OPEN).collect(),
        None => Vec::new(),
    })
}

#[derive(Deserialize)]
pub(crate) struct Changed {
    id: String,
    text: String,
}

#[derive(Deserialize)]
pub(crate) struct AskBody {
    version: String,
    quote: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    question: String,
    #[serde(default)]
    gone: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ReplyBody {
    text: String,
    #[serde(default)]
    request: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct WeaveBody {
    base: String,
    #[serde(default)]
    request: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StatusBody {
    status: String,
    #[serde(default)]
    gone: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct PinBody {
    pinned: bool,
}

#[derive(Deserialize)]
pub(crate) struct ModeBody {
    mode: Option<String>,
    persona: Option<String>,
    instructions: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct PrefsBody {
    default_mode: Option<String>,
    layout: Option<String>,
    pace: Option<String>,
    cap: Option<u32>,
    gap: Option<u32>,
    auto_model: Option<bool>,
    default_persona: Option<String>,
    personas: Option<Vec<margin::PersonaBody>>,
}

struct ReadFacts {
    along: bool,
    voice: String,
    instructions: String,
    cap: u32,
    gap: u32,
    quotes: Vec<String>,
    open: Vec<Thread>,
    limit: usize,
    free: bool,
}

enum Pass {
    Saved(Vec<Thread>, Vec<String>),
    Silent,
    Failed(String),
    Refused(Response),
}

struct Open {
    title: String,
    body: String,
    version: String,
    created: DateTime<Utc>,
}

fn said(code: StatusCode, why: &str, error: &str) -> Response {
    (
        code,
        Json(serde_json::json!({ "why": why, "error": error })),
    )
        .into_response()
}

fn silent() -> Response {
    Json(serde_json::json!({ "note": null })).into_response()
}

fn failed(state: &AppState, error: String) -> Response {
    if error != margin::FULL {
        state.margin.set_problem(Some(error.clone()));
    }
    Json(serde_json::json!({ "note": null, "failed": true, "error": error })).into_response()
}

async fn open_note(state: &AppState, id: &str) -> Result<Open, Box<Response>> {
    if !margin::valid_note(id) {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    let id = id.to_string();
    state
        .read_store(move |store| {
            let note = store
                .notes
                .iter()
                .find(|n| n.id == id)
                .ok_or(StatusCode::NOT_FOUND)?;
            Ok(Open {
                title: note.title.clone(),
                body: note.body.clone(),
                version: version_of(note),
                created: note.created_at,
            })
        })
        .await
        .map_err(|code| Box::new(code.into_response()))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<Response>> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

pub(crate) async fn note_margin(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    let chats = state.chats.clone();
    let view = blocking(move || {
        serde_json::json!({
            "mode": margin::mode_of(&chats, &id, note.created),
            "persona": margin::persona_of(&chats, &id),
            "instructions": margin::instructions_of(&chats, &id),
            "threads": margin::open_on(&chats, &id),
            "resolved": margin::resolved_on(&chats, &id),
        })
    })
    .await;
    match view {
        Ok(view) => Json(view).into_response(),
        Err(refused) => *refused,
    }
}

pub(crate) async fn set_note_mode(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ModeBody>,
) -> Response {
    if let Err(refused) = open_note(&state, &id).await {
        return *refused;
    }
    if (body.mode.is_none() && body.persona.is_none() && body.instructions.is_none())
        || body.mode.as_deref().is_some_and(|m| !margin::valid_mode(m))
        || body
            .instructions
            .as_deref()
            .is_some_and(|t| t.chars().any(|c| c.is_control() && c != '\n'))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let (mode, persona) = (body.mode.clone(), body.persona.clone());
    let instructions = body.instructions.clone();
    let done = blocking(move || {
        if persona
            .as_deref()
            .is_some_and(|p| !margin::persona_known(&margin::prefs(&chats), p))
        {
            return (StatusCode::BAD_REQUEST, None);
        }
        let saved = mode
            .as_deref()
            .is_none_or(|m| margin::set_mode(&chats, &id, m))
            && persona
                .as_deref()
                .is_none_or(|p| margin::set_persona(&chats, &id, p));
        let kept = instructions
            .as_deref()
            .map(|t| margin::set_instructions(&chats, &id, t));
        if saved && kept.as_ref().is_none_or(Option::is_some) {
            (StatusCode::OK, kept.flatten())
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, None)
        }
    })
    .await;
    match done {
        Ok((StatusCode::OK, kept)) => {
            let mut view = serde_json::Map::new();
            if let Some(mode) = body.mode {
                view.insert("mode".into(), mode.into());
            }
            if let Some(persona) = body.persona {
                view.insert("persona".into(), persona.into());
            }
            if let Some(kept) = kept {
                view.insert("instructions".into(), kept.into());
            }
            Json(view).into_response()
        }
        Ok((code, _)) => code.into_response(),
        Err(refused) => *refused,
    }
}

fn settings_view(state: &AppState) -> serde_json::Value {
    let prefs = margin::prefs(&state.chats);
    let builtin = margin::BUILTIN_PERSONAS.iter().map(|(id, name, icon, line, says)| {
        let details = if *id == "reader" {
            margin::READER_DETAILS
        } else {
            says
        };
        serde_json::json!({ "id": id, "name": name, "icon": icon, "line": line, "details": details, "custom": false })
    });
    let custom = prefs.personas.iter().map(|p| {
        serde_json::json!({ "id": p.id, "name": p.name, "icon": p.icon, "line": "", "details": p.says, "custom": true })
    });
    let personas: Vec<serde_json::Value> = builtin.chain(custom).collect();
    serde_json::json!({
        "default_persona": prefs.default_persona,
        "personas": personas,
        "persona_icons": margin::PERSONA_ICONS,
        "most_personas": margin::MOST_PERSONAS,
        "default_mode": prefs.default_mode,
        "layout": prefs.layout,
        "pace": prefs.pace,
        "cap": prefs.cap,
        "caps": margin::DAILY_CAPS,
        "gap": prefs.gap,
        "gaps": margin::READ_GAPS,
        "auto_model": prefs.auto_model,
        "today": margin::reads_today(&state.chats, prefs.cap),
        "problem": state.margin.problem(),
    })
}

pub(crate) async fn settings(State(state): State<AppState>) -> Response {
    let view = state.clone();
    match blocking(move || settings_view(&view)).await {
        Ok(view) => Json(view).into_response(),
        Err(refused) => *refused,
    }
}

fn changed(mut prefs: Prefs, body: PrefsBody) -> Option<Prefs> {
    if let Some(mode) = body.default_mode {
        if !margin::valid_mode(&mode) {
            return None;
        }
        if mode != prefs.default_mode {
            prefs.since = Some(Utc::now());
        }
        prefs.default_mode = mode;
    }
    if let Some(layout) = body.layout {
        if !matches!(layout.as_str(), "both" | "right" | "marks") {
            return None;
        }
        prefs.layout = layout;
    }
    if let Some(pace) = body.pace {
        if !matches!(pace.as_str(), "fast" | "regular" | "slow") {
            return None;
        }
        prefs.pace = pace;
    }
    if let Some(cap) = body.cap {
        if !margin::DAILY_CAPS.contains(&cap) {
            return None;
        }
        prefs.cap = cap;
    }
    if let Some(gap) = body.gap {
        if !margin::READ_GAPS.contains(&gap) {
            return None;
        }
        prefs.gap = gap;
    }
    if let Some(auto_model) = body.auto_model {
        prefs.auto_model = auto_model;
    }
    if let Some(list) = body.personas {
        prefs.personas = margin::valid_personas(list)?;
        if !margin::persona_known(&prefs, &prefs.default_persona) {
            prefs.default_persona = margin::reader_id();
        }
    }
    if let Some(persona) = body.default_persona {
        if !margin::persona_known(&prefs, &persona) {
            return None;
        }
        prefs.default_persona = persona;
    }
    Some(prefs)
}

pub(crate) async fn change_settings(
    State(state): State<AppState>,
    Json(body): Json<PrefsBody>,
) -> Response {
    let held = state.chats.clone();
    let before = match blocking(move || margin::prefs(&held)).await {
        Ok(before) => before,
        Err(refused) => return *refused,
    };
    let flips = body
        .default_mode
        .as_deref()
        .is_some_and(|mode| margin::valid_mode(mode) && mode != before.default_mode);
    if let (true, "along", Some(since)) = (flips, before.default_mode.as_str(), before.since) {
        let notes = state
            .read_store(move |store| {
                Ok(store
                    .notes
                    .iter()
                    .filter(|n| n.created_at >= since && margin::made_by_leo(&n.id))
                    .map(|n| n.id.clone())
                    .collect::<Vec<_>>())
            })
            .await;
        let held = state.chats.clone();
        let pinned = match notes {
            Ok(notes) => blocking(move || margin::pin_modes(&held, &notes, "along")).await,
            Err(code) => return code.into_response(),
        };
        match pinned {
            Ok(true) => {}
            Ok(false) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            Err(refused) => return *refused,
        }
    }
    let view = state.clone();
    let done = blocking(move || {
        let next = changed(margin::prefs(&view.chats), body)?;
        margin::set_prefs(&view.chats, &next).then(|| settings_view(&view))
    })
    .await;
    match done {
        Ok(Some(view)) => Json(view).into_response(),
        Ok(None) => StatusCode::BAD_REQUEST.into_response(),
        Err(refused) => *refused,
    }
}

fn wanted_blocks(body: &ReadBody) -> Result<Vec<String>, Box<Response>> {
    let too_much = body.blocks.len() > margin::MOST_BLOCKS
        || body
            .blocks
            .iter()
            .any(|block| block.chars().count() > margin::MOST_BLOCK_CHARS)
        || body.blocks.iter().map(|b| b.chars().count()).sum::<usize>() > margin::MOST_READ_CHARS;
    if too_much {
        return Err(Box::new(StatusCode::PAYLOAD_TOO_LARGE.into_response()));
    }
    let blocks: Vec<String> = body
        .blocks
        .iter()
        .map(|block| block.trim().to_string())
        .filter(|block| !block.is_empty())
        .collect();
    if blocks.is_empty() {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    Ok(blocks)
}

async fn pass(state: &AppState, id: String, body: ReadBody, through: bool) -> Pass {
    if !margin::valid_version(&body.version) {
        return Pass::Refused(StatusCode::BAD_REQUEST.into_response());
    }
    let blocks = match wanted_blocks(&body) {
        Ok(blocks) => blocks,
        Err(refused) => return Pass::Refused(*refused),
    };
    let note = match open_note(state, &id).await {
        Ok(note) => note,
        Err(refused) => return Pass::Refused(*refused),
    };
    if note.version != body.version {
        return Pass::Refused(StatusCode::CONFLICT.into_response());
    }
    if !blocks
        .iter()
        .all(|block| note.body.contains(block.as_str()))
    {
        return Pass::Refused(StatusCode::BAD_REQUEST.into_response());
    }
    let chats = state.chats.clone();
    let (facts, chats) = {
        let id = id.clone();
        let created = note.created;
        let held = chats.clone();
        let gone = body.gone.clone();
        let keep = body.keep.clone();
        let reads = state.margin.clone();
        let facts = blocking(move || {
            let prefs = margin::prefs(&held);
            let open = margin::open_on(&held, &id);
            ReadFacts {
                limit: margin::room(&open, &gone),
                free: open
                    .iter()
                    .any(|thread| margin::freeable(&reads, thread, &keep)),
                along: margin::mode_of(&held, &id, created) == "along",
                voice: margin::voice_of(&prefs, &margin::persona_of(&held, &id)),
                instructions: margin::instructions_of(&held, &id),
                cap: prefs.cap,
                gap: prefs.gap,
                quotes: open.iter().map(|thread| thread.quote.clone()).collect(),
                open,
            }
        })
        .await;
        match facts {
            Ok(facts) => (facts, chats),
            Err(refused) => return Pass::Refused(*refused),
        }
    };
    if !through {
        if !facts.along {
            return Pass::Refused(said(
                StatusCode::FORBIDDEN,
                "mode",
                "Felix is not reading along in this note.",
            ));
        }
        if !state.margin.was_edited(&id, &note.version) {
            return Pass::Refused(said(
                StatusCode::FORBIDDEN,
                "unedited",
                "Felix reads only what you changed.",
            ));
        }
    }
    if state.steering.any_open() {
        return Pass::Refused(said(
            StatusCode::TOO_MANY_REQUESTS,
            "felix",
            "Felix is answering you.",
        ));
    }
    if !through && facts.quotes.len() >= 2 * margin::MOST_OPEN && !facts.free {
        return Pass::Failed(margin::FULL.into());
    }
    if through && facts.quotes.len() >= facts.limit {
        return Pass::Refused(said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve some, then read through again.",
        ));
    }
    let Some(writer) = state.toolless.clone() else {
        if through {
            state.margin.set_problem(Some(crate::no_writer()));
            return Pass::Refused(said(
                StatusCode::SERVICE_UNAVAILABLE,
                "no-ai",
                &crate::no_writer(),
            ));
        }
        return Pass::Failed(crate::no_writer());
    };
    let gap = if through {
        Duration::ZERO
    } else {
        Duration::from_secs(facts.gap.into())
    };
    let reading = match state.margin.begin(&id, gap) {
        Ok(reading) => reading,
        Err(Refused::Busy) => {
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "Felix is already reading.",
            ))
        }
        Err(Refused::Soon) => {
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "soon",
                "Felix just read this note.",
            ))
        }
    };
    let blocks = if through {
        blocks
    } else {
        let blocks = state.margin.unseen(&id, blocks);
        if blocks.is_empty() {
            return Pass::Silent;
        }
        let counted = chats.clone();
        let cap = facts.cap;
        if !matches!(
            blocking(move || margin::reads_today(&counted, cap)).await,
            Ok(used) if cap == margin::NO_CAP || used < cap
        ) {
            state.margin.forget_blocks(&id, &blocks);
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "daily",
                "Felix has read enough for today.",
            ));
        }
        blocks
    };
    let (system, tag, tokens) = if through {
        (margin::THROUGH_SYSTEM, "note_part", margin::THROUGH_TOKENS)
    } else {
        (margin::READ_SYSTEM, "changed_text", margin::READ_TOKENS)
    };
    let system = margin::instructed(
        &margin::voiced(system, &facts.voice, false),
        &facts.instructions,
    );
    let earlier = if through {
        String::new()
    } else {
        margin::earlier_text(&note.body, &blocks)
    };
    let user = margin::read_prompt(&note.title, &blocks, tag, &earlier);
    let answer = tokio::time::timeout(
        margin::READ_LIMIT,
        tokio::task::spawn_blocking(move || {
            let _reading = reading;
            writer(&system, &user, tokens)
        }),
    )
    .await;
    let forget = |state: &AppState| {
        if !through {
            state.margin.forget_blocks(&id, &blocks);
        }
    };
    let reply = match answer {
        Ok(Ok(Ok(reply))) => reply,
        other => {
            forget(state);
            return Pass::Failed(match other {
                Ok(Ok(Err(e))) => leo_core::writing_said(&e.to_string()),
                _ => margin::PROBLEM.into(),
            });
        }
    };
    let most = body
        .most
        .unwrap_or(margin::MOST_PER_PASS)
        .clamp(1, margin::MOST_PER_PASS)
        .min(facts.limit.saturating_sub(facts.quotes.len()));
    let founds = if through {
        margin::parsed_many(&reply, most)
    } else {
        match margin::parsed(&reply) {
            margin::ReadAnswer::Silent => Some(Vec::new()),
            margin::ReadAnswer::Note(found) => Some(vec![found]),
            margin::ReadAnswer::Invalid => None,
        }
    };
    let Some(founds) = founds else {
        forget(state);
        return Pass::Failed(margin::PROBLEM.into());
    };
    state.margin.set_problem(None);
    let now = match open_note(state, &id).await {
        Ok(now) => now,
        Err(_) => return Pass::Silent,
    };
    let limit = if through {
        facts.limit
    } else {
        2 * margin::MOST_OPEN
    };
    let mut quotes = facts.quotes;
    let mut changed: Vec<(&str, usize, usize)> = body
        .changed
        .iter()
        .take(2 * margin::MOST_OPEN)
        .filter_map(|c| {
            let thread = facts.open.iter().find(|t| t.id == c.id)?;
            let at = margin::by_context(&now.body, thread, &c.text)?;
            Some((c.id.as_str(), at, c.text.len()))
        })
        .collect();
    let mut saved = Vec::new();
    let mut resolved = Vec::new();
    let mut stopped = false;
    for found in founds {
        if !through
            && !blocks
                .iter()
                .any(|block| block.contains(found.quote.as_str()))
        {
            continue;
        }
        let quote = margin::whole_sentences(&now.body, &found.quote, &blocks);
        if margin::crowded(&now.body, &quote, &blocks, &quotes) {
            continue;
        }
        let Some(place) = margin::placed(&now.body, &quote, &blocks) else {
            continue;
        };
        let olds: Vec<String> = changed
            .iter()
            .filter(|(_, at, length)| margin::crowds_at(&now.body, &quote, &blocks, *at, *length))
            .map(|(old, _, _)| old.to_string())
            .collect();
        if through && !olds.is_empty() {
            continue;
        }
        let thread = margin::new_thread(
            &id,
            if through { "through" } else { "read" },
            place,
            &quote,
            vec![margin::felix_says(&found.text)],
        );
        let mut victim = None;
        if !through && quotes.len() >= limit {
            let (held, reads, noted) = (chats.clone(), state.margin.clone(), id.clone());
            let spared = [body.keep.clone(), olds.clone()].concat();
            victim = blocking(move || margin::free_oldest(&held, &reads, &noted, &spared))
                .await
                .ok()
                .flatten();
            if victim.is_none() && olds.is_empty() {
                forget(state);
                return Pass::Failed(margin::FULL.into());
            }
        }
        let held = chats.clone();
        let stored = thread.clone();
        let (reads, keep) = (state.margin.clone(), body.keep.clone());
        let put = blocking(move || {
            margin::put_replacing(&held, &reads, &stored, &olds, &keep, victim, limit)
        })
        .await;
        match put {
            Ok(margin::Swap::Put(gone)) => {
                for old in gone {
                    if let Some(at) = quotes.iter().position(|q| *q == old.quote) {
                        quotes.remove(at);
                    }
                    changed.retain(|(id, _, _)| *id != old.id);
                    resolved.push(old.id);
                }
            }
            Ok(margin::Swap::Kept) => continue,
            _ => {
                stopped = true;
                break;
            }
        }
        quotes.push(quote);
        if !through {
            let (held, reads, noted) = (chats.clone(), state.margin.clone(), id.clone());
            let (keep, fresh) = (body.keep.clone(), thread.id.clone());
            let tidied =
                blocking(move || margin::resolve_untouched(&held, &reads, &noted, &keep, &fresh))
                    .await;
            resolved.extend(tidied.unwrap_or_default());
        }
        saved.push(thread);
    }
    if !through && (!stopped || !saved.is_empty()) {
        let counted = chats.clone();
        let cap = facts.cap;
        let _ = blocking(move || margin::spend_read(&counted, cap)).await;
    }
    if saved.is_empty() && resolved.is_empty() {
        Pass::Silent
    } else {
        Pass::Saved(saved, resolved)
    }
}

pub(crate) async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadBody>,
) -> Response {
    match pass(&state, id, body, false).await {
        Pass::Saved(saved, resolved) => {
            Json(serde_json::json!({ "note": saved.first(), "resolved": resolved })).into_response()
        }
        Pass::Silent => silent(),
        Pass::Failed(error) => failed(&state, error),
        Pass::Refused(refused) => refused,
    }
}

pub(crate) async fn analyze(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadBody>,
) -> Response {
    match pass(&state, id, body, true).await {
        Pass::Saved(saved, _) => Json(serde_json::json!({ "notes": saved })).into_response(),
        Pass::Silent => Json(serde_json::json!({ "notes": [] })).into_response(),
        Pass::Failed(error) => {
            state.margin.set_problem(Some(error.clone()));
            Json(serde_json::json!({ "notes": [], "failed": true, "error": error })).into_response()
        }
        Pass::Refused(refused) => refused,
    }
}

pub(crate) async fn forget(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
) -> Response {
    if !margin::valid_note(&id) || !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(_replying) = state.margin.replying(&thread) else {
        return said(StatusCode::CONFLICT, "replying", "Felix is answering this.");
    };
    let chats = state.chats.clone();
    let (held, wanted) = (id, thread.clone());
    let found = blocking(move || margin::one(&chats, &held, &wanted)).await;
    let found = match found {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(refused) => return *refused,
    };
    if found.messages.iter().any(|m| m.from != "felix") {
        return said(
            StatusCode::CONFLICT,
            "replied",
            "You replied to this, so it stays.",
        );
    }
    let chats = state.chats.clone();
    match blocking(move || margin::remove(&chats, &thread)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(refused) => *refused,
    }
}

async fn written(
    state: &AppState,
    system: impl FnOnce(&std::path::Path) -> String + Send + 'static,
    user: String,
    tokens: u32,
) -> Result<String, Box<Response>> {
    let Some(streamer) = state.toolless_chat.clone() else {
        return Err(Box::new(said(
            StatusCode::SERVICE_UNAVAILABLE,
            "no-ai",
            &crate::no_writer(),
        )));
    };
    let Some(foreground) = state.work.foreground() else {
        return Err(Box::new(said(
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "Felix is working on several answers. Try again in a moment.",
        )));
    };
    let chats = state.chats.clone();
    let started = Instant::now();
    let usage = std::sync::Arc::clone(&state.usage);
    let reply = blocking(move || {
        let _foreground = foreground;
        let reply = streamer(
            &system(&chats),
            &user,
            tokens,
            &mut |_| {},
            &mut || {},
            &|| false,
        );
        let spent = reply.as_ref().ok().and_then(|r| r.spent.clone());
        usage.record(
            Purpose::OtherWriting,
            spent.as_ref(),
            started.elapsed(),
            reply.is_err(),
        );
        reply
    })
    .await?;
    Ok(reply.map(|r| r.text).unwrap_or_default())
}

async fn answered(
    state: &AppState,
    note: &Open,
    thread: &Thread,
    question: &str,
) -> Result<String, Box<Response>> {
    let user = margin::reply_prompt(&note.title, &note.body, thread, question);
    let held = thread.note.clone();
    let asked = question != margin::UNSPOKEN;
    let text = written(
        state,
        move |chats| {
            let prefs = margin::prefs(chats);
            let voice = margin::voice_of(&prefs, &margin::persona_of(chats, &held));
            let said = margin::instructions_of(chats, &held);
            margin::instructed(&margin::voiced(margin::REPLY_SYSTEM, &voice, asked), &said)
        },
        user,
        margin::REPLY_TOKENS,
    )
    .await?;
    let text: String = text.trim().chars().take(margin::MOST_REPLY).collect();
    let text = margin::without_open_fence(&text);
    if text.is_empty() {
        return Err(Box::new(said(
            StatusCode::BAD_GATEWAY,
            "no-answer",
            "Felix could not answer just now. Try again.",
        )));
    }
    Ok(text.to_string())
}

pub(crate) async fn ask(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AskBody>,
) -> Response {
    if !margin::valid_version(&body.version) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let question = body.question.trim().to_string();
    if question.chars().count() > margin::MOST_ASK
        || body.quote.chars().count() > margin::MOST_ASKED
        || body.prefix.chars().count() > margin::MOST_QUOTE
        || body.suffix.chars().count() > margin::MOST_QUOTE
    {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if body.quote.trim().chars().count() < margin::LEAST_QUOTE {
        return said(
            StatusCode::BAD_REQUEST,
            "short",
            "Select a few more words, then ask.",
        );
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    if note.version != body.version {
        return StatusCode::CONFLICT.into_response();
    }
    let Some(place) = margin::placed_exact(&note.body, &body.prefix, &body.quote, &body.suffix)
    else {
        return said(
            StatusCode::CONFLICT,
            "lost",
            "Those words are not in the note any more.",
        );
    };
    let chats = state.chats.clone();
    let held = id.clone();
    let open = match blocking(move || margin::open_on(&chats, &held)).await {
        Ok(open) => open,
        Err(refused) => return *refused,
    };
    let limit = margin::room(&open, &body.gone);
    if open.len() >= limit {
        return said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one, then ask again.",
        );
    }
    let mut thread = margin::new_thread(&id, "ask", place, &body.quote, Vec::new());
    let asked = if question.is_empty() {
        margin::UNSPOKEN
    } else {
        &question
    };
    let text = match answered(&state, &note, &thread, asked).await {
        Ok(text) => text,
        Err(refused) => return *refused,
    };
    thread.messages = vec![margin::felix_says(&text)];
    if !question.is_empty() {
        thread.messages.insert(0, margin::you_say(&question));
    }
    let chats = state.chats.clone();
    let saved = thread.clone();
    match blocking(move || margin::put(&chats, &saved, limit)).await {
        Ok(true) => Json(thread).into_response(),
        Ok(false) => said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one, then ask again.",
        ),
        Err(refused) => *refused,
    }
}

pub(crate) async fn reply(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<ReplyBody>,
) -> Response {
    if !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let text = body.text.trim().to_string();
    if text.chars().count() > margin::MOST_ASK {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if text.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let request = body
        .request
        .filter(|r| !r.is_empty() && r.len() <= insert::MOST_REQUEST);
    if let Some(found) = request.as_deref().and_then(|r| insert::remembered(&id, r)) {
        return Json(found).into_response();
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    let Some(_replying) = state.margin.replying(&thread) else {
        return said(
            StatusCode::CONFLICT,
            "busy",
            "Felix is already answering this.",
        );
    };
    let chats = state.chats.clone();
    let (held, wanted) = (id.clone(), thread.clone());
    let found = blocking(move || margin::one(&chats, &held, &wanted)).await;
    let mut found = match found {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(refused) => return *refused,
    };
    if found.status != "open" {
        return StatusCode::CONFLICT.into_response();
    }
    if found.messages.len() + 2 > margin::MOST_MESSAGES {
        return said(
            StatusCode::CONFLICT,
            "long",
            "This thread is full. Open it in chat to go on.",
        );
    }
    let answer = match answered(&state, &note, &found, &text).await {
        Ok(answer) => answer,
        Err(refused) => return *refused,
    };
    found.messages.push(margin::you_say(&text));
    found.messages.push(margin::felix_says(&answer));
    found.updated_at = Utc::now();
    let chats = state.chats.clone();
    let saved = found.clone();
    match blocking(move || margin::save_messages(&chats, &saved)).await {
        Ok(true) => {
            if let Some(request) = request.as_deref() {
                insert::remember(&id, request, &serde_json::json!(found));
            }
            Json(found).into_response()
        }
        Ok(false) => said(
            StatusCode::CONFLICT,
            "closed",
            "This note was closed while Felix answered.",
        ),
        Err(refused) => *refused,
    }
}

pub(crate) async fn weave(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<WeaveBody>,
) -> Response {
    if !margin::valid_note(&thread) || !margin::valid_version(&body.base) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let request = body
        .request
        .filter(|r| !r.is_empty() && r.len() <= insert::MOST_REQUEST);
    if let Some(found) = request.as_deref().and_then(|r| insert::remembered(&id, r)) {
        return Json(found).into_response();
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    if note.version != body.base {
        return StatusCode::CONFLICT.into_response();
    }
    let Some(_replying) = state.margin.replying(&thread) else {
        return said(
            StatusCode::CONFLICT,
            "busy",
            "Felix is already working on this.",
        );
    };
    let chats = state.chats.clone();
    let (held, wanted) = (id.clone(), thread.clone());
    let found = match blocking(move || margin::one(&chats, &held, &wanted)).await {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(refused) => return *refused,
    };
    if found.status != "open" {
        return StatusCode::CONFLICT.into_response();
    }
    let Some((from, to)) = margin::paragraph_of(&note.body, &found) else {
        return said(
            StatusCode::CONFLICT,
            "lost",
            "Those words are not in the note any more.",
        );
    };
    let passage = note.body[from..to].to_string();
    let before = &note.body[..from];
    if margin::without_open_fence(before).len() != before.len()
        || margin::without_open_fence(&passage).len() != passage.len()
    {
        return said(
            StatusCode::CONFLICT,
            "code",
            "Felix can only work this into text, not into part of a code block.",
        );
    }
    if passage.chars().count() > margin::MOST_WEAVE {
        return said(
            StatusCode::PAYLOAD_TOO_LARGE,
            "long",
            "This paragraph is too long for Felix to rewrite. Split it, then try again.",
        );
    }
    let user = margin::weave_prompt(&note.title, &passage, &found);
    let raw = match written(
        &state,
        |_| margin::WEAVE_SYSTEM.to_string(),
        user,
        margin::WEAVE_TOKENS,
    )
    .await
    {
        Ok(raw) => raw,
        Err(refused) => return *refused,
    };
    let Some(text) = margin::woven(&passage, &raw, &found) else {
        return said(
            StatusCode::BAD_GATEWAY,
            "no-answer",
            "Felix could not work this in just now. Try again.",
        );
    };
    let (base, note_id) = (body.base, id.clone());
    let done = state
        .with_store(move |store| {
            let note = store.find_note_mut(&note_id).ok_or(StatusCode::NOT_FOUND)?;
            if version_of(note) != base {
                return Err(StatusCode::CONFLICT);
            }
            let line = note.body[..from].matches('\n').count() + 1;
            let next = format!("{}{text}{}", &note.body[..from], &note.body[to..]);
            let before = std::mem::replace(&mut note.body, next);
            note.updated_at = Utc::now();
            let applied = Applied {
                note: NoteResponse::from_note(note),
                before,
            };
            save(store)?;
            Ok((applied, line, line + text.matches('\n').count()))
        })
        .await;
    match done {
        Ok((applied, from, to)) => {
            let mut value = serde_json::json!(applied);
            value["from"] = from.into();
            value["to"] = to.into();
            if let Some(request) = request.as_deref() {
                insert::remember(&id, request, &value);
            }
            Json(value).into_response()
        }
        Err(StatusCode::CONFLICT) => said(
            StatusCode::CONFLICT,
            "changed",
            "The note changed while Felix was working this in. Try once more.",
        ),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn pin(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<PinBody>,
) -> Response {
    if !margin::valid_note(&id) || !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    match blocking(move || margin::set_pinned(&chats, &id, &thread, body.pinned)).await {
        Ok(Some(found)) => Json(found).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(refused) => *refused,
    }
}

pub(crate) async fn set_status(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<StatusBody>,
) -> Response {
    if !margin::valid_note(&id) || !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !matches!(body.status.as_str(), "open" | "resolved" | "dismissed") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let done = blocking(move || {
        let limit = margin::room(&margin::open_on(&chats, &id), &body.gone);
        margin::set_status(&chats, &id, &thread, &body.status, limit)
    })
    .await;
    match done {
        Ok(margin::Status::Done(found)) => Json(found).into_response(),
        Ok(margin::Status::Missing) => StatusCode::NOT_FOUND.into_response(),
        Ok(margin::Status::Full) => said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one first.",
        ),
        Err(refused) => *refused,
    }
}
