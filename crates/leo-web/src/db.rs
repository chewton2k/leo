use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, ErrorCode};

pub const FILE: &str = "leo.db";
pub const BEFORE: &str = ".before-database";
const VERSION: i64 = 6;

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
    pinned INTEGER NOT NULL DEFAULT 0,
    reworded INTEGER NOT NULL DEFAULT 0,
    closed_by TEXT NOT NULL DEFAULT 'felix'
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
CREATE TABLE IF NOT EXISTS board_undos (
    id TEXT PRIMARY KEY,
    board TEXT NOT NULL,
    at TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS board_undos_by_board ON board_undos(board);
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
        match header(path).with_context(|| format!("could not read {}", path.display()))? {
            Header::Damaged => return Err(anyhow::Error::new(Damaged)),
            Header::Version(n) if i64::from(n) > VERSION => return Err(newer(n.into())),
            _ => {}
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
        let mut conn = Connection::open(path)
            .map_err(classify)
            .with_context(|| format!("could not open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(classify)?;
        let stored: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(classify)?;
        if stored > VERSION {
            drop(conn);
            return Err(newer(stored));
        }
        set_up(&mut conn, stored).map_err(classify)?;
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

enum Header {
    New,
    Damaged,
    Version(u32),
}

fn header(path: &Path) -> std::io::Result<Header> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Header::New),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::with_capacity(100);
    file.take(100).read_to_end(&mut bytes)?;
    Ok(if bytes.is_empty() {
        Header::New
    } else if bytes.len() < 100 || !bytes.starts_with(b"SQLite format 3\0") {
        Header::Damaged
    } else {
        Header::Version(u32::from_be_bytes([
            bytes[60], bytes[61], bytes[62], bytes[63],
        ]))
    })
}

#[derive(Debug)]
pub struct Damaged;

impl std::fmt::Display for Damaged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the file is not a database")
    }
}

impl std::error::Error for Damaged {}

fn newer(stored: i64) -> anyhow::Error {
    anyhow!("{FILE} was written by a newer version of leo (version {stored}; this leo reads up to {VERSION}). Update leo to open it.")
}

fn classify(e: rusqlite::Error) -> anyhow::Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == ErrorCode::NotADatabase || f.code == ErrorCode::DatabaseCorrupt =>
        {
            anyhow::Error::new(Damaged)
        }
        rusqlite::Error::SqlInputError { msg, .. } => {
            anyhow!("its tables do not match this version of leo ({msg})")
        }
        e => e.into(),
    }
}

fn set_up(conn: &mut Connection, stored: i64) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    add_column(&tx, "note_tasks", "description", "TEXT NOT NULL DEFAULT ''")?;
    add_column(&tx, "vectors", "piece", "TEXT NOT NULL DEFAULT ''")?;
    add_column(&tx, "dictations", "edited", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(&tx, "dictations", "original", "TEXT")?;
    add_column(
        &tx,
        "margin_threads",
        "pinned",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        &tx,
        "margin_threads",
        "reworded",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        &tx,
        "margin_threads",
        "closed_by",
        "TEXT NOT NULL DEFAULT 'felix'",
    )?;
    tx.pragma_update(None, "user_version", VERSION.max(stored))?;
    tx.commit()
}

fn add_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        [table, column],
        |r| r.get(0),
    )?;
    if !present {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))?;
    }
    Ok(())
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
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    let current = all
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned();
    if let Some(current) = current {
        let fresh = cached
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &current));
        if fresh && current.path.exists() {
            return Ok(current);
        }
    }
    let db = match Db::open(&key) {
        Err(e) if e.downcast_ref::<Damaged>().is_some() => {
            put_damaged_aside(&key)?;
            Db::open(&key)
        }
        opened => opened,
    }
    .with_context(|| format!("leo could not open its database at {}", key.display()))?;
    let db = Arc::new(db);
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

static OPENING: Mutex<()> = Mutex::new(());

static SET_ASIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_aside() -> Option<PathBuf> {
    SET_ASIDE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn put_damaged_aside(path: &Path) -> Result<()> {
    let named = |base: &std::ffi::OsStr, end: &str| {
        let mut name = base.to_owned();
        name.push(end);
        PathBuf::from(name)
    };
    let mut stamp = path.as_os_str().to_owned();
    stamp.push(format!(
        ".damaged-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    let ends = ["", "-wal", "-shm"];
    let base = (1..)
        .map(|n| {
            if n == 1 {
                stamp.clone()
            } else {
                named(&stamp, &format!("-{n}")).into_os_string()
            }
        })
        .find(|base| ends.iter().all(|end| !named(base, end).exists()))
        .unwrap_or(stamp);
    for end in ends {
        let from = named(path.as_os_str(), end);
        if from.exists() {
            let to = named(&base, end);
            std::fs::rename(&from, &to).with_context(|| {
                format!(
                    "leo could not set its damaged database aside at {}",
                    to.display()
                )
            })?;
        }
    }
    *SET_ASIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(PathBuf::from(base));
    Ok(())
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
mod opening_trouble {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn future_db(path: &Path, matching: bool) {
        if matching {
            drop(Db::open(path).unwrap());
        }
        let conn = Connection::open(path).unwrap();
        if matching {
            conn.execute_batch("ALTER TABLE chats ADD COLUMN pinned_in_v9 INTEGER NOT NULL DEFAULT 0; CREATE TABLE added_in_v9 (x TEXT);").unwrap();
        } else {
            conn.execute_batch(
                "CREATE TABLE chats (id TEXT PRIMARY KEY, body TEXT); CREATE TABLE added_in_v9 (x TEXT);",
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 9).unwrap();
    }

    fn tables(path: &Path) -> Vec<String> {
        let conn = Connection::open(path).unwrap();
        let mut found = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        found
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect()
    }

    fn user_version(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    fn with_end(path: &Path, end: &str) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(end);
        PathBuf::from(name)
    }

    fn damaged_beside(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("leo.db.damaged-"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_damaged_database_does_not_stop_leo() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let small = tempfile::tempdir().unwrap();
        let path = small.path().join(FILE);
        std::fs::write(&path, b"not a database!!").unwrap();
        let db = try_at(&path).unwrap();
        let aside = damaged_beside(small.path());
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(set_aside(), Some(small.path().join(&aside[0])));
        assert_eq!(
            std::fs::read(small.path().join(&aside[0])).unwrap(),
            b"not a database!!"
        );
        assert_eq!(user_version(db.path()), VERSION);
        drop(db);

        let large = tempfile::tempdir().unwrap();
        let path = large.path().join(FILE);
        std::fs::write(&path, vec![0xa5u8; 8192]).unwrap();
        std::fs::write(with_end(&path, "-wal"), b"stale").unwrap();
        let db = try_at(&path).unwrap();
        let aside = damaged_beside(large.path());
        assert_eq!(aside.len(), 2, "{aside:?}");
        let main = large.path().join(&aside[0]);
        assert_eq!(set_aside(), Some(main.clone()));
        assert_eq!(std::fs::read(&main).unwrap(), vec![0xa5u8; 8192]);
        assert_eq!(std::fs::read(with_end(&main, "-wal")).unwrap(), b"stale");
        assert_eq!(user_version(db.path()), VERSION);
    }

    #[test]
    fn two_openers_of_one_damaged_database_set_it_aside_once() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, vec![0xa5u8; 8192]).unwrap();
        let start = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    start.wait();
                    try_at(&path).unwrap();
                });
            }
        });
        let aside = damaged_beside(dir.path());
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(
            std::fs::read(dir.path().join(&aside[0])).unwrap(),
            vec![0xa5u8; 8192]
        );
    }

    #[test]
    fn corruption_found_after_the_version_read_is_set_aside() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        drop(Db::open(&path).unwrap());
        let mut bytes = std::fs::read(&path).unwrap();
        let size = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
        assert_eq!(bytes[100], 0x05, "the schema spans more than one page");
        let last = u32::from_be_bytes([bytes[108], bytes[109], bytes[110], bytes[111]]);
        let start = (usize::try_from(last).unwrap() - 1) * size;
        bytes[start..start + size].fill(0xa5);
        std::fs::write(&path, &bytes).unwrap();
        let db = try_at(&path).unwrap();
        let aside = damaged_beside(dir.path());
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(set_aside(), Some(dir.path().join(&aside[0])));
        assert_eq!(user_version(db.path()), VERSION);
    }

    #[test]
    fn a_database_from_a_newer_leo_is_left_untouched() {
        for matching in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE);
            future_db(&path, matching);
            std::fs::write(with_end(&path, "-wal"), b"from the newer leo").unwrap();
            let before = std::fs::read(&path).unwrap();
            let said = format!("{:#}", try_at(&path).err().unwrap());
            assert!(said.contains("newer version"), "{said}");
            assert!(
                said.contains(&format!("version 9; this leo reads up to {VERSION}")),
                "{said}"
            );
            assert!(said.chars().count() < 300, "{said}");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(
                std::fs::read(with_end(&path, "-wal")).unwrap(),
                b"from the newer leo"
            );
            assert!(!with_end(&path, "-shm").exists());
            assert!(damaged_beside(dir.path()).is_empty());
        }
    }

    #[test]
    fn a_clashing_schema_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch("CREATE TABLE chats (id TEXT PRIMARY KEY, data TEXT NOT NULL);")
            .unwrap();
        old.pragma_update(None, "user_version", VERSION).unwrap();
        drop(old);
        let before = tables(&path);
        let said = format!("{:#}", try_at(&path).err().unwrap());
        assert!(said.contains("its tables do not match"), "{said}");
        assert!(!said.contains("CREATE"), "{said}");
        assert_eq!(tables(&path), before);
        assert_eq!(user_version(&path), VERSION);
        assert!(damaged_beside(dir.path()).is_empty());
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

    #[test]
    fn a_database_at_version_4_gains_reworded_and_keeps_its_margin_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE margin_threads (id TEXT PRIMARY KEY, note TEXT NOT NULL, quote TEXT NOT NULL, prefix TEXT NOT NULL, suffix TEXT NOT NULL, status TEXT NOT NULL, source TEXT NOT NULL, messages TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0);
             INSERT INTO margin_threads VALUES ('t1', 'n1', 'a quote', 'p', 's', 'open', 'ask', '[]', '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z', 1);",
        )
        .unwrap();
        old.pragma_update(None, "user_version", 4).unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, bool, bool) = c.query_row(
                    "SELECT quote, pinned, reworded FROM margin_threads WHERE id = 't1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                assert_eq!(row, ("a quote".into(), true, false));
                let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(version, VERSION);
                Ok(())
            })
            .unwrap();
        }
    }

    #[test]
    fn a_database_at_version_5_gains_closed_by_with_felix_in_old_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE margin_threads (id TEXT PRIMARY KEY, note TEXT NOT NULL, quote TEXT NOT NULL, prefix TEXT NOT NULL, suffix TEXT NOT NULL, status TEXT NOT NULL, source TEXT NOT NULL, messages TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, reworded INTEGER NOT NULL DEFAULT 0);
             INSERT INTO margin_threads VALUES ('t1', 'n1', 'a quote', 'p', 's', 'resolved', 'read', '[]', '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z', 0, 1);",
        )
        .unwrap();
        old.pragma_update(None, "user_version", 5).unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, String, bool, String) = c.query_row(
                    "SELECT quote, status, reworded, closed_by FROM margin_threads WHERE id = 't1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
                assert_eq!(
                    row,
                    ("a quote".into(), "resolved".into(), true, "felix".into())
                );
                let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(version, 6);
                Ok(())
            })
            .unwrap();
        }
    }
}
