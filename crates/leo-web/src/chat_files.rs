use std::path::Path;

use anyhow::{bail, ensure, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::chats::valid_id;

pub const KEPT_WHOLE: usize = 2_000_000;
pub const PIECE_CHARS: usize = 64_000;
const PIECES_AT_ONCE: usize = 16;
const CUT_NOTE: &str = "\n\n[…the rest of this document is left out]";
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
    crate::chats::db_of(dir).with(|c| insert(c, chat, &doc, text))?;
    Ok(doc)
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
                        added_at: crate::chats::when(&stamp),
                        excerpt,
                    },
                    _ => {
                        let text: String = c.query_row(
                            "SELECT text FROM chat_docs WHERE id = ?1",
                            [&id],
                            |r| r.get(0),
                        )?;
                        let doc = described(id, name, crate::chats::when(&stamp), &text);
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

#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub id: String,
    pub name: String,
    pub chars: usize,
}

pub fn texts(
    dir: &Path,
    chat: &str,
    ids: &[String],
    most: usize,
    heads: usize,
) -> Vec<(Named, String)> {
    let ids: Vec<&String> = ids.iter().filter(|id| valid_doc(id)).collect();
    if !valid_id(chat) || ids.is_empty() {
        return Vec::new();
    }
    let marks = vec!["?"; ids.len()].join(", ");
    crate::chats::db_of(dir)
        .with(|c| {
            let named: Vec<Named> = {
                let mut found = c.prepare(&format!(
                    "SELECT id, name, COALESCE(chars, length(text)) FROM chat_docs WHERE chat = ? AND id IN ({marks}) ORDER BY added_at, id"
                ))?;
                let rows = found.query_map(
                    rusqlite::params_from_iter(
                        std::iter::once(chat).chain(ids.iter().map(|id| id.as_str())),
                    ),
                    |r| {
                        Ok(Named {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            chars: r.get::<_, i64>(2)?.max(0) as usize,
                        })
                    },
                )?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            named
                .into_iter()
                .enumerate()
                .map(|(i, doc)| {
                    let head = if i < heads {
                        range(c, &doc.id, 0, most)?
                    } else {
                        String::new()
                    };
                    Ok((doc, head))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cuts_of(c: &rusqlite::Connection, doc: &str) -> rusqlite::Result<Option<Vec<usize>>> {
    let stored: Option<String> =
        c.query_row("SELECT cuts FROM chat_docs WHERE id = ?1", [doc], |r| {
            r.get(0)
        })?;
    Ok(stored.and_then(|json| serde_json::from_str(&json).ok()))
}

fn range(
    c: &rusqlite::Connection,
    doc: &str,
    from: usize,
    chars: usize,
) -> rusqlite::Result<String> {
    let Some(cuts) = cuts_of(c, doc)? else {
        return c.query_row(
            "SELECT substr(text, ?2, ?3) FROM chat_docs WHERE id = ?1",
            rusqlite::params![doc, from as i64 + 1, chars as i64],
            |r| r.get(0),
        );
    };
    let end = from.saturating_add(chars);
    let mut out = String::new();
    let mut start = 0usize;
    for (n, len) in cuts.iter().enumerate() {
        let stop = start + len;
        if stop > from && start < end {
            let piece: String = c.query_row(
                "SELECT text FROM chat_doc_parts WHERE doc = ?1 AND n = ?2",
                rusqlite::params![doc, n as i64],
                |r| r.get(0),
            )?;
            out.extend(
                piece
                    .chars()
                    .skip(from.saturating_sub(start))
                    .take(end.min(stop) - start.max(from)),
            );
        }
        start = stop;
        if start >= end {
            break;
        }
    }
    Ok(out)
}

pub fn part(dir: &Path, chat: &str, doc: &str, from: usize, chars: usize) -> Option<String> {
    if !valid_id(chat) || !valid_doc(doc) {
        return None;
    }
    crate::chats::db_of(dir)
        .with(|c| {
            let here: i64 = c.query_row(
                "SELECT COUNT(*) FROM chat_docs WHERE id = ?1 AND chat = ?2",
                [doc, chat],
                |r| r.get(0),
            )?;
            if here == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            range(c, doc, from.saturating_sub(1), chars)
        })
        .ok()
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

static ARRIVING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn arriving() -> std::sync::MutexGuard<'static, Vec<String>> {
    ARRIVING.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Intake {
    dir: std::path::PathBuf,
    chat: String,
    name: String,
    doc: String,
    started: bool,
    carry: Vec<u8>,
    pending: String,
    pending_chars: usize,
    pieces: bool,
    head: String,
    cuts: Vec<usize>,
    text_bytes: u64,
    batch: Vec<(i64, String)>,
    stored: i64,
    done: bool,
}

impl Intake {
    pub fn new(dir: &Path, chat: &str, name: &str) -> Result<Intake> {
        ensure!(valid_id(chat), "that is not a chat");
        let doc = uuid::Uuid::new_v4().simple().to_string();
        arriving().push(doc.clone());
        Ok(Intake {
            dir: dir.to_path_buf(),
            chat: chat.to_string(),
            name: name.chars().take(120).collect(),
            doc,
            started: false,
            carry: Vec::new(),
            pending: String::new(),
            pending_chars: 0,
            pieces: false,
            head: String::new(),
            cuts: Vec::new(),
            text_bytes: 0,
            batch: Vec::new(),
            stored: 0,
            done: false,
        })
    }

    fn push(&mut self, text: &str) -> Result<()> {
        let text = if self.started {
            text
        } else {
            text.trim_start()
        };
        if text.is_empty() {
            return Ok(());
        }
        self.started = true;
        self.pending.push_str(text);
        self.pending_chars += text.chars().count();
        if !self.pieces && self.pending_chars > KEPT_WHOLE {
            self.pieces = true;
            self.head = self.pending.chars().take(KEPT_WHOLE).collect();
        }
        if self.pieces {
            while self.pending_chars > PIECE_CHARS {
                self.cut()?;
            }
        }
        Ok(())
    }

    fn cut(&mut self) -> Result<()> {
        let end = self
            .pending
            .char_indices()
            .nth(PIECE_CHARS)
            .map_or(self.pending.len(), |(at, _)| at);
        let at = [
            self.pending[..end].rfind("\n\n").map(|i| i + 2),
            self.pending[..end].rfind('\n').map(|i| i + 1),
        ]
        .into_iter()
        .flatten()
        .find(|at| *at > end / 2)
        .unwrap_or(end);
        let rest = self.pending.split_off(at);
        let piece = std::mem::replace(&mut self.pending, rest);
        self.pending_chars -= piece.chars().count();
        self.keep(piece)
    }

    fn keep(&mut self, piece: String) -> Result<()> {
        self.cuts.push(piece.chars().count());
        self.text_bytes += piece.len() as u64;
        self.batch.push((self.stored, piece));
        self.stored += 1;
        if self.batch.len() >= PIECES_AT_ONCE {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let batch = std::mem::take(&mut self.batch);
        if batch.is_empty() {
            return Ok(());
        }
        let doc = &self.doc;
        crate::chats::db_of(&self.dir).with(|c| {
            let t = c.transaction()?;
            {
                let mut put =
                    t.prepare("INSERT INTO chat_doc_parts (doc, n, text) VALUES (?1, ?2, ?3)")?;
                for (n, text) in &batch {
                    put.execute(rusqlite::params![doc, n, text])?;
                }
            }
            t.commit()
        })
    }

    pub fn finish(mut self, now: DateTime<Utc>) -> Result<Doc> {
        if !self.carry.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.carry)).into_owned();
            self.push(&rest)?;
        }
        let tail = self.pending.trim_end().to_string();
        if !self.pieces {
            ensure!(
                !tail.is_empty(),
                "there is no text in {} for Felix to read",
                self.name
            );
            let doc = add(&self.dir, &self.chat, &self.name, &tail, now)?;
            self.done = true;
            return Ok(doc);
        }
        self.pending.clear();
        if !tail.is_empty() {
            self.keep(tail)?;
        }
        self.flush()?;
        let chars: usize = self.cuts.iter().sum();
        let doc = Doc {
            id: self.doc.clone(),
            chars,
            bytes: self.text_bytes + self.name.len() as u64,
            name: self.name.clone(),
            added_at: now,
            excerpt: excerpt_of(&self.head),
        };
        let kept = format!("{}{CUT_NOTE}", self.head);
        let cuts = serde_json::to_string(&self.cuts)?;
        crate::chats::db_of(&self.dir).with(|c| {
            insert(c, &self.chat, &doc, &kept)?;
            c.execute(
                "UPDATE chat_docs SET cuts = ?2 WHERE id = ?1",
                rusqlite::params![doc.id, cuts],
            )
        })?;
        self.done = true;
        Ok(doc)
    }
}

impl std::io::Write for Intake {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.carry.extend_from_slice(bytes);
        let whole = match std::str::from_utf8(&self.carry) {
            Ok(_) => self.carry.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.carry.len(),
        };
        let rest = self.carry.split_off(whole);
        let text = String::from_utf8_lossy(&std::mem::replace(&mut self.carry, rest)).into_owned();
        self.push(&text).map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Intake {
    fn drop(&mut self) {
        if !self.done && self.stored > 0 {
            let doc = self.doc.clone();
            let _ = crate::chats::db_of(&self.dir)
                .with(|c| c.execute("DELETE FROM chat_doc_parts WHERE doc = ?1", [&doc]));
        }
        arriving().retain(|doc| *doc != self.doc);
    }
}

fn sweep_parts(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let docs: Vec<String> = {
        let mut found = c.prepare("SELECT DISTINCT doc FROM chat_doc_parts")?;
        let rows = found.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for doc in docs {
        let live: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_docs WHERE id = ?1)",
            [&doc],
            |r| r.get(0),
        )?;
        if !live && !arriving().contains(&doc) {
            c.execute("DELETE FROM chat_doc_parts WHERE doc = ?1", [&doc])?;
        }
    }
    Ok(())
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
            sweep_parts(c)?;
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
    fn two_uploads_at_once_are_both_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let both: Vec<_> = (0..2)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    add(&dir, "chat-aaaa-1", &format!("late{i}.txt"), "words", at(2)).is_ok()
                })
            })
            .collect();
        assert!(both.into_iter().all(|one| one.join().unwrap()));
        assert_eq!(list(&dir, "chat-aaaa-1").len(), 2);
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
        let named = Named {
            id: b.id.clone(),
            name: "board.jpg".into(),
            chars: 21,
        };
        assert_eq!(
            texts(&dir, "chat-aaaa-1", std::slice::from_ref(&b.id), 1_000, 1),
            [(named, "Dijkstra uses a heap.".to_string())]
        );
        assert!(
            texts(&dir, "chat-bbbb-2", std::slice::from_ref(&b.id), 1_000, 1).is_empty(),
            "another chat's document is not reachable"
        );
        assert!(remove(&dir, "chat-aaaa-1", &a.id));
        assert!(!remove(&dir, "chat-aaaa-1", "../../notes"));
        assert_eq!(list(&dir, "chat-aaaa-1").len(), 1);
        raw(&dir, "DELETE FROM chat_docs WHERE chat = 'chat-aaaa-1'");
        assert!(list(&dir, "chat-aaaa-1").is_empty());
        assert!(add(&dir, "../escape", "x", "y", at(1)).is_err());
    }

    #[test]
    fn a_chat_holds_any_number_of_documents() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..11 {
            add(
                tmp.path(),
                "chat-full-0001",
                &format!("{i}.txt"),
                "x",
                at(1),
            )
            .unwrap();
        }
        assert_eq!(list(tmp.path(), "chat-full-0001").len(), 11);
    }

    #[test]
    fn a_message_reads_only_its_share_of_a_long_document() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let long: String = (0..5_000_000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let mut ids = Vec::new();
        for i in 0..9 {
            ids.push(
                add(
                    dir,
                    "chat-aaaa-1",
                    &format!("s{i}.txt"),
                    "short words",
                    at(1),
                )
                .unwrap()
                .id,
            );
        }
        ids.push(
            add(dir, "chat-aaaa-1", "long.txt", &long, at(2))
                .unwrap()
                .id,
        );
        let found = texts(dir, "chat-aaaa-1", &ids, 7_000, 10);
        assert_eq!(found.len(), 10);
        assert_eq!(found[0].1, "short words");
        assert_eq!(found[9].0.chars, 5_000_000);
        assert_eq!(found[9].1.chars().count(), 7_000);
        assert!(long.starts_with(&found[9].1));
        let none = texts(dir, "chat-aaaa-1", &ids, 7_000, 3);
        assert_eq!(none[2].1, "short words");
        assert_eq!(none[3].1, "");
        assert_eq!(none[9].0.chars, 5_000_000);
    }

    #[test]
    fn a_part_is_cut_from_the_stored_text_by_characters() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let doc = add(dir, "chat-aaaa-1", "w.txt", "héllo wörld ünd mehr", at(1)).unwrap();
        assert_eq!(part(dir, "chat-aaaa-1", &doc.id, 1, 5).unwrap(), "héllo");
        assert_eq!(
            part(dir, "chat-aaaa-1", &doc.id, 13, 100).unwrap(),
            "ünd mehr"
        );
        assert_eq!(part(dir, "chat-bbbb-2", &doc.id, 1, 5), None);
        assert_eq!(part(dir, "chat-aaaa-1", "../x", 1, 5), None);
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
        let left: Vec<String> = crate::chats::db_of(dir)
            .with(|c| {
                c.prepare("SELECT DISTINCT chat FROM chat_docs")?
                    .query_map([], |r| r.get(0))?
                    .collect()
            })
            .unwrap();
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

    fn long_text(chars: usize) -> String {
        let mut out = String::with_capacity(chars + 64);
        let mut n = 0;
        while out.len() < chars {
            out.push_str(&format!("line {n} of the long document\n"));
            n += 1;
        }
        out.truncate(chars);
        out.trim_end().to_string()
    }

    fn taken(dir: &Path, chat: &str, name: &str, text: &str) -> Doc {
        use std::io::Write;
        let mut intake = Intake::new(dir, chat, name).unwrap();
        for piece in text.as_bytes().chunks(50_000) {
            intake.write_all(piece).unwrap();
        }
        intake.finish(at(1)).unwrap()
    }

    fn count(dir: &Path, sql: &str) -> i64 {
        crate::chats::db_of(dir)
            .with(|c| c.query_row(sql, [], |r| r.get(0)))
            .unwrap()
    }

    #[test]
    fn a_long_document_is_kept_whole_and_read_to_its_end() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let text = long_text(5_000_000);
        let doc = taken(&dir, "chat-aaaa-1", "book.txt", &text);
        assert_eq!(doc.chars, text.len());
        let listed = list(&dir, "chat-aaaa-1");
        assert_eq!(listed[0].chars, doc.chars);
        assert_eq!(listed[0].bytes, text.len() as u64 + 8);
        let last = part(&dir, "chat-aaaa-1", &doc.id, doc.chars - 999, 5_000).unwrap();
        assert_eq!(last, text[text.len() - 1000..]);
        let across = part(&dir, "chat-aaaa-1", &doc.id, 63_990, 40).unwrap();
        assert_eq!(across, text[63_989..64_029]);
        assert!(count(&dir, "SELECT MAX(length(text)) FROM chat_doc_parts") <= PIECE_CHARS as i64);
        let texts = texts(
            &dir,
            "chat-aaaa-1",
            std::slice::from_ref(&doc.id),
            100_000,
            1,
        );
        assert_eq!(texts[0].1, text[..100_000]);
        assert_eq!(texts[0].0.chars, doc.chars);
    }

    #[test]
    fn an_older_leo_still_sees_the_first_two_million_characters() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let text = long_text(2_100_000);
        let doc = taken(&dir, "chat-aaaa-1", "book.txt", &text);
        let kept: String = crate::chats::db_of(&dir)
            .with(|c| {
                c.query_row("SELECT text FROM chat_docs WHERE id = ?1", [&doc.id], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(kept.chars().count(), KEPT_WHOLE + CUT_NOTE.chars().count());
        assert!(kept.starts_with(&text[..KEPT_WHOLE]));
        assert!(kept.ends_with("[…the rest of this document is left out]"));
        assert_eq!(doc.chars, text.len());
    }

    #[test]
    fn a_short_document_keeps_its_one_text_and_no_pieces() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let doc = taken(&dir, "chat-aaaa-1", "a.txt", "  Week 3\n\nHeaps.\n\n  ");
        assert_eq!(doc.chars, 14);
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), 0);
        assert_eq!(part(&dir, "chat-aaaa-1", &doc.id, 1, 6).unwrap(), "Week 3");
    }

    #[test]
    fn a_removed_document_takes_its_pieces() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let text = long_text(2_300_000);
        let first = taken(&dir, "chat-aaaa-1", "a.txt", &text);
        taken(&dir, "chat-aaaa-1", "b.txt", &text);
        taken(&dir, "chat-bbbb-1", "c.txt", &text);
        let held = count(&dir, "SELECT COUNT(*) FROM chat_doc_parts");
        assert!(remove(&dir, "chat-aaaa-1", &first.id));
        assert_eq!(
            count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"),
            held / 3 * 2
        );
        raw(&dir, "DELETE FROM chat_docs WHERE chat = 'chat-aaaa-1'");
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), held / 3);
        raw(&dir, "DELETE FROM chat_docs");
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), 0);
        raw(&dir, "INSERT INTO chat_doc_parts VALUES ('stray', 0, 'x')");
        let arriving = Intake::new(&dir, "chat-aaaa-1", "d.txt").unwrap();
        raw(
            &dir,
            &format!(
                "INSERT INTO chat_doc_parts VALUES ('{}', 0, 'y')",
                arriving.doc
            ),
        );
        tidy_orphans(&dir, at(23));
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), 1);
        drop(arriving);
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), 1);
    }

    #[test]
    fn a_read_that_fails_leaves_no_pieces() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let text = long_text(3_000_000);
        let mut intake = Intake::new(&dir, "chat-aaaa-1", "a.txt").unwrap();
        for piece in text.as_bytes().chunks(50_000) {
            intake.write_all(piece).unwrap();
        }
        assert!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts") > 0);
        drop(intake);
        assert_eq!(count(&dir, "SELECT COUNT(*) FROM chat_doc_parts"), 0);
        assert!(list(&dir, "chat-aaaa-1").is_empty());
        assert!(taken_empty(&dir));
    }

    fn taken_empty(dir: &Path) -> bool {
        use std::io::Write;
        let mut intake = Intake::new(dir, "chat-aaaa-1", "blank.txt").unwrap();
        intake.write_all(b"   \n ").unwrap();
        intake.finish(at(1)).is_err()
    }

    #[test]
    fn an_exported_long_document_is_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let text = long_text(4_500_000);
        let doc = taken(&dir, "chat-aaaa-1", "book.txt", &text);
        let whole = |doc: &str| {
            let mut out = String::new();
            loop {
                let piece =
                    part(&dir, "chat-aaaa-1", doc, out.chars().count() + 1, 1_000_000).unwrap();
                if piece.is_empty() {
                    return out;
                }
                out.push_str(&piece);
            }
        };
        assert_eq!(whole(&doc.id), text);
        let short = taken(&dir, "chat-aaaa-1", "a.txt", "Heaps keep the minimum.");
        assert_eq!(whole(&short.id), "Heaps keep the minimum.");
    }

    #[test]
    fn a_character_split_between_writes_is_kept_whole() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let mut intake = Intake::new(&dir, "chat-aaaa-1", "a.txt").unwrap();
        for byte in "héllo wörld ✓".as_bytes() {
            intake.write_all(&[*byte]).unwrap();
        }
        let doc = intake.finish(at(1)).unwrap();
        assert_eq!(
            part(&dir, "chat-aaaa-1", &doc.id, 1, 100).unwrap(),
            "héllo wörld ✓"
        );
    }
}
