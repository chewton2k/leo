use std::collections::{HashMap, VecDeque};
use std::io::Write;
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

const MOST_SETTINGS_BYTES: u64 = 20_000_000;
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
const STRAY_TEMP_HOURS: u64 = 1;
const PROBLEM: &str = "leo could not read this calendar just now. If it keeps happening, remove it and paste its link again.";
const NOT_A_CALENDAR: &str = "That link does not lead to a calendar. In Google Calendar, copy “Secret address in iCal format”.";

type Expansion = (String, Arc<Vec<Event>>);

#[derive(Default)]
pub(crate) struct Calendar {
    busy: Arc<AtomicBool>,
    expanded: Mutex<VecDeque<Expansion>>,
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

fn legacy_body(notes: &Path, id: &str) -> Option<Vec<u8>> {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT body FROM calendar_feeds WHERE feed = ?1 AND body IS NOT NULL",
                [id],
                |r| r.get::<_, Vec<u8>>(0),
            )
        })
        .ok()
}

fn safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn feeds_dir(notes: &Path) -> PathBuf {
    data(notes).join("calendar")
}

fn feed_name(name: &str) -> Option<(&str, u64)> {
    let (id, hash) = name.strip_suffix(".ics")?.rsplit_once('.')?;
    (safe(id) && hash.len() == 16)
        .then(|| u64::from_str_radix(hash, 16).ok())
        .flatten()
        .map(|hash| (id, hash))
}

fn feed_files(notes: &Path) -> HashMap<String, (u64, PathBuf)> {
    let mut newest: HashMap<String, (std::time::SystemTime, u64, PathBuf)> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(feeds_dir(notes)) else {
        return HashMap::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((id, hash)) = name.to_str().and_then(feed_name) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if newest.get(id).is_none_or(|(at, _, _)| modified > *at) {
            newest.insert(id.to_string(), (modified, hash, entry.path()));
        }
    }
    newest
        .into_iter()
        .map(|(id, (_, hash, path))| (id, (hash, path)))
        .collect()
}

struct Sink {
    file: tempfile::NamedTempFile,
    hash: u64,
    length: u64,
    tail: Vec<u8>,
    calendar: bool,
    tripped: bool,
    most: u64,
}

impl Sink {
    fn new(notes: &Path, most: u64) -> std::io::Result<Sink> {
        let dir = feeds_dir(notes);
        std::fs::create_dir_all(&dir)?;
        Ok(Sink {
            file: tempfile::NamedTempFile::new_in(dir)?,
            hash: 0xcbf2_9ce4_8422_2325,
            length: 0,
            tail: Vec::new(),
            calendar: false,
            tripped: false,
            most,
        })
    }

    fn keep(mut self, notes: &Path, id: &str) -> std::io::Result<u64> {
        self.file.flush()?;
        self.file.as_file().sync_all()?;
        let dir = feeds_dir(notes);
        let target = dir.join(format!("{id}.{:016x}.ics", self.hash));
        if !target.exists() {
            self.file.persist(&target).map_err(|e| e.error)?;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let other = entry.path();
                let same = entry
                    .file_name()
                    .to_str()
                    .and_then(feed_name)
                    .is_some_and(|(other_id, _)| other_id == id);
                if same && other != target {
                    let _ = std::fs::remove_file(other);
                }
            }
        }
        Ok(self.hash)
    }
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.length + bytes.len() as u64 > self.most {
            self.tripped = true;
            return Err(std::io::Error::other(crate::TooBig));
        }
        if !self.calendar {
            let mut seen = std::mem::take(&mut self.tail);
            seen.extend_from_slice(bytes);
            self.calendar = seen.windows(15).any(|w| w == b"BEGIN:VCALENDAR");
            self.tail = seen[seen.len().saturating_sub(14)..].to_vec();
        }
        self.hash = bytes.iter().fold(self.hash, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
        self.length += bytes.len() as u64;
        self.file.write_all(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn keep_bytes(notes: &Path, id: &str, bytes: &[u8]) -> std::io::Result<(u64, PathBuf)> {
    let mut sink = Sink::new(notes, u64::MAX)?;
    sink.write_all(bytes)?;
    let hash = sink.keep(notes, id)?;
    Ok((hash, feeds_dir(notes).join(format!("{id}.{hash:016x}.ics"))))
}

fn forget_files(notes: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(feeds_dir(notes)) else {
        return;
    };
    let stray = std::time::Duration::from_secs(STRAY_TEMP_HOURS * 3600);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let gone = match feed_name(name) {
            Some((id, _)) => !keep.iter().any(|k| k == id),
            None => {
                name.starts_with(".tmp")
                    && entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|at| at.elapsed().ok())
                        .is_some_and(|age| age > stray)
            }
        };
        if gone {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

impl Calendar {
    pub(crate) fn events(
        &self,
        notes: &Path,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<(String, String, Event)> {
        let linking: Linking = read(&settings_path(notes));
        let files = feed_files(notes);
        let kept_most = KEPT_EXPANSIONS.max(linking.calendars.len() * 3);
        let mut out = Vec::new();
        for linked in &linking.calendars {
            if !safe(&linked.id) {
                continue;
            }
            let Some((hash, path)) = files.get(&linked.id).cloned().or_else(|| {
                let (hash, path) =
                    keep_bytes(notes, &linked.id, &legacy_body(notes, &linked.id)?).ok()?;
                let _ = db_for(notes).with(|c| {
                    c.execute(
                        "UPDATE calendar_feeds SET body = NULL WHERE feed = ?1",
                        [&linked.id],
                    )
                });
                Some((hash, path))
            }) else {
                continue;
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
                    let Ok(read) = ics::read_from(
                        || {
                            leo_core::paths::open_regular(&path)
                                .map(|f| std::io::BufReader::with_capacity(64 * 1024, f))
                        },
                        from,
                        to,
                    ) else {
                        continue;
                    };
                    let events = Arc::new(read.events);
                    let mut kept = self.expanded.lock().unwrap_or_else(|e| e.into_inner());
                    kept.push_back((key, Arc::clone(&events)));
                    while kept.len() > kept_most {
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
        .take(MOST_SETTINGS_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MOST_SETTINGS_BYTES
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

enum Failure {
    TooBig,
    NotACalendar,
    Other,
}

struct Got {
    streamed: crate::Streamed,
    hash: Option<u64>,
}

fn fetch_one(
    notes: &Path,
    access: &dyn crate::CalendarAccess,
    id: &str,
    link: &str,
    etag: Option<&str>,
    modified: Option<&str>,
    most: u64,
) -> std::result::Result<Got, Failure> {
    let mut sink = Sink::new(notes, most).map_err(|_| Failure::Other)?;
    let streamed = match access.fetch_into(link, etag, modified, &mut sink) {
        Ok(streamed) => streamed,
        Err(e) if sink.tripped || e.downcast_ref::<crate::TooBig>().is_some() => {
            return Err(Failure::TooBig)
        }
        Err(_) => return Err(Failure::Other),
    };
    if !streamed.written {
        return Ok(Got {
            streamed,
            hash: None,
        });
    }
    if !sink.calendar {
        return Err(Failure::NotACalendar);
    }
    let hash = sink.keep(notes, id).map_err(|_| Failure::Other)?;
    Ok(Got {
        streamed,
        hash: Some(hash),
    })
}

fn record(notes: &Path, id: &str, stamp: &str, got: &std::result::Result<Got, Failure>) {
    let _ = db_for(notes).with(|c| match got {
        Ok(Got { hash: None, .. }) => c.execute(
            "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, problem) VALUES (?1, ?2, ?2, NULL) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, problem = NULL",
            rusqlite::params![id, stamp],
        ),
        Ok(Got {
            streamed,
            hash: Some(_),
        }) => c.execute(
            "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, etag, modified, problem, body) VALUES (?1, ?2, ?2, ?3, ?4, NULL, NULL) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, etag = excluded.etag, modified = excluded.modified, problem = NULL, body = NULL",
            rusqlite::params![id, stamp, streamed.etag, streamed.modified],
        ),
        Err(failure) => {
            let problem = match failure {
                Failure::TooBig => crate::TooBig.to_string(),
                Failure::NotACalendar => NOT_A_CALENDAR.to_string(),
                Failure::Other => PROBLEM.to_string(),
            };
            c.execute(
                "INSERT INTO calendar_feeds (feed, tried_at, problem) VALUES (?1, ?2, ?3) ON CONFLICT(feed) DO UPDATE SET tried_at = excluded.tried_at, problem = excluded.problem",
                rusqlite::params![id, stamp, problem],
            )
        }
    });
}

fn refresh(notes: &Path, access: &dyn crate::CalendarAccess, now: DateTime<Utc>) {
    refresh_due(notes, access, now, None)
}

fn refresh_due(
    notes: &Path,
    access: &dyn crate::CalendarAccess,
    now: DateTime<Utc>,
    wait: Option<i64>,
) {
    let linking: Linking = read(&settings_path(notes));
    let stamp = now.to_rfc3339();
    let files = feed_files(notes);
    for linked in &linking.calendars {
        let before = feed_of(notes, &linked.id);
        let due = wait.is_none_or(|minutes| {
            before
                .tried_at
                .is_none_or(|t| now - t > Duration::minutes(minutes))
        });
        if !due || !safe(&linked.id) {
            continue;
        }
        let have = files.contains_key(&linked.id);
        let (etag, modified) = match have {
            true => (before.etag.as_deref(), before.modified.as_deref()),
            false => (None, None),
        };
        let got = access
            .get(&account(&linked.id))
            .ok()
            .flatten()
            .ok_or(Failure::Other)
            .and_then(|link| {
                fetch_one(
                    notes,
                    access,
                    &linked.id,
                    &link,
                    etag,
                    modified,
                    crate::CALENDAR_BYTES,
                )
            });
        record(notes, &linked.id, &stamp, &got);
    }
    let known: Vec<String> = linking.calendars.iter().map(|c| c.id.clone()).collect();
    let _ = db_for(notes).with(|c| {
        let mut statement = c.prepare("SELECT feed FROM calendar_feeds")?;
        let all: Vec<String> = statement.query_map([], |r| r.get(0))?.flatten().collect();
        for gone in all.iter().filter(|f| !known.contains(f)) {
            c.execute("DELETE FROM calendar_feeds WHERE feed = ?1", [gone])?;
        }
        Ok(())
    });
    forget_files(notes, &known);
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
            refresh_due(notes, access, now, Some(STALE_AFTER_MINUTES));
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
                    refresh_due(
                        &notes,
                        access.as_ref(),
                        Utc::now(),
                        Some(STALE_AFTER_MINUTES),
                    );
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
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let got = fetch_one(
            &notes,
            access.as_ref(),
            &id,
            &link,
            None,
            None,
            crate::CALENDAR_BYTES,
        );
        let named = match &got {
            Ok(Got { hash: Some(_), .. }) => {
                let path = feed_files(&notes).remove(&id).map(|(_, path)| path);
                path.and_then(|path| {
                    ics::read_from(
                        || leo_core::paths::open_regular(&path).map(std::io::BufReader::new),
                        now - Duration::days(1),
                        now + Duration::days(KEPT_DAYS),
                    )
                    .ok()
                })
            }
            _ => None,
        };
        let Some(calendar) = named else {
            forget_files(&notes, &linking.calendars.iter().map(|c| c.id.clone()).collect::<Vec<_>>());
            return Err(match got {
                Err(Failure::TooBig) => crate::TooBig.into(),
                Err(Failure::NotACalendar) => anyhow::anyhow!(NOT_A_CALENDAR),
                _ => anyhow::anyhow!("leo could not open that link. Check that it is the calendar's secret address and that this computer is online."),
            });
        };
        let saved = access.set(&account(&id), &link).and_then(|()| {
            linking.calendars.push(Linked {
                id: id.clone(),
                name: if name.is_empty() {
                    calendar.name.unwrap_or_else(|| "Calendar".into())
                } else {
                    name
                },
                added_at: now,
            });
            let _saving = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            linking.offer = read::<Linking>(&settings_path(&notes)).offer;
            leo_core::recording::write_json(&settings_path(&notes), &linking)
        });
        if let Err(e) = saved {
            let _ = access.delete(&account(&id));
            let others: Vec<String> = read::<Linking>(&settings_path(&notes))
                .calendars
                .into_iter()
                .map(|c| c.id)
                .collect();
            forget_files(&notes, &others);
            return Err(e);
        }
        record(&notes, &id, &now.to_rfc3339(), &got);
        refresh_due(&notes, access.as_ref(), now, Some(STALE_AFTER_MINUTES));
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
        refresh_due(
            &notes,
            access.as_ref(),
            Utc::now(),
            Some(STALE_AFTER_MINUTES),
        );
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
                refresh_due(&notes, access.as_ref(), Utc::now(), Some(wait));
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
        fn health(&self, _: &Path, _: bool) -> Result<serde_json::Value> {
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
                    "INSERT INTO calendar_feeds (feed, fetched_at, tried_at) VALUES ('work', ?1, ?1)",
                    rusqlite::params![Utc::now().to_rfc3339()],
                )
            })
            .unwrap();
        keep_bytes(&notes, "work", MEETINGS.as_bytes()).unwrap();
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
        let file = feed_files(&notes).remove("work").unwrap().1;
        let written = std::fs::metadata(&file).unwrap().modified().unwrap();
        let calendar = Calendar::default();
        let from = at("2026-10-30T00:00:00Z");
        let to = at("2026-11-06T00:00:00Z");
        let before = calendar.events(&notes, from, to);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let access = Counting::default();
        refresh(&notes, &access, Utc::now() + Duration::seconds(5));
        let after = calendar.events(&notes, from, to);
        refresh(&notes, &access, Utc::now() + Duration::seconds(10));
        calendar.events(&notes, from, to);
        assert_eq!(access.0.load(Ordering::SeqCst), 2);
        assert_eq!(before.len(), after.len());
        assert_eq!(
            calendar.expanded.lock().unwrap().len(),
            1,
            "the same bytes were parsed again after each refresh"
        );
        assert_eq!(feed_files(&notes).remove("work").unwrap().1, file);
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            written,
            "the same bytes were written again"
        );
        assert_eq!(std::fs::read_dir(feeds_dir(&notes)).unwrap().count(), 1);
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
                .with(|c| c.execute("DELETE FROM calendar_prompted", []))
                .unwrap();
            keep_bytes(&notes, "work", text.as_bytes()).unwrap();
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

    struct Sized(Vec<u8>);

    impl crate::CalendarAccess for Sized {
        fn fetch(&self, _: &str) -> Result<Vec<u8>> {
            Ok(self.0.clone())
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
    fn a_large_feed_is_kept_in_a_file_and_its_link_removal_deletes_it() {
        let (_dir, notes) = linked(None);
        let mut feed = MEETINGS.replace("END:VCALENDAR\n", "").into_bytes();
        feed.extend(std::iter::repeat_n(b'x', 6_000_000));
        feed.extend_from_slice(b"\nEND:VCALENDAR\n");
        refresh(
            &notes,
            &Sized(feed.clone()),
            Utc::now() + Duration::seconds(5),
        );
        assert_eq!(feed_of(&notes, "work").problem, None);
        let (hash, path) = feed_files(&notes).remove("work").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), feed);
        assert_eq!(hash, crate::vectors::fnv64(feed.iter().copied()));
        let stored: Option<Vec<u8>> = db_for(&notes)
            .with(|c| {
                c.query_row(
                    "SELECT body FROM calendar_feeds WHERE feed = 'work'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert!(stored.is_none(), "the feed is not kept in the database");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut linking: Linking = read(&settings_path(&notes));
        linking.calendars.clear();
        leo_core::recording::write_json(&settings_path(&notes), &linking).unwrap();
        refresh(
            &notes,
            &Sized(Vec::new()),
            Utc::now() + Duration::seconds(10),
        );
        assert!(!path.exists());
        assert!(feed_files(&notes).is_empty());
    }

    #[test]
    fn an_endless_feed_stops_at_its_bound_with_a_message() {
        let (_dir, notes) = linked(None);
        let endless = Sized(vec![b'x'; 5_000]);
        let got = fetch_one(
            &notes,
            &endless,
            "work",
            "https://example.com/a.ics",
            None,
            None,
            1_000,
        );
        assert!(matches!(got, Err(Failure::TooBig)));
        record(&notes, "work", &Utc::now().to_rfc3339(), &got);
        assert_eq!(
            feed_of(&notes, "work").problem.as_deref(),
            Some("This calendar is larger than 1 GB, so leo did not read it.")
        );
        let (_, path) = feed_files(&notes).remove("work").unwrap();
        assert_eq!(
            std::fs::read(path).unwrap(),
            MEETINGS.as_bytes(),
            "the kept copy stays"
        );
        assert_eq!(
            std::fs::read_dir(feeds_dir(&notes)).unwrap().count(),
            1,
            "the partial file was removed"
        );
    }

    #[test]
    fn the_running_hash_is_the_one_hash() {
        let bytes: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
        let (_dir, notes) = linked(None);
        let (hash, _) = keep_bytes(&notes, "work", &bytes).unwrap();
        assert_eq!(hash, crate::vectors::fnv64(bytes.iter().copied()));
    }

    struct Hanging {
        slow: &'static str,
        running: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
        total: std::sync::atomic::AtomicUsize,
    }

    impl crate::CalendarAccess for Hanging {
        fn fetch(&self, url: &str) -> Result<Vec<u8>> {
            self.total.fetch_add(1, Ordering::SeqCst);
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            let slow = url.ends_with(self.slow);
            if slow {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            if slow {
                bail!("timed out");
            }
            Ok(MEETINGS.as_bytes().to_vec())
        }
        fn get(&self, account: &str) -> Result<Option<String>> {
            Ok(Some(format!("https://example.com/{account}")))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn eight_calendars_are_followed_and_a_dead_one_holds_up_only_itself() {
        let (_dir, notes) = linked(None);
        let linking = Linking {
            calendars: (0..8)
                .map(|n| Linked {
                    id: format!("c{n}"),
                    name: format!("Calendar {n}"),
                    added_at: Utc::now(),
                })
                .collect(),
            offer: None,
        };
        leo_core::recording::write_json(&settings_path(&notes), &linking).unwrap();
        let access = Hanging {
            slow: "calendar-link-c3",
            running: Default::default(),
            most: Default::default(),
            total: Default::default(),
        };
        let began = std::time::Instant::now();
        refresh(&notes, &access, Utc::now());
        assert!(began.elapsed() < std::time::Duration::from_secs(3));
        let files = feed_files(&notes);
        assert_eq!(files.len(), 7);
        assert!(!files.contains_key("c3"));
        assert!(feed_of(&notes, "c3").problem.is_some());
        assert!(feed_of(&notes, "c4").problem.is_none());
        assert_eq!(
            access.most.load(Ordering::SeqCst),
            1,
            "two fetches ran at once"
        );
        assert_eq!(access.total.load(Ordering::SeqCst), 8);
        refresh_due(
            &notes,
            &access,
            Utc::now() + Duration::minutes(1),
            Some(STALE_AFTER_MINUTES),
        );
        assert_eq!(
            access.total.load(Ordering::SeqCst),
            8,
            "a link tried a minute ago was tried again"
        );
    }

    struct Matching(Mutex<Vec<(Option<String>, Option<String>)>>);

    impl crate::CalendarAccess for Matching {
        fn fetch(&self, _: &str) -> Result<Vec<u8>> {
            Ok(MEETINGS.as_bytes().to_vec())
        }
        fn fetch_since(
            &self,
            _: &str,
            etag: Option<&str>,
            modified: Option<&str>,
        ) -> Result<crate::Fetched> {
            self.0
                .lock()
                .unwrap()
                .push((etag.map(str::to_string), modified.map(str::to_string)));
            Ok(match etag {
                Some(_) => crate::Fetched {
                    body: None,
                    etag: etag.map(str::to_string),
                    modified: None,
                },
                None => crate::Fetched {
                    body: Some(MEETINGS.as_bytes().to_vec()),
                    etag: Some("v1".into()),
                    modified: None,
                },
            })
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
    fn a_feed_stored_by_an_older_leo_is_kept_in_a_file_and_never_answered_not_modified_without_one()
    {
        let (_dir, notes) = linked(None);
        let _ = std::fs::remove_dir_all(feeds_dir(&notes));
        db_for(&notes)
            .with(|c| {
                c.execute(
                    "UPDATE calendar_feeds SET etag = 'v1', body = ?1 WHERE feed = 'work'",
                    [MEETINGS.as_bytes()],
                )
            })
            .unwrap();
        let calendar = Calendar::default();
        let events = calendar.events(
            &notes,
            at("2026-10-30T00:00:00Z"),
            at("2026-11-06T00:00:00Z"),
        );
        assert!(!events.is_empty());
        assert_eq!(feed_files(&notes).len(), 1);
        assert!(legacy_body(&notes, "work").is_none());

        let (_dir, notes) = linked(None);
        let _ = std::fs::remove_dir_all(feeds_dir(&notes));
        db_for(&notes)
            .with(|c| {
                c.execute(
                    "UPDATE calendar_feeds SET etag = 'v1' WHERE feed = 'work'",
                    [],
                )
            })
            .unwrap();
        let access = Matching(Default::default());
        refresh(&notes, &access, Utc::now() + Duration::seconds(5));
        assert_eq!(access.0.lock().unwrap()[0], (None, None));
        assert_eq!(feed_files(&notes).len(), 1);
        refresh(&notes, &access, Utc::now() + Duration::seconds(10));
        assert_eq!(access.0.lock().unwrap()[1].0.as_deref(), Some("v1"));
        assert_eq!(feed_files(&notes).len(), 1);
    }
}
