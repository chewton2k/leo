use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, ErrorCode};

pub const FILE: &str = leo_core::paths::own::DATABASE;
pub const BEFORE: &str = leo_core::paths::own::BEFORE;
const VERSION: i64 = 7;

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
CREATE TABLE IF NOT EXISTS chat_messages (
    id INTEGER PRIMARY KEY,
    chat TEXT NOT NULL,
    n INTEGER NOT NULL,
    message TEXT NOT NULL,
    UNIQUE (chat, n)
);
CREATE VIRTUAL TABLE IF NOT EXISTS chat_words USING fts5(body, content = '', contentless_delete = 1, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS chat_docs (
    id TEXT PRIMARY KEY,
    chat TEXT NOT NULL,
    name TEXT NOT NULL,
    added_at TEXT NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_docs_by_chat ON chat_docs(chat, added_at);
CREATE TABLE IF NOT EXISTS chat_doc_parts (
    doc TEXT NOT NULL,
    n INTEGER NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (doc, n)
) WITHOUT ROWID;
CREATE TRIGGER IF NOT EXISTS chat_docs_take_their_parts AFTER DELETE ON chat_docs BEGIN
    DELETE FROM chat_doc_parts WHERE doc = old.id;
END;
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
CREATE TABLE IF NOT EXISTS unopened_notes (
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

pub(crate) const INDEX_TABLES: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS notes_text USING fts5(body, content = '', contentless_delete = 1, tokenize = 'trigram case_sensitive 1');
CREATE TABLE IF NOT EXISTS notes_indexed (n INTEGER PRIMARY KEY, note TEXT NOT NULL, hash TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS notes_indexed_by_note ON notes_indexed(note);
CREATE TABLE IF NOT EXISTS notes_whole (note TEXT PRIMARY KEY, hash TEXT NOT NULL);
";

pub struct Db {
    path: PathBuf,
    conn: Mutex<Option<Connection>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rewritten {
    Done,
    NotNeeded,
    Changed,
    Stopped,
}

fn named(path: &Path, end: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(end);
    PathBuf::from(name)
}

fn remove_with_logs(path: &Path) {
    for end in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(named(path, end));
    }
}

fn put_back_finished_rewrite(path: &Path) -> Result<()> {
    if rewriting(path) {
        return Ok(());
    }
    let (copy, ready) = rewrite_files(path);
    if !path.exists() && ready.exists() {
        std::fs::rename(&ready, path).with_context(|| {
            format!(
                "leo could not put its rewritten database at {}",
                path.display()
            )
        })?;
        sync_folder(path);
    }
    remove_with_logs(&ready);
    remove_with_logs(&copy);
    Ok(())
}

fn sync_folder(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let _ = std::fs::File::open(parent).and_then(|f| f.sync_all());
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn connect(path: &Path) -> Result<Connection> {
    #[cfg(test)]
    if hooks::fails("connect") {
        anyhow::bail!("leo's database could not be opened");
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
            let _ =
                std::fs::set_permissions(named(path, end), std::fs::Permissions::from_mode(0o600));
        }
    }
    Ok(conn)
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        put_back_finished_rewrite(path)?;
        let conn = connect(path)?;
        Ok(Db {
            path: path.to_path_buf(),
            conn: Mutex::new(Some(conn)),
        })
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn with<R>(&self, work: impl FnOnce(&mut Connection) -> rusqlite::Result<R>) -> Result<R> {
        let mut held = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if held.is_none() {
            *held = Some(connect(&self.path).context("leo's database could not be opened")?);
        }
        let conn = held.as_mut().expect("a connection was just put here");
        Ok(work(conn)?)
    }

    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        ["", "-wal", "-shm"]
            .iter()
            .filter_map(|end| std::fs::metadata(named(&self.path, end)).ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn give_back(&self) -> Result<bool> {
        let (mode, mut free): (i64, i64) = self.with(|c| {
            Ok((
                c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?,
                c.query_row("PRAGMA freelist_count", [], |r| r.get(0))?,
            ))
        })?;
        if mode != 2 || free < GIVE_BACK_PAGES {
            return Ok(false);
        }
        while free > 0 {
            let now = self.step_back()?;
            if now >= free {
                break;
            }
            free = now;
        }
        self.checkpoint()?;
        Ok(true)
    }

    fn step_back(&self) -> Result<i64> {
        self.with(|c| {
            let mut step = c.prepare(&format!("PRAGMA incremental_vacuum({GIVE_BACK_STEP})"))?;
            let mut rows = step.query([])?;
            while rows.next()?.is_some() {}
            drop(rows);
            c.query_row("PRAGMA freelist_count", [], |r| r.get(0))
        })
    }

    fn checkpoint(&self) -> Result<bool> {
        self.with(|c| {
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .map(|busy| busy != 0)
    }

    pub fn rewrite(&self, stop: &AtomicBool) -> Result<Rewritten> {
        let (mode, free, pages): (i64, i64, i64) = self.with(|c| {
            Ok((
                c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?,
                c.query_row("PRAGMA freelist_count", [], |r| r.get(0))?,
                c.query_row("PRAGMA page_count", [], |r| r.get(0))?,
            ))
        })?;
        if mode != 0 || (free * 4 < pages && pages >= 256) {
            return Ok(Rewritten::NotNeeded);
        }
        if !one_handle(&self.path) {
            return Ok(Rewritten::Stopped);
        }
        let _running = Running::start(&self.path);
        if self.checkpoint()? {
            return Ok(Rewritten::Changed);
        }
        if self.path.file_name() != Some(std::ffi::OsStr::new(FILE)) {
            return Ok(Rewritten::Stopped);
        }
        let (copy, ready) = rewrite_files(&self.path);
        remove_with_logs(&copy);
        remove_with_logs(&ready);
        let outcome = self.copy_and_swap(stop, &copy, &ready);
        if !matches!(outcome, Ok(Rewritten::Done)) {
            remove_with_logs(&copy);
            remove_with_logs(&ready);
        }
        outcome
    }

    fn copy_and_swap(&self, stop: &AtomicBool, copy: &Path, ready: &Path) -> Result<Rewritten> {
        private_empty(copy)?;
        let reader = Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        reader.busy_timeout(std::time::Duration::from_secs(10))?;
        reader.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        let version =
            |c: &Connection| c.query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0));
        let before = version(&reader)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(Rewritten::Stopped);
        }
        let interrupt = reader.get_interrupt_handle();
        let done = AtomicBool::new(false);
        let copied = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    if stop.load(Ordering::Relaxed) {
                        interrupt.interrupt();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            });
            let copied = reader.execute("VACUUM INTO ?1", [copy.to_string_lossy()]);
            done.store(true, Ordering::Relaxed);
            copied
        });
        if copied.is_err() || stop.load(Ordering::Relaxed) {
            return Ok(Rewritten::Stopped);
        }
        #[cfg(test)]
        hooks::reached("copied");
        let after = version(&reader)?;
        if after != before {
            return Ok(Rewritten::Changed);
        }
        if let Err(why) = sound_copy(&reader, copy) {
            leo_core::diag::warn(format!(
                "leo did not swap in its rewritten database: {why:#}"
            ));
            return Ok(Rewritten::Stopped);
        }
        for end in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(named(copy, end));
            anyhow::ensure!(!named(copy, end).exists(), "a log file of the copy stayed");
        }
        std::fs::rename(copy, ready)?;
        #[cfg(test)]
        hooks::reached("checked");
        let mut held = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(test)]
        hooks::reached("swap");
        if !one_handle(&self.path) {
            return Ok(Rewritten::Stopped);
        }
        if version(&reader)? != after {
            return Ok(Rewritten::Changed);
        }
        drop(reader);
        drop(held.take());
        let logs_left = ["-wal", "-shm"]
            .iter()
            .any(|end| named(&self.path, end).exists());
        #[cfg(test)]
        let refused = hooks::fails("rename");
        #[cfg(not(test))]
        let refused = false;
        if logs_left || refused || std::fs::rename(ready, &self.path).is_err() {
            *held = Some(connect(&self.path)?);
            return Ok(Rewritten::Stopped);
        }
        sync_folder(&self.path);
        #[cfg(test)]
        hooks::reached("renamed");
        *held = Some(connect(&self.path)?);
        Ok(Rewritten::Done)
    }
}

fn rewrite_files(path: &Path) -> (PathBuf, PathBuf) {
    use leo_core::paths::own::{DATABASE_REWRITE, DATABASE_REWRITE_READY};
    (
        path.with_file_name(DATABASE_REWRITE),
        path.with_file_name(DATABASE_REWRITE_READY),
    )
}

static REWRITING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static UNCACHED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn rewriting(path: &Path) -> bool {
    REWRITING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|p| p == path)
}

struct Running(PathBuf);

impl Running {
    fn start(path: &Path) -> Running {
        REWRITING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(path.to_path_buf());
        Running(path.to_path_buf())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = REWRITING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = running.iter().position(|p| *p == self.0) {
            running.swap_remove(at);
        }
    }
}

fn private_empty(path: &Path) -> Result<()> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    open.open(path)
        .with_context(|| format!("could not make {}", path.display()))?;
    Ok(())
}

fn sound_copy(reader: &Connection, copy: &Path) -> Result<()> {
    let checked = Connection::open_with_flags(copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let ok: String = checked.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    anyhow::ensure!(ok == "ok", "the copy failed its integrity check ({ok})");
    let mode: i64 = checked.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
    anyhow::ensure!(mode == 2, "the copy does not give space back");
    let user = |c: &Connection| c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0));
    anyhow::ensure!(
        user(&checked)? == user(reader)?,
        "the copy has another version"
    );
    let tables: Vec<String> = {
        let mut found = reader.prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND sql NOT LIKE 'CREATE VIRTUAL%'",
        )?;
        let rows = found.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for table in tables {
        let sql = format!("SELECT count(*) FROM \"{}\"", table.replace('"', "\"\""));
        let count = |c: &Connection| c.query_row(&sql, [], |r| r.get::<_, i64>(0));
        anyhow::ensure!(
            count(&checked)? == count(reader)?,
            "the copy lost rows of {table}"
        );
    }
    Ok(())
}

fn one_handle(path: &Path) -> bool {
    let Ok(this) = std::fs::canonicalize(path) else {
        return false;
    };
    let key = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let uncached = UNCACHED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if uncached
        .iter()
        .any(|other| *other == key || std::fs::canonicalize(other).is_ok_and(|o| o == this))
    {
        return false;
    }
    let cached: Vec<PathBuf> = OPEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    cached.contains(&key)
        && cached
            .iter()
            .filter(|other| **other != key)
            .all(|other| std::fs::canonicalize(other).map_or(true, |o| o != this))
}

#[cfg(test)]
pub(crate) mod hooks {
    use std::cell::RefCell;

    type Hook = Box<dyn FnMut(&str)>;

    thread_local! {
        static AT: RefCell<Option<Hook>> = const { RefCell::new(None) };
        static FAILING: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn on(hook: impl FnMut(&str) + 'static) {
        AT.with(|at| *at.borrow_mut() = Some(Box::new(hook)));
    }

    pub(crate) fn fail(what: &'static str) {
        FAILING.with(|f| f.borrow_mut().push(what));
    }

    pub(crate) fn clear() {
        AT.with(|at| *at.borrow_mut() = None);
        FAILING.with(|f| f.borrow_mut().clear());
    }

    pub(crate) fn reached(point: &str) {
        let taken = AT.with(|at| at.borrow_mut().take());
        if let Some(mut hook) = taken {
            hook(point);
            AT.with(|at| {
                let mut at = at.borrow_mut();
                if at.is_none() {
                    *at = Some(hook);
                }
            });
        }
    }

    pub(crate) fn fails(what: &str) -> bool {
        FAILING.with(|f| f.borrow().contains(&what))
    }

    pub(crate) struct Clear;

    impl Drop for Clear {
        fn drop(&mut self) {
            clear();
        }
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
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.execute_batch(INDEX_TABLES)?;
    add_column(&tx, "note_tasks", "description", "TEXT NOT NULL DEFAULT ''")?;
    add_column(&tx, "vectors", "piece", "TEXT NOT NULL DEFAULT ''")?;
    add_column(&tx, "dictations", "edited", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(&tx, "dictations", "original", "TEXT")?;
    add_column(&tx, "chats", "rows", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(&tx, "chats", "pinned", "INTEGER NOT NULL DEFAULT 0")?;
    if add_column(&tx, "chats", "notes", "TEXT NOT NULL DEFAULT '[]'")? {
        crate::chats::fill_notes(&tx)?;
    }
    add_column(&tx, "chat_docs", "chars", "INTEGER")?;
    add_column(&tx, "chat_docs", "bytes", "INTEGER")?;
    add_column(&tx, "chat_docs", "excerpt", "TEXT")?;
    add_column(&tx, "chat_docs", "cuts", "TEXT")?;
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
    crate::margin::set_up_held_rows(&tx)?;
    add_column(&tx, "held_note_rows", "digest", "TEXT")?;
    tx.pragma_update(None, "user_version", VERSION.max(stored))?;
    tx.commit()
}

fn add_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<bool> {
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
    Ok(!present)
}

const MOST_OPEN: usize = 32;
const GIVE_BACK_PAGES: i64 = 1_024;
const GIVE_BACK_STEP: i64 = 256;

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
    } else {
        let mut uncached = UNCACHED.lock().unwrap_or_else(|e| e.into_inner());
        if !uncached.contains(&key) {
            uncached.push(key);
        }
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
    use leo_core::paths::own;
    let stamp = own::damaged_stamp(chrono::Utc::now());
    let ends = ["", "-wal", "-shm"];
    let base = (1..)
        .map(|n| named(path, &own::damaged_end(&stamp, n)))
        .find(|base| ends.iter().all(|end| !named(base, end).exists()))
        .unwrap_or_else(|| named(path, &own::damaged_end(&stamp, 1)));
    for end in ends {
        let from = named(path, end);
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
    *SET_ASIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(base);
    Ok(())
}

pub fn beside(file: &Path) -> Arc<Db> {
    at(&file.parent().unwrap_or(Path::new(".")).join(FILE))
}

pub fn for_chats(chats_dir: &Path) -> Arc<Db> {
    try_for_chats(chats_dir).unwrap_or_else(|e| panic!("{e:#}"))
}

pub fn beside_of<'a>(dir: &'a Path, only_named: Option<&str>) -> &'a Path {
    if only_named.is_some_and(|name| dir.file_name().is_none_or(|n| n != name)) {
        dir
    } else {
        dir.parent().unwrap_or(dir)
    }
}

pub fn data_dir(chats_dir: &Path) -> &Path {
    beside_of(chats_dir, Some("chats"))
}

pub fn try_for_chats(chats_dir: &Path) -> Result<Arc<Db>> {
    try_at(&data_dir(chats_dir).join(FILE))
}

pub fn put_aside(path: &Path) {
    let aside = named(path, BEFORE);
    if std::fs::rename(path, &aside).is_ok() {
        let _ = std::fs::File::options()
            .write(true)
            .open(&aside)
            .and_then(|f| f.set_modified(std::time::SystemTime::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::{beside_of, data_dir};
    use std::path::Path;

    #[test]
    fn the_data_folder_is_the_parent_except_for_a_chats_folder_by_another_name() {
        assert_eq!(beside_of(Path::new("/d/notes"), None), Path::new("/d"));
        assert_eq!(
            beside_of(Path::new("/d/chats"), Some("chats")),
            Path::new("/d")
        );
        assert_eq!(
            beside_of(Path::new("/d/other"), Some("chats")),
            Path::new("/d/other")
        );
        assert_eq!(beside_of(Path::new("/"), None), Path::new("/"));
        assert_eq!(beside_of(Path::new("/"), Some("chats")), Path::new("/"));
    }

    #[test]
    fn every_helper_finds_the_same_folder_as_before_for_a_default_and_a_custom_data_folder() {
        for data in [
            "/home/u/.local/share/leo",
            "/mnt/my disk/leo notes data",
            "rel/leo",
            "/leo",
        ] {
            let data = Path::new(data);
            let notes = data.join("notes");
            let chats = data.join("chats");
            let old_parent = |p: &Path| p.parent().unwrap_or(p).to_path_buf();
            assert_eq!(crate::keeping::data_dir(&notes), old_parent(&notes));
            assert_eq!(
                crate::chats::dir_for(&notes),
                old_parent(&notes).join("chats")
            );
            assert_eq!(
                crate::calendar::settings_path(&notes),
                old_parent(&notes).join("calendars.json")
            );
            assert_eq!(data_dir(&chats), old_parent(&chats));
            assert_eq!(data_dir(&notes), notes);
            assert_eq!(beside_of(&chats, None), old_parent(&chats));
        }
        assert_eq!(data_dir(Path::new("/")), Path::new("/"));
        assert_eq!(crate::keeping::data_dir(Path::new("/")), Path::new("/"));
    }

    #[test]
    fn the_stores_and_files_kept_beside_the_notes_land_in_the_same_folder_as_before() {
        let tmp = tempfile::tempdir().unwrap();
        for data in [tmp.path().join("leo"), tmp.path().join("my custom data")] {
            let notes = data.join("notes");
            std::fs::create_dir_all(&notes).unwrap();
            assert_eq!(
                crate::graph::Graphs::for_notes(&notes, None).path(),
                data.join("graph.json")
            );
            let _ = crate::vectors::Vectors::for_notes(&notes);
            let _ = crate::captions::Captions::for_notes(&notes);
            assert!(data.join(FILE).is_file(), "{data:?}");
            assert!(!notes.join(FILE).exists());
            assert_eq!(
                crate::jsonline::checked_dir(&data.join("chats"), "cards").unwrap(),
                data.join("notes/.leo/cards")
            );
        }
    }

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
        std::fs::write(named(&path, "-wal"), b"stale").unwrap();
        let db = try_at(&path).unwrap();
        let aside = damaged_beside(large.path());
        assert_eq!(aside.len(), 2, "{aside:?}");
        let main = large.path().join(&aside[0]);
        assert_eq!(set_aside(), Some(main.clone()));
        assert_eq!(std::fs::read(&main).unwrap(), vec![0xa5u8; 8192]);
        assert_eq!(std::fs::read(named(&main, "-wal")).unwrap(), b"stale");
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
            std::fs::write(named(&path, "-wal"), b"from the newer leo").unwrap();
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
                std::fs::read(named(&path, "-wal")).unwrap(),
                b"from the newer leo"
            );
            assert!(!named(&path, "-shm").exists());
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
    fn existing_chat_documents_gain_their_facts_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE chat_docs (id TEXT PRIMARY KEY, chat TEXT NOT NULL, name TEXT NOT NULL, added_at TEXT NOT NULL, text TEXT NOT NULL);
             INSERT INTO chat_docs VALUES ('doc-one', 'chat-one', 'w.pdf', '2026-10-08T01:00:00Z', 'Heaps keep the minimum.');",
        )
        .unwrap();
        old.pragma_update(None, "user_version", VERSION).unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, String, Option<i64>, Option<i64>, Option<String>) = c.query_row(
                    "SELECT name, text, chars, bytes, excerpt FROM chat_docs",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?;
                assert_eq!(
                    row,
                    (
                        "w.pdf".into(),
                        "Heaps keep the minimum.".into(),
                        None,
                        None,
                        None
                    )
                );
                let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(version, VERSION);
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
                assert_eq!(version, VERSION);
                Ok(())
            })
            .unwrap();
        }
    }
}

#[cfg(test)]
mod chat_pins {
    use super::*;

    #[test]
    fn existing_chats_gain_a_pinned_mark_and_keep_their_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        drop(Db::open(&path).unwrap());
        let old = Connection::open(&path).unwrap();
        old.execute_batch("ALTER TABLE chats DROP COLUMN pinned;")
            .unwrap();
        old.execute(
            "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at, rows)
             VALUES ('chat-pin-000001', 'Heaps', 'chat', '[]', '[]', 1, 'about heaps', NULL, 0, '2026-11-01T00:00:00.000000Z', '2026-11-02T00:00:00.000000Z', 1)",
            [],
        )
        .unwrap();
        drop(old);
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                let row: (String, i64, String) = c.query_row(
                    "SELECT title, pinned, updated_at FROM chats WHERE id = 'chat-pin-000001'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                assert_eq!(
                    row,
                    ("Heaps".into(), 0, "2026-11-02T00:00:00.000000Z".into())
                );
                let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(version, VERSION);
                Ok(())
            })
            .unwrap();
        }
        let chats = dir.path().join("chats");
        assert!(crate::chats::set_pinned(&chats, "chat-pin-000001", true));
        assert!(crate::chats::list(&chats)[0].pinned);
    }
}

#[cfg(test)]
mod chat_notes {
    use super::*;

    fn chat(c: &Connection, id: &str, rows: bool, messages: &str) {
        c.execute(
            "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at, rows)
             VALUES (?1, 'Heaps', 'chat', '[]', ?2, 1, '', NULL, 0, '2026-11-01T00:00:00.000000Z', '2026-11-02T00:00:00.000000Z', ?3)",
            rusqlite::params![id, messages, rows],
        )
        .unwrap();
    }

    fn row(c: &Connection, id: &str, n: i64, message: &str) {
        c.execute(
            "INSERT INTO chat_messages (chat, n, message) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, n, message],
        )
        .unwrap();
    }

    fn notes(db: &Db, id: &str) -> String {
        db.with(|c| c.query_row("SELECT notes FROM chats WHERE id = ?1", [id], |r| r.get(0)))
            .unwrap()
    }

    #[test]
    fn existing_chats_are_filled_once_from_both_storage_forms() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        drop(Db::open(&path).unwrap());
        let old = Connection::open(&path).unwrap();
        old.execute_batch("ALTER TABLE chats DROP COLUMN notes;")
            .unwrap();
        chat(&old, "chat-rows-0001", true, "[]");
        row(
            &old,
            "chat-rows-0001",
            0,
            r#"{"role":"user","text":"a","refs":[{"id":"n2","title":"T"},{"id":"n1","title":"T"}]}"#,
        );
        row(&old, "chat-rows-0001", 1, "{not json");
        row(
            &old,
            "chat-rows-0001",
            2,
            r#"{"role":"assistant","text":"b","refs":[{"id":"n-no"}]}"#,
        );
        row(
            &old,
            "chat-rows-0001",
            3,
            r#"{"role":"user","text":"c","selection":{"note":"n3","text":"x"},"refs":[{"id":"n1"}]}"#,
        );
        row(
            &old,
            "chat-rows-0001",
            4,
            r#"{"role":"user","text":"d","refs":"nope","selection":"nope"}"#,
        );
        chat(
            &old,
            "chat-older-001",
            false,
            r#"[{"role":"user","text":"a","refs":[{"id":"n9"}]},"stray",{"role":"user","selection":{"note":"n8"}}]"#,
        );
        chat(&old, "chat-older-002", false, "{broken");
        chat(&old, "chat-plain-001", true, "[]");
        drop(old);
        let db = Db::open(&path).unwrap();
        assert_eq!(notes(&db, "chat-rows-0001"), r#"["n1","n2","n3"]"#);
        assert_eq!(notes(&db, "chat-older-001"), r#"["n8","n9"]"#);
        assert_eq!(notes(&db, "chat-older-002"), "[]");
        assert_eq!(notes(&db, "chat-plain-001"), "[]");
        db.with(|c| c.execute("UPDATE chats SET notes = '[\"kept\"]'", []))
            .unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(notes(&db, "chat-rows-0001"), r#"["kept"]"#);
        assert_eq!(
            db.with(|c| c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)))
                .unwrap(),
            VERSION
        );
    }
}

#[cfg(test)]
mod chat_rows {
    use super::*;

    #[test]
    fn existing_chats_gain_a_rows_mark_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        drop(Db::open(&path).unwrap());
        let old = Connection::open(&path).unwrap();
        let messages = serde_json::json!([
            { "role": "user", "text": "quiz me on heaps" },
            { "role": "assistant", "text": "[[incorrect]] A heap keeps the smallest on top." }
        ])
        .to_string();
        old.execute_batch(
            "DROP TABLE chat_messages; DROP TABLE chat_words; ALTER TABLE chats DROP COLUMN rows;",
        )
        .unwrap();
        old.execute(
            "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at)
             VALUES ('chat-v6-000001', 'Heaps', 'chat', '[]', ?1, 1, 'about heaps', NULL, 2, '2026-11-01T00:00:00.000000Z', '2026-11-01T00:00:00.000000Z')",
            [&messages],
        )
        .unwrap();
        old.execute_batch(
            "INSERT INTO chats_text (id, title, body) VALUES ('chat-v6-000001', 'Heaps', 'quiz me on heaps');
             INSERT INTO reviewed (key, n) VALUES ('chat-v6-000001:00000000000000aa', 1);",
        )
        .unwrap();
        old.pragma_update(None, "user_version", 6).unwrap();
        drop(old);
        let chats = dir.path().join("chats");
        let chat = crate::chats::load(&chats, "chat-v6-000001").unwrap();
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.title, "Heaps");
        assert_eq!(crate::review::count(&chats), 1);
        assert_eq!(crate::chats::search(&chats, "heaps").len(), 1);
        assert_eq!(crate::chats::with_misses(&chats).len(), 1);
        assert_eq!(user_version(&path), VERSION);
        let rows: i64 = at(&path)
            .with(|c| c.query_row("SELECT rows FROM chats", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(rows, 0);
        let mut more = chat.messages.clone();
        more.push(serde_json::json!({ "role": "user", "text": "next" }));
        crate::chats::save(
            &chats,
            "chat-v6-000001",
            serde_json::from_value(serde_json::json!({ "messages": more })).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        let moved = crate::chats::load(&chats, "chat-v6-000001").unwrap();
        assert_eq!(moved.messages, more);
        assert_eq!(moved.title, "Heaps");
        assert_eq!(crate::chats::with_misses(&chats).len(), 1);
        assert_eq!(crate::review::count(&chats), 1);
    }

    fn user_version(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }
}

#[cfg(test)]
mod giving_back {
    use super::*;

    fn filled(path: &Path, rows: usize) -> Db {
        let db = Db::open(path).unwrap();
        db.with(|c| {
            let tx = c.transaction()?;
            tx.execute_batch("CREATE TABLE filler (id INTEGER PRIMARY KEY, body TEXT NOT NULL)")?;
            for n in 0..rows {
                tx.execute(
                    "INSERT INTO filler (id, body) VALUES (?1, ?2)",
                    rusqlite::params![n as i64, format!("{n:06}{}", "x".repeat(1_994))],
                )?;
            }
            tx.commit()?;
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        })
        .unwrap();
        db
    }

    fn delete_below(db: &Db, below: usize) {
        db.with(|c| {
            c.execute("DELETE FROM filler WHERE id < ?1", [below as i64])?;
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        })
        .unwrap();
    }

    fn sound(db: &Db) {
        let answer: String = db
            .with(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(answer, "ok");
    }

    fn kept(db: &Db) -> Vec<(i64, String)> {
        db.with(|c| {
            let mut rows = c.prepare("SELECT id, body FROM filler ORDER BY id")?;
            let found = rows.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            found.collect()
        })
        .unwrap()
    }

    #[test]
    fn a_new_database_is_made_with_incremental_vacuum() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join(FILE)).unwrap();
        let mode: i64 = db
            .with(|c| c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(mode, 2);
    }

    #[test]
    fn deleted_rows_give_their_space_back() {
        let dir = tempfile::tempdir().unwrap();
        let db = filled(&dir.path().join(FILE), 4_010);
        delete_below(&db, 4_000);
        let before = db.bytes();
        let left = kept(&db);
        assert_eq!(left.len(), 10);
        assert!(db.give_back().unwrap());
        let after = db.bytes();
        eprintln!("given back: {before} bytes before, {after} after");
        assert!(after < 1_000_000, "{after}");
        assert_eq!(kept(&db), left);
        sound(&db);
    }

    #[test]
    fn a_little_slack_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let db = filled(&dir.path().join(FILE), 2_000);
        delete_below(&db, 1_000);
        let before = db.bytes();
        assert!(!db.give_back().unwrap());
        assert_eq!(db.bytes(), before);
        sound(&db);
    }

    #[test]
    fn an_old_database_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE filler (id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let mode: i64 = db
            .with(|c| c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(mode, 0);
        db.with(|c| {
            for n in 0..4_000i64 {
                c.execute(
                    "INSERT INTO filler (id, body) VALUES (?1, ?2)",
                    rusqlite::params![n, "x".repeat(2_000)],
                )?;
            }
            Ok(())
        })
        .unwrap();
        delete_below(&db, 4_000);
        assert!(!db.give_back().unwrap());
        sound(&db);
    }

    #[test]
    fn saves_and_reads_carry_on_while_space_is_given_back() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(filled(&dir.path().join(FILE), 6_000));
        delete_below(&db, 5_990);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let (db, done) = (Arc::clone(&db), Arc::clone(&done));
            std::thread::spawn(move || {
                let mut n = 10_000i64;
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    db.with(|c| {
                        c.execute(
                            "INSERT INTO filler (id, body) VALUES (?1, ?2)",
                            rusqlite::params![n, format!("saved {n}")],
                        )?;
                        c.query_row("SELECT body FROM filler WHERE id = ?1", [n], |r| {
                            r.get::<_, String>(0)
                        })
                    })
                    .unwrap();
                    n += 1;
                }
                n - 10_000
            })
        };
        assert!(db.give_back().unwrap());
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        let saved = writer.join().unwrap();
        let rows = kept(&db);
        assert_eq!(rows.len(), 10 + saved as usize);
        assert!(rows[..10]
            .iter()
            .all(|(id, body)| body.starts_with(&format!("{id:06}"))));
        sound(&db);
    }

    #[test]
    #[ignore]
    fn audit_bench_give_back() {
        for megabytes in [50usize, 200, 1_000] {
            let dir = tempfile::tempdir().unwrap();
            let rows = megabytes * 500;
            let db = filled(&dir.path().join(FILE), rows);
            delete_below(&db, rows / 2);
            let before = db.bytes();
            let started = std::time::Instant::now();
            let mut longest = 0f64;
            let mut steps = 0;
            let mut free = i64::MAX;
            loop {
                let at = std::time::Instant::now();
                let now = db.step_back().unwrap();
                longest = longest.max(at.elapsed().as_secs_f64() * 1e3);
                steps += 1;
                if now == 0 || now >= free {
                    break;
                }
                free = now;
            }
            let busy = db.checkpoint().unwrap();
            let total = started.elapsed().as_secs_f64() * 1e3;
            sound(&db);
            eprintln!(
                "give back {megabytes} MB: {before} -> {} bytes, {steps} steps, longest step {longest:.2} ms, whole {total:.0} ms, checkpoint busy {busy}",
                db.bytes()
            );
            assert!(!busy);
        }
    }
}

#[cfg(test)]
mod rewriting {
    use super::*;

    fn old_file(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE t (x TEXT); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000) INSERT INTO t SELECT hex(randomblob(500)) FROM n; DELETE FROM t WHERE rowid % 2 = 0;",
        )
        .unwrap();
    }

    fn mode(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
            .unwrap()
    }

    fn exists(path: &Path, end: &str) -> bool {
        named(path, end).exists()
    }

    fn old_leo(dir: &Path) -> (PathBuf, Arc<Db>) {
        let path = dir.join(FILE);
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch("CREATE TABLE made_before (x TEXT); DROP TABLE made_before;")
            .unwrap();
        drop(raw);
        let db = at(&path);
        db.with(|c| {
            let tx = c.transaction()?;
            for i in 0..5_000 {
                tx.execute(
                    "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at)
                     VALUES (?1, 'Heaps', 'chat', '[]', ?2, 0, '', NULL, 0, '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z')",
                    rusqlite::params![format!("chat-old-{i:05}"), "x".repeat(400)],
                )?;
            }
            tx.execute(
                "INSERT INTO chats_text (id, title, body) VALUES ('chat-old-04999', 'Heaps', 'a min-heap keeps the smallest on top')",
                [],
            )?;
            tx.execute(
                "INSERT INTO vectors (note, at, hash, v) VALUES ('n1', 0, 'h', ?1)",
                [vec![7u8; 1536]],
            )?;
            tx.execute(
                "INSERT INTO dictations (at, text) VALUES ('2026-11-01T00:00:00Z', 'kept dictation')",
                [],
            )?;
            tx.execute("DELETE FROM chats WHERE id < 'chat-old-04000'", [])?;
            tx.commit()
        })
        .unwrap();
        assert_eq!(mode(&path), 0);
        (path, db)
    }

    fn quiet() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn rows(db: &Db, table: &str) -> i64 {
        db.with(|c| c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)))
            .unwrap()
    }

    fn no_leftovers(path: &Path) {
        let (copy, ready) = rewrite_files(path);
        for file in [copy, ready] {
            for log in ["", "-wal", "-shm"] {
                assert!(!exists(&file, log), "{}{log} was left", file.display());
            }
        }
    }

    #[test]
    fn an_old_database_is_rewritten_and_keeps_every_row() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        db.with(|c| {
            c.execute(
                "UPDATE chats SET pinned = 1 WHERE id = 'chat-old-04999'",
                [],
            )
        })
        .unwrap();
        let before = std::fs::metadata(&path).unwrap().len()
            + std::fs::metadata(named(&path, "-wal")).map_or(0, |m| m.len());
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Done);
        assert_eq!(mode(&path), 2);
        assert_eq!(rows(&db, "chats"), 1_000);
        assert_eq!(rows(&db, "chats WHERE pinned = 1"), 1);
        assert_eq!(rows(&db, "vectors"), 1);
        assert_eq!(rows(&db, "dictations"), 1);
        let found: String = db
            .with(|c| {
                c.query_row(
                    "SELECT id FROM chats_text WHERE chats_text MATCH 'smallest'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(found, "chat-old-04999");
        let version: i64 = db
            .with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version, VERSION);
        let after = std::fs::metadata(&path).unwrap().len();
        assert!(after < 1024 * 1024 && after < before, "{before} -> {after}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        no_leftovers(&path);
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::NotNeeded);
    }

    #[test]
    fn a_new_database_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let db = at(&dir.path().join(FILE));
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::NotNeeded);
    }

    #[test]
    fn holders_of_the_handle_keep_working_after_the_swap() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let chats = dir.path().join("chats");
        crate::chats::save(
            &chats,
            "chat-holder-01",
            serde_json::from_value(
                serde_json::json!({ "messages": [{ "role": "user", "text": "before" }] }),
            )
            .unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Done);
        let again = at(&path);
        assert!(Arc::ptr_eq(&again, &db));
        crate::dictation::remember(dir.path(), "after the swap").unwrap();
        assert_eq!(
            crate::chats::load(&chats, "chat-holder-01")
                .unwrap()
                .messages
                .len(),
            1
        );
        assert_eq!(rows(&db, "dictations"), 2);
        drop(db);
        drop(again);
        let fresh = Connection::open(&path).unwrap();
        let n: i64 = fresh
            .query_row("SELECT count(*) FROM dictations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn the_desktop_paths_reach_the_cached_handle() {
        let dir = tempfile::tempdir().unwrap();
        crate::dictation::remember(dir.path(), "hello").unwrap();
        assert_eq!(crate::dictation::recent(dir.path()).unwrap().len(), 1);
        let one = try_at(&dir.path().join(FILE)).unwrap();
        let two = try_at(&dir.path().join(FILE)).unwrap();
        assert!(Arc::ptr_eq(&one, &two));
        assert!(one_handle(&dir.path().join(FILE)));
    }

    #[test]
    fn a_save_during_the_copy_cancels_the_swap() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let writer = Arc::clone(&db);
        hooks::on(move |point| {
            if point == "copied" {
                writer
                    .with(|c| {
                        c.execute(
                            "INSERT INTO dictations (at, text) VALUES ('t', 'during the copy')",
                            [],
                        )
                    })
                    .unwrap();
            }
        });
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Changed);
        assert_eq!(mode(&path), 0);
        assert_eq!(rows(&db, "dictations"), 2);
        no_leftovers(&path);
    }

    #[test]
    fn a_save_just_before_the_swap_cancels_it() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let writer = Arc::clone(&db);
        hooks::on(move |point| {
            if point == "checked" {
                writer
                    .with(|c| {
                        c.execute(
                            "INSERT INTO dictations (at, text) VALUES ('t', 'just before')",
                            [],
                        )
                    })
                    .unwrap();
            }
        });
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Changed);
        assert_eq!(mode(&path), 0);
        assert_eq!(rows(&db, "dictations"), 2);
        no_leftovers(&path);
    }

    #[test]
    fn a_writer_waits_for_the_swap_and_lands_in_the_new_file() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let data = dir.path().to_path_buf();
        let go = Arc::new(AtomicBool::new(false));
        let started = Arc::clone(&go);
        let writer = std::thread::spawn(move || {
            while !started.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            for i in 0..15 {
                crate::dictation::remember(&data, &format!("said {i}")).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        hooks::on(move |point| {
            if point == "swap" {
                go.store(true, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        let outcome = db.rewrite(&quiet()).unwrap();
        writer.join().unwrap();
        assert!(
            matches!(outcome, Rewritten::Done | Rewritten::Changed),
            "{outcome:?}"
        );
        let said: i64 = db
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM dictations WHERE text LIKE 'said %'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(said, 15);
        no_leftovers(&path);
    }

    #[test]
    fn a_stop_during_the_copy_leaves_the_database_alone() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let stop = AtomicBool::new(true);
        assert_eq!(db.rewrite(&stop).unwrap(), Rewritten::Stopped);
        assert_eq!(mode(&path), 0);
        assert_eq!(rows(&db, "chats"), 1_000);
        no_leftovers(&path);
    }

    #[test]
    fn another_open_connection_stops_the_swap() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        let other = Connection::open(&path).unwrap();
        other
            .query_row("SELECT count(*) FROM chats", [], |r| r.get::<_, i64>(0))
            .unwrap();
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Stopped);
        assert_eq!(mode(&path), 0);
        assert_eq!(rows(&db, "chats"), 1_000);
        no_leftovers(&path);
        drop(other);

        let ready = dir.path().join("holding");
        let until = dir.path().join("let-go");
        let mut child = leo_core::children::quiet_command(std::env::current_exe().unwrap())
            .args([
                "db::rewriting::holds_the_database_open",
                "--ignored",
                "--exact",
                "--test-threads=1",
            ])
            .env("LEO_TEST_HOLD", &path)
            .env("LEO_TEST_HOLD_READY", &ready)
            .env("LEO_TEST_HOLD_UNTIL", &until)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let waited = std::time::Instant::now();
        while !ready.exists() && waited.elapsed() < std::time::Duration::from_secs(60) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(ready.exists(), "the other program opened the database");
        let outcome = db.rewrite(&quiet()).unwrap();
        std::fs::write(&until, b"").unwrap();
        child.wait().unwrap();
        assert_eq!(outcome, Rewritten::Stopped);
        assert_eq!(mode(&path), 0);
        assert_eq!(rows(&db, "chats"), 1_000);
        no_leftovers(&path);
    }

    #[test]
    #[ignore = "started by another_open_connection_stops_the_swap"]
    fn holds_the_database_open() {
        let path = PathBuf::from(std::env::var_os("LEO_TEST_HOLD").unwrap());
        let held = Connection::open(&path).unwrap();
        held.query_row("SELECT count(*) FROM chats", [], |r| r.get::<_, i64>(0))
            .unwrap();
        std::fs::write(std::env::var_os("LEO_TEST_HOLD_READY").unwrap(), b"").unwrap();
        let until = PathBuf::from(std::env::var_os("LEO_TEST_HOLD_UNTIL").unwrap());
        let waited = std::time::Instant::now();
        while !until.exists() && waited.elapsed() < std::time::Duration::from_secs(60) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(held);
    }

    #[cfg(unix)]
    #[test]
    fn a_second_spelling_of_the_path_stops_the_rewrite() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let (path, db) = old_leo(&real);
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let again = at(&linked.join(FILE));
        assert!(!Arc::ptr_eq(&again, &db));
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Stopped);
        assert_eq!(mode(&path), 0);
        drop(again);
    }

    #[test]
    fn an_uncached_handle_stops_the_rewrite() {
        let output = leo_core::children::quiet_command(std::env::current_exe().unwrap())
            .args([
                "db::rewriting::checks_rewrite_when_the_cache_is_full",
                "--ignored",
                "--exact",
                "--test-threads=1",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "started by an_uncached_handle_stops_the_rewrite"]
    fn checks_rewrite_when_the_cache_is_full() {
        let dir = tempfile::tempdir().unwrap();
        let filling: Vec<Arc<Db>> = (0..MOST_OPEN + 4)
            .map(|i| {
                let place = dir.path().join(format!("fill-{i}"));
                std::fs::create_dir(&place).unwrap();
                at(&place.join(FILE))
            })
            .collect();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let raw = Connection::open(target.join(FILE)).unwrap();
        raw.execute_batch("CREATE TABLE made_before (x TEXT);")
            .unwrap();
        drop(raw);
        let db = at(&target.join(FILE));
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Stopped);
        assert_eq!(mode(&target.join(FILE)), 0);
        drop(filling);
    }

    #[test]
    fn no_log_file_survives_the_swap() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        db.with(|c| {
            c.execute(
                "INSERT INTO dictations (at, text) VALUES ('t', 'just written')",
                [],
            )
        })
        .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let saw = Arc::clone(&seen);
        let watched = path.clone();
        hooks::on(move |point| {
            if point == "renamed" {
                let copy = rewrite_files(&watched).0;
                for (file, end) in [
                    (&watched, "-wal"),
                    (&watched, "-shm"),
                    (&copy, "-wal"),
                    (&copy, "-shm"),
                ] {
                    if exists(file, end) {
                        saw.lock().unwrap().push(format!("{}{end}", file.display()));
                    }
                }
            }
        });
        assert_eq!(db.rewrite(&quiet()).unwrap(), Rewritten::Done);
        assert!(
            seen.lock().unwrap().is_empty(),
            "log files at the swap: {:?}",
            seen.lock().unwrap()
        );
        no_leftovers(&path);
        let n: i64 = db
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM dictations WHERE text = 'just written'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_leftover_copy_is_removed_at_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        drop(Db::open(&path).unwrap());
        let copy = rewrite_files(&path).0;
        std::fs::write(&copy, b"half a copy").unwrap();
        std::fs::write(named(&copy, "-wal"), b"log").unwrap();
        drop(Db::open(&path).unwrap());
        no_leftovers(&path);
    }

    #[test]
    fn a_ready_copy_takes_the_place_of_a_missing_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let made = Db::open(&path).unwrap();
        made.with(|c| {
            c.execute(
                "INSERT INTO dictations (at, text) VALUES ('t', 'in the copy')",
                [],
            )
        })
        .unwrap();
        drop(made);
        std::fs::rename(&path, rewrite_files(&path).1).unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(rows(&db, "dictations"), 1);
        no_leftovers(&path);
    }

    #[test]
    fn a_ready_copy_beside_a_database_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let made = Db::open(&path).unwrap();
        made.with(|c| {
            c.execute(
                "INSERT INTO dictations (at, text) VALUES ('t', 'the old file')",
                [],
            )
        })
        .unwrap();
        drop(made);
        let other = dir.path().join("other.db");
        drop(Db::open(&other).unwrap());
        std::fs::rename(&other, rewrite_files(&path).1).unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(rows(&db, "dictations"), 1);
        no_leftovers(&path);
    }

    #[test]
    fn a_reopen_that_fails_after_the_swap_gives_errors_not_an_empty_database() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        hooks::on(|point| {
            if point == "swap" {
                hooks::fail("connect");
            }
        });
        assert!(db.rewrite(&quiet()).is_err());
        let chats = dir.path().join("chats");
        let saving = || {
            serde_json::from_value(
                serde_json::json!({ "messages": [{ "role": "user", "text": "hi" }] }),
            )
            .unwrap()
        };
        assert!(
            crate::chats::save(&chats, "chat-after-fail1", saving(), chrono::Utc::now()).is_err()
        );
        assert!(crate::dictation::remember(dir.path(), "lost").is_err());
        hooks::clear();
        assert_eq!(mode(&path), 2);
        crate::chats::save(&chats, "chat-after-fail1", saving(), chrono::Utc::now()).unwrap();
        crate::dictation::remember(dir.path(), "kept").unwrap();
        assert_eq!(rows(&db, "chats"), 1_001);
        assert_eq!(rows(&db, "dictations"), 2);
        no_leftovers(&path);
    }

    #[test]
    fn a_reopen_that_fails_after_a_refused_rename_recovers_the_old_file() {
        let _one = hooks::Clear;
        let dir = tempfile::tempdir().unwrap();
        let (path, db) = old_leo(dir.path());
        hooks::fail("rename");
        hooks::on(|point| {
            if point == "swap" {
                hooks::fail("connect");
            }
        });
        assert!(db.rewrite(&quiet()).is_err());
        assert!(crate::dictation::remember(dir.path(), "lost").is_err());
        hooks::clear();
        assert_eq!(mode(&path), 0);
        crate::dictation::remember(dir.path(), "kept").unwrap();
        assert_eq!(rows(&db, "chats"), 1_000);
        assert_eq!(rows(&db, "dictations"), 2);
        no_leftovers(&path);
    }

    fn synthetic(dir: &Path, megabytes: u64) -> (PathBuf, Arc<Db>) {
        let path = dir.join(FILE);
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch("CREATE TABLE made_before (x TEXT); DROP TABLE made_before;")
            .unwrap();
        drop(raw);
        let db = at(&path);
        let text: String = "a chat message about heaps and trees "
            .repeat(60)
            .chars()
            .take(2_000)
            .collect();
        let mut i = 0u64;
        while db.bytes() < megabytes * 1024 * 1024 {
            db.with(|c| {
                let tx = c.transaction()?;
                for _ in 0..2_000 {
                    tx.execute(
                        "INSERT INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at)
                         VALUES (?1, 't', 'chat', '[]', ?2, 0, '', NULL, 0, '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z')",
                        rusqlite::params![format!("chat-{i:09}"), &text],
                    )?;
                    tx.execute(
                        "INSERT INTO vectors (note, at, hash, v) VALUES (?1, 0, 'h', ?2)",
                        rusqlite::params![format!("n{i:09}"), vec![3u8; 1536]],
                    )?;
                    i += 1;
                }
                tx.commit()?;
                c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            })
            .unwrap();
        }
        db.with(|c| {
            c.execute_batch(
                "DELETE FROM chats WHERE rowid % 2 = 0; DELETE FROM vectors WHERE rowid % 2 = 0;",
            )?;
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        })
        .unwrap();
        (path, db)
    }

    #[test]
    #[ignore = "audit benchmark"]
    fn audit_bench_rewrite() {
        let sizes: Vec<u64> = std::env::var("LEO_BENCH_MB")
            .ok()
            .map(|s| s.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_else(|| vec![50, 200, 1024]);
        for megabytes in sizes {
            let dir = tempfile::tempdir().unwrap();
            let built = std::time::Instant::now();
            let (path, db) = synthetic(dir.path(), megabytes);
            let before = std::fs::metadata(&path).unwrap().len();
            let build_s = built.elapsed().as_secs_f64();
            let writing = Arc::new(AtomicBool::new(true));
            let longest = Arc::new(Mutex::new(0f64));
            let writer = {
                let (db, writing, longest) =
                    (Arc::clone(&db), Arc::clone(&writing), Arc::clone(&longest));
                std::thread::spawn(move || {
                    while writing.load(Ordering::SeqCst) {
                        let asked = std::time::Instant::now();
                        db.with(|c| {
                            c.execute("INSERT INTO dictations (at, text) VALUES ('t', 'tick')", [])
                        })
                        .unwrap();
                        let waited = asked.elapsed().as_secs_f64() * 1000.0;
                        let mut most = longest.lock().unwrap();
                        *most = most.max(waited);
                        drop(most);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                })
            };
            let busy = db.rewrite(&quiet()).unwrap();
            writing.store(false, Ordering::SeqCst);
            writer.join().unwrap();
            assert_eq!(busy, Rewritten::Changed);
            let swapped = Arc::new(Mutex::new(None));
            let at_swap = Arc::clone(&swapped);
            hooks::on(move |point| {
                if point == "swap" {
                    *at_swap.lock().unwrap() = Some(std::time::Instant::now());
                }
            });
            let started = std::time::Instant::now();
            let outcome = db.rewrite(&quiet()).unwrap();
            let ended = std::time::Instant::now();
            hooks::clear();
            assert_eq!(outcome, Rewritten::Done);
            let pause = swapped
                .lock()
                .unwrap()
                .map_or(0.0, |t| (ended - t).as_secs_f64() * 1000.0);
            let total = (ended - started).as_secs_f64() * 1000.0;
            let after = std::fs::metadata(&path).unwrap().len();
            println!(
                "audit_bench_rewrite size_mb={megabytes} file_before={before} file_after={after} build_s={build_s:.1} rewrite_ms={total:.0} ms_per_100mb={:.0} swap_pause_ms={pause:.1} longest_writer_wait_ms={:.1}",
                total * 100.0 * 1024.0 * 1024.0 / before as f64,
                *longest.lock().unwrap()
            );
        }
    }

    #[test]
    fn a_copy_made_with_vacuum_into_takes_the_space_giving_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        old_file(&path);
        assert_eq!(mode(&path), 0);
        let version: String = Connection::open_in_memory()
            .unwrap()
            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
            .unwrap();
        println!("sqlite {version}");
        let reader =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        reader
            .pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .unwrap();
        let copy = dir.path().join("copy.db");
        std::fs::File::create(&copy).unwrap();
        reader
            .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
            .unwrap();
        assert_eq!(mode(&copy), 2);
        let changed: i64 = reader
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        let writer = Connection::open(&path).unwrap();
        let before: i64 = reader
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(changed, before);
        writer.execute("INSERT INTO t VALUES ('x')", []).unwrap();
        let after: i64 = reader
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        assert_ne!(before, after);
        let wal = |p: &Path, end: &str| {
            let mut name = p.as_os_str().to_owned();
            name.push(end);
            PathBuf::from(name).exists()
        };
        assert!(wal(&path, "-wal"));
        drop(reader);
        let other = Connection::open(&path).unwrap();
        other
            .query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
            .unwrap();
        drop(writer);
        assert!(
            wal(&path, "-wal") && wal(&path, "-shm"),
            "another open connection keeps the log files"
        );
        drop(other);
        assert!(
            !wal(&path, "-wal") && !wal(&path, "-shm"),
            "the last writer to close folds and removes them"
        );
    }
}

#[cfg(test)]
mod set_aside_time {
    #[test]
    fn a_file_set_aside_is_stamped_now() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("captions.json");
        std::fs::write(&old, "{}").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
            .unwrap();
        super::put_aside(&old);
        let aside = tmp.path().join("captions.json.before-database");
        let at = std::fs::metadata(&aside).unwrap().modified().unwrap();
        assert!(at.elapsed().is_ok_and(|age| age.as_secs() < 600));
        assert!(!old.exists());
    }
}
