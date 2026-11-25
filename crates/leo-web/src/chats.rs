use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const LONG_CHAT: usize = 400;
pub const MOST_TITLE: usize = 80;
pub const MOST_ABOUT: usize = 110;
const SHORT_QUESTION_WORDS: usize = 6;
pub const CHAT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ref {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub named: bool,
    #[serde(default)]
    pub about: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Memory>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub upto: usize,
    pub hash: String,
    pub text: String,
}

impl Chat {
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            title: self.title.clone(),
            named: self.named,
            about: self.about.clone(),
            count: self.messages.len(),
            updated_at: self.updated_at,
            pinned: false,
            notes: Vec::new(),
        }
    }

    pub fn wants_name(&self) -> bool {
        !self.named && first_text(&self.messages, "assistant").is_some_and(|t| !t.trim().is_empty())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub named: bool,
    pub about: String,
    pub count: usize,
    pub updated_at: DateTime<Utc>,
    pub pinned: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Saving {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub existed: bool,
    #[serde(default)]
    pub from: Option<usize>,
}

#[derive(Debug)]
pub struct Gone;

impl std::fmt::Display for Gone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("that chat was removed")
    }
}

impl std::error::Error for Gone {}

pub fn dir_for(notes_dir: &Path) -> PathBuf {
    crate::db::beside_of(notes_dir, None).join("chats")
}

pub use crate::jsonline::valid_id;

fn first_of<'a>(messages: &'a [serde_json::Value], role: &str) -> Option<&'a serde_json::Value> {
    messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some(role))
}

fn first_text<'a>(messages: &'a [serde_json::Value], role: &str) -> Option<&'a str> {
    first_of(messages, role)
        .and_then(|m| m.get("text"))
        .and_then(|t| t.as_str())
}

pub(crate) fn clipped(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push('…');
    }
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn note_of(refs: &[Ref], messages: &[serde_json::Value]) -> Option<String> {
    let asked = first_of(messages, "user");
    let attached = asked
        .and_then(|m| m.get("refs"))
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
        .and_then(|r| r.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let named = asked
        .and_then(|m| m.get("docs"))
        .and_then(|d| d.as_array())
        .and_then(|d| d.first())
        .and_then(|d| d.as_str())
        .map(str::to_string);
    let used = first_of(messages, "assistant")
        .and_then(|m| m.get("sources"))
        .and_then(|s| s.as_array())
        .and_then(|s| s.first())
        .filter(|s| {
            s.get("why")
                .and_then(|w| w.as_str())
                .is_none_or(|w| w == "open")
        })
        .and_then(|s| s.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    attached
        .or(named)
        .or_else(|| refs.first().map(|r| r.title.clone()))
        .or(used)
        .filter(|t| !t.trim().is_empty())
}

fn title_of(given: &str, refs: &[Ref], messages: &[serde_json::Value]) -> String {
    if !given.trim().is_empty() {
        return clipped(&one_line(given), MOST_TITLE);
    }
    let asked = clipped(
        &one_line(first_text(messages, "user").unwrap_or("")),
        MOST_TITLE,
    );
    if asked.is_empty() {
        return "New chat".to_string();
    }
    let short = asked.split_whitespace().count() <= SHORT_QUESTION_WORDS;
    match note_of(refs, messages) {
        Some(note) if short && !asked.to_lowercase().contains(&note.to_lowercase()) => {
            let asked = asked.trim_end_matches(['?', '.', '!', ' ']);
            clipped(&one_line(&format!("{asked}: {note}")), MOST_TITLE)
        }
        _ => asked,
    }
}

fn about_of(messages: &[serde_json::Value]) -> String {
    let said = first_text(messages, "assistant").unwrap_or("");
    let said = said.replace("[[correct]]", "").replace("[[incorrect]]", "");
    let mut fenced = false;
    let plain: String = said
        .lines()
        .filter(|l| {
            if l.trim_start().starts_with("```") {
                fenced = !fenced;
                return false;
            }
            !fenced
        })
        .map(|l| l.trim_start_matches(['#', '>', '-', '*', ' ', '|']))
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut cleaned = String::with_capacity(plain.len());
    let mut chars = plain.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' && chars.peek() == Some(&'n') {
            let rest: String = chars.clone().take_while(|c| *c != ']').collect();
            if rest.len() > 1 && rest[1..].chars().all(|c| c.is_ascii_digit()) {
                for _ in 0..=rest.len() {
                    chars.next();
                }
                continue;
            }
        }
        if !matches!(c, '*' | '_' | '`') {
            cleaned.push(c);
        }
    }
    clipped(
        &one_line(&cleaned.replace(" .", ".").replace(" ,", ",")),
        MOST_ABOUT,
    )
}

pub fn rename(dir: &Path, id: &str, title: &str) -> bool {
    let title = clipped(&one_line(title), MOST_TITLE);
    if title.is_empty() || !valid_id(id) {
        return false;
    }
    db_of(dir)
        .with(|c| {
            let changed = c.execute(
                "UPDATE chats SET title = ?1, named = 1 WHERE id = ?2",
                rusqlite::params![title, id],
            )?;
            c.execute(
                "UPDATE chats_text SET title = ?1 WHERE id = ?2",
                rusqlite::params![title, id],
            )?;
            Ok(changed > 0)
        })
        .unwrap_or(false)
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn when(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).map_or_else(|_| Utc::now(), |t| t.with_timezone(&Utc))
}

pub(crate) fn db_of(dir: &Path) -> std::sync::Arc<crate::db::Db> {
    crate::db::for_chats(dir)
}

fn text_of(message: &serde_json::Value) -> &str {
    message.get("text").and_then(|t| t.as_str()).unwrap_or("")
}

fn body_of(messages: &[serde_json::Value]) -> String {
    messages
        .iter()
        .map(text_of)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

#[derive(Debug)]
pub struct Conflict {
    pub count: usize,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "that chat holds {} messages now", self.count)
    }
}

impl std::error::Error for Conflict {}

pub struct Saved {
    pub chat: Chat,
    pub count: usize,
    pub pinned: bool,
    pub notes: Vec<String>,
}

impl Saved {
    pub fn summary(&self) -> Summary {
        Summary {
            count: self.count,
            pinned: self.pinned,
            notes: self.notes.clone(),
            ..self.chat.summary()
        }
    }
}

fn write_facts(
    c: &rusqlite::Connection,
    chat: &Chat,
    count: usize,
    notes: &[String],
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at, rows, notes)
         VALUES (?1, ?2, 'chat', ?3, '[]', ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10)
         ON CONFLICT(id) DO UPDATE SET title = ?2, refs = ?3, named = ?4, about = ?5, memory = ?6,
             count = ?7, created_at = ?8, updated_at = ?9, rows = 1, notes = ?10",
        rusqlite::params![
            chat.id,
            chat.title,
            json(&chat.refs),
            chat.named,
            chat.about,
            chat.memory.as_ref().map(json),
            count as i64,
            stamp(chat.created_at),
            stamp(chat.updated_at),
            json(notes),
        ],
    )?;
    Ok(())
}

fn note_ids(messages: &[serde_json::Value], into: &mut std::collections::BTreeSet<String>) {
    for message in messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    {
        let attached = message
            .get("refs")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
            .filter_map(|r| r.get("id"));
        let selected = message.get("selection").and_then(|s| s.get("note"));
        let ids = attached.chain(message.get("note")).chain(selected);
        into.extend(
            ids.filter_map(|id| id.as_str())
                .filter(|id| !id.is_empty())
                .map(str::to_string),
        );
    }
}

fn notes_kept(
    c: &rusqlite::Connection,
    id: &str,
) -> rusqlite::Result<std::collections::BTreeSet<String>> {
    let mut found = std::collections::BTreeSet::new();
    let mut kept = c.prepare("SELECT message FROM chat_messages WHERE chat = ?1")?;
    let mut rows = kept.query([id])?;
    while let Some(row) = rows.next()? {
        if let Ok(message) = serde_json::from_str(&row.get::<_, String>(0)?) {
            note_ids(&[message], &mut found);
        }
    }
    Ok(found)
}

pub(crate) fn fill_notes(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let mut found: std::collections::HashMap<String, std::collections::BTreeSet<String>> =
        std::collections::HashMap::new();
    let mut asked = c.prepare(
        "SELECT chat, message FROM chat_messages WHERE message LIKE '%\"refs\":[{%' OR message LIKE '%\"selection\"%'",
    )?;
    let mut rows = asked.query([])?;
    while let Some(row) = rows.next()? {
        if let Ok(message) = serde_json::from_str(&row.get::<_, String>(1)?) {
            note_ids(&[message], found.entry(row.get(0)?).or_default());
        }
    }
    let mut older = c.prepare("SELECT id, messages FROM chats WHERE rows = 0")?;
    let mut rows = older.query([])?;
    while let Some(row) = rows.next()? {
        if let Ok(messages) =
            serde_json::from_str::<Vec<serde_json::Value>>(&row.get::<_, String>(1)?)
        {
            note_ids(&messages, found.entry(row.get(0)?).or_default());
        }
    }
    let mut set = c.prepare("UPDATE chats SET notes = ?1 WHERE id = ?2")?;
    for (id, ids) in found.into_iter().filter(|(_, ids)| !ids.is_empty()) {
        set.execute(rusqlite::params![json(&ids), id])?;
    }
    Ok(())
}

fn write_title(
    c: &rusqlite::Connection,
    id: &str,
    title: &str,
    fresh: bool,
) -> rusqlite::Result<()> {
    if !fresh {
        c.execute("DELETE FROM chats_text WHERE id = ?1", [id])?;
    }
    c.execute(
        "INSERT INTO chats_text (id, title, body) VALUES (?1, ?2, '')",
        rusqlite::params![id, title],
    )?;
    Ok(())
}

fn rows_from(c: &rusqlite::Connection, id: &str, start: usize) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM chat_words WHERE rowid IN (SELECT id FROM chat_messages WHERE chat = ?1 AND n >= ?2)",
        rusqlite::params![id, start as i64],
    )?;
    c.execute(
        "DELETE FROM chat_messages WHERE chat = ?1 AND n >= ?2",
        rusqlite::params![id, start as i64],
    )?;
    Ok(())
}

fn add_rows(
    c: &rusqlite::Connection,
    id: &str,
    start: usize,
    messages: &[serde_json::Value],
) -> rusqlite::Result<()> {
    let mut row =
        c.prepare_cached("INSERT INTO chat_messages (chat, n, message) VALUES (?1, ?2, ?3)")?;
    let mut words = c.prepare_cached("INSERT INTO chat_words (rowid, body) VALUES (?1, ?2)")?;
    for (i, message) in messages.iter().enumerate() {
        row.execute(rusqlite::params![id, (start + i) as i64, json(message)])?;
        let text = text_of(message);
        if !text.is_empty() {
            words.execute(rusqlite::params![c.last_insert_rowid(), text])?;
        }
    }
    Ok(())
}

fn row_messages(
    c: &rusqlite::Connection,
    id: &str,
    from: usize,
    to: usize,
) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut found = c.prepare_cached(
        "SELECT message FROM chat_messages WHERE chat = ?1 AND n >= ?2 AND n < ?3 ORDER BY n",
    )?;
    let rows = found.query_map(rusqlite::params![id, from as i64, to as i64], |r| {
        r.get::<_, String>(0)
    })?;
    rows.map(|m| m.map(|m| serde_json::from_str(&m).unwrap_or(serde_json::Value::Null)))
        .collect()
}

fn head_of(
    c: &rusqlite::Connection,
    id: &str,
    before: usize,
) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut found = c.prepare_cached(
        "SELECT message FROM chat_messages WHERE chat = ?1 AND n < ?2 ORDER BY n",
    )?;
    let mut rows = found.query(rusqlite::params![id, before as i64])?;
    let mut head = Vec::new();
    while let Some(row) = rows.next()? {
        let message: serde_json::Value =
            serde_json::from_str(&row.get::<_, String>(0)?).unwrap_or(serde_json::Value::Null);
        head.push(message);
        if first_of(&head, "user").is_some() && first_of(&head, "assistant").is_some() {
            break;
        }
    }
    Ok(head)
}

fn first_change(
    c: &rusqlite::Connection,
    id: &str,
    messages: &[serde_json::Value],
) -> rusqlite::Result<usize> {
    let mut found =
        c.prepare_cached("SELECT message FROM chat_messages WHERE chat = ?1 ORDER BY n")?;
    let mut rows = found.query([id])?;
    let mut n = 0;
    while let Some(row) = rows.next()? {
        match messages.get(n) {
            Some(sent) if json(sent) == row.get::<_, String>(0)? => n += 1,
            _ => break,
        }
    }
    Ok(n)
}

fn chat_of(row: &rusqlite::Row) -> rusqlite::Result<(Chat, bool)> {
    let text = |i: usize| -> rusqlite::Result<String> { row.get(i) };
    Ok((
        Chat {
            id: text(0)?,
            title: text(1)?,
            refs: serde_json::from_str(&text(2)?).unwrap_or_default(),
            messages: serde_json::from_str(&text(3)?).unwrap_or_default(),
            named: row.get(4)?,
            about: text(5)?,
            memory: row
                .get::<_, Option<String>>(6)?
                .and_then(|m| serde_json::from_str(&m).ok()),
            created_at: when(&text(7)?),
            updated_at: when(&text(8)?),
        },
        row.get(9)?,
    ))
}

const CHAT_COLUMNS: &str =
    "id, title, refs, CASE WHEN rows = 1 THEN '[]' ELSE messages END, named, about, memory, created_at, updated_at, rows, count";

fn load_page_in(
    c: &rusqlite::Connection,
    id: &str,
    before: Option<usize>,
    most: usize,
) -> rusqlite::Result<Option<Page>> {
    use rusqlite::OptionalExtension;
    let Some(((mut chat, rows), count)) = c
        .query_row(
            &format!("SELECT {CHAT_COLUMNS} FROM chats WHERE id = ?1"),
            [id],
            |row| Ok((chat_of(row)?, row.get::<_, i64>(10)? as usize)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let count = if rows { count } else { chat.messages.len() };
    let to = before.unwrap_or(count).min(count);
    let from = to.saturating_sub(most);
    if rows {
        chat.messages = row_messages(c, id, from, to)?;
    } else {
        chat.messages.truncate(to);
        chat.messages.drain(..from);
    }
    Ok(Some(Page { chat, count, from }))
}

fn load_in(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Chat>> {
    Ok(load_page_in(c, id, None, usize::MAX)?.map(|page| page.chat))
}

#[derive(Debug, Serialize)]
pub struct Page {
    #[serde(flatten)]
    pub chat: Chat,
    pub count: usize,
    pub from: usize,
}

pub const PAGE: usize = 200;

pub fn page(dir: &Path, id: &str, before: Option<usize>, most: usize) -> Option<Page> {
    if !valid_id(id) {
        return None;
    }
    db_of(dir)
        .with(|c| load_page_in(c, id, before, most.max(1)))
        .ok()
        .flatten()
}

struct Facts {
    title: String,
    named: bool,
    memory: Option<Memory>,
    count: usize,
    rows: bool,
    last: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    pinned: bool,
    notes: Vec<String>,
}

fn facts_in(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Facts>> {
    use rusqlite::OptionalExtension;
    c.query_row(
        "SELECT title, named, memory, count, created_at, updated_at, rows,
                CASE WHEN rows = 1
                     THEN (SELECT message FROM chat_messages m WHERE m.chat = chats.id ORDER BY m.n DESC LIMIT 1)
                     WHEN json_valid(messages) THEN json_extract(messages, '$[#-1]') END,
                pinned, notes
         FROM chats WHERE id = ?1",
        [id],
        |row| {
            Ok(Facts {
                title: row.get(0)?,
                named: row.get(1)?,
                memory: row
                    .get::<_, Option<String>>(2)?
                    .and_then(|m| serde_json::from_str(&m).ok()),
                count: row.get::<_, i64>(3)? as usize,
                created_at: when(&row.get::<_, String>(4)?),
                updated_at: when(&row.get::<_, String>(5)?),
                rows: row.get(6)?,
                last: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|m| serde_json::from_str(&m).ok()),
                pinned: row.get(8)?,
                notes: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
            })
        },
    )
    .optional()
}

fn column_in(
    c: &rusqlite::Connection,
    id: &str,
) -> rusqlite::Result<Option<Vec<serde_json::Value>>> {
    let text: String = c.query_row("SELECT messages FROM chats WHERE id = ?1", [id], |r| {
        r.get(0)
    })?;
    Ok(serde_json::from_str(&text).ok())
}

pub fn load(dir: &Path, id: &str) -> Option<Chat> {
    if !valid_id(id) {
        return None;
    }
    db_of(dir).with(|c| load_in(c, id)).ok().flatten()
}

fn summary_of(row: &rusqlite::Row) -> rusqlite::Result<Summary> {
    Ok(Summary {
        id: row.get(0)?,
        title: row.get(1)?,
        named: row.get(2)?,
        about: row.get(3)?,
        count: row.get::<_, i64>(4)? as usize,
        updated_at: when(&row.get::<_, String>(5)?),
        pinned: row.get(6)?,
        notes: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
    })
}

pub(crate) fn with_misses(dir: &Path) -> Vec<Chat> {
    db_of(dir)
        .with(|c| {
            let ids: Vec<String> = {
                let mut found = c.prepare(
                    "SELECT id FROM chats WHERE
                     (rows = 0 AND (messages LIKE '%[[incorrect]]%' OR messages LIKE '%\"quizzes\"%'))
                     OR (rows = 1 AND EXISTS (SELECT 1 FROM chat_messages m WHERE m.chat = chats.id
                         AND (m.message LIKE '%[[incorrect]]%' OR m.message LIKE '%\"quizzes\"%')))",
                )?;
                let rows = found.query_map([], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut chats = Vec::with_capacity(ids.len());
            for id in ids {
                chats.extend(load_in(c, &id)?);
            }
            Ok(chats)
        })
        .unwrap_or_default()
}

pub fn list(dir: &Path) -> Vec<Summary> {
    db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT id, title, named, about, count, updated_at, pinned, notes FROM chats ORDER BY updated_at DESC, id",
            )?;
            let rows = found.query_map([], summary_of)?;
            rows.collect()
        })
        .unwrap_or_default()
}

const MOST_FOUND: usize = 100;

pub fn search(dir: &Path, query: &str) -> Vec<Summary> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(12)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if words.is_empty() {
        return list(dir);
    }
    db_of(dir)
        .with(|c| {
            let ids = |sql: &str, word: &str| -> rusqlite::Result<Vec<String>> {
                let mut found = c.prepare_cached(sql)?;
                let rows = found.query_map([word], |r| r.get(0))?;
                rows.collect()
            };
            let mut found: Option<std::collections::HashMap<String, (bool, usize)>> = None;
            for word in &words {
                let mut these: std::collections::HashMap<String, (bool, usize)> =
                    std::collections::HashMap::new();
                for id in ids("SELECT id FROM chats_text WHERE chats_text MATCH ?1", word)? {
                    these.entry(id).or_default().1 += 1;
                }
                for id in ids(
                    "SELECT m.chat FROM chat_words w JOIN chat_messages m ON m.id = w.rowid WHERE chat_words MATCH ?1",
                    word,
                )? {
                    these.entry(id).or_default().1 += 1;
                }
                for id in ids(
                    "SELECT id FROM chats_text WHERE chats_text MATCH 'title : ' || ?1",
                    word,
                )? {
                    these.entry(id).or_default().0 = true;
                }
                found = Some(match found {
                    None => these,
                    Some(before) => before
                        .into_iter()
                        .filter_map(|(id, (title, n))| {
                            these.get(&id).map(|&(t, m)| (id, (title || t, n + m)))
                        })
                        .collect(),
                });
            }
            let mut summaries = Vec::new();
            let mut one = c.prepare_cached(
                "SELECT id, title, named, about, count, updated_at, pinned, notes FROM chats WHERE id = ?1",
            )?;
            for (id, (title, n)) in found.unwrap_or_default() {
                use rusqlite::OptionalExtension;
                if let Some(summary) = one.query_row([&id], summary_of).optional()? {
                    summaries.push((title, n, summary));
                }
            }
            summaries.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then(b.1.cmp(&a.1))
                    .then(b.2.updated_at.cmp(&a.2.updated_at))
                    .then(a.2.id.cmp(&b.2.id))
            });
            summaries.truncate(MOST_FOUND);
            Ok(summaries.into_iter().map(|(_, _, s)| s).collect())
        })
        .unwrap_or_default()
}

enum Refused {
    Gone,
    Conflict(usize),
}

pub fn save(dir: &Path, id: &str, saving: Saving, now: DateTime<Utc>) -> Result<Chat> {
    save_from(dir, id, saving, now).map(|saved| saved.chat)
}

pub fn save_from(dir: &Path, id: &str, saving: Saving, now: DateTime<Utc>) -> Result<Saved> {
    anyhow::ensure!(valid_id(id), "that is not a chat id");
    let db = db_of(dir);
    let saved = db.with(|c| {
        let tx = c.transaction()?;
        let old = facts_in(&tx, id)?;
        if saving.existed && old.is_none() {
            return Ok(Err(Refused::Gone));
        }
        let column = match &old {
            Some(old) if !old.rows => Some(column_in(&tx, id)?),
            _ => None,
        };
        let held = match (&old, &column) {
            (None, _) => 0,
            (Some(old), None) => old.count,
            (Some(_), Some(list)) => list.as_ref().map_or(0, Vec::len),
        };
        let sent = saving.messages;
        let engaged;
        let mut whole = None;
        let (start, tail) = match saving.from {
            Some(from) => {
                if from > held {
                    return Ok(Err(Refused::Conflict(held)));
                }
                engaged = old.is_none() || from + sent.len() > held;
                match &column {
                    Some(list) => {
                        let mut all = list.clone().unwrap_or_default();
                        all.truncate(from);
                        all.extend(sent);
                        (0, all)
                    }
                    None => (from, sent),
                }
            }
            None => {
                if sent.len() < held {
                    return Ok(Err(Refused::Conflict(held)));
                }
                engaged = old.as_ref().is_none_or(|old| {
                    sent.len() > held
                        || (sent.len() >= LONG_CHAT && sent.last() != old.last.as_ref())
                });
                let list = sent;
                let start = match &old {
                    Some(old) if old.rows => first_change(&tx, id, &list)?,
                    _ => 0,
                };
                let tail = list[start..].to_vec();
                whole = Some(list);
                (start, tail)
            }
        };
        let total = start + tail.len();
        let shown = match &whole {
            Some(list) => list.clone(),
            None if start == 0 => tail.clone(),
            None => {
                let mut shown = head_of(&tx, id, start)?;
                shown.extend(tail.iter().cloned());
                shown
            }
        };
        let created_at = old.as_ref().map_or(now, |old| old.created_at);
        let memory = old.as_ref().and_then(|old| old.memory.clone());
        let updated_at = match &old {
            Some(old) if !engaged => old.updated_at,
            _ => now,
        };
        let old_title = old.as_ref().map(|old| old.title.clone());
        let pinned = old.as_ref().is_some_and(|old| old.pinned);
        let mut notes: std::collections::BTreeSet<String> = old
            .as_ref()
            .map(|old| old.notes.iter().cloned().collect())
            .unwrap_or_default();
        note_ids(&tail, &mut notes);
        let notes: Vec<String> = notes.into_iter().collect();
        let kept_name = old
            .filter(|old| old.named && saving.title.trim().is_empty())
            .map(|old| old.title);
        let refs: Vec<Ref> = saving
            .refs
            .into_iter()
            .take(crate::chat::MOST_ATTACHED)
            .collect();
        let chat = Chat {
            id: id.to_string(),
            named: kept_name.is_some(),
            title: kept_name.unwrap_or_else(|| title_of(&saving.title, &refs, &shown)),
            about: about_of(&shown),
            memory,
            refs,
            messages: shown,
            created_at,
            updated_at,
        };
        rows_from(&tx, id, start)?;
        add_rows(&tx, id, start, &tail)?;
        write_facts(&tx, &chat, total, &notes)?;
        if matches!(column, Some(Some(_))) {
            tx.execute("UPDATE chats SET messages = '[]' WHERE id = ?1", [id])?;
        }
        if column.is_some() || old_title.as_deref() != Some(chat.title.as_str()) {
            write_title(&tx, id, &chat.title, old_title.is_none())?;
        }
        tx.commit()?;
        Ok(Ok(Saved {
            chat,
            count: total,
            pinned,
            notes,
        }))
    })?;
    match saved {
        Ok(saved) => Ok(saved),
        Err(Refused::Gone) => Err(anyhow::Error::new(Gone)),
        Err(Refused::Conflict(count)) => Err(anyhow::Error::new(Conflict { count })),
    }
}

pub fn drop_last(dir: &Path, id: &str, count: usize, total: usize) -> Result<Option<Chat>> {
    anyhow::ensure!(valid_id(id), "that is not a chat id");
    db_of(dir).with(|c| {
        let tx = c.transaction()?;
        let Some(old) = facts_in(&tx, id)? else {
            return Ok(None);
        };
        let column = if old.rows {
            None
        } else {
            Some(column_in(&tx, id)?.unwrap_or_default())
        };
        let held = column.as_ref().map_or(old.count, Vec::len);
        if count == 0 || count > total || held != total {
            return Ok(None);
        }
        let left = total - count;
        match column {
            None => {
                rows_from(&tx, id, left)?;
                let about = about_of(&head_of(&tx, id, left)?);
                let notes = notes_kept(&tx, id)?;
                tx.execute(
                    "UPDATE chats SET count = ?1, about = ?2, notes = ?3 WHERE id = ?4",
                    rusqlite::params![left as i64, about, json(&notes), id],
                )?;
            }
            Some(mut messages) => {
                messages.truncate(left);
                let mut notes = std::collections::BTreeSet::new();
                note_ids(&messages, &mut notes);
                tx.execute(
                    "UPDATE chats SET messages = ?1, count = ?2, about = ?3, notes = ?4 WHERE id = ?5",
                    rusqlite::params![
                        json(&messages),
                        left as i64,
                        about_of(&messages),
                        json(&notes),
                        id
                    ],
                )?;
                tx.execute("DELETE FROM chats_text WHERE id = ?1", [id])?;
                tx.execute(
                    "INSERT INTO chats_text (id, title, body) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, old.title, body_of(&messages)],
                )?;
            }
        }
        tx.commit()?;
        load_in(c, id)
    })
}

pub fn set_memory(dir: &Path, id: &str, memory: Memory) -> bool {
    let json = serde_json::to_string(&memory).unwrap_or_default();
    db_of(dir)
        .with(|c| {
            let tx = c.transaction()?;
            let Some(chat) = facts_in(&tx, id)? else {
                return Ok(false);
            };
            if chat
                .memory
                .as_ref()
                .is_some_and(|old| old.upto > memory.upto)
            {
                return Ok(false);
            }
            tx.execute(
                "UPDATE chats SET memory = ?1 WHERE id = ?2",
                rusqlite::params![json, id],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .unwrap_or(false)
}

pub fn tidy(dir: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let cutoff = stamp(now - chrono::Duration::days(i64::from(days)));
    db_of(dir)
        .with(|c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let old: Vec<(String, String)> = {
                let mut found = tx.prepare(
                    "SELECT id, updated_at FROM chats WHERE updated_at < ?1 AND pinned = 0",
                )?;
                let rows = found.query_map([&cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut gone = 0;
            for (id, updated) in old {
                if tx.execute(
                    "DELETE FROM chats WHERE id = ?1 AND updated_at = ?2 AND updated_at < ?3",
                    rusqlite::params![id, updated, cutoff],
                )? != 0
                {
                    remove_related(&tx, &id)?;
                    gone += 1;
                }
            }
            tx.commit()?;
            Ok(gone)
        })
        .unwrap_or(0)
}

pub fn set_pinned(dir: &Path, id: &str, pinned: bool) -> bool {
    valid_id(id)
        && db_of(dir)
            .with(|c| {
                c.execute(
                    "UPDATE chats SET pinned = ?1 WHERE id = ?2",
                    rusqlite::params![pinned, id],
                )
            })
            .is_ok_and(|changed| changed > 0)
}

pub fn remove(dir: &Path, id: &str) -> bool {
    db_of(dir)
        .with(|c| {
            let tx = c.transaction()?;
            let gone = tx.execute("DELETE FROM chats WHERE id = ?1", [id])?;
            remove_related(&tx, id)?;
            tx.commit()?;
            Ok(gone > 0)
        })
        .unwrap_or(false)
}

fn remove_related(tx: &rusqlite::Transaction<'_>, id: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM chats_text WHERE id = ?1", [id])?;
    rows_from(tx, id, 0)?;
    tx.execute("DELETE FROM chat_docs WHERE chat = ?1", [id])?;
    tx.execute(
        "DELETE FROM reviewed WHERE substr(key, 1, length(?1) + 1) = ?1 || ':'",
        [id],
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn as_column(dir: &Path, id: &str) {
    let chat = load(dir, id).unwrap();
    db_of(dir)
        .with(|c| {
            let tx = c.transaction()?;
            rows_from(&tx, id, 0)?;
            tx.execute(
                "UPDATE chats SET rows = 0, messages = ?1, count = ?2 WHERE id = ?3",
                rusqlite::params![json(&chat.messages), chat.messages.len() as i64, id],
            )?;
            tx.execute("DELETE FROM chats_text WHERE id = ?1", [id])?;
            tx.execute(
                "INSERT INTO chats_text (id, title, body) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, chat.title, body_of(&chat.messages)],
            )?;
            tx.commit()
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chats_saved_under_a_retired_style_still_open_without_rewriting_the_saved_data() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let now = Utc::now();
        let messages = vec![serde_json::json!({
            "role": "assistant", "text": "Saved practice", "interview": {"id": "old-problem"}
        })];
        for mode in ["chat", "study", "interview"] {
            let id = format!("retired-style-{mode}");
            save(
                &dir,
                &id,
                Saving {
                    existed: false,
                    from: None,
                    title: "Saved practice".into(),
                    refs: vec![],
                    messages: messages.clone(),
                },
                now,
            )
            .unwrap();
            db_of(&dir)
                .with(|c| {
                    c.execute(
                        "UPDATE chats SET mode = ?1 WHERE id = ?2",
                        rusqlite::params![mode, id],
                    )?;
                    Ok(())
                })
                .unwrap();
            let loaded = load(&dir, &id).unwrap();
            assert_eq!(loaded.messages, messages);
            save(
                &dir,
                &id,
                Saving {
                    existed: false,
                    from: None,
                    title: "Saved practice".into(),
                    refs: vec![],
                    messages: messages.clone(),
                },
                now,
            )
            .unwrap();
            assert!(list(&dir).iter().any(|s| s.id == id));
            assert!(search(&dir, "practice").iter().any(|s| s.id == id));
            assert_eq!(
                db_of(&dir)
                    .with(|c| c
                        .query_row("SELECT mode FROM chats WHERE id = ?1", [&id], |r| r
                            .get::<_, String>(0)))
                    .unwrap(),
                mode
            );
        }
    }

    #[test]
    fn a_chat_moves_up_only_when_a_message_is_added() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let early = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 7, 9, 0, 0).unwrap();
        let later = early + chrono::Duration::hours(3);
        let one = vec![serde_json::json!({ "role": "user", "text": "hi" })];
        save(
            &dir,
            "chat-order-0001",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: vec![],
                messages: one.clone(),
            },
            early,
        )
        .unwrap();
        let switched = save(
            &dir,
            "chat-order-0001",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: vec![],
                messages: one.clone(),
            },
            later,
        )
        .unwrap();
        assert_eq!(
            switched.updated_at, early,
            "saving an unchanged chat is not activity"
        );
        let mut two = one;
        two.push(serde_json::json!({ "role": "assistant", "text": "hello" }));
        let answered = save(
            &dir,
            "chat-order-0001",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: vec![],
                messages: two,
            },
            later,
        )
        .unwrap();
        assert_eq!(answered.updated_at, later);
    }

    fn said(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "text": text })
    }

    fn at(minute: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 7, 12, minute, 0).unwrap()
    }

    fn saving(messages: Vec<serde_json::Value>) -> Saving {
        Saving {
            existed: false,
            from: None,
            title: String::new(),
            refs: vec![],
            messages,
        }
    }

    #[test]
    fn a_short_starter_question_is_told_apart_by_the_note_it_was_about() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let asked = serde_json::json!({ "role": "user", "text": "Explain this simply", "refs": [{ "id": "n1", "title": "Graph traversals" }] });
        let chat = save(
            &dir,
            "chat-cccc-1",
            saving(vec![asked, said("assistant", "Sure")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "Explain this simply: Graph traversals");

        let answered = serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": "Heaps" }] });
        let chat = save(
            &dir,
            "chat-cccc-2",
            saving(vec![said("user", "What are the key ideas here?"), answered]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "What are the key ideas here: Heaps");

        let chat = save(
            &dir,
            "chat-cccc-3",
            saving(vec![said("user", "quiz me on heaps"), answered_by("Heaps")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "quiz me on heaps");
        let long = "why does breadth first search need a queue and not a stack here";
        let chat = save(
            &dir,
            "chat-cccc-4",
            saving(vec![said("user", long), answered_by("Heaps")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, long);
    }

    #[test]
    fn only_the_open_note_names_a_chat_after_the_reply() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let found = serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": "Heaps", "why": "matches the question" }] });
        let chat = save(
            &dir,
            "chat-eeee-1",
            saving(vec![said("user", "Say only: pong"), found]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "Say only: pong");
        let open = serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": "Heaps", "why": "open" }] });
        let chat = save(
            &dir,
            "chat-eeee-2",
            saving(vec![said("user", "Say only: pong"), open]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "Say only: pong: Heaps");
    }

    fn answered_by(title: &str) -> serde_json::Value {
        serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": title }] })
    }

    #[test]
    fn the_description_is_the_start_of_felixs_first_answer_in_plain_words() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let reply = "[[correct]] ## Breadth-first search\n\n- **BFS** visits a graph level by level [n1], using a `queue` [n12].\n\n```\ncode\n```";
        let chat = save(
            &dir,
            "chat-dddd-1",
            saving(vec![said("user", "q"), said("assistant", reply)]),
            at(1),
        )
        .unwrap();
        assert_eq!(
            chat.about,
            "Breadth-first search BFS visits a graph level by level, using a queue."
        );
        assert_eq!(list(&dir)[0].about, chat.about);
        let long = "word ".repeat(80);
        let chat = save(
            &dir,
            "chat-dddd-2",
            saving(vec![said("user", "q"), said("assistant", &long)]),
            at(1),
        )
        .unwrap();
        assert!(chat.about.ends_with('…') && chat.about.chars().count() <= MOST_ABOUT + 1);
    }

    #[test]
    fn a_name_given_by_the_ai_survives_later_saves_and_is_asked_for_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let first = save(
            &dir,
            "chat-eeee-1",
            saving(vec![said("user", "help")]),
            at(1),
        )
        .unwrap();
        assert!(!first.wants_name());
        let answered = save(
            &dir,
            "chat-eeee-1",
            saving(vec![
                said("user", "help"),
                said("assistant", "BFS uses a queue."),
            ]),
            at(2),
        )
        .unwrap();
        assert!(answered.wants_name());

        assert!(rename(
            &dir,
            "chat-eeee-1",
            "Queues in breadth-first search"
        ));
        let later = save(
            &dir,
            "chat-eeee-1",
            saving(vec![
                said("user", "help"),
                said("assistant", "BFS uses a queue."),
                said("user", "more"),
            ]),
            at(3),
        )
        .unwrap();
        assert_eq!(later.title, "Queues in breadth-first search");
        assert!(!later.wants_name());
        assert!(!rename(&dir, "chat-nope-1", "x"));
        assert!(!rename(&dir, "chat-eeee-1", "   "));
    }

    #[test]
    fn a_chat_is_kept_on_disk_named_after_its_first_question_and_listed_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let first = save(
            &dir,
            "chat-aaaa-1",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: vec![Ref {
                    id: "n1".into(),
                    title: "Heaps".into(),
                }],
                messages: vec![
                    said("user", "  quiz me\non   heaps "),
                    said("assistant", "Sure"),
                ],
            },
            at(1),
        )
        .unwrap();
        assert_eq!(first.title, "quiz me on heaps");
        save(
            &dir,
            "chat-bbbb-2",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: vec![],
                messages: vec![],
            },
            at(2),
        )
        .unwrap();
        let again = save(
            &dir,
            "chat-aaaa-1",
            Saving {
                existed: false,
                from: None,
                title: String::new(),
                refs: first.refs.clone(),
                messages: vec![
                    said("user", "quiz me on heaps"),
                    said("assistant", "Sure"),
                    said("user", "the root"),
                ],
            },
            at(3),
        )
        .unwrap();
        assert_eq!(again.created_at, at(1));
        assert_eq!(again.updated_at, at(3));

        let listed = list(&dir);
        assert_eq!(
            listed.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["chat-aaaa-1", "chat-bbbb-2"]
        );
        assert_eq!(listed[0].count, 3);
        assert_eq!(listed[1].title, "New chat");
        assert_eq!(load(&dir, "chat-aaaa-1").unwrap().refs, first.refs);

        assert!(remove(&dir, "chat-bbbb-2"));
        assert!(!remove(&dir, "chat-bbbb-2"));
        assert_eq!(list(&dir).len(), 1);
    }

    #[test]
    fn old_chats_leave_when_a_limit_is_set_and_stay_forever_otherwise() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let empty = || Saving {
            existed: false,
            from: None,
            title: String::new(),
            refs: vec![],
            messages: vec![said("user", "hi")],
        };
        save(&dir, "chat-old-0001", empty(), at(0)).unwrap();
        let later = at(0) + chrono::Duration::days(40);
        save(&dir, "chat-new-0002", empty(), later).unwrap();
        assert_eq!(tidy(&dir, None, later), 0);
        assert_eq!(list(&dir).len(), 2);
        assert_eq!(tidy(&dir, Some(30), later), 1);
        let left: Vec<String> = list(&dir).into_iter().map(|c| c.id).collect();
        assert_eq!(left, ["chat-new-0002"]);
    }

    #[test]
    fn ids_cannot_reach_outside_the_folder_and_long_chats_keep_every_message() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        for bad in [
            "../../notes/x",
            "short",
            "a/b/c/d/e/f",
            "chat id with spaces",
            "",
        ] {
            assert!(!valid_id(bad), "{bad}");
            assert!(save(
                &dir,
                bad,
                Saving {
                    existed: false,
                    from: None,
                    title: String::new(),
                    refs: vec![],
                    messages: vec![]
                },
                at(0)
            )
            .is_err());
            assert!(load(&dir, bad).is_none());
            assert!(!remove(&dir, bad));
        }
        let messages: Vec<_> = (0..LONG_CHAT + 10)
            .map(|i| said("user", &format!("m{i}")))
            .collect();
        let long = "word ".repeat(100);
        let chat = save(
            &dir,
            "chat-long-123",
            Saving {
                existed: false,
                from: None,
                title: long,
                refs: vec![],
                messages,
            },
            at(0),
        )
        .unwrap();
        assert_eq!(chat.messages.len(), LONG_CHAT + 10);
        assert_eq!(chat.messages[0]["text"], "m0");
        assert_eq!(chat.title.chars().count(), MOST_TITLE + 1);
        assert!(chat.title.ends_with('…'));
        assert!(save(&dir, "../outside", saving(vec![]), at(0)).is_err());
        assert_eq!(list(&dir).len(), 1);
    }

    #[test]
    fn dropping_the_last_pair_keeps_the_rest_and_refuses_a_stale_count() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-drop-1234";
        let messages: Vec<_> = ["q1", "a1", "q2", "a2"]
            .iter()
            .enumerate()
            .map(|(i, t)| said(if i % 2 == 0 { "user" } else { "assistant" }, t))
            .collect();
        save(&dir, id, saving(messages), at(0)).unwrap();
        assert!(drop_last(&dir, id, 2, 3).unwrap().is_none());
        assert!(drop_last(&dir, id, 0, 4).unwrap().is_none());
        assert!(drop_last(&dir, id, 5, 4).unwrap().is_none());
        assert!(drop_last(&dir, "chat-nothing-12", 2, 4).unwrap().is_none());
        assert!(drop_last(&dir, "../x", 2, 4).is_err());
        assert_eq!(load(&dir, id).unwrap().messages.len(), 4);
        let kept = drop_last(&dir, id, 2, 4).unwrap().unwrap();
        assert_eq!(kept.messages.len(), 2);
        assert_eq!(load(&dir, id).unwrap().messages, kept.messages);
        assert_eq!(kept.messages[1]["text"], "a1");
        assert!(drop_last(&dir, id, 2, 4).unwrap().is_none());
    }

    #[test]
    fn deleting_a_chat_removes_its_review_marks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let (gone, longer) = ("chat-aaaa1111", "chat-aaaa1111-2");
        for id in [gone, longer] {
            save(&dir, id, saving(vec![said("user", "quiz me")]), at(0)).unwrap();
        }
        let keys: Vec<String> = [gone, longer]
            .iter()
            .flat_map(|id| (0..3).map(move |i| format!("{id}:{i:016x}")))
            .collect();
        db_of(&dir)
            .with(|c| crate::review::insert_all(c, &keys))
            .unwrap();
        assert_eq!(crate::review::count(&dir), 6);
        assert!(remove(&dir, gone));
        let left: Vec<String> = db_of(&dir)
            .with(|c| {
                let mut q = c.prepare("SELECT key FROM reviewed ORDER BY key")?;
                let rows = q.query_map([], |r| r.get(0))?;
                rows.collect()
            })
            .unwrap();
        assert_eq!(left, keys[3..].to_vec());
        assert!(stored(&dir, gone).is_empty());
        assert_eq!(stored(&dir, longer).len(), 1);
        assert!(search(&dir, "quiz").iter().all(|s| s.id != gone));
        let words: i64 = db_of(&dir)
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM chat_words WHERE chat_words MATCH 'quiz'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(words, 1);
        let rows: i64 = db_of(&dir)
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM chat_messages WHERE chat = ?1",
                    [gone],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn tidy_removes_only_chats_past_the_cutoff() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let now = at(0) + chrono::Duration::days(30);
        let cutoff = at(0);
        for (id, when) in [
            (
                "chat-tidy-before",
                cutoff - chrono::Duration::microseconds(1),
            ),
            ("chat-tidy-exactly", cutoff),
            ("chat-tidy-after1", cutoff + chrono::Duration::seconds(1)),
        ] {
            save(&dir, id, saving(vec![said("user", "hi")]), when).unwrap();
        }
        assert_eq!(tidy(&dir, Some(30), now), 1);
        let mut left: Vec<String> = list(&dir).into_iter().map(|c| c.id).collect();
        left.sort();
        assert_eq!(left, ["chat-tidy-after1", "chat-tidy-exactly"]);
        assert_eq!(tidy(&dir, Some(30), now), 0);
    }

    #[test]
    fn a_pin_survives_every_write_and_never_moves_the_date() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-pin-0001";
        let turns = vec![said("user", "hi"), said("assistant", "hello")];
        save(&dir, id, saving(turns.clone()), at(1)).unwrap();
        assert!(!list(&dir)[0].pinned);
        assert!(set_pinned(&dir, id, true));
        assert_eq!(list(&dir)[0].updated_at, at(1));
        assert!(list(&dir)[0].pinned);
        assert!(search(&dir, "hello")[0].pinned);
        assert!(rename(&dir, id, "A name"));
        let mut more = turns.clone();
        more.push(said("user", "again"));
        let saved = save_from(&dir, id, saving(more.clone()), at(9)).unwrap();
        assert!(saved.summary().pinned);
        assert_eq!(saved.chat.updated_at, at(9));
        assert!(set_memory(
            &dir,
            id,
            Memory {
                upto: 1,
                hash: "h".into(),
                text: "t".into()
            }
        ));
        assert!(drop_last(&dir, id, 1, 3).unwrap().is_some());
        as_column(&dir, id);
        save(&dir, id, saving(more), at(10)).unwrap();
        assert!(list(&dir)[0].pinned);
        assert!(set_pinned(&dir, id, false));
        assert!(!list(&dir)[0].pinned);
        assert!(!set_pinned(&dir, "chat-pin-none1", true));
        assert!(!set_pinned(&dir, "../x", true));
    }

    #[test]
    fn expiry_skips_a_pinned_chat_and_a_deleted_one_still_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        for id in ["chat-pin-old01", "chat-pin-old02", "chat-pin-old03"] {
            save(&dir, id, saving(vec![said("user", "hi")]), at(0)).unwrap();
        }
        assert!(set_pinned(&dir, "chat-pin-old02", true));
        let now = at(0) + chrono::Duration::days(31);
        assert_eq!(tidy(&dir, Some(30), now), 2);
        assert_eq!(list(&dir).len(), 1);
        assert_eq!(list(&dir)[0].id, "chat-pin-old02");
        assert_eq!(tidy(&dir, Some(30), now), 0);
        assert!(remove(&dir, "chat-pin-old02"));
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn a_chat_remembers_the_notes_its_questions_named_through_every_write() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-note-0001";
        let first = serde_json::json!({ "role": "user", "text": "hi", "note": "n-open", "refs": [{ "id": "n-ref", "title": "R" }], "selection": { "note": "n-sel", "text": "x" } });
        let reply = serde_json::json!({ "role": "assistant", "text": "hello", "refs": [{ "id": "n-no" }], "sources": [{ "id": "n-src" }], "note": "n-no" });
        let turns = vec![first, reply];
        let saved = save_from(&dir, id, saving(turns.clone()), at(1)).unwrap();
        let three = vec!["n-open", "n-ref", "n-sel"];
        assert_eq!(saved.summary().notes, three);
        assert_eq!(list(&dir)[0].notes, three);
        assert_eq!(search(&dir, "hello")[0].notes, three);
        let mut more = turns.clone();
        more.push(
            serde_json::json!({ "role": "user", "text": "again", "note": "n-two", "refs": [] }),
        );
        more.push(serde_json::json!({ "role": "user", "text": "bare", "note": "", "selection": { "note": "" } }));
        let mut next = saving(more[2..].to_vec());
        next.from = Some(2);
        let saved = save_from(&dir, id, next, at(2)).unwrap();
        let four = vec!["n-open", "n-ref", "n-sel", "n-two"];
        assert_eq!(saved.summary().notes, four);
        assert!(rename(&dir, id, "A name"));
        assert!(set_pinned(&dir, id, true));
        assert!(set_memory(
            &dir,
            id,
            Memory {
                upto: 1,
                hash: "h".into(),
                text: "t".into()
            }
        ));
        assert_eq!(list(&dir)[0].notes, four);
        assert!(save_from(&dir, id, saving(turns), at(3)).is_err());
        assert_eq!(
            save_from(&dir, id, saving(more.clone()), at(3))
                .unwrap()
                .summary()
                .notes,
            four
        );
        assert!(drop_last(&dir, id, 2, 4).unwrap().is_some());
        assert_eq!(list(&dir)[0].notes, three);
        as_column(&dir, id);
        let mut again = more.clone();
        again.push(
            serde_json::json!({ "role": "user", "text": "last", "refs": [{ "id": "n-late" }] }),
        );
        let saved = save_from(&dir, id, saving(again), at(4)).unwrap();
        assert_eq!(
            saved.summary().notes,
            ["n-late", "n-open", "n-ref", "n-sel", "n-two"]
        );
        assert!(remove(&dir, id));
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn expiry_keeps_a_chat_updated_after_it_was_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let now = at(0) + chrono::Duration::days(31);
        for id in ["chat-old-first", "chat-old-other"] {
            save(&dir, id, saving(vec![said("user", "searchable")]), at(0)).unwrap();
        }
        db_of(&dir)
            .with(|c| {
                c.execute_batch(&format!(
                    "CREATE TRIGGER refresh_other_chat AFTER DELETE ON chats BEGIN
                        UPDATE chats SET updated_at = '{}' WHERE id != old.id;
                    END;",
                    stamp(now)
                ))
            })
            .unwrap();
        assert_eq!(tidy(&dir, Some(30), now), 1);
        let chats = list(&dir);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].updated_at, now);
        assert_eq!(load(&dir, &chats[0].id).unwrap().messages.len(), 1);
        assert_eq!(search(&dir, "searchable").len(), 1);
    }

    fn talk(n: usize) -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| {
                said(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    &format!("m{i}"),
                )
            })
            .collect()
    }

    fn partial(from: usize, messages: Vec<serde_json::Value>) -> Saving {
        Saving {
            from: Some(from),
            ..saving(messages)
        }
    }

    fn stored(dir: &Path, id: &str) -> Vec<(i64, i64, String)> {
        db_of(dir)
            .with(|c| {
                let mut q = c.prepare(
                    "SELECT id, n, message FROM chat_messages WHERE chat = ?1 ORDER BY n",
                )?;
                let rows = q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect()
            })
            .unwrap()
    }

    fn conflict(e: anyhow::Error) -> usize {
        e.downcast_ref::<Conflict>().expect("a conflict").count
    }

    #[test]
    fn a_save_adds_only_the_new_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0001";
        save(&dir, id, saving(talk(400)), at(0)).unwrap();
        db_of(&dir)
            .with(|c| {
                c.execute_batch(
                    "CREATE TEMP TABLE added (n INTEGER);
                     CREATE TEMP TRIGGER counted AFTER INSERT ON chat_messages BEGIN INSERT INTO added VALUES (new.n); END;",
                )
            })
            .unwrap();
        let saved = save_from(
            &dir,
            id,
            partial(400, vec![said("user", "one more")]),
            at(1),
        )
        .unwrap();
        assert_eq!(saved.count, 401);
        assert_eq!(saved.summary().count, 401);
        let added: Vec<i64> = db_of(&dir)
            .with(|c| {
                let mut q = c.prepare("SELECT n FROM added")?;
                let rows = q.query_map([], |r| r.get(0))?;
                rows.collect()
            })
            .unwrap();
        assert_eq!(added, [400]);
        let chat = load(&dir, id).unwrap();
        assert_eq!(chat.messages.len(), 401);
        assert_eq!(chat.messages[400]["text"], "one more");
        assert_eq!(chat.updated_at, at(1));
        assert_eq!(chat.title, "m0");
        db_of(&dir)
            .with(|c| c.execute_batch("DROP TRIGGER counted; DROP TABLE added;"))
            .unwrap();
    }

    #[test]
    fn a_save_from_an_earlier_index_replaces_from_there() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0002";
        save(&dir, id, saving(talk(10)), at(0)).unwrap();
        let before = stored(&dir, id);
        let saved = save_from(&dir, id, partial(6, vec![said("user", "again")]), at(1)).unwrap();
        assert_eq!(saved.count, 7);
        let after = stored(&dir, id);
        assert_eq!(after.len(), 7);
        assert_eq!(after[..6], before[..6]);
        assert!(after[6].2.contains("again"));
        assert_eq!(load(&dir, id).unwrap().updated_at, at(0));
        assert!(search(&dir, "m8").is_empty());
        assert_eq!(search(&dir, "again").len(), 1);
    }

    #[test]
    fn an_edited_earlier_message_rewrites_from_there() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0003";
        save(&dir, id, saving(talk(1)), at(0)).unwrap();
        save_from(&dir, id, partial(1, talk(1_000)[1..].to_vec()), at(0)).unwrap();
        let mut edited = load(&dir, id).unwrap().messages[950..].to_vec();
        edited[0]["quiz"] = serde_json::json!({ "given": "a stack", "correct": false });
        let before = stored(&dir, id);
        save_from(&dir, id, partial(950, edited), at(1)).unwrap();
        let after = stored(&dir, id);
        assert_eq!(after.len(), 1_000);
        assert_eq!(after[..950], before[..950]);
        assert!(after[950].2.contains("a stack"));
        assert_eq!(after[951].2, before[951].2);
        let chat = load(&dir, id).unwrap();
        assert_eq!(chat.messages[950]["quiz"]["given"], "a stack");
        assert_eq!(chat.updated_at, at(0));
    }

    #[test]
    fn a_whole_list_with_an_earlier_change_and_the_same_count_and_last_message_is_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0004";
        save(&dir, id, saving(talk(10)), at(0)).unwrap();
        let before = stored(&dir, id);
        db_of(&dir)
            .with(|c| {
                c.execute_batch(
                    "CREATE TEMP TABLE added (n INTEGER);
                     CREATE TEMP TRIGGER counted AFTER INSERT ON chat_messages BEGIN INSERT INTO added VALUES (new.n); END;",
                )
            })
            .unwrap();
        let mut all = talk(10);
        all[3]["text"] = serde_json::json!("m3 marked");
        save(&dir, id, saving(all), at(1)).unwrap();
        let added: Vec<i64> = db_of(&dir)
            .with(|c| {
                let mut q = c.prepare("SELECT n FROM added ORDER BY n")?;
                let rows = q.query_map([], |r| r.get(0))?;
                rows.collect()
            })
            .unwrap();
        db_of(&dir)
            .with(|c| c.execute_batch("DROP TRIGGER counted; DROP TABLE added;"))
            .unwrap();
        assert_eq!(added, (3..10).collect::<Vec<i64>>());
        let after = stored(&dir, id);
        assert_eq!(after[..3], before[..3]);
        assert_eq!(load(&dir, id).unwrap().messages[3]["text"], "m3 marked");
        assert_eq!(search(&dir, "marked").len(), 1);
    }

    #[test]
    fn a_partial_save_with_the_wrong_start_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0005";
        save(&dir, id, saving(talk(4)), at(0)).unwrap();
        let refused = save_from(&dir, id, partial(5, vec![said("user", "lost")]), at(1));
        assert_eq!(conflict(refused.err().unwrap()), 4);
        assert_eq!(stored(&dir, id).len(), 4);
        assert!(save_from(&dir, "chat-rows-new01", partial(1, talk(1)), at(1)).is_err());
        assert!(load(&dir, "chat-rows-new01").is_none());
    }

    #[test]
    fn a_whole_list_shorter_than_the_stored_chat_is_refused_and_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-0006";
        save(&dir, id, saving(talk(400)), at(0)).unwrap();
        save_from(&dir, id, partial(400, talk(600)), at(1)).unwrap();
        let newest = load(&dir, id).unwrap().messages[800..].to_vec();
        let refused = save(&dir, id, saving(newest), at(2));
        assert_eq!(conflict(refused.err().unwrap()), 1_000);
        assert_eq!(stored(&dir, id).len(), 1_000);
    }

    #[test]
    fn a_chat_longer_than_four_hundred_messages_keeps_them_all() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let whole = save(&dir, "chat-rows-0007", saving(talk(410)), at(0)).unwrap();
        assert_eq!(whole.messages.len(), 410);
        assert_eq!(stored(&dir, "chat-rows-0007").len(), 410);
        assert_eq!(load(&dir, "chat-rows-0007").unwrap().messages, talk(410));
        save(&dir, "chat-rows-0008", saving(talk(1)), at(0)).unwrap();
        let more = save_from(&dir, "chat-rows-0008", partial(1, talk(409)), at(1)).unwrap();
        assert_eq!(more.count, 410);
        assert_eq!(load(&dir, "chat-rows-0008").unwrap().messages.len(), 410);
    }

    #[test]
    fn a_chat_from_before_is_moved_to_rows_on_its_first_save_and_reads_the_same() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-column-001";
        save(&dir, id, saving(talk(6)), at(0)).unwrap();
        as_column(&dir, id);
        assert!(stored(&dir, id).is_empty());
        let column = load(&dir, id).unwrap();
        assert_eq!(column.messages, talk(6));
        let page = page(&dir, id, Some(5), 2).unwrap();
        assert_eq!((page.count, page.from), (6, 3));
        assert_eq!(page.chat.messages, talk(6)[3..5].to_vec());
        assert_eq!(search(&dir, "m5").len(), 1);
        let mut more = talk(7);
        more[6]["text"] = serde_json::json!("seventh");
        save_from(&dir, id, partial(6, more[6..].to_vec()), at(1)).unwrap();
        assert_eq!(stored(&dir, id).len(), 7);
        let moved = load(&dir, id).unwrap();
        assert_eq!(moved.messages, more);
        assert_eq!(moved.title, column.title);
        let (rows, text): (bool, String) = db_of(&dir)
            .with(|c| {
                c.query_row(
                    "SELECT rows, messages FROM chats WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert!(rows);
        assert_eq!(text, "[]");
        assert_eq!(search(&dir, "m5").len(), 1);
        assert_eq!(search(&dir, "seventh").len(), 1);
    }

    #[test]
    fn a_chat_from_before_that_cannot_be_read_keeps_its_saved_text() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-column-002";
        save(&dir, id, saving(talk(2)), at(0)).unwrap();
        as_column(&dir, id);
        db_of(&dir)
            .with(|c| {
                c.execute(
                    "UPDATE chats SET messages = '[{\"role\":' WHERE id = ?1",
                    [id],
                )
            })
            .unwrap();
        assert!(save_from(&dir, id, partial(1, talk(1)), at(1)).is_err());
        save(&dir, id, saving(talk(3)), at(1)).unwrap();
        let text: String = db_of(&dir)
            .with(|c| {
                c.query_row("SELECT messages FROM chats WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(text, "[{\"role\":");
        assert_eq!(load(&dir, id).unwrap().messages, talk(3));
    }

    #[test]
    fn a_chat_is_read_a_page_at_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-rows-page1";
        save(&dir, id, saving(talk(1)), at(0)).unwrap();
        save_from(&dir, id, partial(1, talk(1_000)[1..].to_vec()), at(0)).unwrap();
        let newest = page(&dir, id, None, PAGE).unwrap();
        assert_eq!((newest.count, newest.from), (1_000, 800));
        let earlier = page(&dir, id, Some(newest.from), PAGE).unwrap();
        assert_eq!(earlier.from, 600);
        let mut joined = earlier.chat.messages;
        joined.extend(newest.chat.messages);
        assert_eq!(joined, talk(1_000)[600..].to_vec());
        assert!(page(&dir, id, Some(0), PAGE)
            .unwrap()
            .chat
            .messages
            .is_empty());
        assert!(page(&dir, "chat-rows-none1", None, PAGE).is_none());
    }

    #[test]
    fn chat_search_finds_words_spread_over_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let spread = vec![
            said("user", "tell me about the heap"),
            said("assistant", "a heap keeps the smallest on top"),
            said("user", "and a tree?"),
        ];
        for (id, shape) in [("chat-spread-rows", 0), ("chat-spread-col1", 1)] {
            save(&dir, id, saving(spread.clone()), at(shape)).unwrap();
        }
        as_column(&dir, "chat-spread-col1");
        save(
            &dir,
            "chat-only-heap1",
            saving(vec![said("user", "heap only")]),
            at(5),
        )
        .unwrap();
        let found: Vec<String> = search(&dir, "heap tree")
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(found, ["chat-spread-rows", "chat-spread-col1"]);
        let titled: Vec<String> = search(&dir, "only").into_iter().map(|s| s.id).collect();
        assert_eq!(titled, ["chat-only-heap1"]);

        let topics = [
            "heap", "tree", "queue", "stack", "graph", "sort", "merge", "hash", "trie", "list",
        ];
        let rows = tmp.path().join("rows").join("chats");
        let column = tmp.path().join("column").join("chats");
        for i in 0..40usize {
            let messages: Vec<_> = (0..4)
                .map(|k| {
                    said(
                        if k % 2 == 0 { "user" } else { "assistant" },
                        &format!("{} with {}", topics[(i + k) % 10], topics[(i * 3 + k) % 10]),
                    )
                })
                .collect();
            let id = format!("chat-same-{i:04}");
            save(&rows, &id, saving(messages.clone()), at(0)).unwrap();
            save(&column, &id, saving(messages), at(0)).unwrap();
            as_column(&column, &id);
        }
        let today = |query: &str| -> Vec<String> {
            let matching = query
                .split_whitespace()
                .map(|w| format!("\"{w}\"*"))
                .collect::<Vec<_>>()
                .join(" ");
            let mut ids: Vec<String> = db_of(&column)
                .with(|c| {
                    let mut q = c.prepare("SELECT id FROM chats_text WHERE chats_text MATCH ?1")?;
                    let rows = q.query_map([&matching], |r| r.get(0))?;
                    rows.collect()
                })
                .unwrap();
            ids.sort();
            ids
        };
        for q in 0..30usize {
            let query = format!("{} {}", topics[q % 10], topics[(q * 7 + 3) % 10]);
            let mut now: Vec<String> = search(&rows, &query).into_iter().map(|s| s.id).collect();
            now.sort();
            assert_eq!(now, today(&query), "{query}");
        }
    }

    fn timed<R>(runs: u32, mut work: impl FnMut() -> R) -> f64 {
        let started = std::time::Instant::now();
        for _ in 0..runs {
            std::hint::black_box(work());
        }
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(runs)
    }

    fn log_bytes(dir: &Path) -> u64 {
        let mut name = db_of(dir).path().as_os_str().to_owned();
        name.push("-wal");
        std::fs::metadata(PathBuf::from(name)).map_or(0, |m| m.len())
    }

    fn emptied_log(dir: &Path) {
        db_of(dir)
            .with(|c| c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())))
            .unwrap();
    }

    #[test]
    #[ignore = "audit benchmark"]
    fn audit_p16_saving_a_long_chat_parses_the_old_copy() {
        for n in [400usize, 4_000, 40_000] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("chats");
            let id = "chat-audit-long1";
            let mut all: Vec<_> = (0..n)
                .map(|i| {
                    said(
                        if i % 2 == 0 { "user" } else { "assistant" },
                        &format!("message {i} {}", "about heaps and trees ".repeat(20)),
                    )
                })
                .collect();
            save(&dir, id, saving(all.clone()), at(0)).unwrap();
            emptied_log(&dir);
            all.push(said("user", "one more"));
            save(&dir, id, saving(all.clone()), at(1)).unwrap();
            let written = log_bytes(&dir);
            let save_ms = timed(7, || {
                all.push(said("user", "again"));
                save(&dir, id, saving(all.clone()), at(2)).unwrap()
            });
            println!("audit_eng13 messages={n} save_ms={save_ms:.3} log_bytes_written={written}");
            let id = "chat-audit-rows1";
            save(&dir, id, saving(all[..1].to_vec()), at(0)).unwrap();
            save_from(&dir, id, partial(1, all[1..n].to_vec()), at(0)).unwrap();
            emptied_log(&dir);
            save_from(&dir, id, partial(n, vec![said("user", "one more")]), at(1)).unwrap();
            let written = log_bytes(&dir);
            let mut held = n + 1;
            let save_ms = timed(50, || {
                save_from(&dir, id, partial(held, vec![said("user", "again")]), at(2)).unwrap();
                held += 1;
            });
            let open_ms = timed(20, || {
                page(&dir, id, None, PAGE).unwrap().chat.messages.len()
            });
            println!("audit_eng13_rows messages={n} append_ms={save_ms:.3} log_bytes_written={written} open_newest_page_ms={open_ms:.3}");
        }
    }

    #[test]
    #[ignore = "ten megabytes of chat"]
    fn a_chat_of_ten_megabytes_saves_message_by_message() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let id = "chat-ten-mb-001";
        let text = "a long answer about heaps and trees ".repeat(300);
        save(&dir, id, saving(vec![said("user", "start")]), at(0)).unwrap();
        for n in 1..1_000 {
            save_from(&dir, id, partial(n, vec![said("assistant", &text)]), at(1)).unwrap();
        }
        let chat = load(&dir, id).unwrap();
        assert_eq!(chat.messages.len(), 1_000);
        assert!(json(&chat.messages).len() > 10 * 1024 * 1024);
        assert_eq!(
            page(&dir, id, None, PAGE).unwrap().chat.messages.len(),
            PAGE
        );
    }

    #[test]
    #[ignore = "audit benchmark"]
    fn audit_bench_chat_tidy() {
        for n in [1_000usize, 10_000] {
            for expired in [0usize, n / 10] {
                let tmp = tempfile::tempdir().unwrap();
                let dir = tmp.path().join("chats");
                let now = at(0) + chrono::Duration::days(100);
                for i in 0..n {
                    let when = if i < expired { at(0) } else { now };
                    save(
                        &dir,
                        &format!("chat-tidy-{i:06}"),
                        saving(vec![said("user", "hi")]),
                        when,
                    )
                    .unwrap();
                }
                let started = std::time::Instant::now();
                let gone = tidy(&dir, Some(30), now);
                let first = started.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(gone, expired);
                let again = timed(20, || tidy(&dir, Some(30), now));
                println!("audit_bench_chat_tidy chats={n} expired={expired} first_ms={first:.3} none_left_ms={again:.4}");
            }
        }
    }
}
