use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use leo_core::notes::Note;

pub type Writer = Arc<dyn Fn(&str, &str, u32) -> Result<String> + Send + Sync>;

const BATCH_NOTES: usize = 8;
const BATCH_CHARS: usize = 18_000;
const NOTE_CHARS: usize = 2_500;
const VOCABULARY: usize = 150;
const GROUP_NOTES: usize = 90;
const LINKS_PER_NOTE: usize = 4;
const CONCEPT_CHARS: usize = 48;
const SUMMARY_CHARS: usize = 200;
const SUMMARY_ROWS: usize = 1000;
const SUMMARY_BYTES: usize = 100_000;
const WHY_CHARS: usize = 140;
const READ_TOKENS: u32 = 6_000;
const LINK_TOKENS: u32 = 12_000;
const BASE_ROOM: usize = 64_000;
pub const UPDATE_WHEN_IDLE: std::time::Duration = std::time::Duration::from_secs(120);
pub const RETRY_AUTO_AFTER: std::time::Duration = std::time::Duration::from_secs(600);

pub fn should_update(
    status: &Status,
    idle: std::time::Duration,
    since_auto: Option<std::time::Duration>,
) -> bool {
    let waited = (status.state != "failed" && status.message.is_none())
        || since_auto.is_none_or(|s| s >= RETRY_AUTO_AFTER);
    status.state != "building"
        && status.built_at.is_some()
        && status.requests > 0
        && idle >= UPDATE_WHEN_IDLE
        && waited
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub batch_notes: usize,
    pub batch_chars: usize,
    pub note_chars: usize,
    pub group_notes: usize,
    pub read_tokens: u32,
    pub link_tokens: u32,
    pub at_once: usize,
}

impl Default for Scale {
    fn default() -> Scale {
        Scale {
            batch_notes: BATCH_NOTES,
            batch_chars: BATCH_CHARS,
            note_chars: NOTE_CHARS,
            group_notes: GROUP_NOTES,
            read_tokens: READ_TOKENS,
            link_tokens: LINK_TOKENS,
            at_once: 1,
        }
    }
}

impl Scale {
    pub fn for_room(room: usize) -> Scale {
        let times = (room as f64 / BASE_ROOM as f64).max(1.0);
        let grow = |base: usize, most: f64| (base as f64 * times.min(most)) as usize;
        Scale {
            batch_notes: grow(BATCH_NOTES, 5.0),
            batch_chars: grow(BATCH_CHARS, 6.0).min(room.max(BATCH_CHARS) / 2),
            note_chars: grow(NOTE_CHARS, 6.0),
            group_notes: grow(GROUP_NOTES, 2.0),
            read_tokens: (f64::from(READ_TOKENS) * times.min(4.0)) as u32,
            link_tokens: (f64::from(LINK_TOKENS) * times.min(2.0)) as u32,
            at_once: match room {
                r if r >= BASE_ROOM => 3,
                r if r >= 40_000 => 2,
                _ => 1,
            },
        }
    }
}

pub const KINDS: [&str; 5] = [
    "same idea",
    "same method",
    "builds on",
    "applies",
    "contrasts",
];

#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub body: String,
    pub directory: String,
}

pub fn sources(notes: &[Note]) -> Vec<Source> {
    notes
        .iter()
        .map(|n| Source {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            directory: n.directory.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Cache {
    #[serde(default)]
    pub notes: BTreeMap<String, Read>,
    #[serde(default)]
    pub links: Vec<NoteLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pairs: BTreeMap<String, Pair>,
    #[serde(default)]
    pub built_at: Option<String>,
}

impl Cache {
    pub fn all_links(&self) -> impl Iterator<Item = &NoteLink> {
        self.links
            .iter()
            .chain(self.pairs.values().flat_map(|p| &p.links))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Read {
    pub hash: String,
    #[serde(default)]
    pub summary: String,
    pub concepts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Pair {
    pub hash: String,
    pub links: Vec<NoteLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteLink {
    pub a: String,
    pub b: String,
    pub kind: String,
    pub strength: u8,
    pub why: String,
}

fn fnv(bytes: impl IntoIterator<Item = u8>) -> String {
    format!("{:016x}", crate::vectors::fnv64(bytes))
}

fn hash_of(source: &Source) -> String {
    fnv(source.title.bytes().chain([0]).chain(source.body.bytes()))
}

fn key(label: &str) -> String {
    label.to_lowercase()
}

fn tidy(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn concept_label(raw: &str) -> Option<String> {
    let joined = tidy(raw);
    let trimmed = joined.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '"' | '\'' | '`' | '*' | '-' | '#'
        )
    });
    let label: String = trimmed.chars().take(CONCEPT_CHARS).collect();
    let label = label.trim().to_string();
    (!label.is_empty()).then_some(label)
}

fn clip(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push_str("\n…");
    }
    out
}

fn class_name(directory: &str) -> &str {
    if directory.is_empty() {
        "unfiled"
    } else {
        directory.split('/').next().unwrap_or(directory)
    }
}

pub fn plan(stale: &[&Source]) -> Vec<Vec<usize>> {
    plan_at(stale, &Scale::default())
}

pub fn plan_at(stale: &[&Source], scale: &Scale) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for (i, source) in stale.iter().enumerate() {
        let size = source.title.chars().count() + source.body.chars().count().min(scale.note_chars);
        let full = batches
            .last()
            .is_none_or(|b| b.len() >= scale.batch_notes || chars + size > scale.batch_chars);
        if full {
            batches.push(Vec::new());
            chars = 0;
        }
        if let Some(batch) = batches.last_mut() {
            batch.push(i);
        }
        chars += size;
    }
    batches
}

pub fn json_in(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

struct Concept {
    label: String,
    notes: BTreeSet<String>,
}

fn concepts(sources: &[Source], cache: &Cache) -> BTreeMap<String, Concept> {
    let mut out: BTreeMap<String, Concept> = BTreeMap::new();
    for source in sources {
        let Some(read) = cache.notes.get(&source.id) else {
            continue;
        };
        for label in &read.concepts {
            out.entry(key(label))
                .or_insert_with(|| Concept {
                    label: label.clone(),
                    notes: BTreeSet::new(),
                })
                .notes
                .insert(source.id.clone());
        }
    }
    out
}

fn vocabulary(table: &BTreeMap<String, Concept>) -> Vec<String> {
    let mut list: Vec<(&String, &Concept)> = table.iter().collect();
    list.sort_by(|a, b| b.1.notes.len().cmp(&a.1.notes.len()).then(a.0.cmp(b.0)));
    list.into_iter()
        .take(VOCABULARY)
        .map(|(_, c)| c.label.clone())
        .collect()
}

const READ_RULES: &str = "\
You read a student's notes so they can be connected into a knowledge graph across classes.

For each note, give:
- summary: one sentence of at most 25 words saying what the note teaches. Use interpretable language: plain words someone new to the subject can follow.
- concepts: 3 to 8 key ideas and methods, as short noun phrases of 1 to 4 words: specific concepts, techniques, methods, algorithms, theorems, people or results. Include the methods used, not only the topic (for example \"dynamic programming\", \"proof by induction\", \"Fourier transform\"). Leave out generic words such as \"introduction\", \"overview\", \"lecture\", \"notes\" or \"example\".
When a concept is already in the vocabulary, use exactly that name, so the same idea always has the same name. Name a new concept the way a textbook would.

Reply with JSON only, no other text, in this shape:
{\"notes\": [{\"id\": \"n1\", \"summary\": \"How breadth-first search explores a graph level by level\", \"concepts\": [\"breadth-first search\", \"queue\"]}]}";

pub fn read_prompt(batch: &[&Source], vocabulary: &[String]) -> (String, String) {
    read_prompt_at(batch, vocabulary, NOTE_CHARS)
}

pub fn read_prompt_at(
    batch: &[&Source],
    vocabulary: &[String],
    note_chars: usize,
) -> (String, String) {
    let mut user = String::from("<vocabulary>\n");
    for word in vocabulary {
        user.push_str(word);
        user.push('\n');
    }
    user.push_str("</vocabulary>\n\n");
    for (i, source) in batch.iter().enumerate() {
        user.push_str(&format!(
            "<note id=\"n{}\" class=\"{}\">\n# {}\n{}\n</note>\n\n",
            i + 1,
            class_name(&source.directory),
            source.title,
            clip(&source.body, note_chars)
        ));
    }
    user.push_str("Give the summary and key concepts of each note, as JSON.");
    (READ_RULES.to_string(), user)
}

fn short_id(value: Option<&Value>, count: usize) -> Option<usize> {
    value
        .and_then(Value::as_str)
        .and_then(|id| id.trim().trim_start_matches('n').parse::<usize>().ok())
        .filter(|i| (1..=count).contains(i))
}

pub fn parse_read(reply: &str, count: usize) -> Option<Vec<(String, Vec<String>)>> {
    let value = json_in(reply)?;
    let notes = value.get("notes")?.as_array()?;
    let mut out = vec![(String::new(), Vec::new()); count];
    for note in notes {
        let Some(index) = short_id(note.get("id"), count) else {
            continue;
        };
        let summary = note
            .get("summary")
            .and_then(Value::as_str)
            .map(tidy)
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        let mut list = Vec::new();
        for raw in note
            .get("concepts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(label) = concept_label(raw) {
                if seen.insert(key(&label)) {
                    list.push(label);
                }
            }
        }
        out[index - 1] = (summary.chars().take(SUMMARY_CHARS).collect(), list);
    }
    Some(out)
}

const LINK_RULES: &str = "\
You connect a student's notes into a knowledge graph, so they can study related material together, especially across different classes.

Each note has an id, its class, a one-line summary and its key ideas and methods. Connect two notes when studying them together helps: they use the same method or technique, cover the same idea, one builds on the other, one applies the other, or they contrast two approaches to the same problem. Prefer connections between different classes, since those are the ones a student misses. Connect notes in the same class only when the connection is specific, never just because they share a class or a broad subject.

Reply with JSON only, no other text, in this shape:
{\"links\": [{\"a\": \"n1\", \"b\": \"n7\", \"kind\": \"same method\", \"strength\": 2, \"why\": \"Both break the problem into overlapping subproblems with dynamic programming\"}]}
kind is one of: same idea, same method, builds on, applies, contrasts. For builds on and applies, a is the note that builds on or applies b. strength is 1 (loosely related), 2 (clearly related) or 3 (worth studying together). why is one sentence of at most 15 words, specific to the two notes. Give each note at most 4 connections, its most useful ones, and leave a note unconnected rather than invent a link.";

fn note_line(n: usize, source: &Source, read: Option<&Read>) -> String {
    let summary = read.map(|r| r.summary.as_str()).unwrap_or("");
    let ideas = read.map(|r| r.concepts.join(", ")).unwrap_or_default();
    format!(
        "n{n} [{}] {} | {} | ideas: {}\n",
        class_name(&source.directory),
        tidy(&source.title),
        summary,
        ideas
    )
}

pub fn link_prompt(notes: &[&Source], cache: &Cache) -> (String, String) {
    let mut user = String::from("<notes>\n");
    for (i, source) in notes.iter().enumerate() {
        user.push_str(&note_line(i + 1, source, cache.notes.get(&source.id)));
    }
    user.push_str("</notes>\n\nConnect the notes worth studying together, as JSON.");
    (LINK_RULES.to_string(), user)
}

pub fn parse_links(reply: &str, notes: &[&Source]) -> Option<Vec<NoteLink>> {
    let value = json_in(reply)?;
    let links = value.get("links")?.as_array()?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for link in links {
        let (Some(a), Some(b)) = (
            short_id(link.get("a"), notes.len()),
            short_id(link.get("b"), notes.len()),
        ) else {
            continue;
        };
        if a == b {
            continue;
        }
        let (ida, idb) = (notes[a - 1].id.clone(), notes[b - 1].id.clone());
        if !seen.insert(ordered(&ida, &idb)) {
            continue;
        }
        let kind = link
            .get("kind")
            .and_then(Value::as_str)
            .map(|k| tidy(k).to_lowercase())
            .filter(|k| KINDS.contains(&k.as_str()))
            .unwrap_or_else(|| "same idea".to_string());
        let strength = link
            .get("strength")
            .and_then(Value::as_u64)
            .unwrap_or(2)
            .clamp(1, 3) as u8;
        let why: String = link
            .get("why")
            .and_then(Value::as_str)
            .map(tidy)
            .unwrap_or_default()
            .chars()
            .take(WHY_CHARS)
            .collect();
        out.push(NoteLink {
            a: ida,
            b: idb,
            kind,
            strength,
            why,
        });
        if out.len() >= notes.len() * LINKS_PER_NOTE {
            break;
        }
    }
    Some(out)
}

fn group_count_at(notes: usize, group_notes: usize) -> usize {
    notes.div_ceil(group_notes.max(1)).max(1)
}

fn link_key(source: &Source, read: &Read) -> String {
    fnv(note_line(0, source, Some(read)).bytes())
}

pub struct Work<'a> {
    pub notes: Vec<&'a Source>,
    left: Option<BTreeSet<String>>,
    pub fresh: BTreeSet<String>,
}

fn chunks<'a>(list: &[&'a Source], group_notes: usize) -> Vec<Vec<&'a Source>> {
    let groups = group_count_at(list.len(), group_notes);
    let size = list.len().div_ceil(groups).max(1);
    list.chunks(size).map(|c| c.to_vec()).collect()
}

pub type Near = crate::vectors::Near;

fn may_relate(a: &[&Source], b: &[&Source], near: Option<&Near>) -> bool {
    let Some(near) = near else {
        return true;
    };
    let known = |s: &Source| near.get(&s.id);
    if a.iter().chain(b).any(|s| known(s).is_none()) {
        return true;
    }
    a.iter().any(|x| {
        b.iter().any(|y| {
            known(x).is_some_and(|n| n.contains(&y.id))
                || known(y).is_some_and(|n| n.contains(&x.id))
        })
    })
}

pub fn link_work_near<'a>(
    sources: &'a [Source],
    cache: &Cache,
    scale: &Scale,
    near: Option<&Near>,
) -> Vec<Work<'a>> {
    let mut fresh: Vec<&Source> = Vec::new();
    let mut settled: Vec<&Source> = Vec::new();
    for source in sources {
        let Some(read) = cache
            .notes
            .get(&source.id)
            .filter(|r| r.hash == hash_of(source))
        else {
            continue;
        };
        if read.linked.as_deref() == Some(link_key(source, read).as_str()) {
            settled.push(source);
        } else {
            fresh.push(source);
        }
    }
    fresh.sort_by(|a, b| a.id.cmp(&b.id));
    settled.sort_by(|a, b| a.id.cmp(&b.id));
    let new_groups = chunks(&fresh, scale.group_notes);
    let old_groups = if settled.is_empty() {
        Vec::new()
    } else {
        chunks(&settled, scale.group_notes)
    };
    let ids = |list: &[&Source]| list.iter().map(|s| s.id.clone()).collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for (i, group) in new_groups.iter().enumerate() {
        for other in &new_groups[i..] {
            let same = std::ptr::eq(group, other);
            if !same && !may_relate(group, other, near) {
                continue;
            }
            let mut notes = group.clone();
            if !same {
                notes.extend(other.iter().copied());
            }
            if notes.len() < 2 {
                continue;
            }
            let mut touched = ids(group);
            touched.extend(ids(other));
            out.push(Work {
                notes,
                left: (!same).then(|| ids(group)),
                fresh: touched,
            });
        }
        for old in &old_groups {
            if !may_relate(group, old, near) {
                continue;
            }
            let mut notes = group.clone();
            notes.extend(old.iter().copied());
            out.push(Work {
                notes,
                left: Some(ids(group)),
                fresh: ids(group),
            });
        }
    }
    out
}

pub fn stale<'a>(sources: &'a [Source], cache: &Cache) -> Vec<&'a Source> {
    sources
        .iter()
        .filter(|s| {
            cache
                .notes
                .get(&s.id)
                .is_none_or(|read| read.hash != hash_of(s))
        })
        .collect()
}

pub fn requests_needed_near(
    sources: &[Source],
    cache: &Cache,
    scale: &Scale,
    near: Option<&Near>,
) -> usize {
    let stale = stale(sources, cache);
    let reads = plan_at(&stale, scale).len();
    if reads == 0 {
        return link_work_near(sources, cache, scale, near).len();
    }
    let unread: BTreeSet<&str> = stale.iter().map(|s| s.id.as_str()).collect();
    let mut guess = cache.clone();
    for source in sources.iter().filter(|s| unread.contains(s.id.as_str())) {
        guess.notes.insert(
            source.id.clone(),
            Read {
                hash: hash_of(source),
                ..Read::default()
            },
        );
    }
    reads + link_work_near(sources, &guess, scale, near).len()
}

fn settle_old_pairs(sources: &[Source], cache: &mut Cache) {
    if cache.pairs.is_empty() {
        return;
    }
    let old: Vec<NoteLink> = std::mem::take(&mut cache.pairs)
        .into_values()
        .flat_map(|p| p.links)
        .collect();
    cache.links.extend(old);
    for source in sources {
        if let Some(read) = cache.notes.get_mut(&source.id) {
            if read.hash == hash_of(source) && read.linked.is_none() {
                read.linked = Some(link_key(source, read));
            }
        }
    }
}

pub type Write<'a> = &'a (dyn Fn(&str, &str, u32) -> Result<String> + Sync);

pub fn build(
    sources: &[Source],
    cache: &mut Cache,
    write: Write,
    progress: &mut dyn FnMut(usize, usize),
) -> Vec<String> {
    build_at(sources, cache, write, progress, &Scale::default())
}

fn in_parallel<T: Send>(jobs: Vec<Box<dyn FnOnce() -> T + Send + '_>>) -> Vec<Option<T>> {
    std::thread::scope(|scope| {
        let running: Vec<_> = jobs.into_iter().map(|job| scope.spawn(job)).collect();
        running.into_iter().map(|job| job.join().ok()).collect()
    })
}

pub fn build_at(
    sources: &[Source],
    cache: &mut Cache,
    write: Write,
    progress: &mut dyn FnMut(usize, usize),
    scale: &Scale,
) -> Vec<String> {
    build_near(sources, cache, write, progress, scale, None)
}

pub fn build_near(
    sources: &[Source],
    cache: &mut Cache,
    write: Write,
    progress: &mut dyn FnMut(usize, usize),
    scale: &Scale,
    near: Option<&Near>,
) -> Vec<String> {
    let present: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    cache.notes.retain(|id, _| present.contains(id.as_str()));
    settle_old_pairs(sources, cache);
    cache
        .links
        .retain(|l| present.contains(l.a.as_str()) && present.contains(l.b.as_str()));
    let stale = stale(sources, cache);
    let batches = plan_at(&stale, scale);
    let mut total = requests_needed_near(sources, cache, scale, near);
    let mut problems = Vec::new();
    let mut worked = batches.is_empty();
    let mut finished = 0;
    progress(0, total);
    let expected = || "the AI did not answer in the expected form".to_string();

    for round in batches.chunks(scale.at_once.max(1)) {
        let vocabulary = vocabulary(&concepts(sources, cache));
        let groups: Vec<Vec<&Source>> = round
            .iter()
            .map(|batch| batch.iter().map(|&i| stale[i]).collect())
            .collect();
        let jobs = groups
            .iter()
            .map(|notes| {
                let (system, user) = read_prompt_at(notes, &vocabulary, scale.note_chars);
                Box::new(move || {
                    write(&system, &user, scale.read_tokens)
                        .map_err(|e| e.to_string())
                        .and_then(|reply| parse_read(&reply, notes.len()).ok_or_else(expected))
                })
                    as Box<dyn FnOnce() -> Result<Vec<(String, Vec<String>)>, String> + Send + '_>
            })
            .collect();
        for (notes, read) in groups.iter().zip(in_parallel(jobs)) {
            match read.unwrap_or_else(|| Err("reading notes stopped unexpectedly".into())) {
                Ok(list) => {
                    worked = true;
                    for (source, (summary, concepts)) in notes.iter().zip(list) {
                        let mut read = Read {
                            hash: hash_of(source),
                            summary,
                            concepts,
                            linked: None,
                        };
                        let key = link_key(source, &read);
                        let before = cache.notes.get(&source.id).and_then(|r| r.linked.clone());
                        read.linked = before.filter(|k| *k == key);
                        cache.notes.insert(source.id.clone(), read);
                    }
                }
                Err(e) => problems.push(e),
            }
            finished += 1;
            progress(finished, total);
        }
    }

    let work = link_work_near(sources, cache, scale, near);
    let redo: BTreeSet<String> = work.iter().flat_map(|w| w.fresh.iter().cloned()).collect();
    let (replaced, kept): (Vec<NoteLink>, Vec<NoteLink>) = std::mem::take(&mut cache.links)
        .into_iter()
        .partition(|l| redo.contains(&l.a) || redo.contains(&l.b));
    cache.links = kept;
    total = batches.len() + work.len();
    progress(batches.len(), total);
    let mut failed: BTreeSet<String> = BTreeSet::new();
    for round in work.chunks(scale.at_once.max(1)) {
        let snapshot: &Cache = cache;
        let jobs = round
            .iter()
            .map(|job| {
                let (system, user) = link_prompt(&job.notes, snapshot);
                Box::new(move || {
                    write(&system, &user, scale.link_tokens)
                        .map_err(|e| e.to_string())
                        .and_then(|reply| parse_links(&reply, &job.notes).ok_or_else(expected))
                }) as Box<dyn FnOnce() -> Result<Vec<NoteLink>, String> + Send + '_>
            })
            .collect();
        let replies = in_parallel(jobs);
        for (job, linked) in round.iter().zip(replies) {
            match linked.unwrap_or_else(|| Err("linking notes stopped unexpectedly".into())) {
                Ok(mut links) => {
                    worked = true;
                    if let Some(left) = &job.left {
                        links.retain(|l| left.contains(&l.a) != left.contains(&l.b));
                    }
                    cache.links.extend(links);
                }
                Err(e) => {
                    failed.extend(job.fresh.iter().cloned());
                    problems.push(e);
                }
            }
            finished += 1;
            progress(finished, total);
        }
    }
    for link in replaced {
        let held = [&link.a, &link.b]
            .into_iter()
            .all(|id| !redo.contains(id) || failed.contains(id));
        if held
            && !cache
                .links
                .iter()
                .any(|l| (&l.a, &l.b) == (&link.a, &link.b))
        {
            cache.links.push(link);
        }
    }
    for source in sources {
        if failed.contains(&source.id) {
            continue;
        }
        if let Some(read) = cache.notes.get_mut(&source.id) {
            if read.hash == hash_of(source) {
                read.linked = Some(link_key(source, read));
            }
        }
    }
    progress(total, total);
    if worked {
        cache.built_at = Some(chrono::Utc::now().to_rfc3339());
    }
    problems
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a < b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    pub state: &'static str,
    pub done: usize,
    pub total: usize,
    pub message: Option<String>,
    pub notes: usize,
    pub read: usize,
    pub stale: usize,
    pub requests: usize,
    pub built_at: Option<String>,
}

#[derive(Debug, Clone)]
struct Job {
    state: &'static str,
    done: usize,
    total: usize,
    message: Option<String>,
}

pub struct Graphs {
    path: PathBuf,
    db: Arc<crate::db::Db>,
    writer: Option<Writer>,
    room: Option<crate::Room>,
    vectors: Option<Arc<crate::vectors::Vectors>>,
    near: Mutex<Option<(u64, Arc<Near>)>>,
    held: Mutex<Option<Arc<Cache>>>,
    job: Mutex<Job>,
    auto: Mutex<Option<std::time::Instant>>,
}

impl Graphs {
    pub fn with_room(mut self, room: Option<crate::Room>) -> Graphs {
        self.room = room;
        self
    }

    pub fn with_vectors(mut self, vectors: Arc<crate::vectors::Vectors>) -> Graphs {
        self.vectors = Some(vectors);
        self
    }

    pub fn near(&self) -> Option<Arc<Near>> {
        let vectors = self.vectors.as_ref()?;
        if vectors.is_empty() {
            return None;
        }
        let version = vectors.version();
        let mut held = self.near.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, near)) = held.as_ref().filter(|(at, _)| *at == version) {
            let _ = at;
            return Some(Arc::clone(near));
        }
        let near = Arc::new(vectors.neighbours(crate::vectors::NEIGHBOURS));
        *held = Some((version, Arc::clone(&near)));
        Some(near)
    }

    pub fn scale(&self) -> Scale {
        self.room
            .as_ref()
            .map_or_else(Scale::default, |room| Scale::for_room(room()))
    }

    pub fn writer(&self) -> Option<Writer> {
        self.writer.clone()
    }

    pub fn new(path: PathBuf, writer: Option<Writer>) -> Graphs {
        Graphs {
            db: crate::db::beside(&path),
            path,
            writer,
            room: None,
            vectors: None,
            near: Mutex::new(None),
            held: Mutex::new(None),
            auto: Mutex::new(None),
            job: Mutex::new(Job {
                state: "idle",
                done: 0,
                total: 0,
                message: None,
            }),
        }
    }

    pub fn for_notes(notes_dir: &Path, writer: Option<Writer>) -> Graphs {
        let base = notes_dir.parent().unwrap_or(notes_dir);
        Graphs::new(base.join("graph.json"), writer)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn bring_in_file(&self) {
        let Some(old) = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
        else {
            return;
        };
        if self.write_all(&old, true).is_ok() {
            crate::db::put_aside(&self.path);
        }
    }

    pub fn load(&self) -> Arc<Cache> {
        self.bring_in_file();
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = held.as_ref() {
            return Arc::clone(cache);
        }
        let cache = Arc::new(self.read_cache());
        *held = Some(Arc::clone(&cache));
        cache
    }

    fn read_cache(&self) -> Cache {
        self.db
            .with(|c| {
                let mut cache = Cache::default();
                {
                    let mut found = c.prepare("SELECT id, data FROM graph_notes")?;
                    let rows = found
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                    for row in rows {
                        let (id, data) = row?;
                        if let Ok(read) = serde_json::from_str(&data) {
                            cache.notes.insert(id, read);
                        }
                    }
                }
                {
                    let mut found = c.prepare("SELECT data FROM graph_links ORDER BY n")?;
                    let rows = found.query_map([], |r| r.get::<_, String>(0))?;
                    for row in rows {
                        if let Ok(link) = serde_json::from_str(&row?) {
                            cache.links.push(link);
                        }
                    }
                }
                {
                    let mut found = c.prepare("SELECT key, data FROM graph_meta")?;
                    let rows = found
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                    for row in rows {
                        let (key, data) = row?;
                        match key.as_str() {
                            "built_at" => {
                                cache.built_at = serde_json::from_str(&data).ok().flatten()
                            }
                            "pairs" => {
                                cache.pairs = serde_json::from_str(&data).unwrap_or_default()
                            }
                            _ => {}
                        }
                    }
                }
                Ok(cache)
            })
            .unwrap_or_default()
    }

    pub(crate) fn note_summaries(&self, notes: &[Note]) -> BTreeMap<String, String> {
        self.db
            .with(|c| {
                let mut out = BTreeMap::new();
                let mut found =
                    c.prepare("SELECT data FROM graph_notes WHERE id = ?1 AND length(data) <= ?2")?;
                for note in notes.iter().take(SUMMARY_ROWS) {
                    let data = found
                        .query_row(rusqlite::params![&note.id, SUMMARY_BYTES as i64], |r| {
                            r.get::<_, String>(0)
                        });
                    let Some(read) = data
                        .ok()
                        .and_then(|data| serde_json::from_str::<Read>(&data).ok())
                    else {
                        continue;
                    };
                    let hash = crate::vectors::hash_of(note);
                    if read.hash == hash && !read.summary.trim().is_empty() {
                        out.insert(
                            note.id.clone(),
                            read.summary.chars().take(SUMMARY_CHARS).collect(),
                        );
                    }
                }
                Ok(out)
            })
            .unwrap_or_default()
    }

    fn save(&self, cache: &Cache) -> Result<()> {
        self.write_all(cache, false)
    }

    fn write_all(&self, cache: &Cache, replace: bool) -> Result<()> {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let written = self.db.with(|c| {
            let tx = c.transaction()?;
            let stored: HashMap<String, String> = {
                let mut found = tx.prepare("SELECT id, data FROM graph_notes")?;
                let rows = found
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for (id, read) in &cache.notes {
                let data = serde_json::to_string(read).unwrap_or_default();
                if replace || stored.get(id) != Some(&data) {
                    tx.execute(
                        "INSERT OR REPLACE INTO graph_notes (id, data) VALUES (?1, ?2)",
                        [id, &data],
                    )?;
                }
            }
            for id in stored.keys().filter(|id| !cache.notes.contains_key(*id)) {
                tx.execute("DELETE FROM graph_notes WHERE id = ?1", [id])?;
            }
            tx.execute("DELETE FROM graph_links", [])?;
            for (n, link) in cache.links.iter().enumerate() {
                tx.execute(
                    "INSERT INTO graph_links (n, data) VALUES (?1, ?2)",
                    rusqlite::params![n as i64, serde_json::to_string(link).unwrap_or_default()],
                )?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO graph_meta (key, data) VALUES ('built_at', ?1)",
                [serde_json::to_string(&cache.built_at).unwrap_or_default()],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO graph_meta (key, data) VALUES ('pairs', ?1)",
                [serde_json::to_string(&cache.pairs).unwrap_or_default()],
            )?;
            tx.commit()
        });
        if written.is_ok() {
            *held = None;
        }
        written
    }

    fn job(&self) -> Job {
        self.job
            .lock()
            .map(|j| j.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn building(&self) -> Option<(usize, usize)> {
        let job = self.job();
        (job.state == "building").then_some((job.done, job.total))
    }

    fn set(&self, change: impl FnOnce(&mut Job)) {
        let mut job = self.job.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut job);
    }

    pub fn status(&self, sources: &[Source]) -> Status {
        let cache = self.load();
        let job = self.job();
        let read = sources
            .iter()
            .filter(|s| cache.notes.contains_key(&s.id))
            .count();
        Status {
            state: job.state,
            done: job.done,
            total: job.total,
            message: job.message,
            notes: sources.len(),
            read,
            stale: stale(sources, &cache).len(),
            requests: requests_needed_near(sources, &cache, &self.scale(), self.near().as_deref()),
            built_at: cache.built_at.clone(),
        }
    }

    pub fn settled(&self, sources: &[Source]) -> bool {
        self.writer.is_none() || self.settled_by(&self.status(sources))
    }

    fn settled_by(&self, status: &Status) -> bool {
        status.state != "building" && (status.built_at.is_none() || status.requests == 0)
    }

    pub fn update_if_due(
        self: &Arc<Self>,
        sources: Vec<Source>,
        idle: std::time::Duration,
    ) -> (bool, bool) {
        if self.writer.is_none() {
            return (false, true);
        }
        let since = self
            .auto
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|at| at.elapsed());
        let status = self.status(&sources);
        if !should_update(&status, idle, since) {
            return (false, self.settled_by(&status));
        }
        *self.auto.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
        self.start(sources);
        (true, false)
    }

    pub fn start(self: &Arc<Self>, sources: Vec<Source>) {
        {
            let mut job = self.job.lock().unwrap_or_else(|e| e.into_inner());
            if job.state == "building" {
                return;
            }
            let Some(_) = self.writer else {
                job.state = "failed";
                job.message = Some(crate::no_writer());
                return;
            };
            *job = Job {
                state: "building",
                done: 0,
                total: 0,
                message: None,
            };
        }
        let graphs = Arc::clone(self);
        if std::thread::Builder::new()
            .name("leo-graph".into())
            .spawn(move || graphs.run(&sources))
            .is_err()
        {
            self.set(|job| {
                job.state = "failed";
                job.message = Some("Could not start the knowledge graph worker.".to_string());
            });
        }
    }

    fn run(&self, sources: &[Source]) {
        struct BuildGuard<'a> {
            graphs: &'a Graphs,
            finished: bool,
        }
        impl Drop for BuildGuard<'_> {
            fn drop(&mut self) {
                if !self.finished {
                    self.graphs.set(|job| {
                        job.state = "failed";
                        job.message = Some(
                            "The knowledge graph stopped unexpectedly; Update tries again."
                                .to_string(),
                        );
                    });
                }
            }
        }
        let mut guard = BuildGuard {
            graphs: self,
            finished: false,
        };
        let Some(writer) = self.writer.clone() else {
            return;
        };
        let mut cache = (*self.load()).clone();
        let scale = self.scale();
        let near = self.near();
        let problems = build_near(
            sources,
            &mut cache,
            &|system: &str, user: &str, most: u32| writer(system, user, most),
            &mut |done, total| {
                self.set(|job| {
                    job.done = done;
                    job.total = total;
                })
            },
            &scale,
            near.as_deref(),
        );
        let saved = self.save(&cache);
        self.set(|job| {
            let total = job.total.max(1);
            match (&saved, problems.first()) {
                (Err(e), _) => {
                    job.state = "failed";
                    job.message = Some(format!("Could not save the knowledge graph: {e}"));
                }
                (Ok(()), Some(first)) if cache.built_at.is_none() || problems.len() >= total => {
                    job.state = "failed";
                    job.message = Some(first.clone());
                }
                (Ok(()), Some(first)) => {
                    job.state = "done";
                    job.message = Some(format!(
                        "Some notes could not be read this time ({first}); Update tries them again."
                    ));
                }
                (Ok(()), None) => {
                    job.state = "done";
                    job.message = None;
                }
            }
        });
        guard.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn source(id: &str, title: &str, body: &str, directory: &str) -> Source {
        Source {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            directory: directory.into(),
        }
    }

    fn library() -> Vec<Source> {
        vec![
            source(
                "a",
                "Graph traversals",
                "BFS uses a queue. See [[Stacks]].",
                "cs130",
            ),
            source("b", "Stacks", "LIFO. Used by DFS.", "cs130"),
            source("c", "Scheduling", "Round robin keeps a run queue.", "cs162"),
        ]
    }

    fn short_ids(user: &str) -> Vec<(String, String)> {
        user.lines()
            .filter_map(|line| {
                let (id, rest) = line.split_once(' ')?;
                (id.starts_with('n') && id[1..].parse::<usize>().is_ok())
                    .then(|| (id.to_string(), rest.to_string()))
            })
            .collect()
    }

    fn fake<'a>(
        reads: &'a AtomicUsize,
        links: &'a AtomicUsize,
    ) -> impl Fn(&str, &str, u32) -> Result<String> + 'a {
        move |system: &str, user: &str, _| {
            if system.starts_with("You read") {
                reads.fetch_add(1, Ordering::SeqCst);
                let mut notes = Vec::new();
                for (i, part) in user.split("<note id=").skip(1).enumerate() {
                    let (summary, concepts) = if part.contains("BFS") {
                        (
                            "Breadth-first search",
                            r#"["Breadth-First Search", "queue"]"#,
                        )
                    } else if part.contains("LIFO") {
                        ("Stacks and DFS", r#"["stack", "depth-first search"]"#)
                    } else {
                        ("Round robin scheduling", r#"["Queue", "round robin"]"#)
                    };
                    notes.push(format!(
                        r#"{{"id": "n{}", "summary": "{summary}", "concepts": {concepts}}}"#,
                        i + 1
                    ));
                }
                Ok(format!(
                    "```json\n{{\"notes\": [{}]}}\n```",
                    notes.join(",")
                ))
            } else {
                links.fetch_add(1, Ordering::SeqCst);
                let ids = short_ids(user);
                let find = |title: &str| {
                    ids.iter()
                        .find(|(_, rest)| rest.contains(title))
                        .map(|(id, _)| id.clone())
                        .unwrap_or_else(|| "n99".into())
                };
                let (a, b, c) = (find("Graph traversals"), find("Stacks"), find("Scheduling"));
                Ok(format!(
                    r#"Here: {{"links": [
                        {{"a": "{a}", "b": "{c}", "kind": "Same Idea", "strength": 3, "why": "both  keep work waiting in a queue"}},
                        {{"a": "{a}", "b": "{b}", "kind": "builds on", "strength": 9, "why": "DFS needs a stack"}},
                        {{"a": "{c}", "b": "{a}", "kind": "same idea", "strength": 1, "why": "duplicate pair"}},
                        {{"a": "{a}", "b": "{a}", "kind": "same idea", "why": "self"}},
                        {{"a": "{a}", "b": "n99", "kind": "same idea", "why": "unknown"}},
                        {{"a": "{b}", "b": "{c}", "kind": "made up", "why": "odd kind"}}
                    ]}}"#
                ))
            }
        }
    }

    #[test]
    fn library_summaries_use_only_current_cache_entries() {
        let dir = tempfile::tempdir().unwrap();
        let graphs = Graphs::for_notes(&dir.path().join("notes"), None);
        let mut note = Note::new(
            "Queues",
            "FIFO queues process the earliest item first.",
            vec![],
            "CS130",
        );
        let mut cache = Cache::default();
        cache.notes.insert(
            note.id.clone(),
            Read {
                hash: hash_of(&sources(&[note.clone()])[0]),
                summary: "FIFO queue ordering".into(),
                concepts: vec![],
                linked: None,
            },
        );
        graphs.save(&cache).unwrap();
        assert_eq!(
            graphs
                .note_summaries(&[note.clone()])
                .get(&note.id)
                .unwrap(),
            "FIFO queue ordering"
        );
        note.body = "Updated wording".into();
        assert!(graphs.note_summaries(&[note]).is_empty());
    }

    #[test]
    fn the_note_hash_is_unchanged() {
        let title = "Graph traversals";
        let body = "BFS uses a queue — très vite.";
        let read = source("a", title, body, "cs130");
        assert_eq!(hash_of(&read), "f29dc93fe22f93cd");
        let note = Note::new(title, body, Vec::new(), "cs130");
        assert_eq!(hash_of(&read), crate::vectors::hash_of(&note));
    }

    #[test]
    fn the_graph_updates_itself_only_when_built_before_changed_and_left_alone() {
        let minutes = |m: u64| std::time::Duration::from_secs(m * 60);
        let ready = Status {
            state: "done",
            done: 0,
            total: 0,
            message: None,
            notes: 3,
            read: 3,
            stale: 1,
            requests: 2,
            built_at: Some("2026-10-09T00:00:00Z".into()),
        };
        assert!(should_update(&ready, minutes(2), None));
        assert!(
            !should_update(&ready, minutes(1), None),
            "someone is still using leo"
        );
        assert!(
            !should_update(
                &Status {
                    requests: 0,
                    ..ready.clone()
                },
                minutes(5),
                None
            ),
            "nothing changed"
        );
        assert!(
            !should_update(
                &Status {
                    built_at: None,
                    ..ready.clone()
                },
                minutes(5),
                None
            ),
            "never built: the first build is the user's call"
        );
        assert!(!should_update(
            &Status {
                state: "building",
                ..ready.clone()
            },
            minutes(5),
            None
        ));
        let failed = Status {
            state: "failed",
            ..ready.clone()
        };
        assert!(
            !should_update(&failed, minutes(5), Some(minutes(3))),
            "a failed try waits"
        );
        assert!(should_update(&failed, minutes(5), Some(minutes(11))));
        let partly = Status {
            message: Some("Some notes could not be read this time".into()),
            ..ready.clone()
        };
        assert!(
            !should_update(&partly, minutes(5), Some(minutes(3))),
            "a build that left notes unread waits too"
        );
        assert!(should_update(&partly, minutes(5), Some(minutes(11))));
        assert!(
            should_update(&ready, minutes(5), Some(minutes(1))),
            "after a good update, new edits are picked up"
        );
    }

    #[test]
    fn an_idle_update_starts_one_build_and_not_another_while_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let notes = [source(
            "a",
            "Graph traversals",
            "BFS uses a queue.",
            "cs130",
        )];
        let started = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&started);
        let writer: Writer = Arc::new(move |_: &str, _: &str, _: u32| {
            count.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(200));
            Ok(r#"{"notes": [{"id": "n1", "summary": "BFS", "concepts": ["queue"]}]}"#.into())
        });
        let graphs = Arc::new(Graphs::new(dir.path().join("graph.json"), Some(writer)));
        let idle = std::time::Duration::from_secs(300);
        assert_eq!(
            graphs.update_if_due(notes.to_vec(), idle),
            (false, true),
            "never built yet, so nothing to do before the first build"
        );
        assert!(graphs.settled(&notes));
        let cache = Cache {
            built_at: Some("2026-10-09T00:00:00Z".into()),
            ..Cache::default()
        };
        std::fs::write(graphs.path(), serde_json::to_string(&cache).unwrap()).unwrap();
        assert!(!graphs.settled(&notes), "an unread note is waiting");
        assert_eq!(graphs.update_if_due(notes.to_vec(), idle), (true, false));
        assert_eq!(
            graphs.update_if_due(notes.to_vec(), idle),
            (false, false),
            "one at a time, and still building"
        );
        assert!(!graphs.settled(&notes), "still building");
        let waited = std::time::Instant::now();
        while graphs.status(&notes).state == "building" {
            assert!(waited.elapsed().as_secs() < 5);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(started.load(Ordering::SeqCst) >= 1);
        assert_eq!(
            graphs.load().notes.get("a").map(|r| r.summary.as_str()),
            Some("BFS")
        );
    }

    #[test]
    fn a_bigger_model_reads_more_notes_at_once_and_runs_requests_side_by_side() {
        assert_eq!(
            Scale::for_room(14_000),
            Scale {
                batch_chars: 9_000,
                at_once: 1,
                ..Scale::default()
            },
            "a small local model gets batches that fit its context"
        );
        let wide = Scale::for_room(360_000);
        assert_eq!(
            (
                wide.batch_notes,
                wide.note_chars,
                wide.group_notes,
                wide.at_once
            ),
            (40, 14_062, 180, 3)
        );
        assert_eq!(Scale::for_room(57_600).at_once, 2);

        let many: Vec<Source> = (0..36)
            .map(|i| {
                source(
                    &format!("{i:03}"),
                    &format!("Note {i}"),
                    "BFS uses a queue.",
                    "cs130",
                )
            })
            .collect();
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let calls = AtomicUsize::new(0);
        let write = |system: &str, user: &str, _: u32| -> Result<String> {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(60));
            running.fetch_sub(1, Ordering::SeqCst);
            if system.starts_with("You read") {
                let notes: Vec<String> = (1..=user.split("<note id=").count() - 1)
                    .map(|i| {
                        format!(r#"{{"id": "n{i}", "summary": "BFS", "concepts": ["queue"]}}"#)
                    })
                    .collect();
                Ok(format!("{{\"notes\": [{}]}}", notes.join(",")))
            } else {
                Ok(r#"{"links": []}"#.into())
            }
        };
        let scale = Scale {
            batch_notes: 6,
            at_once: 3,
            ..Scale::default()
        };
        let mut cache = Cache::default();
        let mut seen = Vec::new();
        let problems = build_at(
            &many,
            &mut cache,
            &write,
            &mut |done, total| seen.push((done, total)),
            &scale,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(most.load(Ordering::SeqCst), 3, "three requests at a time");
        assert_eq!(cache.notes.len(), 36);
        assert!(cache
            .notes
            .values()
            .all(|r| r.summary == "BFS" && r.linked.is_some()));
        let (done, total) = *seen.last().unwrap();
        assert_eq!(done, total);
        assert_eq!(calls.load(Ordering::SeqCst), total);
        assert!(
            seen.windows(2).all(|w| w[0].0 <= w[1].0),
            "progress only moves forward"
        );
    }

    #[test]
    fn notes_are_read_once_and_linked_to_each_other_with_reasons() {
        let (reads, links) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let write = fake(&reads, &links);
        let sources = library();
        let mut cache = Cache::default();
        let mut steps = Vec::new();
        let problems = build(&sources, &mut cache, &write, &mut |d, t| steps.push((d, t)));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            (reads.load(Ordering::SeqCst), links.load(Ordering::SeqCst)),
            (1, 1)
        );
        assert_eq!(steps.last(), Some(&(2, 2)));
        assert_eq!(cache.notes["a"].summary, "Breadth-first search");
        assert_eq!(cache.notes["a"].concepts, ["Breadth-First Search", "queue"]);

        let edge = |a: &str, b: &str| {
            cache
                .all_links()
                .find(|e| (e.a == a && e.b == b) || (e.a == b && e.b == a))
                .unwrap()
        };
        let across = edge("a", "c");
        assert_eq!(across.kind, "same idea");
        assert_eq!(across.strength, 3, "the first of a repeated pair is kept");
        assert_eq!(across.why, "both keep work waiting in a queue");
        assert_eq!(edge("a", "b").strength, 3, "strength is clamped");
        assert_eq!(edge("b", "c").kind, "same idea");
        assert_eq!(cache.all_links().count(), 3);
        let ideas = concepts(&sources, &cache);
        assert!(ideas["queue"].notes.contains("a"));
        assert!(
            ideas["queue"].notes.contains("c"),
            "Queue and queue are one idea"
        );
        assert_eq!(cache.notes["c"].summary, "Round robin scheduling");

        let again = build(&sources, &mut cache, &write, &mut |_, _| {});
        assert!(again.is_empty());
        assert_eq!(
            (reads.load(Ordering::SeqCst), links.load(Ordering::SeqCst)),
            (1, 1),
            "nothing changed, nothing asked"
        );
        assert_eq!(
            requests_needed_near(&sources, &cache, &Scale::default(), None),
            0
        );

        let mut changed = sources.clone();
        changed[1].body = "LIFO. Used by DFS and recursion.".into();
        assert_eq!(
            requests_needed_near(&changed, &cache, &Scale::default(), None),
            2
        );
        build(&changed, &mut cache, &write, &mut |_, _| {});
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "only the edited note is read again"
        );
        assert_eq!(
            links.load(Ordering::SeqCst),
            1,
            "the same summaries and ideas need no new links"
        );

        changed.remove(2);
        build(&changed, &mut cache, &write, &mut |_, _| {});
        assert!(
            !cache.notes.contains_key("c"),
            "a deleted note leaves the map"
        );
        assert!(cache.all_links().all(|e| e.a != "c" && e.b != "c"));
    }

    #[test]
    fn groups_that_share_nothing_in_meaning_are_not_asked_about() {
        let many: Vec<Source> = (0..200)
            .map(|i| source(&format!("{i:03}"), &format!("Note {i}"), "x", ""))
            .collect();
        let mut cache = Cache::default();
        for s in &many {
            cache.notes.insert(
                s.id.clone(),
                Read {
                    hash: hash_of(s),
                    summary: "s".into(),
                    concepts: vec![],
                    linked: None,
                },
            );
        }
        let scale = Scale::default();
        let all = link_work_near(&many, &cache, &scale, None).len();
        let mut near = Near::new();
        for s in &many {
            near.insert(s.id.clone(), BTreeSet::new());
        }
        near.get_mut("000").unwrap().insert("199".into());
        let close = link_work_near(&many, &cache, &scale, Some(&near));
        assert_eq!(all, 6);
        assert_eq!(
            close.len(),
            4,
            "each group with itself, plus the one pair that relates"
        );
        near.remove("150");
        assert_eq!(
            link_work_near(&many, &cache, &scale, Some(&near)).len(),
            5,
            "a group holding a note not yet read for meaning is linked against every group"
        );
        assert!(requests_needed_near(&many, &cache, &scale, Some(&near)) >= 4);
    }

    #[test]
    fn a_large_library_is_linked_in_groups_covering_every_pair_once() {
        let many: Vec<Source> = (0..200)
            .map(|i| source(&format!("{i:03}"), &format!("Note {i}"), "x", ""))
            .collect();
        let mut cache = Cache::default();
        for s in &many {
            cache.notes.insert(
                s.id.clone(),
                Read {
                    hash: hash_of(s),
                    summary: "s".into(),
                    concepts: vec![],
                    linked: None,
                },
            );
        }
        let work = link_work_near(&many, &cache, &Scale::default(), None);
        assert_eq!(work.len(), 6);
        let mut covered = BTreeSet::new();
        for w in &work {
            let ids: Vec<&str> = w.notes.iter().map(|s| s.id.as_str()).collect();
            for (i, a) in ids.iter().enumerate() {
                for b in &ids[i + 1..] {
                    covered.insert(ordered(a, b));
                }
            }
        }
        assert_eq!(
            covered.len(),
            200 * 199 / 2,
            "every pair of notes is seen by some request"
        );

        let cross = work.iter().find(|w| w.left.is_some()).unwrap();
        let left = cross.left.clone().unwrap();
        let at = |pick: &dyn Fn(&&Source) -> bool, nth: usize| {
            cross
                .notes
                .iter()
                .enumerate()
                .filter(|(_, s)| pick(s))
                .nth(nth)
                .unwrap()
                .0
                + 1
        };
        let l1 = at(&|s: &&Source| left.contains(&s.id), 0);
        let l2 = at(&|s: &&Source| left.contains(&s.id), 1);
        let r1 = at(&|s: &&Source| !left.contains(&s.id), 0);
        let reply = format!(
            r#"{{"links": [{{"a": "n{l1}", "b": "n{r1}", "kind": "same idea", "why": "across"}}, {{"a": "n{l1}", "b": "n{l2}", "kind": "same idea", "why": "inside"}}]}}"#
        );
        let first = cross.notes[0].id.clone();
        let size = cross.notes.len();
        let write = |_: &str, user: &str, _: u32| -> Result<String> {
            let lines = short_ids(user);
            if lines.len() == size
                && lines[0].1.starts_with(&format!(
                    "[unfiled] Note {}",
                    first.trim_start_matches('0').parse::<usize>().unwrap_or(0)
                ))
            {
                Ok(reply.clone())
            } else {
                Ok(r#"{"links": []}"#.into())
            }
        };
        build(&many, &mut cache, &write, &mut |_, _| {});
        let kept = &cache.links;
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            kept[0].why, "across",
            "a pair inside one group is left to that group's own request"
        );
        assert!(
            link_work_near(&many, &cache, &Scale::default(), None).is_empty(),
            "every note is linked now"
        );
    }

    fn linked_library(count: usize) -> (Vec<Source>, Cache) {
        let many: Vec<Source> = (0..count)
            .map(|i| source(&format!("{i:03}"), &format!("Note {i}"), "x", ""))
            .collect();
        let mut cache = Cache::default();
        for s in &many {
            let mut read = Read {
                hash: hash_of(s),
                summary: format!("about {}", s.id),
                concepts: vec![],
                linked: None,
            };
            read.linked = Some(link_key(s, &read));
            cache.notes.insert(s.id.clone(), read);
        }
        cache.links.push(NoteLink {
            a: "000".into(),
            b: "001".into(),
            kind: "same idea".into(),
            strength: 2,
            why: "kept".into(),
        });
        (many, cache)
    }

    #[test]
    fn a_new_note_is_linked_to_the_others_without_redoing_their_links() {
        let (mut many, mut cache) = linked_library(200);
        many.push(source("new", "Fresh note", "brand new", ""));
        let calls = AtomicUsize::new(0);
        let seen = Mutex::new(Vec::new());
        let write = |_: &str, user: &str, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::SeqCst);
            if user.contains("<note id=") {
                return Ok(
                    r#"{"notes": [{"id": "n1", "summary": "fresh", "concepts": []}]}"#.into(),
                );
            }
            let lines = short_ids(user);
            seen.lock().unwrap().push(lines.len());
            let new = lines
                .iter()
                .find(|(_, l)| l.contains("Fresh note"))
                .unwrap()
                .0
                .clone();
            let other = lines
                .iter()
                .find(|(_, l)| !l.contains("Fresh note"))
                .unwrap()
                .0
                .clone();
            let third = lines
                .iter()
                .filter(|(_, l)| !l.contains("Fresh note"))
                .nth(1)
                .unwrap()
                .0
                .clone();
            Ok(format!(
                r#"{{"links": [{{"a": "{new}", "b": "{other}", "kind": "same idea", "why": "new one"}}, {{"a": "{other}", "b": "{third}", "kind": "same idea", "why": "old pair"}}]}}"#
            ))
        };
        assert_eq!(
            requests_needed_near(&many, &cache, &Scale::default(), None),
            1 + 3
        );
        let problems = build(&many, &mut cache, &write, &mut |_, _| {});
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1 + 3,
            "one read, then the new note against each group"
        );
        assert!(
            seen.lock().unwrap().iter().all(|n| *n <= 68),
            "{:?}",
            seen.lock().unwrap()
        );
        assert!(
            cache.links.iter().any(|l| l.why == "kept"),
            "old links stay"
        );
        assert_eq!(cache.links.iter().filter(|l| l.why == "new one").count(), 3);
        assert!(
            !cache.links.iter().any(|l| l.why == "old pair"),
            "links between two old notes are not redone"
        );
        assert_eq!(
            requests_needed_near(&many, &cache, &Scale::default(), None),
            0
        );
    }

    #[test]
    fn a_new_note_whose_links_failed_is_tried_again() {
        let (mut many, mut cache) = linked_library(5);
        many.push(source("new", "Fresh note", "brand new", ""));
        let read_only = |_: &str, user: &str, _: u32| -> Result<String> {
            if user.contains("<note id=") {
                Ok(r#"{"notes": [{"id": "n1", "summary": "fresh", "concepts": []}]}"#.into())
            } else {
                anyhow::bail!("rate limited")
            }
        };
        let problems = build(&many, &mut cache, &read_only, &mut |_, _| {});
        assert_eq!(problems, ["rate limited"]);
        assert_eq!(cache.notes["new"].linked, None);
        assert_eq!(
            requests_needed_near(&many, &cache, &Scale::default(), None),
            1
        );
        assert!(cache.links.iter().any(|l| l.why == "kept"));
    }

    #[test]
    fn a_failed_link_request_keeps_the_notes_existing_links_until_it_works() {
        let (mut many, mut cache) = linked_library(5);
        many[0].body = "edited".into();
        let read_only = |_: &str, user: &str, _: u32| -> Result<String> {
            if user.contains("<note id=") {
                Ok(r#"{"notes": [{"id": "n1", "summary": "fresh", "concepts": []}]}"#.into())
            } else {
                anyhow::bail!("rate limited")
            }
        };
        let problems = build(&many, &mut cache, &read_only, &mut |_, _| {});
        assert_eq!(problems, ["rate limited"]);
        assert_eq!(cache.notes["000"].linked, None);
        assert!(cache.links.iter().any(|l| l.why == "kept"));
        let working = |_: &str, user: &str, _: u32| -> Result<String> {
            let ids = short_ids(user);
            let a = ids
                .iter()
                .find(|(_, l)| l.contains("Note 0"))
                .unwrap()
                .0
                .clone();
            let b = ids
                .iter()
                .find(|(_, l)| l.contains("Note 2"))
                .unwrap()
                .0
                .clone();
            Ok(format!(
                r#"{{"links": [{{"a": "{a}", "b": "{b}", "kind": "same idea", "strength": 2, "why": "new"}}]}}"#
            ))
        };
        let problems = build(&many, &mut cache, &working, &mut |_, _| {});
        assert!(problems.is_empty());
        assert!(cache.links.iter().all(|l| l.why != "kept"));
        assert!(cache.links.iter().any(|l| l.why == "new"));
    }

    #[test]
    fn a_map_saved_by_an_older_leo_keeps_its_links_and_asks_for_nothing() {
        let sources = library();
        let mut cache = Cache::default();
        for s in &sources {
            cache.notes.insert(
                s.id.clone(),
                Read {
                    hash: hash_of(s),
                    summary: "s".into(),
                    concepts: vec![],
                    linked: None,
                },
            );
        }
        cache.pairs.insert(
            "1:0-0".into(),
            Pair {
                hash: "old".into(),
                links: vec![NoteLink {
                    a: "a".into(),
                    b: "c".into(),
                    kind: "same idea".into(),
                    strength: 3,
                    why: "from before".into(),
                }],
            },
        );
        let asked = AtomicUsize::new(0);
        let write = |_: &str, _: &str, _: u32| -> Result<String> {
            asked.fetch_add(1, Ordering::SeqCst);
            Ok(r#"{"links": []}"#.into())
        };
        build(&sources, &mut cache, &write, &mut |_, _| {});
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        assert!(cache.pairs.is_empty());
        assert_eq!(cache.links.len(), 1);
        assert!(cache.all_links().any(|e| e.why == "from before"));
    }

    #[test]
    fn a_failed_reply_leaves_the_notes_to_try_again() {
        let sources = library();
        let mut cache = Cache::default();
        let write = |_: &str, _: &str, _: u32| -> Result<String> {
            Ok("Sorry, I can't help with that.".into())
        };
        let problems = build(&sources, &mut cache, &write, &mut |_, _| {});
        assert!(!problems.is_empty());
        assert!(cache.notes.is_empty());
        assert!(cache.built_at.is_none());
        assert_eq!(stale(&sources, &cache).len(), 3);
        let down = |_: &str, _: &str, _: u32| -> Result<String> {
            anyhow::bail!("no AI for writing is chosen")
        };
        let problems = build(&sources, &mut cache, &down, &mut |_, _| {});
        assert_eq!(problems[0], "no AI for writing is chosen");
    }

    #[test]
    fn batches_respect_note_and_size_limits() {
        let many: Vec<Source> = (0..20)
            .map(|i| {
                source(
                    &i.to_string(),
                    "t",
                    &"x".repeat(if i == 3 { 30_000 } else { 100 }),
                    "",
                )
            })
            .collect();
        let refs: Vec<&Source> = many.iter().collect();
        let batches = plan(&refs);
        assert!(batches
            .iter()
            .all(|b| !b.is_empty() && b.len() <= BATCH_NOTES));
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 20);
        let (_, user) = read_prompt(&[&many[3]], &[]);
        assert!(
            user.chars().count() < NOTE_CHARS + 400,
            "a long note is clipped"
        );
    }

    #[test]
    fn a_written_cache_is_seen_by_the_next_load() {
        let dir = tempfile::tempdir().unwrap();
        let graphs = Graphs::for_notes(&dir.path().join("notes"), None);
        assert!(graphs.load().notes.is_empty());
        let one = Read {
            summary: "heaps".into(),
            ..Read::default()
        };
        let mut cache = Cache::default();
        cache.notes.insert("n1".into(), one);
        graphs.save(&cache).unwrap();
        assert_eq!(graphs.load().notes["n1"].summary, "heaps");
        assert!(Arc::ptr_eq(&graphs.load(), &graphs.load()));

        let mut file = Cache::default();
        file.notes.insert(
            "n2".into(),
            Read {
                summary: "queues".into(),
                ..Read::default()
            },
        );
        std::fs::write(graphs.path(), serde_json::to_string(&file).unwrap()).unwrap();
        let seen = graphs.load();
        assert_eq!(seen.notes["n2"].summary, "queues");
    }

    #[test]
    fn concept_names_are_tidied() {
        assert_eq!(
            concept_label("  **Big-O   notation.** "),
            Some("Big-O notation".into())
        );
        assert_eq!(concept_label("- queue"), Some("queue".into()));
        assert_eq!(concept_label("  ... "), None);
        assert_eq!(
            concept_label(&"x".repeat(200)).unwrap().chars().count(),
            CONCEPT_CHARS
        );
    }

    #[test]
    fn a_build_runs_in_the_background_and_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let writer: Writer = Arc::new(|system: &str, _: &str, _| {
            if system.starts_with("You read") {
                Ok(r#"{"notes": [{"id": "n1", "summary": "one", "concepts": ["queue"]}, {"id": "n2", "summary": "two", "concepts": ["stack"]}, {"id": "n3", "summary": "three", "concepts": ["queue"]}]}"#.into())
            } else {
                Ok(r#"{"links": [{"a": "n1", "b": "n2", "kind": "contrasts", "strength": 2, "why": "a queue and a stack order work oppositely"}]}"#.into())
            }
        });
        let graphs = Arc::new(Graphs::for_notes(&dir.path().join("notes"), Some(writer)));
        let sources = library();
        assert_eq!(graphs.status(&sources).requests, 2);
        graphs.start(sources.clone());
        let started = std::time::Instant::now();
        while graphs.status(&sources).state == "building" {
            assert!(started.elapsed().as_secs() < 10, "the build never finished");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let status = graphs.status(&sources);
        assert_eq!(status.state, "done", "{status:?}");
        assert_eq!((status.read, status.stale, status.requests), (3, 0, 0));
        assert!(
            !graphs.load().notes.is_empty(),
            "the build is kept in leo's database"
        );
        assert!(
            graphs
                .load()
                .all_links()
                .any(|e| e.kind == "contrasts"
                    && e.why == "a queue and a stack order work oppositely")
        );

        let without = Arc::new(Graphs::for_notes(&dir.path().join("other/notes"), None));
        without.start(sources.clone());
        assert_eq!(without.status(&sources).state, "failed");
    }
}
