use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use axum::{
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::ics::{self, Event};
use crate::reply::error;
use crate::AppState;

pub const MOST_BYTES: u64 = 5_000_000;
const MOST_CALENDARS: usize = 5;
const STALE_AFTER_MINUTES: i64 = 15;
const LOOK_BACK_HOURS: i64 = 3;
const LOOK_AHEAD_DAYS: i64 = 7;
const KEPT_DAYS: i64 = 14;
const LIVE_MINUTES: i64 = 2;
const KEPT_EXPANSIONS: usize = 12;
const MOST_DAYS_SHOWN: i64 = 43;
const MOST_REACH_DAYS: i64 = 400;
const OFFER_BEFORE_MINUTES: i64 = 2;
const OFFER_AFTER_MINUTES: i64 = 10;
const PROMPTED_KEPT_DAYS: i64 = 2;
const OFFER_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

type Expansion = (String, Arc<Vec<Event>>);

#[derive(Default)]
pub(crate) struct Calendar {
    busy: Arc<AtomicBool>,
    expanded: Mutex<VecDeque<Expansion>>,
    bodies: Mutex<std::collections::HashMap<String, (String, i64, u64)>>,
    settings: Mutex<()>,
}

#[derive(Debug, Default)]
struct Feed {
    fetched_at: Option<DateTime<Utc>>,
    tried_at: Option<DateTime<Utc>>,
    etag: Option<String>,
    modified: Option<String>,
    problem: Option<String>,
}

fn db_for(notes: &Path) -> Arc<crate::db::Db> {
    crate::db::at(&data(notes).join(crate::db::FILE))
}

fn time_of(text: Option<String>) -> Option<DateTime<Utc>> {
    text.and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
        .map(|t| t.with_timezone(&Utc))
}

fn feed_of(notes: &Path, id: &str) -> Feed {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT fetched_at, tried_at, etag, modified, problem FROM calendar_feeds WHERE feed = ?1",
                [id],
                |r| {
                    Ok(Feed {
                        fetched_at: time_of(r.get(0)?),
                        tried_at: time_of(r.get(1)?),
                        etag: r.get(2)?,
                        modified: r.get(3)?,
                        problem: r.get(4)?,
                    })
                },
            )
        })
        .unwrap_or_default()
}

fn body_of(notes: &Path, id: &str) -> Option<(DateTime<Utc>, Vec<u8>)> {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT fetched_at, body FROM calendar_feeds WHERE feed = ?1 AND body IS NOT NULL",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
        })
        .ok()
        .and_then(|(at, body)| time_of(at).map(|at| (at, body)))
}

fn stamp_of(notes: &Path, id: &str) -> Option<(String, i64)> {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT fetched_at, length(body) FROM calendar_feeds WHERE feed = ?1 AND body IS NOT NULL",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?)),
            )
        })
        .ok()
        .and_then(|(at, length)| at.map(|at| (at, length)))
}

impl Calendar {
    pub(crate) fn events(
        &self,
        notes: &Path,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<(String, String, Event)> {
        let linking: Linking = read(&settings_path(notes));
        let mut out = Vec::new();
        for linked in &linking.calendars {
            let Some((fetched, length)) = stamp_of(notes, &linked.id) else {
                continue;
            };
            let known = self
                .bodies
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&linked.id)
                .filter(|(at, len, _)| *at == fetched && *len == length)
                .map(|(_, _, hash)| *hash);
            let mut read = None;
            let hash = match known {
                Some(hash) => hash,
                None => {
                    let Some((_, body)) = body_of(notes, &linked.id) else {
                        continue;
                    };
                    let hash = crate::vectors::fnv64(body.iter().copied());
                    self.bodies
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(linked.id.clone(), (fetched, length, hash));
                    read = Some(body);
                    hash
                }
            };
            let key = format!(
                "{}|{hash:016x}|{}|{}",
                linked.id,
                from.timestamp(),
                to.timestamp()
            );
            let kept = self
                .expanded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, e)| Arc::clone(e));
            let events = match kept {
                Some(events) => events,
                None => {
                    let Some(body) = read.or_else(|| body_of(notes, &linked.id).map(|(_, b)| b))
                    else {
                        continue;
                    };
                    let events =
                        Arc::new(ics::read(&String::from_utf8_lossy(&body), from, to).events);
                    let mut kept = self.expanded.lock().unwrap_or_else(|e| e.into_inner());
                    kept.push_back((key, Arc::clone(&events)));
                    while kept.len() > KEPT_EXPANSIONS {
                        kept.pop_front();
                    }
                    events
                }
            };
            out.extend(
                events
                    .iter()
                    .map(|e| (linked.id.clone(), linked.name.clone(), e.clone())),
            );
        }
        out.sort_by_key(|(_, _, e)| e.start);
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Linked {
    id: String,
    name: String,
    added_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Linking {
    calendars: Vec<Linked>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offer: Option<bool>,
}

fn data(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).to_path_buf()
}

pub fn settings_path(notes: &Path) -> PathBuf {
    data(notes).join("calendars.json")
}

fn account(id: &str) -> String {
    format!("calendar-link-{id}")
}

fn read<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    use std::io::Read;
    let Ok(file) = leo_core::paths::open_regular(path) else {
        return T::default();
    };
    let mut bytes = Vec::new();
    if file
        .take(MOST_BYTES * 4 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MOST_BYTES * 4
    {
        return T::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn link_of(text: &str) -> Result<String> {
    let text = text.trim();
    let text = match text.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => text.to_string(),
    };
    let Some((scheme, rest)) = text.split_once("://") else {
        bail!("That is not a calendar link. Copy the whole address, starting with https://.");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || rest.contains(char::is_whitespace) {
        bail!("That is not a calendar link. Copy the whole address, starting with https://.");
    }
    if scheme != "https" {
        bail!("Use the calendar's https:// address.");
    }
    if text.len() > 2000 {
        bail!("That link is too long to be a calendar address.");
    }
    Ok(text)
}

fn parsed(bytes: &[u8], now: DateTime<Utc>) -> Result<ics::Calendar> {
    let text = String::from_utf8_lossy(bytes);
    if !text.contains("BEGIN:VCALENDAR") {
        bail!("That link does not lead to a calendar. In Google Calendar, copy “Secret address in iCal format”.");
    }
    Ok(ics::read(
        &text,
        now - Duration::days(1),
        now + Duration::days(KEPT_DAYS),
    ))
}

fn refresh(notes: &Path, access: &dyn crate::CalendarAccess, now: DateTime<Utc>) {
    let linking: Linking = read(&settings_path(notes));
    let db = db_for(notes);
    let stamp = now.to_rfc3339();
    for linked in &linking.calendars {
        let before = feed_of(notes, &linked.id);
        let fetched = access
            .get(&account(&linked.id))
            .and_then(|link| link.ok_or_else(|| anyhow::anyhow!("the link is missing")))
            .and_then(|link| {
                access.fetch_since(&link, before.etag.as_deref(), before.modified.as_deref())
            });
        let problem = "leo could not read this calendar just now. If it keeps happening, remove it and paste its link again.";
        let _ = db.with(|c| match &fetched {
            Ok(crate::Fetched { body: None, .. }) => c.execute(
                "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, problem) VALUES (?1, ?2, ?2, NULL) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, problem = NULL",
                rusqlite::params![linked.id, stamp],
            ),
            Ok(crate::Fetched { body: Some(bytes), etag, modified })
                if bytes.len() as u64 <= MOST_BYTES && String::from_utf8_lossy(bytes).contains("BEGIN:VCALENDAR") =>
            {
                let stored: Option<Vec<u8>> = c
                    .query_row("SELECT body FROM calendar_feeds WHERE feed = ?1", [&linked.id], |r| r.get(0))
                    .ok()
                    .flatten();
                if stored.as_deref() == Some(bytes.as_slice()) {
                    return c.execute(
                        "UPDATE calendar_feeds SET fetched_at = ?2, tried_at = ?2, etag = ?3, modified = ?4, problem = NULL WHERE feed = ?1",
                        rusqlite::params![linked.id, stamp, etag, modified],
                    );
                }
                c.execute(
                    "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, etag, modified, problem, body) VALUES (?1, ?2, ?2, ?3, ?4, NULL, ?5) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, etag = excluded.etag, modified = excluded.modified, problem = NULL, body = excluded.body",
                    rusqlite::params![linked.id, stamp, etag, modified, bytes],
                )
            }
            _ => c.execute(
                "INSERT INTO calendar_feeds (feed, tried_at, problem) VALUES (?1, ?2, ?3) ON CONFLICT(feed) DO UPDATE SET tried_at = excluded.tried_at, problem = excluded.problem",
                rusqlite::params![linked.id, stamp, problem],
            ),
        });
    }
    let known: Vec<String> = linking.calendars.iter().map(|c| c.id.clone()).collect();
    let _ = db.with(|c| {
        let mut statement = c.prepare("SELECT feed FROM calendar_feeds")?;
        let all: Vec<String> = statement.query_map([], |r| r.get(0))?.flatten().collect();
        for gone in all.iter().filter(|f| !known.contains(f)) {
            c.execute("DELETE FROM calendar_feeds WHERE feed = ?1", [gone])?;
        }
        Ok(())
    });
}

fn synced(notes: &Path, linking: &Linking) -> Option<DateTime<Utc>> {
    linking
        .calendars
        .iter()
        .map(|c| feed_of(notes, &c.id).fetched_at)
        .collect::<Option<Vec<_>>>()
        .and_then(|times| times.into_iter().min())
}

fn calendars_json(notes: &Path, linking: &Linking) -> Vec<serde_json::Value> {
    linking
        .calendars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "added_at": c.added_at,
                "problem": feed_of(notes, &c.id).problem,
            })
        })
        .collect()
}

fn status(notes: &Path, calendar: &Calendar, busy: bool, now: DateTime<Utc>) -> serde_json::Value {
    let linking: Linking = read(&settings_path(notes));
    let hour = DateTime::from_timestamp(now.timestamp().div_euclid(3600) * 3600, 0).unwrap_or(now);
    let from = hour - Duration::hours(LOOK_BACK_HOURS);
    let to = hour + Duration::days(LOOK_AHEAD_DAYS) + Duration::hours(1);
    let events: Vec<Event> = calendar
        .events(notes, from, to)
        .into_iter()
        .map(|(_, _, e)| e)
        .filter(|e| !e.all_day && e.end >= now - Duration::minutes(30))
        .collect();
    serde_json::json!({
        "connected": !linking.calendars.is_empty(),
        "calendars": calendars_json(notes, &linking),
        "synced": synced(notes, &linking),
        "refreshing": busy,
        "events": events,
        "offer": linking.offer != Some(false),
    })
}

fn starting(
    notes: &Path,
    calendar: &Calendar,
    live: bool,
    now: DateTime<Utc>,
) -> Vec<crate::app_settings::EventStarting> {
    let linking: Linking = read(&settings_path(notes));
    if live || linking.calendars.is_empty() || linking.offer == Some(false) {
        return Vec::new();
    }
    let db = db_for(notes);
    let old = (now - Duration::days(PROMPTED_KEPT_DAYS)).to_rfc3339();
    let _ = db.with(|c| c.execute("DELETE FROM calendar_prompted WHERE start < ?1", [old]));
    let hour = DateTime::from_timestamp(now.timestamp().div_euclid(3600) * 3600, 0).unwrap_or(now);
    calendar
        .events(notes, hour - Duration::hours(1), hour + Duration::hours(2))
        .into_iter()
        .map(|(_, _, e)| e)
        .filter(|e| {
            !e.all_day
                && e.start - Duration::minutes(OFFER_BEFORE_MINUTES) <= now
                && now < e.start + Duration::minutes(OFFER_AFTER_MINUTES)
        })
        .filter(|e| {
            db.with(|c| {
                c.execute(
                    "INSERT OR IGNORE INTO calendar_prompted (uid, start) VALUES (?1, ?2)",
                    [&e.id, &e.start.to_rfc3339()],
                )
            })
            .is_ok_and(|added| added == 1)
        })
        .map(|e| crate::app_settings::EventStarting {
            online: [&e.location, &e.context]
                .iter()
                .any(|text| text.contains("http://") || text.contains("https://")),
            uid: e.id,
            title: e.title,
            start: e.start,
        })
        .collect()
}

fn offer(
    notes: &Path,
    calendar: &Calendar,
    api: &dyn crate::app_settings::AppApi,
    live: bool,
    now: DateTime<Utc>,
) {
    for event in starting(notes, calendar, live, now) {
        api.event_starting(event);
    }
}

fn catch_up(
    notes: &Path,
    calendar: &Calendar,
    access: &dyn crate::CalendarAccess,
    now: DateTime<Utc>,
) {
    if stale(notes, now, STALE_AFTER_MINUTES) {
        if let Some(_busy) = Busy::take(&calendar.busy) {
            refresh(notes, access, now);
        }
    }
}

pub(crate) async fn offer_to_record(state: AppState) {
    let Some(api) = state.app_settings.clone() else {
        return;
    };
    let mut every = tokio::time::interval(OFFER_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        let notes = state.notes_dir.clone();
        let live = crate::record::live(&state.recording);
        let calendar = Arc::clone(&state.calendar);
        let access = state.calendar_access.clone();
        let api = Arc::clone(&api);
        let _ = tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            if let Some(access) = access {
                catch_up(&notes, &calendar, access.as_ref(), now);
            }
            offer(&notes, &calendar, api.as_ref(), live, now)
        })
        .await;
    }
}

fn stale(notes: &Path, now: DateTime<Utc>, minutes: i64) -> bool {
    let linking: Linking = read(&settings_path(notes));
    linking.calendars.iter().any(|c| {
        feed_of(notes, &c.id)
            .tried_at
            .is_none_or(|t| now - t > Duration::minutes(minutes))
    })
}

struct Busy(Arc<AtomicBool>);

impl Busy {
    fn take(flag: &Arc<AtomicBool>) -> Option<Busy> {
        (!flag.swap(true, Ordering::AcqRel)).then(|| Busy(Arc::clone(flag)))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let notes = state.notes_dir.clone();
    let now = Utc::now();
    let access = state.calendar_access.clone();
    let calendar = Arc::clone(&state.calendar);
    let shown = tokio::task::spawn_blocking(move || {
        if let (Some(access), true) = (access, stale(&notes, now, STALE_AFTER_MINUTES)) {
            if let Some(busy) = Busy::take(&calendar.busy) {
                let notes = notes.clone();
                std::thread::spawn(move || {
                    let _busy = busy;
                    refresh(&notes, access.as_ref(), Utc::now());
                });
            }
        }
        let busy = calendar.busy.load(Ordering::Acquire);
        status(&notes, &calendar, busy, now)
    })
    .await;
    match shown {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendars could not be read.",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct Add {
    link: String,
    #[serde(default)]
    name: String,
}

pub(crate) async fn add(State(state): State<AppState>, Json(body): Json<Add>) -> Response {
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let link = match link_of(&body.link) {
        Ok(link) => link,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    let notes = state.notes_dir.clone();
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return error(
            StatusCode::CONFLICT,
            "leo is reading your calendars right now. Try again in a moment.",
        );
    };
    let name: String = body.name.trim().chars().take(80).collect();
    let shared = Arc::clone(&state.calendar);
    let done = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _busy = busy;
        let mut linking: Linking = read(&settings_path(&notes));
        if linking.calendars.len() >= MOST_CALENDARS {
            bail!("leo can follow up to five calendars. Remove one first.");
        }
        let now = Utc::now();
        let bytes = access.fetch(&link).map_err(|_| {
            anyhow::anyhow!("leo could not open that link. Check that it is the calendar's secret address and that this computer is online.")
        })?;
        let calendar = parsed(&bytes, now)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        access.set(&account(&id), &link)?;
        linking.calendars.push(Linked {
            id: id.clone(),
            name: if name.is_empty() {
                calendar.name.unwrap_or_else(|| "Calendar".into())
            } else {
                name
            },
            added_at: now,
        });
        {
            let _saving = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            linking.offer = read::<Linking>(&settings_path(&notes)).offer;
            if let Err(e) = leo_core::recording::write_json(&settings_path(&notes), &linking) {
                let _ = access.delete(&account(&id));
                return Err(e);
            }
        }
        refresh(&notes, access.as_ref(), now);
        Ok(status(&notes, &shared, false, now))
    })
    .await;
    match done {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendar could not be added.",
        ),
    }
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let notes = state.notes_dir.clone();
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return error(
            StatusCode::CONFLICT,
            "leo is reading your calendars right now. Try again in a moment.",
        );
    };
    let calendar = Arc::clone(&state.calendar);
    let done = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _busy = busy;
        let mut linking: Linking = read(&settings_path(&notes));
        let before = linking.calendars.len();
        linking.calendars.retain(|c| c.id != id);
        if linking.calendars.len() == before {
            bail!("That calendar is already gone.");
        }
        access.delete(&account(&id))?;
        {
            let _saving = calendar.settings.lock().unwrap_or_else(|e| e.into_inner());
            linking.offer = read::<Linking>(&settings_path(&notes)).offer;
            leo_core::recording::write_json(&settings_path(&notes), &linking)?;
        }
        refresh(&notes, access.as_ref(), Utc::now());
        Ok(status(&notes, &calendar, false, Utc::now()))
    })
    .await;
    match done {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => error(StatusCode::NOT_FOUND, e),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendar could not be removed.",
        ),
    }
}

pub(crate) async fn refresh_now(State(state): State<AppState>) -> Response {
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let notes = state.notes_dir.clone();
    let calendar = Arc::clone(&state.calendar);
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return match tokio::task::spawn_blocking(move || {
            status(&notes, &calendar, true, Utc::now())
        })
        .await
        {
            Ok(value) => Json(value).into_response(),
            Err(_) => error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The calendars could not be read.",
            ),
        };
    };
    let done = tokio::task::spawn_blocking(move || {
        let _busy = busy;
        refresh(&notes, access.as_ref(), Utc::now());
        status(&notes, &calendar, false, Utc::now())
    })
    .await;
    match done {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendars could not be read.",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct Change {
    offer: bool,
}

pub(crate) async fn change(State(state): State<AppState>, Json(body): Json<Change>) -> Response {
    let notes = state.notes_dir.clone();
    let calendar = Arc::clone(&state.calendar);
    let busy = state.calendar.busy.load(Ordering::Acquire);
    let done = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _saving = calendar.settings.lock().unwrap_or_else(|e| e.into_inner());
        let mut linking: Linking = read(&settings_path(&notes));
        linking.offer = Some(body.offer);
        leo_core::recording::write_json(&settings_path(&notes), &linking)?;
        Ok(status(&notes, &calendar, busy, Utc::now()))
    })
    .await;
    match done {
        Ok(Ok(value)) => Json(value).into_response(),
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "That calendar setting could not be saved.",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct Window {
    from: String,
    to: String,
    #[serde(default)]
    live: Option<String>,
}

fn window_of(w: &Window, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let from = DateTime::parse_from_rfc3339(&w.from)
        .ok()?
        .with_timezone(&Utc);
    let to = DateTime::parse_from_rfc3339(&w.to)
        .ok()?
        .with_timezone(&Utc);
    let reach = Duration::days(MOST_REACH_DAYS);
    (to > from
        && to - from <= Duration::days(MOST_DAYS_SHOWN)
        && from > now - reach
        && to < now + reach)
        .then_some((from, to))
}

pub(crate) fn marker_of(context: &str) -> Option<&str> {
    let at = context.find("leo-task:")? + "leo-task:".len();
    let rest = &context[at..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(rest.len());
    (end >= 8).then(|| &rest[..end])
}

pub(crate) async fn schedule(
    State(state): State<AppState>,
    Query(window): Query<Window>,
) -> Response {
    let now = Utc::now();
    let Some((from, to)) = window_of(&window, now) else {
        return error(
            StatusCode::BAD_REQUEST,
            "Ask for at most six weeks, within a year of today.",
        );
    };
    let notes = state.notes_dir.clone();
    let live = matches!(window.live.as_deref(), Some("1" | "true"));
    let wait = if live {
        LIVE_MINUTES
    } else {
        STALE_AFTER_MINUTES
    };
    if let (Some(access), true) = (state.calendar_access.clone(), stale(&notes, now, wait)) {
        if let Some(busy) = Busy::take(&state.calendar.busy) {
            let notes = notes.clone();
            std::thread::spawn(move || {
                let _busy = busy;
                refresh(&notes, access.as_ref(), Utc::now());
            });
        }
    }
    let tasks = match crate::routes::tasks::all_listed(&state).await {
        Ok(all) => all,
        Err(code) => return code.into_response(),
    };
    let calendar = Arc::clone(&state.calendar);
    let busy = state.calendar.busy.load(Ordering::Acquire);
    let shown = tokio::task::spawn_blocking(move || {
        let linking: Linking = read(&settings_path(&notes));
        let events: Vec<serde_json::Value> = calendar
            .events(&notes, from, to)
            .into_iter()
            .map(|(feed, name, event)| {
                let task = marker_of(&event.context).and_then(|m| {
                    tasks
                        .iter()
                        .find(|t| t.id.starts_with(m))
                        .map(|t| t.id.clone())
                });
                let mut value = serde_json::json!(event);
                value["id"] = serde_json::json!(format!("{feed}:{}", event.id));
                value["feed"] = serde_json::json!(feed);
                value["calendar"] = serde_json::json!(name);
                value["task"] = serde_json::json!(task);
                value
            })
            .collect();
        serde_json::json!({
            "connected": !linking.calendars.is_empty(),
            "calendars": calendars_json(&notes, &linking),
            "synced": synced(&notes, &linking),
            "refreshing": busy,
            "events": events,
            "tasks": tasks,
        })
    })
    .await;
    match shown {
        Ok(value) => Json(value).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[cfg(unix)]
    #[test]
    fn a_linked_calendar_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        std::fs::write(&real, r#"{"kept":true}"#).unwrap();
        let link = dir.path().join("calendars.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(read::<serde_json::Value>(&real)["kept"].as_bool().unwrap());
        assert!(read::<serde_json::Value>(&link).is_null());
    }

    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) secrets: Mutex<HashMap<String, String>>,
        pub(crate) pages: Mutex<HashMap<String, String>>,
    }

    impl crate::CalendarAccess for Fake {
        fn fetch(&self, url: &str) -> Result<Vec<u8>> {
            self.pages
                .lock()
                .unwrap()
                .get(url)
                .map(|t| t.clone().into_bytes())
                .ok_or_else(|| anyhow::anyhow!("offline"))
        }
        fn get(&self, account: &str) -> Result<Option<String>> {
            Ok(self.secrets.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<()> {
            self.secrets
                .lock()
                .unwrap()
                .insert(account.into(), value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.secrets.lock().unwrap().remove(account);
            Ok(())
        }
    }

    const MEETINGS: &str = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:sync\nSUMMARY:Design sync\nDTSTART;TZID=America/Los_Angeles:20261030T090000\nDTEND;TZID=America/Los_Angeles:20261030T093000\nRRULE:FREQ=DAILY\nEND:VEVENT\nBEGIN:VEVENT\nUID:holiday\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261105\nDTEND;VALUE=DATE:20261106\nEND:VEVENT\nEND:VCALENDAR\n";

    #[derive(Default)]
    struct Asked(Mutex<Vec<(String, String, DateTime<Utc>)>>);

    impl crate::app_settings::AppApi for Asked {
        fn view(&self) -> Result<serde_json::Value> {
            Ok(serde_json::json!({}))
        }
        fn change(&self, _: serde_json::Value) -> Result<serde_json::Value> {
            self.view()
        }
        fn health(&self, _: &Path) -> Result<serde_json::Value> {
            self.view()
        }
        fn event_starting(&self, e: crate::app_settings::EventStarting) {
            self.0.lock().unwrap().push((e.uid, e.title, e.start));
        }
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn linked(offer: Option<bool>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        let linking = Linking {
            calendars: vec![Linked {
                id: "work".into(),
                name: "Work".into(),
                added_at: Utc::now(),
            }],
            offer,
        };
        leo_core::recording::write_json(&settings_path(&notes), &linking).unwrap();
        db_for(&notes)
            .with(|c| {
                c.execute(
                    "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, body) VALUES ('work', ?1, ?1, ?2)",
                    rusqlite::params![Utc::now().to_rfc3339(), MEETINGS.as_bytes()],
                )
            })
            .unwrap();
        (dir, notes)
    }

    #[derive(Default)]
    struct Counting(std::sync::atomic::AtomicUsize);

    impl crate::CalendarAccess for Counting {
        fn fetch(&self, _: &str) -> Result<Vec<u8>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(MEETINGS.as_bytes().to_vec())
        }
        fn get(&self, _: &str) -> Result<Option<String>> {
            Ok(Some("https://example.com/a.ics".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn refreshing_with_the_same_bytes_keeps_the_expansion() {
        let (_dir, notes) = linked(None);
        db_for(&notes)
            .with(|c| {
                c.execute_batch(
                    "CREATE TABLE audit_writes (n INTEGER);
                     CREATE TRIGGER audit_body AFTER UPDATE OF body ON calendar_feeds BEGIN INSERT INTO audit_writes VALUES (1); END;",
                )
            })
            .unwrap();
        let calendar = Calendar::default();
        let from = at("2026-10-30T00:00:00Z");
        let to = at("2026-11-06T00:00:00Z");
        let before = calendar.events(&notes, from, to);
        let access = Counting::default();
        refresh(&notes, &access, Utc::now() + Duration::seconds(5));
        let after = calendar.events(&notes, from, to);
        refresh(&notes, &access, Utc::now() + Duration::seconds(10));
        calendar.events(&notes, from, to);
        let writes: i64 = db_for(&notes)
            .with(|c| c.query_row("SELECT COUNT(*) FROM audit_writes", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(access.0.load(Ordering::SeqCst), 2);
        assert_eq!(before.len(), after.len());
        assert_eq!(
            calendar.expanded.lock().unwrap().len(),
            1,
            "the same bytes were parsed again after each refresh"
        );
        assert_eq!(writes, 0, "the same bytes were written again");
    }

    struct Serving(&'static str);

    impl crate::CalendarAccess for Serving {
        fn fetch(&self, _: &str) -> Result<Vec<u8>> {
            Ok(self.0.as_bytes().to_vec())
        }
        fn get(&self, _: &str) -> Result<Option<String>> {
            Ok(Some("https://example.com/a.ics".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_changed_feed_is_expanded_again() {
        let (_dir, notes) = linked(None);
        let calendar = Calendar::default();
        let from = at("2026-10-30T00:00:00Z");
        let to = at("2026-11-06T00:00:00Z");
        let before = calendar.events(&notes, from, to);
        assert!(before.iter().any(|(_, _, e)| e.title == "Holiday"));
        let fewer = MEETINGS.replace(
            "BEGIN:VEVENT\nUID:holiday\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261105\nDTEND;VALUE=DATE:20261106\nEND:VEVENT\n",
            "",
        );
        assert_ne!(fewer, MEETINGS);
        refresh(
            &notes,
            &Serving(fewer.leak()),
            Utc::now() + Duration::seconds(5),
        );
        let after = calendar.events(&notes, from, to);
        assert!(!after.iter().any(|(_, _, e)| e.title == "Holiday"));
        assert_eq!(after.len(), before.len() - 1);
        assert_eq!(calendar.expanded.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_status_window_is_steady_so_the_expansion_is_reused() {
        let (_dir, notes) = linked(None);
        let calendar = Calendar::default();
        let now = at("2026-10-30T16:10:00Z");
        status(&notes, &calendar, false, now);
        status(&notes, &calendar, false, now + Duration::seconds(37));
        status(&notes, &calendar, false, now + Duration::minutes(20));
        assert_eq!(calendar.expanded.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_stale_calendar_is_refreshed_once_before_offering_and_a_fresh_one_is_not() {
        let (_dir, notes) = linked(None);
        let calendar = Calendar::default();
        let access = Counting::default();
        let now = Utc::now();
        catch_up(&notes, &calendar, &access, now);
        assert_eq!(access.0.load(Ordering::SeqCst), 0);
        let later = now + Duration::minutes(STALE_AFTER_MINUTES + 1);
        catch_up(&notes, &calendar, &access, later);
        catch_up(&notes, &calendar, &access, later);
        assert_eq!(access.0.load(Ordering::SeqCst), 1);
        assert!(!calendar.busy.load(Ordering::SeqCst));
    }

    #[test]
    fn an_event_with_a_link_is_online_and_one_without_is_in_person() {
        let (_dir, notes) = linked(None);
        let now = at("2026-10-30T16:00:00Z");
        let seen = |text: &str| {
            let calendar = Calendar::default();
            db_for(&notes)
                .with(|c| {
                    c.execute("DELETE FROM calendar_prompted", [])?;
                    c.execute("UPDATE calendar_feeds SET body = ?1", [text.as_bytes()])
                })
                .unwrap();
            starting(&notes, &calendar, false, now)
                .into_iter()
                .map(|e| e.online)
                .collect::<Vec<_>>()
        };
        let with = |line: &str| {
            MEETINGS.replace(
                "SUMMARY:Design sync",
                &format!("SUMMARY:Design sync\n{line}"),
            )
        };
        assert_eq!(seen(MEETINGS), [false]);
        assert_eq!(seen(&with("LOCATION:Room 4")), [false]);
        assert_eq!(seen(&with("LOCATION:https://meet.example.com/abc")), [true]);
        assert_eq!(
            seen(&with("DESCRIPTION:Join http://zoom.example.com/j/1")),
            [true]
        );
    }

    fn asked(notes: &Path, live: bool, now: DateTime<Utc>) -> Vec<(String, DateTime<Utc>)> {
        let api = Asked::default();
        offer(notes, &Calendar::default(), &api, live, now);
        let seen = api.0.lock().unwrap();
        seen.iter().map(|(_, t, s)| (t.clone(), *s)).collect()
    }

    #[test]
    fn a_calendar_event_is_offered_once_from_two_minutes_before_its_start() {
        let (_dir, notes) = linked(None);
        let start = at("2026-10-30T16:00:00Z");
        assert!(asked(&notes, false, at("2026-10-30T15:57:59Z")).is_empty());
        assert_eq!(
            asked(&notes, false, at("2026-10-30T15:58:00Z")),
            [("Design sync".to_string(), start)]
        );
        assert!(asked(&notes, false, at("2026-10-30T16:05:00Z")).is_empty());
        let api = Asked::default();
        offer(
            &notes,
            &Calendar::default(),
            &api,
            false,
            at("2026-10-31T15:59:00Z"),
        );
        assert_eq!(
            api.0.lock().unwrap()[0].0,
            "sync@1793462400",
            "the next day is its own event"
        );
    }

    #[test]
    fn a_calendar_event_is_not_offered_from_ten_minutes_after_its_start() {
        let (_dir, notes) = linked(None);
        assert!(asked(&notes, false, at("2026-10-30T16:10:00Z")).is_empty());
        assert_eq!(asked(&notes, false, at("2026-10-30T16:09:59Z")).len(), 1);
    }

    #[test]
    fn a_calendar_offer_waits_while_recording_and_skips_all_day_events() {
        let (_dir, notes) = linked(None);
        assert!(asked(&notes, true, at("2026-10-30T16:01:00Z")).is_empty());
        assert_eq!(asked(&notes, false, at("2026-10-30T16:02:00Z")).len(), 1);
        let holiday = ics::read(
            MEETINGS,
            at("2026-11-04T00:00:00Z"),
            at("2026-11-07T00:00:00Z"),
        )
        .events
        .into_iter()
        .find(|e| e.all_day)
        .unwrap();
        for minutes in [-2, 0, 5] {
            assert!(
                asked(&notes, false, holiday.start + Duration::minutes(minutes))
                    .iter()
                    .all(|(title, _)| title != "Holiday")
            );
        }
    }

    #[test]
    fn a_calendar_offer_follows_the_local_time_across_daylight_saving() {
        let (_dir, notes) = linked(None);
        assert_eq!(asked(&notes, false, at("2026-10-30T15:59:00Z")).len(), 1);
        assert!(asked(&notes, false, at("2026-11-02T16:00:00Z")).is_empty());
        assert_eq!(
            asked(&notes, false, at("2026-11-02T16:59:00Z")),
            [("Design sync".to_string(), at("2026-11-02T17:00:00Z"))]
        );
        let kept: Vec<String> = db_for(&notes)
            .with(|c| {
                let mut rows = c.prepare("SELECT start FROM calendar_prompted")?;
                let all = rows.query_map([], |r| r.get(0))?.flatten().collect();
                Ok(all)
            })
            .unwrap();
        assert_eq!(
            kept,
            ["2026-11-02T17:00:00+00:00"],
            "rows older than two days are dropped"
        );
    }

    #[test]
    fn a_calendar_offer_is_off_when_the_switch_is_off() {
        let (_dir, notes) = linked(Some(false));
        assert!(asked(&notes, false, at("2026-10-30T16:00:00Z")).is_empty());
        let (_dir, notes) = linked(Some(true));
        assert_eq!(asked(&notes, false, at("2026-10-30T16:00:00Z")).len(), 1);
    }

    #[test]
    fn links_are_https_or_webcal_and_anything_else_says_what_to_copy() {
        assert_eq!(
            link_of(" webcal://calendar.google.com/calendar/ical/x/private-abc/basic.ics ")
                .unwrap(),
            "https://calendar.google.com/calendar/ical/x/private-abc/basic.ics"
        );
        assert!(link_of("https://calendar.google.com/calendar/ical/x/basic.ics").is_ok());
        for bad in [
            "",
            "calendar.google.com",
            "http://example.com/a.ics",
            "file:///etc/passwd",
            "https:// spaced",
        ] {
            assert!(link_of(bad).is_err(), "{bad}");
        }
    }
}
