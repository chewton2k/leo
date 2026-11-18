use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::chats::valid_id;

pub const MOST_FILES: usize = 10;
pub const UPLOAD_BYTES: usize = 40 * 1024 * 1024;
pub const ORPHAN_HOURS: i64 = 24;
pub const EXCERPT_CHARS: usize = 320;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Doc {
    pub id: String,
    pub name: String,
    pub chars: usize,
    pub bytes: u64,
    pub added_at: DateTime<Utc>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Held {
    pub chat: String,
    pub docs: usize,
    pub bytes: u64,
    pub last: Option<DateTime<Utc>>,
}

fn valid_doc(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn insert(c: &rusqlite::Connection, chat: &str, doc: &Doc, text: &str) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT OR IGNORE INTO chat_docs (id, chat, name, added_at, text, chars, bytes, excerpt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            doc.id,
            chat,
            doc.name,
            doc.added_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            text,
            doc.chars as i64,
            doc.bytes as i64,
            doc.excerpt
        ],
    )
}

fn described(id: String, name: String, added_at: DateTime<Utc>, text: &str) -> Doc {
    Doc {
        id,
        chars: text.chars().count(),
        bytes: (text.len() + name.len()) as u64,
        name,
        added_at,
        excerpt: excerpt_of(text),
    }
}

fn excerpt_of(text: &str) -> String {
    let mut out = String::new();
    for line in text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
    {
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
        if out.chars().count() >= EXCERPT_CHARS {
            break;
        }
    }
    out.chars().take(EXCERPT_CHARS).collect()
}

fn count_in(c: &rusqlite::Connection, chat: &str) -> rusqlite::Result<usize> {
    c.query_row(
        "SELECT COUNT(*) FROM chat_docs WHERE chat = ?1",
        [chat],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as usize)
}

pub fn count(dir: &Path, chat: &str) -> usize {
    if !valid_id(chat) {
        return 0;
    }
    crate::chats::db_of(dir)
        .with(|c| count_in(c, chat))
        .unwrap_or_default()
}

pub fn add(dir: &Path, chat: &str, name: &str, text: &str, now: DateTime<Utc>) -> Result<Doc> {
    if !valid_id(chat) {
        bail!("that is not a chat");
    }
    let doc = described(
        uuid::Uuid::new_v4().simple().to_string(),
        name.chars().take(120).collect(),
        now,
        text,
    );
    let added = crate::chats::db_of(dir).with(|c| {
        let tx = c.transaction()?;
        if count_in(&tx, chat)? >= MOST_FILES {
            return Ok(false);
        }
        insert(&tx, chat, &doc, text)?;
        tx.commit()?;
        Ok(true)
    })?;
    if !added {
        bail!("a chat holds up to {MOST_FILES} documents; remove one first");
    }
    Ok(doc)
}

fn added_at(stamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(stamp).map_or_else(|_| Utc::now(), |t| t.with_timezone(&Utc))
}

type Listed = (
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
);

pub fn list(dir: &Path, chat: &str) -> Vec<Doc> {
    if !valid_id(chat) {
        return Vec::new();
    }
    crate::chats::db_of(dir)
        .with(|c| {
            let rows: Vec<Listed> = {
                let mut found = c.prepare(
                    "SELECT id, name, added_at, chars, bytes, excerpt FROM chat_docs WHERE chat = ?1 ORDER BY added_at, id",
                )?;
                let rows = found.query_map([chat], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut docs = Vec::with_capacity(rows.len());
            for (id, name, stamp, chars, bytes, excerpt) in rows {
                let doc = match (chars, bytes, excerpt) {
                    (Some(chars), Some(bytes), Some(excerpt)) => Doc {
                        id,
                        name,
                        chars: chars as usize,
                        bytes: bytes as u64,
                        added_at: added_at(&stamp),
                        excerpt,
                    },
                    _ => {
                        let text: String = c.query_row(
                            "SELECT text FROM chat_docs WHERE id = ?1",
                            [&id],
                            |r| r.get(0),
                        )?;
                        let doc = described(id, name, added_at(&stamp), &text);
                        c.execute(
                            "UPDATE chat_docs SET chars = ?2, bytes = ?3, excerpt = ?4 WHERE id = ?1",
                            rusqlite::params![doc.id, doc.chars as i64, doc.bytes as i64, doc.excerpt],
                        )?;
                        doc
                    }
                };
                docs.push(doc);
            }
            Ok(docs)
        })
        .unwrap_or_default()
}

pub fn texts(dir: &Path, chat: &str, ids: &[String]) -> Vec<(String, String)> {
    let ids: Vec<&String> = ids.iter().filter(|id| valid_doc(id)).collect();
    if !valid_id(chat) || ids.is_empty() {
        return Vec::new();
    }
    let marks = vec!["?"; ids.len()].join(", ");
    crate::chats::db_of(dir)
        .with(|c| {
            let mut found = c.prepare(&format!(
                "SELECT name, text FROM chat_docs WHERE chat = ? AND id IN ({marks}) ORDER BY added_at, id"
            ))?;
            let rows = found.query_map(
                rusqlite::params_from_iter(std::iter::once(chat).chain(ids.iter().map(|id| id.as_str()))),
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn remove(dir: &Path, chat: &str, doc: &str) -> bool {
    valid_doc(doc)
        && crate::chats::db_of(dir)
            .with(|c| {
                c.execute(
                    "DELETE FROM chat_docs WHERE id = ?1 AND chat = ?2",
                    [doc, chat],
                )
            })
            .is_ok_and(|n| n > 0)
}

pub fn remove_all(dir: &Path, chat: &str) -> bool {
    crate::chats::db_of(dir)
        .with(|c| c.execute("DELETE FROM chat_docs WHERE chat = ?1", [chat]))
        .is_ok_and(|n| n > 0)
}

pub fn held(dir: &Path) -> Vec<Held> {
    crate::chats::db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT chat, COUNT(*), SUM(length(text) + length(name)), MAX(added_at)
                 FROM chat_docs GROUP BY chat ORDER BY SUM(length(text) + length(name)) DESC, chat",
            )?;
            let rows = found.query_map([], |r| {
                Ok(Held {
                    chat: r.get(0)?,
                    docs: r.get::<_, i64>(1)? as usize,
                    bytes: r.get::<_, i64>(2)?.max(0) as u64,
                    last: r
                        .get::<_, Option<String>>(3)?
                        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                        .map(|t| t.with_timezone(&Utc)),
                })
            })?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn tidy_orphans(dir: &Path, now: DateTime<Utc>) -> usize {
    let cutoff = (now - chrono::Duration::hours(ORPHAN_HOURS))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    const ORPHANS: &str = "FROM chat_docs WHERE chat NOT IN (SELECT id FROM chats) GROUP BY chat HAVING MAX(added_at) < ?1";
    crate::chats::db_of(dir)
        .with(|c| {
            let t = c.transaction()?;
            let orphans: i64 = t.query_row(
                &format!("SELECT COUNT(*) FROM (SELECT chat {ORPHANS})"),
                [&cutoff],
                |r| r.get(0),
            )?;
            t.execute(
                &format!("DELETE FROM chat_docs WHERE chat IN (SELECT chat {ORPHANS})"),
                [&cutoff],
            )?;
            t.commit()?;
            Ok(orphans as usize)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 8, hour, 0, 0).unwrap()
    }

    fn raw(dir: &Path, sql: &str) {
        crate::chats::db_of(dir)
            .with(|c| c.execute_batch(sql))
            .unwrap();
    }

    #[test]
    fn the_list_reads_no_document_text() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let doc = add(
            &dir,
            "chat-aaaa-1",
            "w.pdf",
            "Week 3\n\nHeaps keep the minimum.",
            at(1),
        )
        .unwrap();
        raw(&dir, "UPDATE chat_docs SET text = ''");
        let listed = list(&dir, "chat-aaaa-1");
        assert_eq!(listed, vec![doc.clone()]);
        assert_eq!(listed[0].chars, 31);
        assert_eq!(listed[0].bytes, 31 + 5);
        assert_eq!(listed[0].excerpt, "Week 3\nHeaps keep the minimum.");
    }

    #[test]
    fn documents_from_before_are_described_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let fresh = add(&dir, "chat-aaaa-1", "new.txt", "Newer words.", at(2)).unwrap();
        raw(
            &dir,
            "INSERT INTO chat_docs (id, chat, name, added_at, text) VALUES ('00000000000000000000000000000001', 'chat-aaaa-1', 'old.txt', '2026-10-08T01:00:00.000000Z', 'Old  words\n\nsecond line')",
        );
        let listed = list(&dir, "chat-aaaa-1");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "old.txt");
        assert_eq!(listed[0].chars, 23);
        assert_eq!(listed[0].bytes, 23 + 7);
        assert_eq!(listed[0].excerpt, "Old words\nsecond line");
        assert_eq!(listed[1], fresh);
        let described: i64 = crate::chats::db_of(&dir)
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM chat_docs WHERE chars IS NOT NULL AND bytes IS NOT NULL AND excerpt IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(described, 2);
        raw(&dir, "UPDATE chat_docs SET text = ''");
        assert_eq!(list(&dir, "chat-aaaa-1"), listed);
    }

    #[test]
    fn two_uploads_at_once_cannot_pass_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        for i in 0..MOST_FILES - 1 {
            add(&dir, "chat-aaaa-1", &format!("{i}.txt"), "words", at(1)).unwrap();
        }
        let both: Vec<_> = (0..2)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    add(&dir, "chat-aaaa-1", &format!("late{i}.txt"), "words", at(2)).is_ok()
                })
            })
            .collect();
        let added = both
            .into_iter()
            .map(|one| one.join().unwrap())
            .filter(|&ok| ok)
            .count();
        assert_eq!(added, 1);
        assert_eq!(list(&dir, "chat-aaaa-1").len(), MOST_FILES);
        assert_eq!(count(&dir, "chat-aaaa-1"), MOST_FILES);
    }

    #[test]
    fn a_document_shows_its_first_lines_as_a_small_example() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let doc = add(
            &dir,
            "chat-xxxx-1",
            "w.pdf",
            "  Week 3\n\n```\nHeaps   keep\tthe minimum\n",
            at(1),
        )
        .unwrap();
        assert_eq!(doc.excerpt, "Week 3\nHeaps keep the minimum");
        let long = "word ".repeat(400);
        let doc = add(&dir, "chat-xxxx-1", "l.txt", &long, at(2)).unwrap();
        assert_eq!(doc.excerpt.chars().count(), EXCERPT_CHARS);
        assert_eq!(
            list(&dir, "chat-xxxx-1")[0].excerpt,
            "Week 3\nHeaps keep the minimum"
        );
    }

    #[test]
    fn documents_keep_only_their_text_and_belong_to_one_chat() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let a = add(
            &dir,
            "chat-aaaa-1",
            "slides.pdf",
            "Heaps keep the minimum at the root.",
            at(1),
        )
        .unwrap();
        let b = add(
            &dir,
            "chat-aaaa-1",
            "board.jpg",
            "Dijkstra uses a heap.",
            at(2),
        )
        .unwrap();
        add(&dir, "chat-bbbb-2", "other.txt", "elsewhere", at(3)).unwrap();
        assert_eq!(a.chars, 35);
        let listed = list(&dir, "chat-aaaa-1");
        assert_eq!(
            listed.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["slides.pdf", "board.jpg"]
        );
        assert_eq!(
            texts(&dir, "chat-aaaa-1", std::slice::from_ref(&b.id)),
            [("board.jpg".to_string(), "Dijkstra uses a heap.".to_string())]
        );
        assert!(
            texts(&dir, "chat-bbbb-2", std::slice::from_ref(&b.id)).is_empty(),
            "another chat's document is not reachable"
        );
        assert!(remove(&dir, "chat-aaaa-1", &a.id));
        assert!(!remove(&dir, "chat-aaaa-1", "../../notes"));
        assert_eq!(list(&dir, "chat-aaaa-1").len(), 1);
        assert!(remove_all(&dir, "chat-aaaa-1"));
        assert!(list(&dir, "chat-aaaa-1").is_empty());
        assert!(add(&dir, "../escape", "x", "y", at(1)).is_err());
    }

    #[test]
    fn a_chat_holds_a_limited_number_of_documents() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..MOST_FILES {
            add(
                tmp.path(),
                "chat-full-0001",
                &format!("{i}.txt"),
                "x",
                at(1),
            )
            .unwrap();
        }
        let refused = add(tmp.path(), "chat-full-0001", "one more.txt", "x", at(1));
        assert!(refused.unwrap_err().to_string().contains("up to 10"));
    }

    #[test]
    fn documents_for_a_chat_that_never_started_are_cleared_after_a_day() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        add(dir, "chat-lost-0001", "a.txt", "x", at(1)).unwrap();
        add(dir, "chat-kept-0002", "b.txt", "x", at(1)).unwrap();
        crate::chats::save(
            dir,
            "chat-kept-0002",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [] })).unwrap(),
            at(1),
        )
        .unwrap();
        assert_eq!(tidy_orphans(dir, at(12)), 0, "not a day old yet");
        let tomorrow = at(1) + chrono::Duration::hours(25);
        assert_eq!(tidy_orphans(dir, tomorrow), 1);
        let left: Vec<String> = held(dir).into_iter().map(|h| h.chat).collect();
        assert_eq!(left, ["chat-kept-0002"]);
    }

    #[test]
    fn a_failed_chat_lookup_never_deletes_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        add(dir, "chat-kept-0002", "b.txt", "x", at(1)).unwrap();
        crate::chats::save(
            dir,
            "chat-kept-0002",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [] })).unwrap(),
            at(1),
        )
        .unwrap();
        crate::chats::db_of(dir)
            .with(|c| c.execute_batch("ALTER TABLE chats RENAME TO chats_busy"))
            .unwrap();
        let tomorrow = at(1) + chrono::Duration::hours(25);
        assert_eq!(tidy_orphans(dir, tomorrow), 0);
        assert_eq!(list(dir, "chat-kept-0002").len(), 1);
    }
}
