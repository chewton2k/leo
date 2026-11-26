use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use leo_core::notes::Note;
use leo_core::store::{Changed, Store};

use crate::chat::flat;
use crate::db::Db;
use crate::vectors::fnv;

pub const MOST_COPIED: usize = 8 * 1024 * 1024;
const MOST_LOOKED: usize = 16 * 1024 * 1024;
const ROW: usize = 1024 * 1024;
const BATCH_NOTES: usize = 50;
const BATCH_BYTES: usize = 2 * 1024 * 1024;
const FEW_IDS: usize = 16;
pub const RETRY: std::time::Duration = std::time::Duration::from_secs(30);

pub type Stamp = (u64, u64);

pub fn stamp_of(store: &Store) -> Stamp {
    (store.fingerprint(), store.changes_since(0).0)
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn hash_of(text: &str) -> String {
    fnv(text.bytes())
}

fn rows(text: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > ROW {
        let mut cut = None;
        for (at, c) in rest.char_indices() {
            if at + c.len_utf8() > ROW {
                break;
            }
            if !c.is_alphanumeric() {
                cut = Some(at + c.len_utf8());
            }
        }
        let (head, tail) = rest.split_at(cut?);
        out.push(head);
        rest = tail;
    }
    out.push(rest);
    Some(out)
}

pub enum Job {
    Put {
        id: String,
        hash: String,
        text: String,
    },
    Whole {
        id: String,
        hash: String,
    },
    Drop {
        id: String,
    },
}

pub struct Work {
    pub jobs: Vec<Job>,
    pub more: bool,
    stamp: Stamp,
    live: Option<usize>,
    reset: bool,
}

impl Work {
    pub fn busy(&self) -> bool {
        self.more || self.reset || !self.jobs.is_empty()
    }
}

#[derive(Default)]
struct Walk {
    looked: u64,
    queue: VecDeque<String>,
    full: bool,
}

pub struct Candidates {
    at: Stamp,
    by_word: HashMap<String, HashSet<String>>,
    whole: HashSet<String>,
}

impl Candidates {
    pub fn usable(&self, store: &Store) -> bool {
        self.at == stamp_of(store)
    }

    pub fn holds_text_of(&self, id: &str) -> bool {
        !self.whole.contains(id)
    }

    pub fn has(&self, id: &str, word: &str) -> bool {
        self.by_word.get(word).is_some_and(|ids| ids.contains(id))
    }
}

pub struct SearchIndex {
    db: Arc<Db>,
    known: Mutex<HashMap<String, String>>,
    walk: Mutex<Walk>,
    synced: Mutex<Option<Stamp>>,
    broken: AtomicBool,
    pub(crate) wake: tokio::sync::Notify,
    #[cfg(test)]
    commits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    longest: Mutex<std::time::Duration>,
}

enum Op {
    Clear(String),
    Row(String, String),
    Seal(String, String),
    Whole(String, String),
}

impl SearchIndex {
    pub fn for_notes(notes_dir: &Path) -> SearchIndex {
        let base = crate::db::beside_of(notes_dir, None);
        SearchIndex::new(crate::db::at(&base.join(crate::db::FILE)))
    }

    pub fn new(db: Arc<Db>) -> SearchIndex {
        let (known, broken) = match db.with(|c| {
            let mut found = HashMap::new();
            let mut statement = c.prepare(
                "SELECT note, MIN(hash) FROM notes_indexed GROUP BY note HAVING MIN(hash) <> ''
                 UNION ALL SELECT note, hash FROM notes_whole",
            )?;
            let mut all = statement.query([])?;
            while let Some(row) = all.next()? {
                found.insert(row.get(0)?, row.get(1)?);
            }
            Ok(found)
        }) {
            Ok(known) => (known, false),
            Err(_) => (HashMap::new(), true),
        };
        SearchIndex {
            db,
            known: Mutex::new(known),
            walk: Default::default(),
            synced: Default::default(),
            broken: AtomicBool::new(broken),
            wake: tokio::sync::Notify::new(),
            #[cfg(test)]
            commits: Default::default(),
            #[cfg(test)]
            longest: Default::default(),
        }
    }

    pub fn stale(&self, store: &Store, most_bytes: usize) -> Work {
        let mut walk = locked(&self.walk);
        let stamp = stamp_of(store);
        if self.broken.load(Ordering::Acquire) {
            return Work {
                jobs: Vec::new(),
                more: true,
                stamp,
                live: None,
                reset: true,
            };
        }
        let known = locked(&self.known);
        let mut jobs = Vec::new();
        let (generation, changed) = store.changes_since(walk.looked);
        match changed {
            Changed::Everything => {
                walk.queue = store.notes.iter().map(|n| n.id.clone()).collect();
                walk.full = true;
                let live: HashSet<&str> = store.notes.iter().map(|n| n.id.as_str()).collect();
                jobs.extend(
                    known
                        .keys()
                        .filter(|id| !live.contains(id.as_str()))
                        .map(|id| Job::Drop { id: id.clone() }),
                );
            }
            Changed::Ids(ids) => walk.queue.extend(ids),
        }
        walk.looked = generation;
        let by_id: Option<HashMap<&str, &Note>> = (walk.queue.len() > FEW_IDS)
            .then(|| store.notes.iter().map(|n| (n.id.as_str(), n)).collect());
        let mut copied = 0;
        let mut looked = 0;
        while let Some(id) = walk.queue.pop_front() {
            let note = match &by_id {
                Some(by_id) => by_id.get(id.as_str()).copied(),
                None => store.notes.iter().find(|n| n.id == id),
            };
            match note {
                None => {
                    if known.contains_key(&id) {
                        jobs.push(Job::Drop { id });
                    }
                }
                Some(note) => {
                    let text = flat(&note.body);
                    looked += text.len();
                    let hash = hash_of(&text);
                    if known.get(&id) != Some(&hash) {
                        if rows(&text).is_none() {
                            jobs.push(Job::Whole { id, hash });
                        } else {
                            copied += text.len();
                            jobs.push(Job::Put { id, hash, text });
                        }
                    }
                }
            }
            if copied >= most_bytes || looked >= MOST_LOOKED {
                break;
            }
        }
        let more = !walk.queue.is_empty();
        let live = (!more && walk.full).then(|| {
            walk.full = false;
            store
                .notes
                .iter()
                .map(|n| n.id.as_str())
                .collect::<HashSet<_>>()
                .len()
        });
        Work {
            jobs,
            more,
            stamp,
            live,
            reset: false,
        }
    }

    pub fn apply(&self, work: Work) -> Result<()> {
        if work.reset {
            return self.reset();
        }
        match self.write(work) {
            Ok(true) => Ok(()),
            Ok(false) => self.reset(),
            Err(e) => {
                leo_core::diag::warn(format!(
                    "the search index failed and will be rebuilt: {e:#}"
                ));
                self.reset()
            }
        }
    }

    fn write(&self, work: Work) -> Result<bool> {
        let mut ops: Vec<Op> = Vec::new();
        let mut bytes = 0;
        let mut notes = 0;
        for job in work.jobs {
            match job {
                Job::Drop { id } => ops.push(Op::Clear(id)),
                Job::Whole { id, hash } => {
                    ops.push(Op::Clear(id.clone()));
                    ops.push(Op::Whole(id, hash));
                }
                Job::Put { id, hash, text } => {
                    ops.push(Op::Clear(id.clone()));
                    let pieces = rows(&text)
                        .ok_or_else(|| anyhow::anyhow!("a long note reached the index"))?;
                    for piece in pieces {
                        if bytes + piece.len() > BATCH_BYTES && !ops.is_empty() {
                            self.flush(std::mem::take(&mut ops))?;
                            bytes = 0;
                            notes = 0;
                        }
                        bytes += piece.len();
                        ops.push(Op::Row(id.clone(), piece.to_string()));
                    }
                    ops.push(Op::Seal(id, hash));
                }
            }
            notes += 1;
            if notes >= BATCH_NOTES {
                self.flush(std::mem::take(&mut ops))?;
                bytes = 0;
                notes = 0;
            }
        }
        if !ops.is_empty() {
            self.flush(ops)?;
        }
        if work.more {
            return Ok(true);
        }
        let known = locked(&self.known).len();
        self.db.with(|c| {
            let mut synced = locked(&self.synced);
            if *synced == Some(work.stamp) {
                return Ok(true);
            }
            if let Some(live) = work.live {
                let count = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
                let text = count("SELECT count(*) FROM notes_text")?;
                let indexed = count("SELECT count(*) FROM notes_indexed")?;
                let notes = count("SELECT count(DISTINCT note) FROM notes_indexed")?;
                let whole = count("SELECT count(*) FROM notes_whole")?;
                if text != indexed || (notes + whole) as usize != live || known != live {
                    return Ok(false);
                }
            }
            *synced = Some(work.stamp);
            Ok(true)
        })
    }

    fn flush(&self, ops: Vec<Op>) -> Result<()> {
        #[cfg(test)]
        let started = std::time::Instant::now();
        self.db.with(|c| {
            *locked(&self.synced) = None;
            let tx = c.transaction()?;
            for op in &ops {
                match op {
                    Op::Clear(id) => {
                        tx.prepare_cached(
                            "DELETE FROM notes_text WHERE rowid IN (SELECT n FROM notes_indexed WHERE note = ?1)",
                        )?
                        .execute([id])?;
                        tx.prepare_cached("DELETE FROM notes_indexed WHERE note = ?1")?
                            .execute([id])?;
                        tx.prepare_cached("DELETE FROM notes_whole WHERE note = ?1")?
                            .execute([id])?;
                    }
                    Op::Row(id, text) => {
                        tx.prepare_cached("INSERT INTO notes_indexed(note, hash) VALUES (?1, '')")?
                            .execute([id])?;
                        let n = tx.last_insert_rowid();
                        tx.prepare_cached("INSERT INTO notes_text(rowid, body) VALUES (?1, ?2)")?
                            .execute(rusqlite::params![n, text])?;
                    }
                    Op::Seal(id, hash) => {
                        tx.prepare_cached("UPDATE notes_indexed SET hash = ?2 WHERE note = ?1")?
                            .execute([id, hash])?;
                    }
                    Op::Whole(id, hash) => {
                        tx.prepare_cached("INSERT INTO notes_whole(note, hash) VALUES (?1, ?2)")?
                            .execute([id, hash])?;
                    }
                }
            }
            tx.commit()
        })?;
        let mut known = locked(&self.known);
        for op in ops {
            match op {
                Op::Clear(id) => {
                    known.remove(&id);
                }
                Op::Seal(id, hash) | Op::Whole(id, hash) => {
                    known.insert(id, hash);
                }
                Op::Row(..) => {}
            }
        }
        #[cfg(test)]
        {
            self.commits.fetch_add(1, Ordering::Relaxed);
            let mut longest = locked(&self.longest);
            *longest = (*longest).max(started.elapsed());
        }
        Ok(())
    }

    fn reset(&self) -> Result<()> {
        *locked(&self.synced) = None;
        *locked(&self.walk) = Walk::default();
        locked(&self.known).clear();
        self.db.with(|c| {
            c.execute_batch(
                "DROP TABLE IF EXISTS notes_text; DROP TABLE IF EXISTS notes_indexed; DROP TABLE IF EXISTS notes_whole;",
            )?;
            c.execute_batch(crate::db::INDEX_TABLES)
        })?;
        self.broken.store(false, Ordering::Release);
        Ok(())
    }

    pub fn candidates(&self, words: &[String]) -> Option<Candidates> {
        if words.is_empty() || words.iter().any(|w| w.chars().count() < 3) {
            return None;
        }
        let distinct: HashSet<&String> = words.iter().collect();
        let found = self.db.with(|c| {
            let Some(at) = *locked(&self.synced) else {
                return Ok(None);
            };
            let mut by_word = HashMap::new();
            for word in distinct {
                let mut statement = c.prepare_cached(
                    "SELECT i.note FROM notes_text t JOIN notes_indexed i ON i.n = t.rowid WHERE t.notes_text MATCH ?1",
                )?;
                let ids = statement
                    .query_map([format!("\"{word}\"")], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<HashSet<_>>>()?;
                by_word.insert(word.clone(), ids);
            }
            let mut statement = c.prepare_cached("SELECT note FROM notes_whole")?;
            let whole = statement
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok(Some(Candidates { at, by_word, whole }))
        });
        match found {
            Ok(found) => found,
            Err(_) => {
                self.broken.store(true, Ordering::Release);
                self.wake.notify_one();
                None
            }
        }
    }

    pub fn for_query(&self, query: &str) -> Option<Candidates> {
        let words: Vec<String> = crate::search::words_of(query)
            .into_iter()
            .map(|(w, _)| w)
            .collect();
        self.candidates(&words)
    }

    pub fn for_question(&self, question: &str) -> Option<Candidates> {
        self.candidates(&crate::chat::question_terms(question))
    }
}

#[cfg(test)]
pub(crate) fn bench_library(n: usize) -> (tempfile::TempDir, Store) {
    const WORDS: &[&str] = &[
        "the", "graph", "search", "queue", "stack", "heap", "tree", "node", "edge", "visit",
        "order", "breadth", "depth", "first", "lecture", "exam", "proof", "lemma", "theorem",
        "plant", "light", "sugar", "cell", "energy", "market", "price", "supply", "demand",
        "history", "empire", "river", "trade", "language", "grammar", "verb", "noun", "memory",
        "cache", "process", "thread", "lock", "network", "packet", "route", "signal", "matrix",
        "vector", "integral", "limit", "series", "function", "variable", "loop", "array", "list",
        "hash", "table", "sort", "merge", "split", "and", "of", "to", "in", "is", "that", "with",
    ];
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    for i in 0..n {
        let mut state = (i as u64).wrapping_mul(6_364_136_223_846_793_005) ^ 0x9e37_79b9;
        let mut body = String::new();
        while body.len() < 5000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            body.push_str(WORDS[(state >> 33) as usize % WORDS.len()]);
            body.push(' ');
        }
        if i % 100 == 0 {
            body.push_str("quokka");
        }
        store
            .create_note(
                format!("Lecture {i} {}", WORDS[i % WORDS.len()]),
                body,
                &format!("Course {}", i % 20),
            )
            .unwrap();
    }
    if n > 20_000 {
        return (dir, store);
    }
    store.save().unwrap();
    let store = Store::load_from(&dir.path().join("notes")).unwrap();
    (dir, store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Cache;
    use crate::search::{search_with, Why};
    use rusqlite::Connection;

    fn library(notes: &[(&str, &str)]) -> (tempfile::TempDir, Store, SearchIndex) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        for (title, body) in notes {
            store.create_note(*title, *body, "").unwrap();
        }
        store.save().unwrap();
        let index = SearchIndex::for_notes(&store.notes_dir);
        (dir, store, index)
    }

    fn catch_up(index: &SearchIndex, store: &Store) {
        loop {
            let work = index.stale(store, MOST_COPIED);
            let last = !work.more;
            index.apply(work).unwrap();
            if last {
                break;
            }
        }
    }

    fn found(
        store: &Store,
        query: &str,
        indexed: Option<&Candidates>,
    ) -> Vec<(String, Option<Why>)> {
        search_with(store, &Cache::default(), query, indexed)
            .into_iter()
            .map(|h| (h.note.title.clone(), h.why))
            .collect()
    }

    fn through(index: &SearchIndex, store: &Store, query: &str) -> Vec<(String, Option<Why>)> {
        let candidates = index.for_query(query);
        let usable = candidates.as_ref().filter(|c| c.usable(store));
        assert!(usable.is_some(), "the index was not used for {query}");
        found(store, query, usable)
    }

    #[test]
    fn the_bundled_sqlite_takes_the_index_options_and_finds_substrings() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE notes_text USING fts5(body, content = '', contentless_delete = 1, tokenize = 'trigram case_sensitive 1');
             CREATE TABLE notes_indexed (n INTEGER PRIMARY KEY, note TEXT NOT NULL, hash TEXT NOT NULL);",
        )
        .unwrap();
        let rows = [
            ("a", "the heap keeps order"),
            ("b", "Heaps Keep"),
            ("c", "中文的堆很好 and ß straße"),
            ("d", "i\u{307}stanbul sigma ας"),
        ];
        for (note, text) in rows {
            conn.execute(
                "INSERT INTO notes_indexed(note, hash) VALUES (?1, 'h')",
                [note],
            )
            .unwrap();
            let n = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO notes_text(rowid, body) VALUES (?1, ?2)",
                rusqlite::params![n, text],
            )
            .unwrap();
        }
        let find = |word: &str| -> Vec<String> {
            let mut statement = conn
                .prepare("SELECT DISTINCT note FROM notes_indexed WHERE n IN (SELECT rowid FROM notes_text WHERE notes_text MATCH ?1) ORDER BY note")
                .unwrap();
            statement
                .query_map([format!("\"{word}\"")], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(find("heap"), ["a"]);
        assert_eq!(find("Heap"), ["b"]);
        assert_eq!(find("eap k"), ["a"]);
        assert_eq!(find("堆很好"), ["c"]);
        assert_eq!(find("straße"), ["c"]);
        assert_eq!(find("ας"), Vec::<String>::new());
        assert_eq!(find("sigma ας"), ["d"]);
        assert_eq!(find("stan"), ["d"]);
        conn.execute(
            "DELETE FROM notes_text WHERE rowid IN (SELECT n FROM notes_indexed WHERE note = ?1)",
            ["a"],
        )
        .unwrap();
        assert_eq!(find("heap"), Vec::<String>::new());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM notes_text", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn a_saved_note_is_found_through_the_index_after_one_pass() {
        let (_d, mut store, index) = library(&[
            ("Heaps", "A heap keeps the minimum first."),
            ("Trees", "A tree has a root."),
        ]);
        assert!(
            index.for_query("minimum").is_none(),
            "nothing is indexed yet"
        );
        catch_up(&index, &store);
        assert_eq!(
            through(&index, &store, "minimum"),
            found(&store, "minimum", None)
        );
        assert_eq!(through(&index, &store, "minimum")[0].0, "Heaps");
        store
            .create_note("Graphs", "A graph has an edge.", "")
            .unwrap();
        store.save().unwrap();
        catch_up(&index, &store);
        assert_eq!(through(&index, &store, "edge")[0].0, "Graphs");
    }

    #[test]
    fn an_edit_made_outside_leo_is_indexed_after_the_next_refresh() {
        let (_d, mut store, index) = library(&[("Heaps", "A heap keeps the minimum first.")]);
        catch_up(&index, &store);
        let file = std::fs::read_dir(&store.notes_dir)
            .unwrap()
            .flatten()
            .find(|e| e.path().extension().is_some_and(|x| x == "md"))
            .unwrap()
            .path();
        let text = std::fs::read_to_string(&file).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, text.replace("minimum", "maximum")).unwrap();
        store.refresh().unwrap();
        catch_up(&index, &store);
        assert!(through(&index, &store, "minimum").is_empty());
        assert_eq!(through(&index, &store, "maximum")[0].0, "Heaps");
    }

    #[test]
    fn a_deleted_note_leaves_the_index() {
        let (_d, mut store, index) = library(&[("Heaps", "minimum"), ("Trees", "minimum root")]);
        catch_up(&index, &store);
        let id = store.notes[0].id.clone();
        store.delete_notes(&[id]);
        store.save().unwrap();
        catch_up(&index, &store);
        let rows: i64 = index
            .db
            .with(|c| c.query_row("SELECT count(*) FROM notes_indexed", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(through(&index, &store, "minimum").len(), 1);
    }

    #[test]
    fn a_search_before_the_index_caught_up_uses_the_old_path_and_finds_the_note() {
        let (_d, mut store, index) = library(&[("Heaps", "minimum")]);
        catch_up(&index, &store);
        store.create_note("Late", "minimum arrived", "").unwrap();
        store.save().unwrap();
        let candidates = index.for_query("arrived");
        let usable = candidates.as_ref().filter(|c| c.usable(&store));
        assert!(usable.is_none());
        let hits = found(&store, "arrived", usable);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "Late");
    }

    #[test]
    fn a_one_or_two_letter_word_uses_the_old_path() {
        let (_d, store, index) = library(&[("Heaps", "a heap keeps it")]);
        catch_up(&index, &store);
        assert!(index.for_query("a heap").is_none());
        assert!(index.for_query("in").is_none());
        assert!(index.for_query("").is_none());
        assert!(index.for_query("heap").is_some());
        assert!(index.for_query("堆").is_none());
    }

    #[test]
    fn an_index_error_falls_back_and_rebuilds() {
        let (_d, store, index) = library(&[("Heaps", "minimum"), ("Trees", "root")]);
        catch_up(&index, &store);
        index
            .db
            .with(|c| c.execute_batch("DROP TABLE notes_text"))
            .unwrap();
        assert!(index.for_query("minimum").is_none());
        assert!(index.for_query("minimum").is_none());
        assert!(locked(&index.synced).is_some());
        catch_up(&index, &store);
        assert_eq!(through(&index, &store, "minimum")[0].0, "Heaps");
    }

    #[test]
    fn rows_missing_from_the_index_are_found_by_the_count_check() {
        let (_d, store, index) = library(&[("Heaps", "minimum"), ("Trees", "root")]);
        catch_up(&index, &store);
        index
            .db
            .with(|c| {
                c.execute(
                    "DELETE FROM notes_text WHERE rowid = (SELECT min(n) FROM notes_indexed)",
                    [],
                )
            })
            .unwrap();
        let missing = index
            .for_query("minimum")
            .map(|c| c.has(&store.notes[0].id, "minimum"));
        assert_eq!(
            missing,
            Some(false),
            "until the next full pass the search may miss it"
        );
        let again = SearchIndex::new(Arc::clone(&index.db));
        catch_up(&again, &store);
        assert!(
            again.for_query("minimum").is_none(),
            "the count check reset the index"
        );
        catch_up(&again, &store);
        assert_eq!(through(&again, &store, "minimum")[0].0, "Heaps");
    }

    #[test]
    fn candidates_read_before_a_save_are_not_used_after_it() {
        let (_d, mut store, index) = library(&[("Heaps", "minimum")]);
        catch_up(&index, &store);
        let candidates = index.for_query("minimum").unwrap();
        assert!(candidates.usable(&store));
        store.create_note("New", "minimum again", "").unwrap();
        store.save().unwrap();
        catch_up(&index, &store);
        assert!(!candidates.usable(&store));
        let usable = candidates.usable(&store).then_some(&candidates);
        assert_eq!(
            found(&store, "minimum", usable),
            found(&store, "minimum", None)
        );
        assert_eq!(found(&store, "minimum", None).len(), 2);
        let asked = "tell me about the minimum";
        let cache = Cache::default();
        let before: Vec<&str> = crate::chat::matches_with(&store, &cache, asked, 8, usable)
            .iter()
            .map(|n| n.title.as_str())
            .collect();
        assert_eq!(before.len(), 2);
    }

    #[test]
    fn a_note_with_a_run_longer_than_a_row_is_searched_whole() {
        let run = "x".repeat(3 * 1024 * 1024);
        let body = format!("{run} afterwards");
        let (_d, store, index) = library(&[("Long", &body), ("Short", "afterwards too")]);
        catch_up(&index, &store);
        let rows: i64 = index
            .db
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM notes_indexed WHERE note = ?1",
                    [&store.notes[0].id],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 0);
        let candidates = index.for_query("afterwards").unwrap();
        assert!(!candidates.holds_text_of(&store.notes[0].id));
        assert_eq!(through(&index, &store, "afterwards").len(), 2);
        assert_eq!(through(&index, &store, &"x".repeat(2000)).len(), 1);
    }

    #[test]
    fn a_note_edited_by_an_older_leo_is_re_indexed_on_start() {
        let (_d, mut store, index) =
            library(&[("One", "minimum"), ("Two", "root"), ("Three", "edge")]);
        catch_up(&index, &store);
        let id = store.notes[0].id.clone();
        store.find_note_mut(&id).unwrap().body = "maximum".into();
        store.save().unwrap();
        let start = SearchIndex::new(Arc::clone(&index.db));
        assert!(start.for_query("maximum").is_none());
        let work = start.stale(&store, MOST_COPIED);
        assert!(locked(&start.synced).is_none());
        start.apply(work).unwrap();
        assert_eq!(*locked(&start.synced), Some(stamp_of(&store)));
        assert!(through(&start, &store, "minimum").is_empty());
        assert_eq!(through(&start, &store, "maximum")[0].0, "One");
    }

    #[test]
    fn a_first_index_is_written_in_batches() {
        let notes: Vec<(String, String)> = (0..450)
            .map(|i| (format!("Note {i}"), format!("body {i} word")))
            .collect();
        let refs: Vec<(&str, &str)> = notes
            .iter()
            .map(|(t, b)| (t.as_str(), b.as_str()))
            .collect();
        let (_d, store, index) = library(&refs);
        catch_up(&index, &store);
        assert_eq!(
            index.commits.load(Ordering::Relaxed),
            450_usize.div_ceil(BATCH_NOTES)
        );
        assert_eq!(through(&index, &store, "word").len(), 450);
    }

    #[test]
    fn a_long_note_is_indexed_in_rows_and_found_whole() {
        let mut body = "lorem ipsum ".repeat(87_380);
        body.push_str("leftside rightside ");
        body.push_str(&"dolor sit ".repeat(230_000));
        assert!(body.len() > 3 * ROW);
        let (_d, store, index) = library(&[("Long", &body)]);
        catch_up(&index, &store);
        let rows: i64 = index
            .db
            .with(|c| c.query_row("SELECT count(*) FROM notes_indexed", [], |r| r.get(0)))
            .unwrap();
        assert!(rows >= 3, "{rows}");
        for word in [
            "leftside",
            "rightside",
            "ipsum",
            "dolor",
            "orem ips",
            "sit dolor",
        ] {
            assert_eq!(through(&index, &store, word).len(), 1, "{word}");
        }
        assert!(through(&index, &store, "ipsumdolor").is_empty());
    }

    #[test]
    fn a_pass_copies_no_more_than_its_budget() {
        let body = "word ".repeat(200_000);
        let notes: Vec<(String, String)> =
            (0..30).map(|i| (format!("N{i}"), body.clone())).collect();
        let refs: Vec<(&str, &str)> = notes
            .iter()
            .map(|(t, b)| (t.as_str(), b.as_str()))
            .collect();
        let (_d, store, index) = library(&refs);
        let work = index.stale(&store, MOST_COPIED);
        let copied: usize = work
            .jobs
            .iter()
            .map(|j| match j {
                Job::Put { text, .. } => text.len(),
                _ => 0,
            })
            .sum();
        assert!(copied <= MOST_COPIED + body.len(), "{copied}");
        assert!(work.more);
    }

    const SPECIAL: &[&str] = &[
        "straße",
        "STRASSE",
        "İstanbul",
        "ΟΔΟΣ",
        "οδός",
        "Σίσυφος",
        "中文笔记",
        "堆是一种树",
        "日本語のノート",
        "ひらがなカタカナ",
        "naïve",
        "café",
        "CAFÉ",
        "e-mail",
        "snake_case",
        "hello-world",
        "x_y-z",
        "ǅungla",
        "ﬁnish",
    ];

    fn next(state: &mut u64) -> usize {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 33) as usize
    }

    fn vocabulary(state: &mut u64) -> Vec<String> {
        let syllables = [
            "ba", "ke", "lo", "mi", "nu", "ra", "so", "ti", "ve", "zu", "th", "ch",
        ];
        (0..400)
            .map(|_| {
                let count = 1 + next(state) % 4;
                (0..count)
                    .map(|_| syllables[next(state) % syllables.len()])
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    #[ignore = "differential test over 500 searches"]
    fn the_index_changes_no_search_result() {
        let mut state = 7_u64;
        let words = vocabulary(&mut state);
        let mut notes: Vec<(String, String)> = Vec::new();
        for i in 0..2_200 {
            let mut body = String::new();
            for _ in 0..(20 + next(&mut state) % 60) {
                let word = if i >= 2_000 && next(&mut state).is_multiple_of(3) {
                    SPECIAL[next(&mut state) % SPECIAL.len()].to_string()
                } else {
                    words[next(&mut state) % words.len()].clone()
                };
                body.push_str(&word);
                body.push(match next(&mut state) % 9 {
                    0 => '\n',
                    1 => '-',
                    2 => '_',
                    3 => ',',
                    _ => ' ',
                });
            }
            let title = format!(
                "{} {} {i}",
                words[next(&mut state) % words.len()],
                words[next(&mut state) % words.len()]
            );
            notes.push((title, body));
        }
        let refs: Vec<(&str, &str)> = notes
            .iter()
            .map(|(t, b)| (t.as_str(), b.as_str()))
            .collect();
        let (_d, store, index) = library(&refs);
        catch_up(&index, &store);
        let cache = Cache::default();
        let mut queries: Vec<String> = Vec::new();
        for _ in 0..500 {
            let one = |state: &mut u64| -> String {
                match next(state) % 5 {
                    0 => SPECIAL[next(state) % SPECIAL.len()].to_string(),
                    1 => {
                        let word: Vec<char> = words[next(state) % words.len()].chars().collect();
                        let size = 1 + next(state) % word.len().min(12);
                        let at = next(state) % (word.len() - size + 1);
                        word[at..at + size].iter().collect()
                    }
                    2 => words[next(state) % words.len()].to_uppercase(),
                    _ => words[next(state) % words.len()].clone(),
                }
            };
            let count = 1 + next(&mut state) % 3;
            queries.push(
                (0..count)
                    .map(|_| one(&mut state))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        let mut indexed_searches = 0;
        let mut asked = 0;
        for query in &queries {
            let today = found(&store, query, None);
            if let Some(candidates) = index.for_query(query) {
                assert!(candidates.usable(&store));
                indexed_searches += 1;
                assert_eq!(
                    found(&store, query, Some(&candidates)),
                    today,
                    "search {query:?}"
                );
            }
            let question = format!("tell me about {query}");
            let before: Vec<&str> = crate::chat::matches_with(&store, &cache, &question, 50, None)
                .iter()
                .map(|n| n.id.as_str())
                .collect();
            if let Some(candidates) = index.for_question(&question) {
                asked += 1;
                let after: Vec<&str> =
                    crate::chat::matches_with(&store, &cache, &question, 50, Some(&candidates))
                        .iter()
                        .map(|n| n.id.as_str())
                        .collect();
                assert_eq!(after, before, "question {question:?}");
            }
        }
        println!("searches through the index: {indexed_searches} of 500, questions: {asked}");
        assert!(indexed_searches > 150 && asked > 150);
    }

    fn median(runs: usize, mut work: impl FnMut()) -> f64 {
        let mut times: Vec<f64> = (0..runs)
            .map(|_| {
                let started = std::time::Instant::now();
                work();
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        times[times.len() / 2]
    }

    #[test]
    #[ignore = "benchmark: release, LEO_BENCH_NOTES=5000,10000,50000"]
    fn search_index_benchmark() {
        let sizes: Vec<usize> = std::env::var("LEO_BENCH_NOTES")
            .unwrap_or_else(|_| "5000,10000".into())
            .split(',')
            .map(|n| n.parse().unwrap())
            .collect();
        let cache = Cache::default();
        for n in sizes {
            let built = std::time::Instant::now();
            let (dir, mut store) = bench_library(n);
            println!(
                "fixture notes={n} built_ms={:.0}",
                built.elapsed().as_secs_f64() * 1000.0
            );
            let index = SearchIndex::for_notes(&store.notes_dir);
            let first = std::time::Instant::now();
            let mut rounds = 0;
            let mut most_copied = 0;
            loop {
                let work = index.stale(&store, MOST_COPIED);
                most_copied = most_copied.max(
                    work.jobs
                        .iter()
                        .map(|j| match j {
                            Job::Put { text, .. } => text.len(),
                            _ => 0,
                        })
                        .sum::<usize>(),
                );
                let last = !work.more;
                index.apply(work).unwrap();
                rounds += 1;
                if last {
                    break;
                }
            }
            let text: u64 = store.notes.iter().map(|n| n.body.len() as u64).sum();
            index
                .db
                .with(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)"))
                .unwrap();
            let bytes = std::fs::metadata(index.db.path()).unwrap().len();
            println!(
                "first_index notes={n} ms={:.0} rounds={rounds} longest_with_ms={:.1} most_text_copied_bytes={most_copied} index_bytes={bytes} note_text_bytes={text} ratio={:.2}",
                first.elapsed().as_secs_f64() * 1000.0,
                locked(&index.longest).as_secs_f64() * 1000.0,
                bytes as f64 / text as f64
            );
            for query in ["queue", "quokka", "queue stack", "a queue"] {
                let words = crate::search::words_of(query);
                let (mut today, mut with, mut text_today, mut text_with, mut fetch) =
                    (vec![], vec![], vec![], vec![], vec![]);
                for _ in 0..5 {
                    today.push(median(3, || {
                        std::hint::black_box(found(&store, query, None));
                    }));
                    with.push(median(3, || {
                        let c = index.for_query(query);
                        std::hint::black_box(found(
                            &store,
                            query,
                            c.as_ref().filter(|c| c.usable(&store)),
                        ));
                    }));
                    text_today.push(median(3, || {
                        std::hint::black_box(crate::search::ranked(
                            store.notes.iter().map(crate::search::Text::of),
                            &cache,
                            &words,
                            None,
                        ));
                    }));
                    let c = index.for_query(query);
                    fetch.push(median(3, || {
                        std::hint::black_box(index.for_query(query));
                    }));
                    text_with.push(median(3, || {
                        std::hint::black_box(crate::search::ranked(
                            store.notes.iter().map(crate::search::Text::of),
                            &cache,
                            &words,
                            c.as_ref(),
                        ));
                    }));
                }
                let m = |mut v: Vec<f64>| {
                    v.sort_by(f64::total_cmp);
                    v[v.len() / 2]
                };
                println!(
                    "search notes={n} query={query:?} indexed={} whole_search_today_ms={:.2} whole_search_index_ms={:.2} text_stage_today_ms={:.2} text_stage_index_ms={:.2} candidates_fetch_ms={:.2}",
                    index.for_query(query).is_some(),
                    m(today), m(with), m(text_today), m(text_with), m(fetch)
                );
            }
            let question = "how does a queue differ from a stack";
            let (mut a, mut b) = (vec![], vec![]);
            for _ in 0..5 {
                a.push(median(3, || {
                    std::hint::black_box(crate::chat::matches_with(
                        &store, &cache, question, 50, None,
                    ));
                }));
                b.push(median(3, || {
                    let c = index.for_question(question);
                    std::hint::black_box(crate::chat::matches_with(
                        &store,
                        &cache,
                        question,
                        50,
                        c.as_ref().filter(|c| c.usable(&store)),
                    ));
                }));
            }
            a.sort_by(f64::total_cmp);
            b.sort_by(f64::total_cmp);
            println!(
                "felix_matches notes={n} today_ms={:.2} index_ms={:.2}",
                a[2], b[2]
            );
            let mut apply_ms = Vec::new();
            for i in 0..11 {
                let id = store.notes[i * 7].id.clone();
                store
                    .find_note_mut(&id)
                    .unwrap()
                    .body
                    .push_str(" edited quokka");
                let started = std::time::Instant::now();
                catch_up(&index, &store);
                apply_ms.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            apply_ms.sort_by(f64::total_cmp);
            println!(
                "one_save_indexed notes={n} stale_and_apply_ms={:.3}",
                apply_ms[5]
            );
            drop(dir);
        }
    }

    #[test]
    #[ignore = "benchmark: release, one 20 MB note"]
    fn search_index_benchmark_one_big_note() {
        for (label, body) in [
            (
                "ordinary_words",
                "alpha beta gamma delta epsilon zeta ".repeat(20 * 1024 * 1024 / 36),
            ),
            ("one_unbroken_run", "x".repeat(20 * 1024 * 1024)),
        ] {
            let (_d, store, index) = library(&[("Big", &body), ("Small", "alpha")]);
            let started = std::time::Instant::now();
            let mut most_copied = 0;
            loop {
                let work = index.stale(&store, MOST_COPIED);
                most_copied = most_copied.max(
                    work.jobs
                        .iter()
                        .map(|j| match j {
                            Job::Put { text, .. } => text.len(),
                            _ => 0,
                        })
                        .sum::<usize>(),
                );
                let last = !work.more;
                index.apply(work).unwrap();
                if last {
                    break;
                }
            }
            println!(
                "big_note {label} ms={:.0} longest_with_ms={:.1} text_in_one_pass_bytes={most_copied}",
                started.elapsed().as_secs_f64() * 1000.0,
                locked(&index.longest).as_secs_f64() * 1000.0
            );
        }
    }
}
