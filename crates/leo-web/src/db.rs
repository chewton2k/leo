use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use rusqlite::Connection;

pub const FILE: &str = "leo.db";
pub const BEFORE: &str = ".before-database";
const VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dictations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    text TEXT NOT NULL,
    edited INTEGER NOT NULL DEFAULT 0,
    original TEXT
);
CREATE TABLE IF NOT EXISTS interviews (
    id TEXT PRIMARY KEY,
    updated TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS interviews_by_time ON interviews(updated);
CREATE TABLE IF NOT EXISTS interview_keys (
    interview TEXT NOT NULL,
    challenge TEXT NOT NULL,
    status TEXT NOT NULL,
    updated TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (interview, challenge)
);
CREATE TABLE IF NOT EXISTS interview_problem_undo (
    id TEXT PRIMARY KEY,
    interview TEXT NOT NULL,
    updated TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS interview_problem_undo_by_time ON interview_problem_undo(updated);
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    mode TEXT NOT NULL,
    refs TEXT NOT NULL,
    messages TEXT NOT NULL,
    named INTEGER NOT NULL,
    about TEXT NOT NULL,
    memory TEXT,
    count INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chats_by_time ON chats(updated_at);
CREATE VIRTUAL TABLE IF NOT EXISTS chats_text USING fts5(id UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS chat_docs (
    id TEXT PRIMARY KEY,
    chat TEXT NOT NULL,
    name TEXT NOT NULL,
    added_at TEXT NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_docs_by_chat ON chat_docs(chat, added_at);
CREATE TABLE IF NOT EXISTS reviewed (
    key TEXT PRIMARY KEY,
    n INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS captions (
    key TEXT PRIMARY KEY,
    caption TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS vectors (
    note TEXT NOT NULL,
    at INTEGER NOT NULL,
    hash TEXT NOT NULL,
    v BLOB NOT NULL,
    piece TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (note, at)
);
CREATE TABLE IF NOT EXISTS graph_notes (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS graph_links (
    n INTEGER PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    updated TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS note_tasks (
    id TEXT PRIMARY KEY,
    note TEXT NOT NULL,
    text TEXT NOT NULL,
    before TEXT NOT NULL,
    after TEXT NOT NULL,
    kind TEXT,
    promoted INTEGER NOT NULL,
    sent_at TEXT
);
CREATE TABLE IF NOT EXISTS calendar_feeds (
    feed TEXT PRIMARY KEY,
    fetched_at TEXT,
    tried_at TEXT NOT NULL,
    etag TEXT,
    modified TEXT,
    problem TEXT,
    body BLOB
);
CREATE TABLE IF NOT EXISTS calendar_prompted (
    uid TEXT NOT NULL,
    start TEXT NOT NULL,
    PRIMARY KEY (uid, start)
);
CREATE TABLE IF NOT EXISTS reading (
    note TEXT PRIMARY KEY,
    anchor TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reading_by_time ON reading(at);
CREATE TABLE IF NOT EXISTS graph_meta (
    key TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS margin_threads (
    id TEXT PRIMARY KEY,
    note TEXT NOT NULL,
    quote TEXT NOT NULL,
    prefix TEXT NOT NULL,
    suffix TEXT NOT NULL,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    messages TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS margin_threads_by_note ON margin_threads(note, status, created_at);
CREATE TABLE IF NOT EXISTS margin_modes (
    note TEXT PRIMARY KEY,
    mode TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS margin_personas (
    note TEXT PRIMARY KEY,
    persona TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS margin_instructions (
    note TEXT PRIMARY KEY,
    text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS writeup_pending (
    note TEXT PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS margin_state (
    key TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS timings (
    kind TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_days (
    day TEXT NOT NULL,
    purpose TEXT NOT NULL,
    model TEXT NOT NULL,
    calls INTEGER NOT NULL,
    input INTEGER NOT NULL,
    cached INTEGER NOT NULL,
    output INTEGER NOT NULL,
    PRIMARY KEY (day, purpose, model)
);
CREATE TABLE IF NOT EXISTS usage_calls (
    at INTEGER NOT NULL,
    purpose TEXT NOT NULL,
    model TEXT NOT NULL,
    failed INTEGER NOT NULL,
    unreported INTEGER NOT NULL,
    estimated INTEGER NOT NULL,
    ms INTEGER NOT NULL,
    input INTEGER NOT NULL,
    cached INTEGER NOT NULL,
    output INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_calls_by_time ON usage_calls(at);
CREATE TABLE IF NOT EXISTS usage_tools (
    at INTEGER NOT NULL,
    failed INTEGER NOT NULL,
    reused INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_tools_by_time ON usage_tools(at);
";

pub struct Db {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("could not open {}", path.display()))?;
        }
        let mut conn =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.execute_batch(SCHEMA)?;
        let tx = conn.transaction()?;
        let has_description: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('note_tasks') WHERE name = 'description')",
            [],
            |r| r.get(0),
        )?;
        if !has_description {
            tx.execute_batch(
                "ALTER TABLE note_tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';",
            )?;
        }
        let has_piece: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('vectors') WHERE name = 'piece')",
            [],
            |r| r.get(0),
        )?;
        if !has_piece {
            tx.execute_batch("ALTER TABLE vectors ADD COLUMN piece TEXT NOT NULL DEFAULT '';")?;
        }
        let has_edited: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('dictations') WHERE name = 'edited')",
            [],
            |r| r.get(0),
        )?;
        if !has_edited {
            tx.execute_batch(
                "ALTER TABLE dictations ADD COLUMN edited INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        let has_original: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('dictations') WHERE name = 'original')",
            [],
            |r| r.get(0),
        )?;
        if !has_original {
            tx.execute_batch("ALTER TABLE dictations ADD COLUMN original TEXT;")?;
        }
        let has_pinned: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('margin_threads') WHERE name = 'pinned')",
            [],
            |r| r.get(0),
        )?;
        if !has_pinned {
            tx.execute_batch(
                "ALTER TABLE margin_threads ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for end in ["", "-wal", "-shm"] {
                let mut name = path.as_os_str().to_owned();
                name.push(end);
                let _ = std::fs::set_permissions(name, std::fs::Permissions::from_mode(0o600));
            }
        }
        Ok(Db {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn with<R>(&self, work: impl FnOnce(&mut Connection) -> rusqlite::Result<R>) -> Result<R> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(work(&mut conn)?)
    }

    pub fn bytes(&self) -> u64 {
        ["", "-wal", "-shm"]
            .iter()
            .filter_map(|end| {
                let mut name = self.path.as_os_str().to_owned();
                name.push(end);
                std::fs::metadata(PathBuf::from(name)).ok()
            })
            .map(|m| m.len())
            .sum()
    }

    pub fn tidy(&self) -> Result<()> {
        self.with(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"))
    }
}

const MOST_OPEN: usize = 32;

static OPEN: OnceLock<Mutex<HashMap<PathBuf, Arc<Db>>>> = OnceLock::new();

pub fn at(path: &Path) -> Arc<Db> {
    try_at(path).unwrap_or_else(|e| panic!("{e:#}"))
}

pub fn try_at(path: &Path) -> Result<Arc<Db>> {
    let key = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let all = OPEN.get_or_init(Default::default);
    let cached = all
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned();
    if let Some(db) = &cached {
        if db.path.exists() {
            return Ok(Arc::clone(db));
        }
    }
    let db = Arc::new(
        Db::open(&key)
            .with_context(|| format!("leo could not open its database at {}", key.display()))?,
    );
    let mut held = all.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(current) = held.get(&key) {
        if cached.as_ref().is_none_or(|old| !Arc::ptr_eq(old, current)) {
            return Ok(Arc::clone(current));
        }
    }
    let mut removed = Vec::new();
    if let Some(old) = held.remove(&key) {
        removed.push(old);
    }
    if held.len() >= MOST_OPEN {
        let unused: Vec<_> = held
            .iter()
            .filter(|(_, db)| Arc::strong_count(db) == 1)
            .map(|(key, _)| key.clone())
            .collect();
        for key in unused {
            if let Some(old) = held.remove(&key) {
                removed.push(old);
            }
        }
    }
    if held.len() < MOST_OPEN {
        held.insert(key, Arc::clone(&db));
    }
    drop(held);
    drop(removed);
    Ok(db)
}

pub fn beside(file: &Path) -> Arc<Db> {
    at(&file.parent().unwrap_or(Path::new(".")).join(FILE))
}

pub fn for_chats(chats_dir: &Path) -> Arc<Db> {
    try_for_chats(chats_dir).unwrap_or_else(|e| panic!("{e:#}"))
}

pub fn data_dir(chats_dir: &Path) -> &Path {
    if chats_dir.file_name().is_some_and(|n| n == "chats") {
        chats_dir.parent().unwrap_or(chats_dir)
    } else {
        chats_dir
    }
}

pub fn try_for_chats(chats_dir: &Path) -> Result<Arc<Db>> {
    try_at(&data_dir(chats_dir).join(FILE))
}

pub fn put_aside(path: &Path) {
    let mut name = path.as_os_str().to_owned();
    name.push(BEFORE);
    let _ = std::fs::rename(path, PathBuf::from(name));
}

#[cfg(test)]
mod tests {
    #[test]
    fn opening_the_database_preserves_expired_retired_problem_recovery_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leo.db");
        let db = super::Db::open(&path).unwrap();
        db.with(|c| {
            c.execute("INSERT INTO interview_problem_undo VALUES ('old-undo', 'old-practice', '2000-01-01T00:00:00Z', '{\"kept\":true}')", [])?;
            Ok(())
        }).unwrap();
        drop(db);
        let reopened = super::Db::open(&path).unwrap();
        assert_eq!(
            reopened
                .with(|c| c.query_row(
                    "SELECT data FROM interview_problem_undo WHERE id = 'old-undo'",
                    [],
                    |r| r.get::<_, String>(0)
                ))
                .unwrap(),
            "{\"kept\":true}"
        );
    }

    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_database_and_its_journal_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let db = Db::open(&path).unwrap();
        for end in ["", "-wal", "-shm"] {
            let mut name = path.as_os_str().to_owned();
            name.push(end);
            let mode = std::fs::metadata(&name).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{end}");
        }
        drop(db);
    }

    #[test]
    fn database_cache_is_bounded_while_existing_users_keep_their_handles() {
        let temp = tempfile::tempdir().unwrap();
        let first = at(&temp.path().join("first/leo.db"));
        for index in 0..MOST_OPEN + 5 {
            let _ = at(&temp.path().join(format!("{index}/leo.db")));
        }
        assert!(OPEN.get().unwrap().lock().unwrap().len() <= MOST_OPEN);
        assert_eq!(
            first
                .with(|c| c.query_row("SELECT 1", [], |row| row.get::<_, i32>(0)))
                .unwrap(),
            1
        );
    }

    #[test]
    fn the_database_is_made_once_with_full_text_search_and_shared_by_everyone_who_asks() {
        let dir = tempfile::tempdir().unwrap();
        let one = at(&dir.path().join(FILE));
        let two = beside(&dir.path().join("graph.json"));
        assert!(Arc::ptr_eq(&one, &two));
        let three = for_chats(&dir.path().join("chats"));
        assert!(
            Arc::ptr_eq(&one, &three),
            "the chats folder's database is the one beside it"
        );
        one.with(|c| {
            c.execute("INSERT INTO chats_text (id, title, body) VALUES ('a', 'Heaps', 'a min-heap keeps the smallest on top')", [])?;
            let found: String = c.query_row("SELECT id FROM chats_text WHERE chats_text MATCH 'smallest'", [], |r| r.get(0))?;
            assert_eq!(found, "a");
            Ok(())
        })
        .unwrap();
        assert!(one.bytes() > 0);
        let version: i64 = one
            .with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version, VERSION);
    }
}

#[cfg(test)]
mod moving_in {
    use crate::{captions, graph, vectors};

    #[test]
    fn json_files_from_before_the_database_are_brought_in_and_kept_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        std::fs::write(data.join("captions.json"), r#"{"k": "a heap drawing"}"#).unwrap();
        std::fs::write(
            data.join("graph.json"),
            serde_json::json!({ "notes": { "n1": { "hash": "h", "summary": "heaps", "concepts": ["heap"] } }, "links": [], "built_at": "2026-10-01" }).to_string(),
        )
        .unwrap();
        std::fs::write(
            data.join("meaning.json"),
            r#"{"notes": {"n1": {"hash": "h", "pieces": [[0, "AACAPwAAAAA="]]}}}"#,
        )
        .unwrap();

        let seen = captions::Captions::for_notes(&data.join("notes"));
        assert_eq!(seen.get("k").as_deref(), Some("a heap drawing"));
        assert!(data.join("captions.json.before-database").is_file());

        let graphs = graph::Graphs::for_notes(&data.join("notes"), None);
        let cache = graphs.load();
        assert_eq!(cache.notes["n1"].summary, "heaps");
        assert_eq!(cache.built_at.as_deref(), Some("2026-10-01"));
        assert!(data.join("graph.json.before-database").is_file());

        let index = vectors::Vectors::for_notes(&data.join("notes"));
        assert_eq!(index.len(), 1);
        assert!(data.join("meaning.json.before-database").is_file());

        assert!(
            data.join(super::FILE).is_file(),
            "everything lives in one database"
        );
    }
}

#[cfg(test)]
mod task_descriptions {
    use super::*;

    #[test]
    fn existing_vectors_gain_an_empty_piece_mark_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE vectors (note TEXT NOT NULL, at INTEGER NOT NULL, hash TEXT NOT NULL, v BLOB NOT NULL, PRIMARY KEY (note, at));
             INSERT INTO vectors VALUES ('note-one', 0, 'abc', x'00');",
        )
        .unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, String) =
                    c.query_row("SELECT hash, piece FROM vectors", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?;
                assert_eq!(row, ("abc".into(), "".into()));
                Ok(())
            })
            .unwrap();
        }
    }

    #[test]
    fn existing_checkbox_settings_gain_empty_descriptions_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(SCHEMA).unwrap();
        old.execute(
            "INSERT INTO note_tasks VALUES ('task-one', 'note-one', 'Read', '', '', NULL, 1, NULL)",
            [],
        )
        .unwrap();
        old.pragma_update(None, "user_version", 1).unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, String, i64) = c.query_row(
                    "SELECT text, description, promoted FROM note_tasks WHERE id = 'task-one'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                assert_eq!(row, ("Read".into(), "".into(), 1));
                Ok(())
            })
            .unwrap();
        }
    }
}
