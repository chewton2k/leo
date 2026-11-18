use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use leo_core::notes::{checkbox_line, Note};
use leo_core::store::Store;
use leo_core::tasks::{self as core, Located};
use serde::{Deserialize, Serialize};

use crate::jsonline;

pub const MOST_TASKS: usize = 10_000;
pub const MOST_TITLE: usize = 300;
pub const MOST_DESCRIPTION: usize = 4_000;

static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn description_of(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    if text.chars().count() > MOST_DESCRIPTION {
        return Err("Keep the task description within 4,000 characters.");
    }
    Ok(text.to_string())
}
pub const MOST_LISTED: usize = 500;
pub const KINDS: [&str; 6] = [
    "assignment",
    "exam",
    "meeting",
    "follow-up",
    "deadline",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "other")]
    pub kind: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub time: Option<NaiveTime>,
    #[serde(default)]
    pub end: Option<NaiveTime>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repeat: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done_on: Vec<NaiveDate>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub moved: BTreeMap<NaiveDate, Slot>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub date: NaiveDate,
    #[serde(default)]
    pub time: Option<NaiveTime>,
    #[serde(default)]
    pub end: Option<NaiveTime>,
}

pub const MOST_DONE_DAYS: usize = 400;
pub const MOST_MOVED: usize = 400;

pub fn clean_repeat(days: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = days.iter().copied().filter(|d| *d < 7).collect();
    out.sort_unstable();
    out.dedup();
    out
}

pub fn repeats_on(repeat: &[u8], start: Option<NaiveDate>, day: NaiveDate) -> bool {
    use chrono::Datelike;
    !repeat.is_empty()
        && repeat.contains(&(day.weekday().num_days_from_monday() as u8))
        && start.is_none_or(|start| day >= start)
}

pub fn occurs_on(
    repeat: &[u8],
    start: Option<NaiveDate>,
    moved: &BTreeMap<NaiveDate, Slot>,
    day: NaiveDate,
) -> Option<NaiveDate> {
    if !moved.contains_key(&day) && repeats_on(repeat, start, day) {
        return Some(day);
    }
    moved
        .iter()
        .find(|(on, slot)| slot.date == day && repeats_on(repeat, start, **on))
        .map(|(on, _)| *on)
}

pub fn move_occurrence(task: &mut Task, on: NaiveDate, slot: Slot) {
    if slot.date == on && slot.time == task.time && slot.end == task.end {
        task.moved.remove(&on);
    } else {
        task.moved.insert(on, slot);
    }
    while task.moved.len() > MOST_MOVED {
        task.moved.pop_first();
    }
}

const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

pub fn rrule_of(repeat: &[u8]) -> Option<String> {
    let days: Vec<&str> = repeat
        .iter()
        .filter_map(|d| WEEKDAYS.get(*d as usize).copied())
        .collect();
    (!days.is_empty()).then(|| format!("RRULE:FREQ=WEEKLY;BYDAY={}", days.join(",")))
}

fn other() -> String {
    "other".into()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub id: String,
    pub note: String,
    pub text: String,
    pub description: String,
    pub before: String,
    pub after: String,
    pub kind: Option<String>,
    pub promoted: bool,
    pub sent_at: Option<DateTime<Utc>>,
}

pub fn kind_of(text: &str) -> String {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["exam", "midterm", "final", "quiz "]) {
        "exam"
    } else if has(&[
        "submit",
        "homework",
        "assignment",
        "essay",
        "problem set",
        "lab report",
        "hw ",
    ]) {
        "assignment"
    } else if has(&["meeting", "meet with", "attend", "call with"]) {
        "meeting"
    } else if has(&[
        "send",
        "email",
        "follow up",
        "follow-up",
        "reply",
        "contact",
    ]) {
        "follow-up"
    } else {
        "other"
    }
    .into()
}

pub fn clean_kind(kind: &str) -> String {
    let k = kind.trim().to_lowercase();
    if KINDS.contains(&k.as_str()) {
        k
    } else {
        "other".into()
    }
}

fn fnv(text: &str) -> u64 {
    crate::vectors::fnv64(text.bytes())
}

pub fn note_task_id(note: &str, text: &str) -> String {
    format!("n{:016x}", fnv(&format!("{note}\0{}", text.trim())))
}

pub use crate::jsonline::valid_id;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listed {
    pub id: String,
    pub source: &'static str,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub done: bool,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub end: Option<NaiveTime>,
    pub note: Option<NoteRef>,
    pub line: Option<usize>,
    pub found: bool,
    pub promoted: bool,
    pub sent_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repeat: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub done_on: Vec<NaiveDate>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub moved: BTreeMap<NaiveDate, Slot>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoteRef {
    pub id: String,
    pub title: String,
    pub folder: String,
}

pub fn tasks_in(note: &Note, metas: &[&Meta]) -> Vec<Listed> {
    let mut out: Vec<Listed> = Vec::new();
    let lines: Vec<&str> = note.body.lines().collect();
    let note_ref = NoteRef {
        id: note.id.clone(),
        title: note.title.clone(),
        folder: note.directory.clone(),
    };
    let mut claimed: HashSet<usize> = HashSet::new();
    for meta in metas {
        let at = match core::locate(&note.body, &meta.text, &meta.before, &meta.after) {
            Located::At(n) => Some(n),
            _ => None,
        };
        if let Some(n) = at {
            claimed.insert(n);
        }
        let text = at
            .and_then(|n| checkbox_line(lines[n]).map(|c| c.text.to_string()))
            .unwrap_or_else(|| meta.text.clone());
        let done = at
            .and_then(|n| checkbox_line(lines[n]))
            .is_some_and(|c| c.ticked);
        let mut task = listed(
            &meta.id,
            &text,
            done,
            meta.kind.clone(),
            &note_ref,
            at,
            at.is_some(),
            meta.promoted,
            meta.sent_at,
        );
        task.description = meta.description.clone();
        out.push(task);
    }
    for n in core::action_items(&note.body) {
        if claimed.contains(&n) {
            continue;
        }
        let Some(found) = checkbox_line(lines[n]) else {
            continue;
        };
        if found.text.trim().is_empty() {
            continue;
        }
        let id = note_task_id(&note.id, found.text);
        if out.iter().any(|t| t.id == id) {
            continue;
        }
        out.push(listed(
            &id,
            found.text,
            found.ticked,
            None,
            &note_ref,
            Some(n),
            true,
            false,
            None,
        ));
    }
    out
}

pub fn note_tasks(store: &Store, metas: &[Meta]) -> Vec<Listed> {
    let mut by_note: BTreeMap<&str, Vec<&Meta>> = BTreeMap::new();
    for meta in metas {
        by_note.entry(meta.note.as_str()).or_default().push(meta);
    }
    store
        .notes
        .iter()
        .flat_map(|note| {
            let own = by_note.get(note.id.as_str()).map_or(&[][..], Vec::as_slice);
            tasks_in(note, own)
        })
        .collect()
}

pub fn note_task(store: &Store, metas: &[Meta], note: &str, id: &str) -> Option<Listed> {
    let note = store.notes.iter().find(|n| n.id == note)?;
    let own: Vec<&Meta> = metas.iter().filter(|m| m.note == note.id).collect();
    tasks_in(note, &own).into_iter().find(|t| t.id == id)
}

pub fn find_listed(
    listed: &ListedCache,
    generation: &AtomicU64,
    store: &Store,
    metas: &[Meta],
    stored: &[Task],
    id: &str,
) -> Option<Listed> {
    let kept = listed
        .find(generation, store, metas, stored, id)
        .filter(|t| t.source == "note")?;
    note_task(store, metas, &kept.note.as_ref()?.id, id)
}

#[allow(clippy::too_many_arguments)]
fn listed(
    id: &str,
    text: &str,
    done: bool,
    kind: Option<String>,
    note: &NoteRef,
    line: Option<usize>,
    found: bool,
    promoted: bool,
    sent_at: Option<DateTime<Utc>>,
) -> Listed {
    let due = core::due_in(text);
    Listed {
        id: id.to_string(),
        source: "note",
        description: String::new(),
        title: text.trim().chars().take(MOST_TITLE).collect(),
        kind: kind.unwrap_or_else(|| kind_of(text)),
        done,
        date: due.map(|d| d.date),
        time: due.and_then(|d| d.time),
        end: None,
        note: Some(note.clone()),
        line,
        found,
        promoted,
        sent_at,
        repeat: Vec::new(),
        done_on: Vec::new(),
        moved: BTreeMap::new(),
    }
}

pub fn standalone(task: &Task, store: Option<&Store>) -> Listed {
    let note = task.note.as_deref().and_then(|id| {
        store
            .and_then(|s| s.notes.iter().find(|n| n.id == id))
            .map(|n| NoteRef {
                id: n.id.clone(),
                title: n.title.clone(),
                folder: n.directory.clone(),
            })
    });
    Listed {
        id: task.id.clone(),
        source: "leo",
        title: task.title.clone(),
        description: task.description.clone(),
        kind: task.kind.clone(),
        done: task.done,
        date: task.date,
        time: task.time,
        end: task.end,
        note,
        line: None,
        found: true,
        promoted: false,
        sent_at: task.sent_at,
        repeat: task.repeat.clone(),
        done_on: task.done_on.clone(),
        moved: task.moved.clone(),
    }
}

pub(crate) fn in_order(all: &mut [Listed]) {
    all.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(a.date.is_none().cmp(&b.date.is_none()))
            .then(a.date.cmp(&b.date))
            .then(a.time.is_none().cmp(&b.time.is_none()))
            .then(a.time.cmp(&b.time))
            .then(a.title.cmp(&b.title))
    });
}

pub struct ListedIndex {
    pub all: Vec<Listed>,
    by_id: HashMap<String, usize>,
}

type Kept = (u64, Vec<Meta>, Vec<Task>, Arc<ListedIndex>);

#[derive(Default)]
pub struct ListedCache {
    kept: Mutex<Option<Kept>>,
    #[cfg(test)]
    builds: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    cloned: std::sync::atomic::AtomicUsize,
}

impl ListedCache {
    pub fn index(
        &self,
        generation: &AtomicU64,
        store: &Store,
        metas: &[Meta],
        stored: &[Task],
    ) -> Arc<ListedIndex> {
        let at = generation.load(Ordering::Acquire);
        if let Some((kept_at, kept_metas, kept_stored, index)) =
            self.kept.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        {
            if *kept_at == at && kept_metas == metas && kept_stored == stored {
                return Arc::clone(index);
            }
        }
        #[cfg(test)]
        self.builds.fetch_add(1, Ordering::Relaxed);
        let mut all = note_tasks(store, metas);
        all.extend(stored.iter().map(|t| standalone(t, Some(store))));
        in_order(&mut all);
        let by_id = all
            .iter()
            .enumerate()
            .rev()
            .map(|(at, task)| (task.id.clone(), at))
            .collect();
        let index = Arc::new(ListedIndex { all, by_id });
        *self.kept.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((at, metas.to_vec(), stored.to_vec(), Arc::clone(&index)));
        index
    }

    pub fn all(
        &self,
        generation: &AtomicU64,
        store: &Store,
        metas: &[Meta],
        stored: &[Task],
    ) -> Vec<Listed> {
        let index = self.index(generation, store, metas, stored);
        #[cfg(test)]
        self.cloned
            .fetch_add(index.all.len().min(MOST_LISTED), Ordering::Relaxed);
        index.all.iter().take(MOST_LISTED).cloned().collect()
    }

    pub fn find(
        &self,
        generation: &AtomicU64,
        store: &Store,
        metas: &[Meta],
        stored: &[Task],
        id: &str,
    ) -> Option<Listed> {
        let index = self.index(generation, store, metas, stored);
        let at = *index.by_id.get(id)?;
        #[cfg(test)]
        self.cloned.fetch_add(1, Ordering::Relaxed);
        Some(index.all[at].clone())
    }

    #[cfg(test)]
    pub fn builds(&self) -> usize {
        self.builds.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn cloned(&self) -> usize {
        self.cloned.load(Ordering::Relaxed)
    }
}

pub fn ics_of(task: &Listed, stamp: DateTime<Utc>) -> String {
    let escape = |s: &str| {
        s.replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,")
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\\n")
    };
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//leo//tasks//EN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}@leo", task.id),
        format!("DTSTAMP:{}", stamp.format("%Y%m%dT%H%M%SZ")),
        format!("SUMMARY:{}", escape(&task.title)),
    ];
    if let Some(date) = task.date {
        lines.extend(when_lines(
            date,
            task.time,
            task.end,
            "DTSTART",
            "DTEND",
            rrule_of(&task.repeat).is_none(),
        ));
    }
    let mut moves = Vec::new();
    if let Some(rule) = rrule_of(&task.repeat) {
        lines.push(rule);
        for (on, slot) in &task.moved {
            let [was, _] = when_lines(*on, task.time, task.end, "", "", false);
            lines.push(format!("EXDATE{was}"));
            moves.push("BEGIN:VEVENT".to_string());
            moves.push(format!("UID:{}@leo", task.id));
            moves.push(format!("DTSTAMP:{}", stamp.format("%Y%m%dT%H%M%SZ")));
            moves.push(format!("RECURRENCE-ID{was}"));
            moves.push(format!("SUMMARY:{}", escape(&task.title)));
            moves.extend(when_lines(
                slot.date, slot.time, slot.end, "DTSTART", "DTEND", false,
            ));
            moves.push("END:VEVENT".into());
        }
    }
    let from = task
        .note
        .as_ref()
        .map(|n| format!(" · {}", escape(&n.title)))
        .unwrap_or_default();
    let about = if task.description.is_empty() {
        String::new()
    } else {
        format!("{}\\n\\n", escape(&task.description))
    };
    lines.push(format!(
        "DESCRIPTION:{about}From leo{from} · leo-task:{}",
        task.id.chars().take(12).collect::<String>()
    ));
    lines.push("END:VEVENT".into());
    lines.extend(moves);
    lines.push("END:VCALENDAR".into());
    lines.join("\r\n") + "\r\n"
}

fn when_lines(
    date: NaiveDate,
    time: Option<NaiveTime>,
    end: Option<NaiveTime>,
    start_name: &str,
    end_name: &str,
    lead_in: bool,
) -> [String; 2] {
    match time {
        Some(time) => {
            let at = date.and_time(time);
            let half_hour = chrono::Duration::minutes(30);
            let start = end.map_or(if lead_in { at - half_hour } else { at }, |_| at);
            let stop = end.map_or(if lead_in { at } else { at + half_hour }, |e| {
                date.and_time(e)
            });
            [
                format!("{start_name}:{}", start.format("%Y%m%dT%H%M%S")),
                format!("{end_name}:{}", stop.format("%Y%m%dT%H%M%S")),
            ]
        }
        None => [
            format!("{start_name};VALUE=DATE:{}", date.format("%Y%m%d")),
            format!(
                "{end_name};VALUE=DATE:{}",
                (date + chrono::Duration::days(1)).format("%Y%m%d")
            ),
        ],
    }
}

pub fn db_of(chats: &Path) -> anyhow::Result<std::sync::Arc<crate::db::Db>> {
    crate::db::try_for_chats(chats)
}

#[cfg(test)]
pub fn tasks_dir(chats: &Path) -> std::path::PathBuf {
    chats
        .parent()
        .unwrap_or(chats)
        .join("notes")
        .join(".leo")
        .join("tasks")
}

impl jsonline::Line for Task {
    const DIR: &'static str = "tasks";
    fn id(&self) -> &str {
        &self.id
    }
    fn stamp(&self) -> DateTime<Utc> {
        self.updated_at
    }
    fn deleted_at(&mut self, now: DateTime<Utc>) {
        self.deleted = true;
        self.updated_at = now;
    }
}

fn read_all(chats: &Path) -> Vec<Task> {
    let mut out: Vec<Task> = jsonline::read_all(chats, None);
    out.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
    out
}

pub fn load_all(chats: &Path) -> Vec<Task> {
    read_all(chats)
        .into_iter()
        .filter(|t| !t.deleted)
        .take(MOST_TASKS)
        .collect()
}

pub fn load(chats: &Path, id: &str) -> Option<Task> {
    if !valid_id(id) {
        return None;
    }
    jsonline::load::<Task>(chats, id).filter(|t| !t.deleted)
}

pub fn count(chats: &Path) -> usize {
    load_all(chats).len()
}

pub fn put(chats: &Path, task: &Task) -> anyhow::Result<()> {
    jsonline::write_file(chats, task)
}

pub fn remove(chats: &Path, id: &str) -> bool {
    let Some(task) = load(chats, id) else {
        return false;
    };
    jsonline::write_gone(chats, &task, Utc::now()).is_ok()
}

pub fn forget_deleted(chats: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    jsonline::forget_deleted::<Task>(chats, now - chrono::Duration::days(i64::from(days)), |t| {
        t.deleted
    })
}

pub fn metas(chats: &Path) -> Vec<Meta> {
    db_of(chats)
        .and_then(|db| db.with(|c| {
            let mut statement = c.prepare("SELECT id, note, text, before, after, kind, promoted, sent_at, description FROM note_tasks ORDER BY rowid DESC LIMIT ?1")?;
            let rows = statement.query_map([MOST_TASKS as i64], |r| {
                Ok(Meta {
                    id: r.get(0)?,
                    note: r.get(1)?,
                    text: r.get(2)?,
                    description: r.get(8)?,
                    before: r.get(3)?,
                    after: r.get(4)?,
                    kind: r.get::<_, Option<String>>(5)?,
                    promoted: r.get::<_, i64>(6)? != 0,
                    sent_at: r
                        .get::<_, Option<String>>(7)?
                        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                        .map(|t| t.with_timezone(&Utc)),
                })
            })?;
            Ok(rows.flatten().collect::<Vec<_>>())
        }))
        .unwrap_or_default()
}

pub fn put_meta(chats: &Path, meta: &Meta) -> anyhow::Result<()> {
    let sent = meta.sent_at.map(|t| t.to_rfc3339());
    db_of(chats)?.with(|c| {
        c.execute(
            "INSERT INTO note_tasks (id, note, text, before, after, kind, promoted, sent_at, description) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(id) DO UPDATE SET note = excluded.note, text = excluded.text, before = excluded.before, after = excluded.after, kind = excluded.kind, promoted = excluded.promoted, sent_at = excluded.sent_at, description = excluded.description",
            rusqlite::params![meta.id, meta.note, meta.text, meta.before, meta.after, meta.kind, i64::from(meta.promoted), sent, meta.description],
        )
    })?;
    Ok(())
}

pub fn remove_meta(chats: &Path, id: &str) -> bool {
    db_of(chats)
        .and_then(|db| db.with(|c| c.execute("DELETE FROM note_tasks WHERE id = ?1", [id])))
        .is_ok_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(body: &str) -> (tempfile::TempDir, Store, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store
            .create_note("Lecture 5", body, vec![], "bio")
            .unwrap()
            .id
            .clone();
        (dir, store, id)
    }

    #[test]
    fn action_items_become_tasks_and_examples_do_not() {
        let body = "## Notes\n- [ ] an example checklist\n## Action items\n- [ ] Submit Homework 4 — due October 16, 2026 at 11:59 pm.\n- [x] Email Maya the slides\n## Check yourself\n- [ ] What is a heap?";
        let (_d, store, id) = store_with(body);
        let tasks = note_tasks(&store, &[]);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Submit Homework 4 — due October 16, 2026 at 11:59 pm.",
                "Email Maya the slides"
            ]
        );
        assert_eq!(tasks[0].date, NaiveDate::from_ymd_opt(2026, 10, 16));
        assert_eq!(tasks[0].time, NaiveTime::from_hms_opt(23, 59, 0));
        assert_eq!(tasks[0].kind, "assignment");
        assert!(tasks[1].done);
        assert_eq!(tasks[1].kind, "follow-up");
        assert_eq!(
            tasks[0].id,
            note_task_id(&id, "Submit Homework 4 — due October 16, 2026 at 11:59 pm.")
        );
    }

    #[test]
    fn a_promoted_checkbox_is_a_task_and_is_marked_when_it_cannot_be_found() {
        let body = "Plan\n- [ ] Ask tutor about the exam\nEnd";
        let (_d, store, id) = store_with(body);
        let meta = Meta {
            id: note_task_id(&id, "Ask tutor about the exam"),
            note: id.clone(),
            text: "Ask tutor about the exam".into(),
            description: String::new(),
            before: "Plan".into(),
            after: "End".into(),
            kind: Some("exam".into()),
            promoted: true,
            sent_at: None,
        };
        let tasks = note_tasks(&store, std::slice::from_ref(&meta));
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].found && tasks[0].promoted);
        assert_eq!((tasks[0].line, tasks[0].kind.as_str()), (Some(1), "exam"));
        let gone = Meta {
            text: "Something else".into(),
            ..meta
        };
        let tasks = note_tasks(&store, &[gone]);
        assert!(!tasks[0].found);
    }

    #[test]
    fn tasks_sort_open_first_then_by_date_and_an_ics_names_its_task() {
        let now = Utc::now();
        let task = |id: &str, done: bool, date: Option<NaiveDate>| {
            standalone(
                &Task {
                    id: id.into(),
                    title: id.into(),
                    description: String::new(),
                    kind: "other".into(),
                    done,
                    date,
                    time: None,
                    end: None,
                    note: None,
                    sent_at: None,
                    created_at: now,
                    updated_at: now,
                    repeat: Vec::new(),
                    done_on: Vec::new(),
                    moved: BTreeMap::new(),
                    deleted: false,
                },
                None,
            )
        };
        let mut all = vec![
            task("undated", false, None),
            task("done", true, NaiveDate::from_ymd_opt(2026, 1, 1)),
            task("later", false, NaiveDate::from_ymd_opt(2026, 12, 1)),
            task("sooner", false, NaiveDate::from_ymd_opt(2026, 11, 1)),
        ];
        in_order(&mut all);
        let order: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(order, ["sooner", "later", "undated", "done"]);
        let ics = ics_of(&all[0], now);
        assert!(ics.contains("DTSTART;VALUE=DATE:20261101"));
        assert!(ics.contains("DTEND;VALUE=DATE:20261102"));
        assert!(ics.contains("leo-task:sooner"));
        assert_eq!(kind_of("Biology midterm"), "exam");
        assert_eq!(clean_kind("Party"), "other");
    }

    fn plain(id: &str, title: &str, at: DateTime<Utc>) -> Task {
        Task {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            kind: "other".into(),
            done: false,
            date: None,
            time: None,
            end: None,
            note: None,
            sent_at: None,
            created_at: at,
            updated_at: at,
            repeat: Vec::new(),
            done_on: Vec::new(),
            moved: BTreeMap::new(),
            deleted: false,
        }
    }

    #[test]
    fn live_tasks_are_listed_however_many_tombstones_sit_beside_them() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let folder = dir.path().join("notes/.leo/tasks");
        std::fs::create_dir_all(&folder).unwrap();
        for n in 0..MOST_TASKS * 3 {
            let id = format!("gone-{n:06}");
            let mut gone = plain(&id, "Gone", now);
            gone.deleted = true;
            std::fs::write(
                folder.join(format!("{id}.json")),
                serde_json::to_string(&gone).unwrap(),
            )
            .unwrap();
        }
        for n in 0..100 {
            put(&chats, &plain(&format!("live-{n:06}"), "Live", now)).unwrap();
        }
        assert_eq!(load_all(&chats).len(), 100);
        assert_eq!(count(&chats), 100);
    }

    #[test]
    fn a_deleted_task_leaves_its_file_only_for_the_trash_period() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        put(&chats, &plain("task-0001", "Old", now)).unwrap();
        put(&chats, &plain("task-0002", "Kept", now)).unwrap();
        assert!(remove(&chats, "task-0001"));
        let file = dir.path().join("notes/.leo/tasks/task-0001.json");
        assert_eq!(forget_deleted(&chats, None, now), 0);
        assert_eq!(forget_deleted(&chats, Some(30), now), 0);
        assert!(file.exists());
        let later = now + chrono::Duration::days(31);
        let copy = dir.path().join("notes/.leo/tasks/task-0002 (copy).json");
        std::fs::copy(&file, &copy).unwrap();
        let old = std::fs::read_to_string(&copy).unwrap();
        std::fs::write(&copy, old.replace("task-0001", "task-0002")).unwrap();
        assert_eq!(forget_deleted(&chats, Some(30), later), 1);
        assert!(!file.exists());
        assert!(
            copy.exists(),
            "only a tombstone under its own name is removed"
        );
        assert_eq!(load_all(&chats).len(), 1);
        assert!(load(&chats, "task-0002").is_some());
    }

    #[test]
    fn tasks_live_in_files_beside_the_notes_and_the_newest_merged_line_wins() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        put(&chats, &plain("task-0001", "Write essay", now)).unwrap();
        let file = dir.path().join("notes/.leo/tasks/task-0001.json");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 1);
        let newer = plain(
            "task-0001",
            "Write the essay",
            now + chrono::Duration::seconds(5),
        );
        let older = plain("task-0001", "Old title", now - chrono::Duration::seconds(5));
        std::fs::write(
            &file,
            format!(
                "{}\n{}\n{}\n",
                serde_json::to_string(&older).unwrap(),
                serde_json::to_string(&newer).unwrap(),
                text.trim()
            ),
        )
        .unwrap();
        assert_eq!(load(&chats, "task-0001").unwrap().title, "Write the essay");
        assert!(remove(&chats, "task-0001"));
        assert!(load(&chats, "task-0001").is_none());
        assert!(load_all(&chats).is_empty());
        assert!(
            file.exists(),
            "a deletion is a newer line, not a missing file"
        );
        let mut back = newer.clone();
        back.updated_at = Utc::now() + chrono::Duration::seconds(10);
        put(&chats, &back).unwrap();
        assert_eq!(load_all(&chats).len(), 1);
    }

    #[test]
    fn a_repeating_task_falls_on_its_weekdays_from_its_start() {
        let monday = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        assert!(repeats_on(&[0, 2], Some(monday), monday));
        assert!(!repeats_on(
            &[0, 2],
            Some(monday),
            monday.succ_opt().unwrap()
        ));
        assert!(!repeats_on(
            &[0],
            Some(monday),
            monday - chrono::Duration::days(7)
        ));
        assert_eq!(clean_repeat(&[9, 2, 0, 2]), vec![0, 2]);
        assert_eq!(
            rrule_of(&[0, 4]).as_deref(),
            Some("RRULE:FREQ=WEEKLY;BYDAY=MO,FR")
        );
    }

    #[test]
    fn a_moved_occurrence_keeps_its_original_day_as_its_name() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let wednesday = monday + chrono::Duration::days(2);
        let thursday = monday + chrono::Duration::days(3);
        let three = NaiveTime::from_hms_opt(15, 0, 0);
        let mut task = plain("task-0011", "Gym", Utc::now());
        task.repeat = vec![0, 2];
        task.date = Some(monday);
        task.time = NaiveTime::from_hms_opt(9, 0, 0);
        let none = BTreeMap::new();
        assert_eq!(
            occurs_on(&task.repeat, task.date, &none, wednesday),
            Some(wednesday)
        );
        move_occurrence(
            &mut task,
            wednesday,
            Slot {
                date: thursday,
                time: three,
                end: None,
            },
        );
        assert_eq!(
            occurs_on(&task.repeat, task.date, &task.moved, wednesday),
            None
        );
        assert_eq!(
            occurs_on(&task.repeat, task.date, &task.moved, thursday),
            Some(wednesday)
        );
        assert_eq!(
            occurs_on(&task.repeat, task.date, &task.moved, monday),
            Some(monday)
        );
        let (time, end) = (task.time, task.end);
        move_occurrence(
            &mut task,
            wednesday,
            Slot {
                date: wednesday,
                time,
                end,
            },
        );
        assert!(task.moved.is_empty(), "moving it back forgets the move");
        for week in 0..(MOST_MOVED as i64 + 5) {
            let on = monday + chrono::Duration::weeks(week);
            move_occurrence(
                &mut task,
                on,
                Slot {
                    date: on,
                    time: three,
                    end: None,
                },
            );
        }
        assert_eq!(task.moved.len(), MOST_MOVED);
        assert!(
            !task.moved.contains_key(&monday),
            "the oldest moves go first"
        );
    }

    #[test]
    fn a_task_without_moves_is_written_exactly_as_before() {
        let task = plain("task-0012", "Plain", Utc::now());
        let text = serde_json::to_string(&task).unwrap();
        assert!(!text.contains("moved"), "{text}");
        let back: Task = serde_json::from_str(&text).unwrap();
        assert!(back.moved.is_empty());
    }

    #[test]
    fn calendar_files_skip_a_moved_day_and_add_it_where_it_went() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let mut task = plain("task-0013", "Gym", Utc::now());
        task.repeat = vec![0];
        task.date = Some(monday);
        task.time = NaiveTime::from_hms_opt(9, 0, 0);
        task.end = NaiveTime::from_hms_opt(10, 0, 0);
        let next = monday + chrono::Duration::weeks(1);
        move_occurrence(
            &mut task,
            next,
            Slot {
                date: next + chrono::Duration::days(1),
                time: NaiveTime::from_hms_opt(18, 0, 0),
                end: NaiveTime::from_hms_opt(19, 0, 0),
            },
        );
        let ics = ics_of(&standalone(&task, None), Utc::now());
        assert!(ics.contains("EXDATE:20261109T090000\r\n"), "{ics}");
        assert!(ics.contains("RECURRENCE-ID:20261109T090000\r\n"), "{ics}");
        assert!(
            ics.contains("DTSTART:20261110T180000\r\nDTEND:20261110T190000"),
            "{ics}"
        );
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), 2);
    }

    #[test]
    fn a_midnight_deadline_that_repeats_stays_on_its_weekday_and_a_single_one_keeps_its_lead_in() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let mut task = plain("task-0014", "Problem set due", Utc::now());
        task.repeat = vec![0];
        task.date = Some(monday);
        task.time = NaiveTime::from_hms_opt(0, 0, 0);
        let next = monday + chrono::Duration::weeks(1);
        move_occurrence(
            &mut task,
            next,
            Slot {
                date: next + chrono::Duration::days(1),
                time: NaiveTime::from_hms_opt(0, 0, 0),
                end: None,
            },
        );
        let ics = ics_of(&standalone(&task, None), Utc::now());
        assert!(
            ics.contains("DTSTART:20261102T000000\r\nDTEND:20261102T003000\r\n"),
            "{ics}"
        );
        assert!(ics.contains("EXDATE:20261109T000000\r\n"), "{ics}");
        assert!(ics.contains("RECURRENCE-ID:20261109T000000\r\n"), "{ics}");
        task.repeat.clear();
        task.moved.clear();
        let once = ics_of(&standalone(&task, None), Utc::now());
        assert!(
            once.contains("DTSTART:20261101T233000\r\nDTEND:20261102T000000"),
            "{once}"
        );
    }
}

#[cfg(all(test, unix))]
mod file_boundaries {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn tasks_never_read_write_or_erase_through_linked_directories_or_files() {
        for segment in ["notes", "notes/.leo", "notes/.leo/tasks"] {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let chats = dir.path().join("chats");
            let link = dir.path().join(segment);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(outside.path(), &link).unwrap();
            let task: Task = serde_json::from_value(serde_json::json!({"id":"task-safe-01","title":"Keep inside","created_at":Utc::now(),"updated_at":Utc::now()})).unwrap();
            assert!(put(&chats, &task).is_err(), "{segment}");
            assert!(load_all(&chats).is_empty());
            assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        }
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        std::fs::create_dir_all(tasks_dir(&chats)).unwrap();
        let outside = dir.path().join("private.json");
        let task: Task = serde_json::from_value(serde_json::json!({"id":"task-safe-01","title":"Keep inside","created_at":Utc::now(),"updated_at":Utc::now()})).unwrap();
        std::fs::write(&outside, serde_json::to_string(&task).unwrap()).unwrap();
        let file = tasks_dir(&chats).join("task-safe-01.json");
        symlink(&outside, &file).unwrap();
        assert!(load(&chats, &task.id).is_none());
        assert!(load_all(&chats).is_empty());
        assert!(put(&chats, &task).is_err());
        assert!(!remove(&chats, &task.id));
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            serde_json::to_string(&task).unwrap()
        );
    }

    #[test]
    fn planted_legacy_temporary_paths_are_never_opened() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        std::fs::create_dir_all(tasks_dir(&chats)).unwrap();
        let outside = dir.path().join("private.txt");
        std::fs::write(&outside, "keep").unwrap();
        let task: Task = serde_json::from_value(serde_json::json!({"id":"task-safe-02","title":"Safe write","created_at":Utc::now(),"updated_at":Utc::now()})).unwrap();
        let planted = tasks_dir(&chats).join(format!(".{}.{}.tmp", task.id, std::process::id()));
        symlink(&outside, &planted).unwrap();
        put(&chats, &task).unwrap();
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "keep");
        assert_eq!(load(&chats, &task.id).unwrap(), task);
    }
}
