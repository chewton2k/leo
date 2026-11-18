use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use leo_core::attachments;
use leo_core::notes::Note;
use leo_core::store::{Changed, Store};

pub struct Picture {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub type Seer = Arc<dyn Fn(&str, &str, Vec<Picture>) -> Result<String> + Send + Sync>;

pub const DESCRIBE: &str = "You describe pictures from someone's study notes for a reader who cannot see them. Say what kind of picture it is, then its content: any text, labels and numbers, the structure of a diagram, a chart's axes and trend, code, or the steps shown, and what it explains. Use interpretable language. Two to six sentences. Reply with the description only.";
const MOST_PICTURE_BYTES: u64 = 20 * 1024 * 1024;
const RETRY_FAILED_AFTER: Duration = Duration::from_secs(60 * 60);
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);

pub struct Captions {
    db: Arc<crate::db::Db>,
    failed: Mutex<HashMap<String, Instant>>,
    pruned: Mutex<Option<Instant>>,
    wait: Duration,
    generation: AtomicU64,
    scanned: Mutex<BTreeMap<String, Vec<PathBuf>>>,
}

impl Captions {
    pub fn for_notes(notes_dir: &Path) -> Captions {
        let base = crate::db::beside_of(notes_dir, None);
        Captions::at(base.join("captions.json"))
    }

    pub fn at(path: PathBuf) -> Captions {
        let db = crate::db::beside(&path);
        let old: Option<BTreeMap<String, String>> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        if let Some(old) = old {
            let saved = db.with(|c| {
                let tx = c.transaction()?;
                for (key, caption) in &old {
                    tx.execute(
                        "INSERT OR IGNORE INTO captions (key, caption) VALUES (?1, ?2)",
                        [key, caption],
                    )?;
                }
                tx.commit()
            });
            if saved.is_ok() {
                crate::db::put_aside(&path);
            }
        }
        Captions {
            db,
            failed: Mutex::default(),
            pruned: Mutex::default(),
            wait: RETRY_FAILED_AFTER,
            generation: AtomicU64::new(0),
            scanned: Mutex::default(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn moved(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn waiting(&self) -> bool {
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        let before = failed.len();
        failed.retain(|_, at| at.elapsed() < self.wait);
        if failed.len() != before {
            self.moved();
        }
        !failed.is_empty()
    }

    fn failed_lately(&self, key: &str) -> bool {
        self.failed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .is_some_and(|at| at.elapsed() < self.wait)
    }

    pub fn get(&self, key: &str) -> Option<String> {
        use rusqlite::OptionalExtension;
        self.db
            .with(|c| {
                c.query_row("SELECT caption FROM captions WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()
            })
            .ok()
            .flatten()
    }

    pub fn put(&self, key: String, caption: String) {
        let _ = self.db.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO captions (key, caption) VALUES (?1, ?2)",
                [&key, &caption],
            )
        });
        self.moved();
    }

    fn keys(&self) -> HashSet<String> {
        self.db
            .with(|c| {
                let mut statement = c.prepare("SELECT key FROM captions")?;
                let keys = statement
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<HashSet<String>>>();
                keys
            })
            .unwrap_or_default()
    }

    pub fn forget_stale(&self) {
        {
            let mut pruned = self.pruned.lock().unwrap_or_else(|e| e.into_inner());
            if pruned.is_some_and(|at| at.elapsed() < PRUNE_EVERY) {
                return;
            }
            *pruned = Some(Instant::now());
        }
        let stored = self.keys();
        let stale: Vec<&String> = stored
            .iter()
            .filter(|key| {
                key.rsplitn(3, ':')
                    .nth(2)
                    .is_none_or(|path| key_of(Path::new(path)).as_ref() != Some(*key))
            })
            .collect();
        if stale.is_empty() {
            return;
        }
        let _ = self.db.with(|c| {
            let tx = c.transaction()?;
            for key in stale {
                tx.execute("DELETE FROM captions WHERE key = ?1", [key])?;
            }
            tx.commit()
        });
        self.moved();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.db
            .with(|c| c.query_row("SELECT COUNT(*) FROM captions", [], |r| r.get::<_, i64>(0)))
            .map_or(0, |n| n.max(0) as usize)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn key_of(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{}:{}:{modified}", path.display(), meta.len()))
}

pub fn pictures_of(notes_dir: &Path, dir: &str, body: &str) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for line in body.lines() {
        for (_, shown) in attachments::pictures_in(line) {
            if let Some(path) = attachments::resolve(notes_dir, dir, &shown.target) {
                if !out.iter().any(|(_, p)| *p == path) {
                    out.push((shown.alt.clone(), path));
                }
            }
        }
    }
    out
}

pub fn captioned(notes_dir: &Path, dir: &str, body: &str, captions: &Captions) -> String {
    let mut out = String::with_capacity(body.len());
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let found = attachments::pictures_in(line);
        if found.is_empty() {
            out.push_str(line);
            continue;
        }
        let mut at = 0;
        for (range, shown) in found {
            out.push_str(&line[at..range.end]);
            at = range.end;
            let caption = attachments::resolve(notes_dir, dir, &shown.target)
                .and_then(|path| key_of(&path))
                .and_then(|key| captions.get(&key));
            if let Some(caption) = caption {
                out.push_str(&format!(" [Picture: {caption}]"));
            }
        }
        out.push_str(&line[at..]);
    }
    out
}

pub fn picture(path: &Path) -> Option<Picture> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MOST_PICTURE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = attachments::mime_of(attachments::kind_of(&bytes)?)?.to_string();
    Some(Picture { mime, bytes })
}

pub fn uncaptioned(
    store: &Store,
    captions: &Captions,
    most: usize,
    changed: &Changed,
) -> Vec<PathBuf> {
    uncaptioned_with(store, captions, most, changed, |note| {
        pictures_of(&store.notes_dir, &note.directory, &note.body)
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    })
}

fn uncaptioned_with(
    store: &Store,
    captions: &Captions,
    most: usize,
    changed: &Changed,
    read: impl Fn(&Note) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut scanned = captions.scanned.lock().unwrap_or_else(|e| e.into_inner());
    let look = |note: &Note, scanned: &mut BTreeMap<String, Vec<PathBuf>>| {
        let paths = read(note);
        if !paths.is_empty() {
            scanned.insert(note.id.clone(), paths);
        }
    };
    match changed {
        Changed::Everything => {
            scanned.clear();
            for note in &store.notes {
                look(note, &mut scanned);
            }
        }
        Changed::Ids(ids) if !ids.is_empty() => {
            let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
            for id in &wanted {
                scanned.remove(*id);
            }
            for note in store
                .notes
                .iter()
                .filter(|n| wanted.contains(n.id.as_str()))
            {
                look(note, &mut scanned);
            }
        }
        Changed::Ids(_) => {}
    }
    let mut out: Vec<PathBuf> = Vec::new();
    if scanned.is_empty() {
        return out;
    }
    let stored = captions.keys();
    for path in scanned.values().flatten() {
        if out.len() >= most {
            break;
        }
        let Some(key) = key_of(path) else {
            continue;
        };
        if !stored.contains(&key) && !captions.failed_lately(&key) && !out.contains(path) {
            out.push(path.clone());
        }
    }
    out
}

pub fn may_forget(store: &Store) -> bool {
    !store.notes.is_empty() && store.unreadable.is_empty()
}

pub fn describe(path: &Path, seer: &Seer, question: &str) -> Result<String> {
    let picture = picture(path).ok_or_else(|| anyhow::anyhow!("that picture cannot be read"))?;
    let user = if question.trim().is_empty() {
        "Describe this picture.".to_string()
    } else {
        format!(
            "Describe this picture, and answer this about it: {}",
            question.trim()
        )
    };
    let said = seer(DESCRIBE, &user, vec![picture])?;
    Ok(said.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
pub fn caption_some(store: &Store, captions: &Captions, seer: &Seer, most: usize) -> usize {
    caption_paths(
        &uncaptioned(store, captions, most, &Changed::Everything),
        captions,
        seer,
    )
}

pub fn caption_paths(paths: &[PathBuf], captions: &Captions, seer: &Seer) -> usize {
    let mut done = 0;
    for path in paths {
        let path = path.as_path();
        let Some(key) = key_of(path) else {
            continue;
        };
        match describe(path, seer, "") {
            Ok(caption) if !caption.is_empty() => {
                captions.put(key, caption);
                done += 1;
            }
            _ => {
                captions
                    .failed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key, Instant::now());
                captions.moved();
                break;
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 64]);
        bytes
    }

    #[test]
    fn a_picture_that_cannot_be_captioned_is_set_aside_so_the_next_one_gets_its_turn() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        std::fs::create_dir_all(notes.join("attachments")).unwrap();
        std::fs::write(notes.join("attachments/a.png"), png()).unwrap();
        let mut other = png();
        other.push(1);
        std::fs::write(notes.join("attachments/b.png"), other).unwrap();
        store
            .create_note(
                "Two",
                "![a](attachments/a.png) ![b](attachments/b.png)",
                vec![],
                "",
            )
            .unwrap();
        let captions = Captions::for_notes(&notes);
        let failing: Seer = Arc::new(|_: &str, _: &str, _: Vec<Picture>| anyhow::bail!("no"));
        assert!(!captions.waiting());
        assert_eq!(
            uncaptioned(&store, &captions, 10, &Changed::Everything).len(),
            2
        );
        assert_eq!(caption_some(&store, &captions, &failing, 10), 0);
        assert_eq!(
            uncaptioned(&store, &captions, 10, &Changed::Everything).len(),
            1
        );
        assert_eq!(caption_some(&store, &captions, &failing, 10), 0);
        assert!(uncaptioned(&store, &captions, 10, &Changed::Everything).is_empty());
        assert!(captions.waiting(), "the loop comes back for them");
    }

    fn library_of_three(notes: &Path) -> Store {
        let mut store = Store::load_from(notes).unwrap();
        std::fs::create_dir_all(notes.join("attachments")).unwrap();
        std::fs::write(notes.join("attachments/a.png"), png()).unwrap();
        store
            .create_note("Pictured", "![a](attachments/a.png)", vec![], "")
            .unwrap();
        store.create_note("Plain", "words", vec![], "").unwrap();
        store
            .create_note("Other", "more words", vec![], "")
            .unwrap();
        store
    }

    fn reads_in(
        store: &Store,
        captions: &Captions,
        changed: &Changed,
        reads: &std::cell::Cell<usize>,
    ) -> Vec<PathBuf> {
        uncaptioned_with(store, captions, 10, changed, |note| {
            reads.set(reads.get() + 1);
            pictures_of(&store.notes_dir, &note.directory, &note.body)
                .into_iter()
                .map(|(_, path)| path)
                .collect()
        })
    }

    #[test]
    fn an_unchanged_library_is_not_scanned_again() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = library_of_three(&notes);
        let captions = Captions::for_notes(&notes);
        let reads = std::cell::Cell::new(0);
        let (seen, changed) = store.changes_since(0);
        assert_eq!(reads_in(&store, &captions, &changed, &reads).len(), 1);
        assert_eq!(reads.get(), 3);
        for _ in 0..3 {
            let (next, changed) = store.changes_since(seen);
            assert_eq!(next, seen);
            assert_eq!(reads_in(&store, &captions, &changed, &reads).len(), 1);
        }
        assert_eq!(reads.get(), 3, "nothing changed, so no note was read");
        let plain = store
            .notes
            .iter()
            .find(|n| n.title == "Plain")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&plain).unwrap().body = "other words".into();
        let (_, changed) = store.changes_since(seen);
        assert_eq!(reads_in(&store, &captions, &changed, &reads).len(), 1);
        assert_eq!(reads.get(), 4, "only the edited note was read");
    }

    #[test]
    fn a_picture_added_to_a_note_is_found_and_one_taken_out_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = library_of_three(&notes);
        let captions = Captions::for_notes(&notes);
        let (mut seen, changed) = store.changes_since(0);
        assert_eq!(uncaptioned(&store, &captions, 10, &changed).len(), 1);
        let pictured = store
            .notes
            .iter()
            .find(|n| n.title == "Pictured")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&pictured).unwrap().body = "no picture now".into();
        let (next, changed) = store.changes_since(seen);
        seen = next;
        assert!(uncaptioned(&store, &captions, 10, &changed).is_empty());
        std::fs::write(notes.join("attachments/b.png"), [png(), vec![1]].concat()).unwrap();
        let plain = store
            .notes
            .iter()
            .find(|n| n.title == "Plain")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&plain).unwrap().body = "![b](attachments/b.png)".into();
        let (next, changed) = store.changes_since(seen);
        seen = next;
        assert_eq!(uncaptioned(&store, &captions, 10, &changed).len(), 1);
        store.delete_notes(&[plain]);
        let (_, changed) = store.changes_since(seen);
        assert!(uncaptioned(&store, &captions, 10, &changed).is_empty());
    }

    #[test]
    fn catching_up_from_the_feed_finds_what_a_full_pass_finds() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        std::fs::create_dir_all(notes.join("attachments")).unwrap();
        for i in 0..6u8 {
            std::fs::write(
                notes.join(format!("attachments/p{i}.png")),
                [png(), vec![i]].concat(),
            )
            .unwrap();
        }
        let captions = Captions::for_notes(&notes);
        let (mut seen, changed) = store.changes_since(0);
        uncaptioned(&store, &captions, usize::MAX, &changed);
        let mut ids = Vec::new();
        for step in 0..24usize {
            match step % 4 {
                0 => {
                    let body = format!("![p](attachments/p{}.png)", step % 6);
                    ids.push(
                        store
                            .create_note(format!("Note {step}"), body, vec![], "")
                            .unwrap()
                            .id
                            .clone(),
                    );
                }
                1 => {
                    let id = ids[step % ids.len()].clone();
                    store.find_note_mut(&id).unwrap().body =
                        format!("![p](attachments/p{}.png) again", (step + 1) % 6);
                }
                2 => {
                    let id = ids[step % ids.len()].clone();
                    store.find_note_mut(&id).unwrap().body = "plain words".into();
                }
                _ => {
                    let id = ids.remove(step % ids.len());
                    store.delete_notes(&[id]);
                }
            }
            if step % 3 == 2 {
                captions.put(
                    key_of(&notes.join(format!("attachments/p{}.png", step % 6))).unwrap(),
                    "seen".into(),
                );
            }
            let (next, changed) = store.changes_since(seen);
            seen = next;
            let fresh = Captions::for_notes(&notes);
            let from_feed = uncaptioned(&store, &captions, usize::MAX, &changed);
            let from_scratch = uncaptioned(&store, &fresh, usize::MAX, &Changed::Everything);
            let mut from_feed = from_feed;
            let mut from_scratch = from_scratch;
            from_feed.sort();
            from_scratch.sort();
            assert_eq!(from_feed, from_scratch, "step {step}");
        }
    }

    #[test]
    fn a_failed_picture_is_offered_again_when_its_wait_ends() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let store = library_of_three(&notes);
        let mut captions = Captions::for_notes(&notes);
        captions.wait = Duration::from_millis(50);
        let failing: Seer = Arc::new(|_: &str, _: &str, _: Vec<Picture>| anyhow::bail!("no"));
        let before = captions.generation();
        let paths = uncaptioned(&store, &captions, 10, &Changed::Everything);
        assert_eq!(caption_paths(&paths, &captions, &failing), 0);
        let failed_at = captions.generation();
        assert!(failed_at > before, "a failure moves the stamp");
        assert!(captions.waiting());
        assert!(uncaptioned(&store, &captions, 10, &Changed::Ids(vec![])).is_empty());
        std::thread::sleep(Duration::from_millis(80));
        assert!(!captions.waiting());
        assert!(
            captions.generation() > failed_at,
            "the wait ending moves the stamp"
        );
        assert_eq!(
            uncaptioned(&store, &captions, 10, &Changed::Ids(vec![])).len(),
            1
        );
    }

    #[test]
    fn stale_captions_are_dropped_and_current_ones_kept() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        let attachments = notes.join("attachments");
        std::fs::create_dir_all(&attachments).unwrap();
        let shown = attachments.join("a.png");
        let unshown = attachments.join("b.png");
        let deleted = attachments.join("c.png");
        for path in [&shown, &unshown, &deleted] {
            std::fs::write(path, png()).unwrap();
        }
        store
            .create_note("One", "![a](attachments/a.png)", vec![], "")
            .unwrap();
        let captions = Captions::for_notes(&notes);
        let live = key_of(&shown).unwrap();
        let elsewhere = key_of(&unshown).unwrap();
        let gone = key_of(&deleted).unwrap();
        std::fs::remove_file(&deleted).unwrap();
        captions.put(live.clone(), "kept".into());
        captions.put(elsewhere.clone(), "trashed note".into());
        captions.put(gone.clone(), "deleted".into());
        captions.put(format!("{}:1:0", shown.display()), "changed".into());
        assert!(uncaptioned(&store, &captions, 10, &Changed::Everything).is_empty());
        assert!(may_forget(&store));
        captions.forget_stale();
        assert_eq!(captions.get(&live).as_deref(), Some("kept"));
        assert_eq!(captions.get(&elsewhere).as_deref(), Some("trashed note"));
        assert_eq!(captions.get(&gone), None);
        assert_eq!(captions.len(), 2);
    }

    #[test]
    fn pictures_get_a_caption_once_and_felix_reads_it_inline() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        std::fs::create_dir_all(notes.join("attachments")).unwrap();
        std::fs::write(notes.join("attachments/heap.png"), png()).unwrap();
        store
            .create_note(
                "Heaps",
                "A heap:\n![min heap](attachments/heap.png) and ![[missing.png]]",
                vec![],
                "",
            )
            .unwrap();
        let captions = Captions::for_notes(&notes);
        assert_eq!(
            uncaptioned(&store, &captions, 10, &Changed::Everything).len(),
            1
        );
        let calls = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&calls);
        let seer: Seer = Arc::new(move |system: &str, user: &str, pictures: Vec<Picture>| {
            assert!(system.contains("Use interpretable language"));
            assert_eq!(user, "Describe this picture.");
            assert_eq!(pictures[0].mime, "image/png");
            *counted.lock().unwrap() += 1;
            Ok("A min heap\n with 2 at the root.".into())
        });
        assert_eq!(caption_some(&store, &captions, &seer, 10), 1);
        assert_eq!(
            caption_some(&store, &captions, &seer, 10),
            0,
            "already captioned"
        );
        assert_eq!(*calls.lock().unwrap(), 1);
        let body = &store.notes[0].body;
        assert_eq!(
            captioned(&notes, "", body, &Captions::for_notes(&notes)),
            "A heap:\n![min heap](attachments/heap.png) [Picture: A min heap with 2 at the root.] and ![[missing.png]]",
            "captions are kept on disk"
        );
    }

    #[test]
    #[ignore = "benchmark: run by hand with --ignored --nocapture, release"]
    fn p_c10_benchmark_idle_tick() {
        use std::time::Instant;
        let n: usize = std::env::var("LEO_BENCH_NOTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        for i in 0..n {
            let folder = notes.join(format!("Course {}", i % 20));
            std::fs::create_dir_all(&folder).unwrap();
            let mut body = String::new();
            while body.len() < 5000 {
                body.push_str(&format!(
                    "Lecture {i} words about graphs and queues {} ",
                    body.len()
                ));
            }
            std::fs::write(folder.join(format!("Lecture {i}.md")), body).unwrap();
        }
        let mut store = Store::load_from(&notes).unwrap();
        assert_eq!(store.notes.len(), n);
        let captions = Captions::for_notes(&notes);
        let writer: crate::Writer = Arc::new(|_: &str, _: &str, _: u32| anyhow::bail!("no"));
        let graphs = Arc::new(crate::graph::Graphs::for_notes(&notes, Some(writer)));
        let mut cache = crate::graph::Cache::default();
        for note in &store.notes {
            cache.notes.insert(
                note.id.clone(),
                crate::graph::Read {
                    hash: crate::vectors::hash_of(note),
                    summary: format!("A summary of {}", note.title),
                    concepts: vec!["queue".into(), "heap".into()],
                    linked: None,
                },
            );
        }
        cache.built_at = Some("2026-11-17T00:00:00Z".into());
        std::fs::write(graphs.path(), serde_json::to_string(&cache).unwrap()).unwrap();
        graphs.load();
        let idle = Duration::from_secs(60);
        let time = |mut f: Box<dyn FnMut() + '_>| {
            let mut v: Vec<f64> = (0..7)
                .map(|_| {
                    let at = Instant::now();
                    f();
                    at.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[3]
        };
        let (mut seen, first) = store.changes_since(0);
        uncaptioned(&store, &captions, 4, &first);
        let plan = graphs.status(&crate::graph::sources(&store.notes));
        let key = (store.fingerprint(), graphs.inputs());
        let caption_none = time(Box::new(|| {
            let (_, changed) = store.changes_since(seen);
            std::hint::black_box(uncaptioned(&store, &captions, 4, &changed));
        }));
        let graph_none = time(Box::new(|| {
            assert_eq!(key, (store.fingerprint(), graphs.inputs()));
            let status = graphs.live(&plan);
            std::hint::black_box((graphs.due(&status, idle), graphs.settled_by(&status)));
        }));
        let ids: Vec<String> = store.notes.iter().take(7).map(|n| n.id.clone()).collect();
        let mut caption_times = Vec::new();
        for id in &ids {
            store.find_note_mut(id).unwrap().body.push_str(" edited");
            let at = Instant::now();
            let (next, changed) = store.changes_since(seen);
            std::hint::black_box(uncaptioned(&store, &captions, 4, &changed));
            caption_times.push(at.elapsed().as_secs_f64() * 1000.0);
            seen = next;
        }
        caption_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let caption_one = caption_times[3];
        let graph_one = time(Box::new(|| {
            let sources = crate::graph::sources(&store.notes);
            std::hint::black_box(graphs.status(&sources));
        }));
        let reads = std::cell::Cell::new(0);
        for _ in 0..10 {
            let (next, changed) = store.changes_since(seen);
            seen = next;
            uncaptioned_with(&store, &captions, 4, &changed, |_| {
                reads.set(reads.get() + 1);
                Vec::new()
            });
        }
        println!("P-C10 after notes={n} caption_scan_unchanged_ms={caption_none:.4} caption_scan_one_changed_ms={caption_one:.4} graph_part_unchanged_ms={graph_none:.5} graph_part_one_changed_ms={graph_one:.3} note_reads_over_10_ticks={}", reads.get());
    }
}
