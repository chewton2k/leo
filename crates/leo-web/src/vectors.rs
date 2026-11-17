use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine;
use leo_core::notes::Note;
use leo_core::store::Store;
use serde::{Deserialize, Serialize};

pub type Meaning = Arc<dyn Fn(&[String], bool) -> Result<Vec<Vec<f32>>> + Send + Sync>;

pub const PIECE_CHARS: usize = 1200;
pub const MOST_PIECES: usize = 12;
pub const PIECES_PER_TICK: usize = 256;
pub const CLOSE_ENOUGH: f32 = 0.62;
pub const NEIGHBOURS: usize = 24;
const MOST_FOR_NEIGHBOURS: usize = 6000;

pub type Near = BTreeMap<String, std::collections::BTreeSet<String>>;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub hash: String,
    pub pieces: Vec<(usize, Vec<f32>)>,
    pub marks: BTreeMap<usize, String>,
    pub whole: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default)]
    notes: BTreeMap<String, Stored>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    hash: String,
    pieces: Vec<(usize, String)>,
}

pub struct Vectors {
    db: Arc<crate::db::Db>,
    known: Mutex<BTreeMap<String, Entry>>,
    version: std::sync::atomic::AtomicU64,
    held: Mutex<Option<Held>>,
    failed: Mutex<HashMap<String, (String, std::time::Instant)>>,
}

struct Held {
    most: usize,
    means: BTreeMap<String, Vec<f32>>,
    near: Near,
}

fn nearest_to(
    id: &str,
    v: &[f32],
    means: &BTreeMap<String, Vec<f32>>,
    most: usize,
) -> std::collections::BTreeSet<String> {
    let mut scored: Vec<(f32, &String)> = means
        .iter()
        .filter(|(other, _)| other.as_str() != id)
        .map(|(other, w)| (cosine(v, w), other))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(most)
        .map(|(_, other)| other.clone())
        .collect()
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn from_blob(bytes: &[u8]) -> Option<Vec<f32>> {
    bytes.len().is_multiple_of(4).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    })
}

fn decode(text: &str) -> Option<Vec<f32>> {
    from_blob(
        &base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()?,
    )
}

fn fnv(bytes: impl Iterator<Item = u8>) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn hash_of(note: &Note) -> String {
    fnv(note.title.bytes().chain([0]).chain(note.body.bytes()))
}

fn mark_of(piece: &str) -> String {
    fnv(piece.bytes())
}

pub fn pieces_of(note: &Note) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    let mut at = 0usize;
    for paragraph in note.body.split("\n\n") {
        let size = paragraph.chars().count();
        if !current.is_empty() && current.chars().count() + size > PIECE_CHARS {
            out.push((start, std::mem::take(&mut current)));
            start = at;
        }
        if size > PIECE_CHARS {
            let chars: Vec<char> = paragraph.chars().collect();
            for (n, part) in chars.chunks(PIECE_CHARS).enumerate() {
                out.push((at + n * PIECE_CHARS, part.iter().collect()));
            }
            start = at + size + 2;
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(paragraph);
        }
        at += size + 2;
    }
    if !current.trim().is_empty() || out.is_empty() {
        out.push((start, current));
    }
    if out.len() > 1 {
        out.retain(|(_, text)| !text.trim().is_empty());
    }
    if let Some(first) = out.first_mut() {
        first.1 = format!("{}\n\n{}", note.title, first.1).trim().to_string();
    }
    out
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub struct Work {
    pub id: String,
    pub hash: String,
    pub pieces: Vec<(usize, String)>,
    pub kept: Vec<(usize, String, Vec<f32>)>,
}

impl Vectors {
    pub fn for_notes(notes_dir: &Path) -> Vectors {
        let base = notes_dir.parent().unwrap_or(notes_dir);
        Vectors::at(base.join("meaning.json"))
    }

    pub fn at(path: PathBuf) -> Vectors {
        let db = crate::db::beside(&path);
        let old: Option<OnDisk> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        if let Some(old) = old {
            let saved = db.with(|c| {
                let tx = c.transaction()?;
                for (id, stored) in &old.notes {
                    for (at, v) in &stored.pieces {
                        if let Some(v) = decode(v) {
                            tx.execute(
                                "INSERT OR REPLACE INTO vectors (note, at, hash, v) VALUES (?1, ?2, ?3, ?4)",
                                rusqlite::params![id, *at as i64, stored.hash, to_blob(&v)],
                            )?;
                        }
                    }
                }
                tx.commit()
            });
            if saved.is_ok() {
                crate::db::put_aside(&path);
            }
        }
        let rows: Vec<(String, i64, String, Vec<u8>, String)> = db
            .with(|c| {
                let mut found =
                    c.prepare("SELECT note, at, hash, v, piece FROM vectors ORDER BY note, at")?;
                let rows = found.query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?;
                rows.collect()
            })
            .unwrap_or_default();
        let mut known: BTreeMap<String, Entry> = BTreeMap::new();
        for (note, at, hash, v, mark) in rows {
            let Some(v) = from_blob(&v) else { continue };
            let at = at.max(0) as usize;
            let entry = known.entry(note).or_insert_with(|| Entry {
                hash: hash.clone(),
                pieces: Vec::new(),
                marks: BTreeMap::new(),
                whole: false,
            });
            entry.pieces.push((at, v));
            if !mark.is_empty() {
                entry.marks.insert(at, mark);
            }
        }
        Vectors {
            db,
            known: Mutex::new(known),
            version: std::sync::atomic::AtomicU64::new(0),
            held: Mutex::new(None),
            failed: Mutex::new(HashMap::new()),
        }
    }

    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn changed(&self) {
        self.version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn neighbours(&self, most: usize) -> Near {
        let means: BTreeMap<String, Vec<f32>> = {
            let Ok(known) = self.known.lock() else {
                return Near::new();
            };
            if known.len() > MOST_FOR_NEIGHBOURS {
                return Near::new();
            }
            known
                .iter()
                .filter_map(|(id, entry)| {
                    let width = entry.pieces.first()?.1.len();
                    let mut sum = vec![0f32; width];
                    for (_, v) in &entry.pieces {
                        sum.iter_mut().zip(v).for_each(|(s, x)| *s += x);
                    }
                    let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
                    (norm > 0.0).then(|| (id.clone(), sum.into_iter().map(|x| x / norm).collect()))
                })
                .collect()
        };
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let old = held.take().filter(|old| old.most == most);
        let near: Near = {
            let moved = |id: &String| {
                old.as_ref()
                    .is_none_or(|old| old.means.get(id) != means.get(id))
            };
            let changed: Vec<&Vec<f32>> = means
                .iter()
                .filter(|(id, _)| moved(id))
                .map(|(_, v)| v)
                .collect();
            means
                .iter()
                .map(|(id, v)| {
                    let kept = old
                        .as_ref()
                        .and_then(|old| old.near.get(id))
                        .filter(|list| {
                            list.len() == most
                                && !moved(id)
                                && list.iter().all(|other| !moved(other))
                                && {
                                    let worst = list
                                        .iter()
                                        .map(|other| cosine(v, &means[other]))
                                        .fold(f32::INFINITY, f32::min);
                                    changed.iter().all(|c| cosine(v, c) < worst)
                                }
                        });
                    let list = kept
                        .cloned()
                        .unwrap_or_else(|| nearest_to(id, v, &means, most));
                    (id.clone(), list)
                })
                .collect()
        };
        *held = Some(Held {
            most,
            means,
            near: near.clone(),
        });
        near
    }

    pub fn len(&self) -> usize {
        self.known.lock().map(|k| k.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stale(&self, store: &Store, most_pieces: usize) -> Vec<Work> {
        let Ok(mut known) = self.known.lock() else {
            return Vec::new();
        };
        let mut failed = self.failed.lock().unwrap_or_else(|e| e.into_inner());
        failed.retain(|_, (_, at)| at.elapsed() < crate::graph::RETRY_AUTO_AFTER);
        let mut out = Vec::new();
        let mut pieces = 0;
        for note in &store.notes {
            if pieces >= most_pieces {
                break;
            }
            let hash = hash_of(note);
            if failed.get(&note.id).is_some_and(|(was, _)| *was == hash) {
                continue;
            }
            if known
                .get(&note.id)
                .is_some_and(|e| e.whole && e.hash == hash)
            {
                continue;
            }
            let entry = known.get(&note.id);
            let current = entry.is_some_and(|e| e.hash == hash);
            let mut found = pieces_of(note);
            let mut kept = Vec::new();
            if let Some(entry) = entry {
                if current {
                    found.retain(|(at, _)| !entry.pieces.iter().any(|(indexed, _)| indexed == at));
                } else {
                    found.retain(|(at, text)| {
                        let mark = mark_of(text);
                        let old = entry.marks.iter().find(|(_, known)| **known == mark);
                        let vector = old.and_then(|(was, _)| {
                            entry.pieces.iter().find(|(indexed, _)| indexed == was)
                        });
                        match vector {
                            Some((_, v)) => {
                                kept.push((*at, mark, v.clone()));
                                false
                            }
                            None => true,
                        }
                    });
                }
            }
            if current && found.is_empty() {
                if let Some(entry) = known.get_mut(&note.id) {
                    entry.whole = true;
                }
            }
            found.truncate(most_pieces.saturating_sub(pieces));
            if found.is_empty() && kept.is_empty() {
                continue;
            }
            pieces += found.len();
            out.push(Work {
                id: note.id.clone(),
                hash,
                pieces: found,
                kept,
            });
        }
        out
    }

    pub fn forget_gone(&self, store: &Store) -> usize {
        let Ok(mut known) = self.known.lock() else {
            return 0;
        };
        let live: std::collections::HashSet<&str> =
            store.notes.iter().map(|n| n.id.as_str()).collect();
        let gone: Vec<String> = known
            .keys()
            .filter(|id| !live.contains(id.as_str()))
            .cloned()
            .collect();
        if gone.is_empty() {
            return 0;
        }
        for id in &gone {
            known.remove(id);
        }
        drop(known);
        let _ = self.db.with(|c| {
            let tx = c.transaction()?;
            for id in &gone {
                tx.execute("DELETE FROM vectors WHERE note = ?1", [id])?;
            }
            tx.commit()
        });
        self.changed();
        gone.len()
    }

    pub fn put(&self, work: &[Work], vectors: Vec<Vec<f32>>) {
        let Ok(mut known) = self.known.lock() else {
            return;
        };
        let mut given = vectors.into_iter();
        for item in work {
            let mut fresh: Vec<(usize, String, Vec<f32>)> = item
                .pieces
                .iter()
                .filter_map(|(at, text)| given.next().map(|v| (*at, mark_of(text), v)))
                .collect();
            if fresh.len() != item.pieces.len() {
                continue;
            }
            let merge = known.get(&item.id).is_some_and(|e| e.hash == item.hash);
            if !merge {
                fresh.extend(item.kept.iter().cloned());
            }
            let saved = self.db.with(|c| {
                let tx = c.transaction()?;
                if !merge { tx.execute("DELETE FROM vectors WHERE note = ?1", [&item.id])?; }
                for (at, mark, v) in &fresh {
                    tx.execute(
                        "INSERT OR REPLACE INTO vectors (note, at, hash, v, piece) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![item.id, *at as i64, item.hash, to_blob(v), mark],
                    )?;
                }
                tx.commit()
            });
            if saved.is_err() {
                continue;
            }
            let mut entry = known
                .remove(&item.id)
                .filter(|_| merge)
                .unwrap_or_else(|| Entry {
                    hash: item.hash.clone(),
                    pieces: Vec::new(),
                    marks: BTreeMap::new(),
                    whole: false,
                });
            entry.whole = false;
            for (at, mark, v) in fresh {
                entry.marks.insert(at, mark);
                entry.pieces.push((at, v));
            }
            entry.pieces.sort_by_key(|(at, _)| *at);
            known.insert(item.id.clone(), entry);
        }
        drop(known);
        self.changed();
    }

    pub fn nearest(&self, query: &[f32], most: usize, least: f32) -> Vec<(String, f32)> {
        let Ok(known) = self.known.lock() else {
            return Vec::new();
        };
        let mut scored: Vec<(String, f32)> = known
            .iter()
            .filter_map(|(id, entry)| {
                entry
                    .pieces
                    .iter()
                    .map(|(_, v)| cosine(query, v))
                    .fold(None, |best: Option<f32>, s| {
                        Some(best.map_or(s, |b| b.max(s)))
                    })
                    .filter(|s| *s >= least)
                    .map(|s| (id.clone(), s))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(most);
        scored
    }
}

pub fn close_to(
    meaning: Option<&Meaning>,
    vectors: &Vectors,
    text: &str,
    most: usize,
) -> Vec<(String, f32)> {
    let Some(meaning) = meaning else {
        return Vec::new();
    };
    if vectors.is_empty() || text.trim().is_empty() {
        return Vec::new();
    }
    meaning(&[text.to_string()], true)
        .ok()
        .and_then(|mut v| v.pop())
        .map(|q| vectors.nearest(&q, most, CLOSE_ENOUGH))
        .unwrap_or_default()
}

pub fn catch_up(store: &Store, vectors: &Vectors, meaning: &Meaning, most_pieces: usize) -> usize {
    let work = vectors.stale(store, most_pieces);
    vectors.forget_gone(store);
    read_in(vectors, meaning, &work)
}

pub fn read_in(vectors: &Vectors, meaning: &Meaning, work: &[Work]) -> usize {
    if work.is_empty() {
        return 0;
    }
    let texts: Vec<String> = work
        .iter()
        .flat_map(|w| w.pieces.iter().map(|(_, t)| t.clone()))
        .collect();
    let found = if texts.is_empty() {
        Ok(Vec::new())
    } else {
        meaning(&texts, false)
    };
    match found {
        Ok(found) if found.len() == texts.len() => {
            vectors.put(work, found);
            let mut failed = vectors.failed.lock().unwrap_or_else(|e| e.into_inner());
            work.iter().for_each(|w| {
                failed.remove(&w.id);
            });
            work.len()
        }
        _ if work.len() > 1 => work
            .iter()
            .map(|one| read_in(vectors, meaning, std::slice::from_ref(one)))
            .sum(),
        _ => {
            vectors
                .failed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(
                    work[0].id.clone(),
                    (work[0].hash.clone(), std::time::Instant::now()),
                );
            0
        }
    }
}

#[cfg(test)]
pub fn fake_meaning() -> Meaning {
    tests::fake()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fake() -> Meaning {
        Arc::new(|texts: &[String], _query: bool| {
            Ok(texts
                .iter()
                .map(|t| {
                    let t = t.to_lowercase();
                    let mut v = vec![
                        f32::from(u8::from(
                            t.contains("queue") || t.contains("bfs") || t.contains("breadth"),
                        )),
                        f32::from(u8::from(t.contains("plant") || t.contains("light"))),
                        0.1,
                    ];
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                    v.iter_mut().for_each(|x| *x /= norm);
                    v
                })
                .collect())
        })
    }

    fn note(title: &str, body: &str) -> Note {
        let mut store =
            Store::load_from(&tempfile::tempdir().unwrap().keep().join("notes")).unwrap();
        store.create_note(title, body, vec![], "").unwrap().clone()
    }

    #[test]
    fn an_edit_reads_only_the_pieces_whose_text_changed_even_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let part = |c: &str| c.repeat(PIECE_CHARS - 100);
        let body = |last: &str| format!("{}\n\n{}\n\n{}", part("a"), part("b"), last);
        let id = store
            .create_note("Three", body(&part("c")), vec![], "")
            .unwrap()
            .id
            .clone();
        let read = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&read);
        let counting: Meaning = Arc::new(move |texts: &[String], query: bool| {
            seen.lock().unwrap().push(texts.len());
            fake()(texts, query)
        });
        let vectors = Vectors::for_notes(&store.notes_dir);
        catch_up(&store, &vectors, &counting, 100);
        assert_eq!(*read.lock().unwrap(), [3]);

        store.find_note_mut(&id).unwrap().body = body(&part("d"));
        catch_up(&store, &vectors, &counting, 100);
        assert_eq!(*read.lock().unwrap(), [3, 1]);
        assert_eq!(vectors.known.lock().unwrap()[&id].pieces.len(), 3);
        assert!(vectors.stale(&store, 100).is_empty());

        let ends = |vectors: &Vectors| {
            let known = vectors.known.lock().unwrap();
            let pieces = &known[&id].pieces;
            (pieces[1].1.clone(), pieces[2].1.clone())
        };
        let before = ends(&vectors);
        store.find_note_mut(&id).unwrap().body = format!("intro\n\n{}", body(&part("d")));
        catch_up(&store, &vectors, &counting, 100);
        assert_eq!(
            *read.lock().unwrap(),
            [3, 1, 1],
            "pieces that only moved keep their vectors"
        );
        assert_eq!(ends(&vectors), before);
        assert!(vectors.stale(&store, 100).is_empty());

        let vectors = Vectors::for_notes(&store.notes_dir);
        store.find_note_mut(&id).unwrap().body = format!("{}\n\n{}", part("a"), part("b"));
        catch_up(&store, &vectors, &counting, 100);
        assert_eq!(
            *read.lock().unwrap(),
            [3, 1, 1, 1],
            "only the first piece, which lost its intro, is read again"
        );
        assert_eq!(vectors.known.lock().unwrap()[&id].pieces.len(), 2);
        assert!(vectors.stale(&store, 100).is_empty());
        let again = Vectors::for_notes(&store.notes_dir);
        assert_eq!(again.known.lock().unwrap()[&id].pieces.len(), 2);
    }

    #[test]
    fn a_note_that_cannot_be_embedded_does_not_stop_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        for n in 0..100 {
            let body = if n == 0 {
                "POISON".to_string()
            } else {
                format!("note {n}")
            };
            store
                .create_note(format!("Note {n}"), body, vec![], "")
                .unwrap();
        }
        let tries = Arc::new(Mutex::new(0usize));
        let seen = Arc::clone(&tries);
        let failing: Meaning = Arc::new(move |texts: &[String], query: bool| {
            if texts.iter().any(|t| t.contains("POISON")) {
                *seen.lock().unwrap() += 1;
                anyhow::bail!("this text cannot be read");
            }
            fake()(texts, query)
        });
        let vectors = Vectors::for_notes(&store.notes_dir);
        for _ in 0..3 {
            let work = vectors.stale(&store, PIECES_PER_TICK);
            read_in(&vectors, &failing, &work);
        }
        assert_eq!(vectors.len(), 99);
        assert!(*tries.lock().unwrap() <= 2);
        assert!(vectors.stale(&store, PIECES_PER_TICK).is_empty());

        let bad = store
            .notes
            .iter()
            .find(|n| n.body == "POISON")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&bad).unwrap().body = "fixed".into();
        let work = vectors.stale(&store, PIECES_PER_TICK);
        assert_eq!(work.len(), 1, "an edited note is tried again at once");
        read_in(&vectors, &failing, &work);
        assert_eq!(vectors.len(), 100);
    }

    #[test]
    fn a_fully_indexed_library_is_not_split_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let ids: Vec<String> = ["One", "Two", "Three"]
            .iter()
            .map(|title| {
                store
                    .create_note(*title, format!("{title} text"), vec![], "")
                    .unwrap()
                    .id
                    .clone()
            })
            .collect();
        let vectors = Vectors::for_notes(&store.notes_dir);
        catch_up(&store, &vectors, &fake(), 100);
        assert!(vectors.stale(&store, 100).is_empty());
        assert!(vectors.stale(&store, 100).is_empty());
        assert!(vectors.known.lock().unwrap().values().all(|e| e.whole));

        store.find_note_mut(&ids[1]).unwrap().body = "changed".into();
        let work = vectors.stale(&store, 100);
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].id, ids[1]);
    }

    #[test]
    fn long_notes_are_indexed_to_the_end_in_bounded_batches_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let body = format!(
            "{}\n\nPlants turn light into sugar.",
            "x".repeat(PIECE_CHARS * 30)
        );
        let id = store
            .create_note("Long", body, vec![], "")
            .unwrap()
            .id
            .clone();
        let vectors = Vectors::for_notes(&store.notes_dir);
        for _ in 0..4 {
            let work = vectors.stale(&store, 8);
            assert!(work.iter().map(|w| w.pieces.len()).sum::<usize>() <= 8);
            read_in(&vectors, &fake(), &work);
        }
        let again = Vectors::for_notes(&store.notes_dir);
        assert!(again.stale(&store, 8).is_empty());
        assert_eq!(
            close_to(Some(&fake()), &again, "plants and light", 5)[0].0,
            id
        );
    }

    #[test]
    fn nearest_notes_updated_after_a_few_changes_match_a_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let vectors = Vectors::at(dir.path().join("meaning.json"));
        let mut seed = 7u32;
        let mut vector = move || -> Vec<f32> {
            let mut v: Vec<f32> = (0..8)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
                })
                .collect();
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            v
        };
        let mut place = |id: String, hash: &str| {
            let work = Work {
                id,
                hash: hash.into(),
                pieces: vec![(0, "text".into())],
                kept: Vec::new(),
            };
            vectors.put(&[work], vec![vector()]);
        };
        for n in 0..60 {
            place(format!("note-{n:02}"), "first");
        }
        let full = |vectors: &Vectors| {
            *vectors.held.lock().unwrap() = None;
            vectors.neighbours(5)
        };
        assert_eq!(vectors.neighbours(5), full(&vectors));
        for round in 0..6 {
            for n in [3 + round, 20 + round * 2, 59 - round] {
                place(format!("note-{n:02}"), &format!("round-{round}"));
            }
            place(format!("new-{round}"), "first");
            vectors
                .known
                .lock()
                .unwrap()
                .remove(&format!("note-{:02}", 40 + round));
            let updated = vectors.neighbours(5);
            assert_eq!(updated, full(&vectors), "round {round}");
            assert_eq!(updated.len(), 60);
        }
    }

    #[test]
    fn every_note_knows_its_nearest_notes_and_the_list_is_rebuilt_only_after_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let a = store
            .create_note("BFS", "queue", vec![], "")
            .unwrap()
            .id
            .clone();
        let b = store
            .create_note("Breadth first", "queue order", vec![], "")
            .unwrap()
            .id
            .clone();
        let c = store
            .create_note("Plants", "light", vec![], "")
            .unwrap()
            .id
            .clone();
        let vectors = Vectors::for_notes(&store.notes_dir);
        let before = vectors.version();
        catch_up(&store, &vectors, &fake(), 100);
        assert!(vectors.version() > before);
        let near = vectors.neighbours(1);
        assert_eq!(near[&a].iter().next(), Some(&b));
        assert_eq!(near[&c].len(), 1);
        let graphs = crate::graph::Graphs::new(dir.path().join("graph.json"), None)
            .with_vectors(Arc::new(Vectors::for_notes(&store.notes_dir)));
        let first = graphs.near().unwrap();
        assert!(
            Arc::ptr_eq(&first, &graphs.near().unwrap()),
            "kept until the vectors change"
        );
    }

    #[test]
    fn a_note_is_cut_into_pieces_at_paragraphs_with_the_title_first() {
        let body = format!(
            "{}\n\n{}\n\n{}",
            "a".repeat(700),
            "b".repeat(700),
            "c".repeat(3000)
        );
        let pieces = pieces_of(&note("Heaps", &body));
        assert!(pieces[0].1.starts_with("Heaps\n\naaa"));
        assert_eq!(pieces[1].0, 702, "pieces remember where they start");
        assert!(pieces
            .iter()
            .all(|(_, t)| t.chars().count() <= PIECE_CHARS + 10));
        assert_eq!(pieces.len(), 5);
        assert_eq!(pieces_of(&note("Empty", "")).len(), 1);
        let long: String = (0..40)
            .map(|i| format!("{} {i}\n\n", "x".repeat(900)))
            .collect();
        assert!(pieces_of(&note("Long", &long)).len() > MOST_PIECES);
        let wide = "é".repeat(2500);
        assert_eq!(
            pieces_of(&note("Accents", &wide)).len(),
            3,
            "cut by characters, never bytes"
        );
    }

    #[test]
    fn notes_are_read_once_and_found_by_meaning_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let bfs = store
            .create_note(
                "Graph search",
                "Breadth first takes the oldest from a queue.",
                vec![],
                "",
            )
            .unwrap()
            .id
            .clone();
        store
            .create_note("Biology", "Plants turn light into sugar.", vec![], "")
            .unwrap();
        let vectors = Vectors::for_notes(&store.notes_dir);
        let meaning = fake();
        assert_eq!(catch_up(&store, &vectors, &meaning, 100), 2);
        assert_eq!(
            catch_up(&store, &vectors, &meaning, 100),
            0,
            "nothing changed"
        );
        let again = Vectors::for_notes(&store.notes_dir);
        assert_eq!(again.len(), 2);
        let close = close_to(Some(&meaning), &again, "how does BFS work?", 5);
        assert_eq!(close.len(), 1);
        assert_eq!(close[0].0, bfs);
        store.find_note_mut(&bfs).unwrap().body = "Now about plants.".into();
        assert_eq!(
            again.stale(&store, 100).len(),
            1,
            "an edited note is read again"
        );
        let gone = store.notes[1].id.clone();
        store.delete_notes(&[gone]);
        assert_eq!(again.forget_gone(&store), 1);
        assert!(
            close_to(None, &again, "bfs", 5).is_empty(),
            "no model, no meaning search"
        );
    }
}
