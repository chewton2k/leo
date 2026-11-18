use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WANTS_READ_CHARS: usize = 4000;
pub const CONTEXT_READ_CHARS: usize = 8000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Profile {
    pub context: String,
    pub wants: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub speakers: bool,
}

impl Profile {
    pub fn checked(self) -> Profile {
        Profile {
            context: self.context.trim().to_string(),
            wants: self.wants.trim().to_string(),
            speakers: self.speakers,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Passage {
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
    #[serde(default)]
    pub speaker: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Point {
    pub at_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Trace {
    pub at: DateTime<Utc>,
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub id: String,
    pub started: DateTime<Utc>,
    pub passages: Vec<Passage>,
    pub points: Vec<Point>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub wants: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub trace: Vec<Trace>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
}
const PAGE_BYTES: usize = 512 * 1024;
const HOUR: u64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "Stored")]
pub struct Head {
    pub id: String,
    pub started: DateTime<Utc>,
    pub context: String,
    pub wants: String,
    pub warnings: Vec<String>,
    pub trace: Vec<Trace>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub copied_from: String,
    pub speakers: Vec<String>,
    pub passages: usize,
    pub points: usize,
    pub revision: u64,
}

impl Head {
    fn of(source: &Archive) -> Head {
        Head {
            id: source.id.clone(),
            started: source.started,
            context: source.context.clone(),
            wants: source.wants.clone(),
            warnings: source.warnings.clone(),
            trace: source.trace.clone(),
            from: source.from.clone(),
            copied_from: String::new(),
            speakers: Vec::new(),
            passages: 0,
            points: 0,
            revision: 0,
        }
    }
}

#[derive(Deserialize)]
struct Stored {
    id: String,
    started: DateTime<Utc>,
    #[serde(default)]
    context: String,
    #[serde(default)]
    wants: String,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    trace: Vec<Trace>,
    #[serde(default)]
    from: String,
    #[serde(default)]
    copied_from: String,
    #[serde(default)]
    speakers: Vec<String>,
    passages: Rows,
    points: Rows,
    #[serde(default)]
    revision: u64,
}

impl From<Stored> for Head {
    fn from(s: Stored) -> Head {
        Head {
            id: s.id,
            started: s.started,
            context: s.context,
            wants: s.wants,
            warnings: s.warnings,
            trace: s.trace,
            from: s.from,
            copied_from: s.copied_from,
            speakers: if s.revision == 0 {
                s.passages.speakers
            } else {
                s.speakers
            },
            passages: s.passages.count,
            points: s.points.count,
            revision: s.revision,
        }
    }
}

#[derive(Default)]
struct Rows {
    count: usize,
    speakers: Vec<String>,
}

#[derive(Deserialize)]
struct Speaker {
    #[serde(default)]
    speaker: String,
}

impl<'de> Deserialize<'de> for Rows {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Rows, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = Rows;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a count or a list")
            }
            fn visit_u64<E>(self, count: u64) -> std::result::Result<Rows, E> {
                Ok(Rows {
                    count: count as usize,
                    speakers: Vec::new(),
                })
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Rows, A::Error> {
                let mut rows = Rows::default();
                while let Some(Speaker { speaker }) = seq.next_element()? {
                    rows.count += 1;
                    if !rows.speakers.contains(&speaker) {
                        rows.speakers.push(speaker);
                    }
                }
                Ok(rows)
            }
        }
        d.deserialize_any(Visit)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Passage,
    Point,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Passage => "passages",
            Kind::Point => "points",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub kind: Kind,
    pub seq: u64,
    pub text: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    Stale,
    NoRow,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Stale => "This transcript was changed since it was opened",
            Refused::NoRow => "That line is not in this transcript",
        })
    }
}

impl std::error::Error for Refused {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub passages: Option<u64>,
    pub points: Option<u64>,
}

impl Cursor {
    pub const START: Cursor = Cursor {
        passages: Some(0),
        points: Some(0),
    };
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub head: Head,
    pub passages: Vec<(u64, Passage)>,
    pub points: Vec<(u64, Point)>,
    pub at: Cursor,
    pub next: Option<Cursor>,
}

pub struct Sources {
    pub notes: PathBuf,
    pub note: String,
    pub heads: Vec<Head>,
}

impl Sources {
    pub fn of(notes: &Path, note: &str) -> Result<Sources> {
        Ok(Sources {
            notes: notes.to_path_buf(),
            note: note.to_string(),
            heads: list(notes, note)?,
        })
    }

    pub fn rows(
        &self,
        head: &Head,
        passage: impl FnMut(u64, Passage) -> Result<bool>,
        point: impl FnMut(u64, Point) -> Result<bool>,
    ) -> Result<()> {
        rows(&self.notes, &self.note, head, passage, point)
    }
}

pub fn valid_id(id: &str) -> bool {
    (8..=80).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn root(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("recording-sources")
}

pub fn folder(notes: &Path, note: &str) -> Result<PathBuf> {
    if !valid_id(note) {
        bail!("Invalid recording note id");
    }
    crate::paths::contained_path(
        notes.parent().unwrap_or(notes),
        &Path::new("recording-sources").join(note),
    )
}

fn prepare(dir: &Path) -> Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        anyhow::ensure!(
            !meta.file_type().is_symlink(),
            "Recording files cannot use symbolic links"
        );
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_with(path: &Path, fill: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    prepare(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent folder"))?,
    )?;
    crate::paths::write_atomic_with(path, fill)
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_with(path, |w| Ok(serde_json::to_writer_pretty(w, value)?))
}

fn checked(id: &str) -> Result<&str> {
    anyhow::ensure!(valid_id(id), "Invalid recording id");
    Ok(id)
}

fn head_name(id: &str) -> String {
    format!("{id}.json")
}

fn lines_name(id: &str, revision: impl std::fmt::Display, kind: Kind) -> String {
    format!("{id}.{revision}.{}.jsonl", kind.word())
}

fn lines_of(name: &str) -> Option<(&str, u64)> {
    let rest = name.strip_suffix(".jsonl")?;
    let (rest, kind) = rest.rsplit_once('.')?;
    if kind != "passages" && kind != "points" {
        return None;
    }
    let (id, revision) = rest.split_once('.')?;
    Some((id, revision.parse().ok()?))
}

fn head_at(path: &Path) -> Result<Head> {
    let mut file = crate::paths::open_regular(path)?;
    if file.metadata()?.len() <= 1 << 20 {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes)?;
        return Ok(serde_json::from_slice(&bytes)?);
    }
    Ok(serde_json::from_reader(BufReader::new(file))?)
}

fn older_than_an_hour(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age.as_secs() > HOUR)
}

pub fn head(notes: &Path, note: &str, id: &str) -> Result<Head> {
    let head = head_at(&folder(notes, note)?.join(head_name(checked(id)?)))?;
    anyhow::ensure!(head.id == id, "This recording file names another recording");
    Ok(head)
}

#[cfg(test)]
thread_local!(static STOP_AFTER: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") });

fn after(step: &'static str) -> Result<()> {
    #[cfg(test)]
    anyhow::ensure!(STOP_AFTER.with(|s| s.get()) != step, "stopped after {step}");
    let _ = step;
    Ok(())
}

pub struct Out<'a> {
    w: &'a mut dyn Write,
    count: usize,
    speakers: Vec<String>,
}

impl Out<'_> {
    fn row(&mut self, seq: u64, row: &impl Serialize) -> Result<()> {
        serde_json::to_writer(&mut *self.w, &(seq, row))?;
        self.w.write_all(b"\n")?;
        self.count += 1;
        Ok(())
    }

    fn passage(&mut self, seq: u64, p: &Passage) -> Result<()> {
        if !self.speakers.contains(&p.speaker) {
            self.speakers.push(p.speaker.clone());
        }
        self.row(seq, p)
    }
}

fn keep_copy(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        return Ok(());
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) => write_with(to, |w| {
            std::io::copy(&mut crate::paths::open_regular(from)?, w)?;
            Ok(())
        }),
    }
}

fn keep_backup(dir: &Path, old: &Head) -> Result<()> {
    let id = &old.id;
    let original = dir.join(format!("{id}.original"));
    if original.exists() || dir.join(lines_name(id, "original", Kind::Passage)).exists() {
        return Ok(());
    }
    if old.revision == 0 {
        return keep_copy(&dir.join(head_name(id)), &original);
    }
    for kind in [Kind::Point, Kind::Passage] {
        keep_copy(
            &dir.join(lines_name(id, old.revision, kind)),
            &dir.join(lines_name(id, "original", kind)),
        )?;
    }
    Ok(())
}

fn remove_quietly(path: &Path) {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => crate::diag::warn(format!(
            "could not remove the old recording file {}: {e}",
            path.display()
        )),
        _ => {}
    }
}

fn commit(
    dir: &Path,
    old: Option<&Head>,
    head: &mut Head,
    backup: bool,
    fill: impl FnOnce(&mut Out, &mut Out) -> Result<()>,
) -> Result<()> {
    let id = head.id.clone();
    write_with(
        &dir.join(lines_name(&id, head.revision, Kind::Passage)),
        |pw| {
            write_with(
                &dir.join(lines_name(&id, head.revision, Kind::Point)),
                |qw| {
                    let mut passages = Out {
                        w: pw,
                        count: 0,
                        speakers: Vec::new(),
                    };
                    let mut points = Out {
                        w: qw,
                        count: 0,
                        speakers: Vec::new(),
                    };
                    fill(&mut passages, &mut points)?;
                    head.passages = passages.count;
                    head.points = points.count;
                    head.speakers = passages.speakers;
                    Ok(())
                },
            )
        },
    )?;
    after("pair")?;
    if let Some(old) = old.filter(|o| o.revision == 0) {
        let (mut passages, mut points) = (0usize, 0usize);
        read_lines(
            &dir.join(lines_name(&id, head.revision, Kind::Passage)),
            Some(0),
            &mut |_, _: Passage| {
                passages += 1;
                Ok(true)
            },
        )?;
        read_lines(
            &dir.join(lines_name(&id, head.revision, Kind::Point)),
            Some(0),
            &mut |_, _: Point| {
                points += 1;
                Ok(true)
            },
        )?;
        anyhow::ensure!(
            passages == head.passages
                && points == head.points
                && passages >= old.passages
                && points >= old.points,
            "The rewritten recording did not read back whole"
        );
    }
    if let Some(old) = old.filter(|_| backup) {
        keep_backup(dir, old)?;
        after("backup")?;
    }
    write_json(&dir.join(head_name(&id)), head)?;
    after("header")?;
    if let Some(old) = old.filter(|o| o.revision > 0 && o.revision != head.revision) {
        for kind in [Kind::Passage, Kind::Point] {
            remove_quietly(&dir.join(lines_name(&id, old.revision, kind)));
        }
    }
    Ok(())
}

pub fn save_from(
    notes: &Path,
    note: &str,
    base: &Head,
    passages: impl IntoIterator<Item = Passage>,
    points: impl IntoIterator<Item = Point>,
) -> Result<()> {
    let dir = folder(notes, note)?;
    let old = head_at(&dir.join(head_name(checked(&base.id)?))).ok();
    let mut head = base.clone();
    head.copied_from = String::new();
    head.revision = old.as_ref().map_or(0, |o| o.revision) + 1;
    commit(&dir, old.as_ref(), &mut head, false, |pout, qout| {
        for (n, p) in passages.into_iter().enumerate() {
            pout.passage(n as u64 + 1, &p)?;
        }
        for (n, p) in points.into_iter().enumerate() {
            qout.row(n as u64 + 1, &p)?;
        }
        Ok(())
    })
}

pub fn save(notes: &Path, note: &str, source: &Archive) -> Result<()> {
    save_from(
        notes,
        note,
        &Head::of(source),
        source.passages.iter().cloned(),
        source.points.iter().cloned(),
    )
}

pub fn rows(
    notes: &Path,
    note: &str,
    head: &Head,
    passage: impl FnMut(u64, Passage) -> Result<bool>,
    point: impl FnMut(u64, Point) -> Result<bool>,
) -> Result<()> {
    rows_from(notes, note, head, Cursor::START, passage, point)
}

fn rows_from(
    notes: &Path,
    note: &str,
    head: &Head,
    from: Cursor,
    mut passage: impl FnMut(u64, Passage) -> Result<bool>,
    mut point: impl FnMut(u64, Point) -> Result<bool>,
) -> Result<()> {
    let dir = folder(notes, note)?;
    let id = checked(&head.id)?;
    if from.passages.is_none() && from.points.is_none() {
        return Ok(());
    }
    if head.revision == 0 {
        return stream_old(&dir.join(head_name(id)), from, &mut passage, &mut point);
    }
    read_lines(
        &dir.join(lines_name(id, head.revision, Kind::Passage)),
        from.passages,
        &mut passage,
    )?;
    read_lines(
        &dir.join(lines_name(id, head.revision, Kind::Point)),
        from.points,
        &mut point,
    )
}

fn seq_of(line: &[u8]) -> Option<u64> {
    let rest = line.strip_prefix(b"[")?;
    let end = rest.iter().position(|b| !b.is_ascii_digit())?;
    std::str::from_utf8(&rest[..end]).ok()?.parse().ok()
}

fn read_lines<T: serde::de::DeserializeOwned>(
    path: &Path,
    from: Option<u64>,
    f: &mut dyn FnMut(u64, T) -> Result<bool>,
) -> Result<()> {
    let Some(from) = from else {
        return Ok(());
    };
    let mut reader = BufReader::new(crate::paths::open_regular(path)?);
    let mut line = Vec::new();
    let mut started = from == 0;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if !started {
            if seq_of(&line) != Some(from) {
                continue;
            }
            started = true;
        }
        let (seq, row): (u64, T) = serde_json::from_slice(&line)?;
        if !f(seq, row)? {
            return Ok(());
        }
    }
}

type Failed = std::cell::RefCell<Option<anyhow::Error>>;

struct Shared {
    failed: Failed,
    open: std::cell::Cell<u8>,
}

const ENOUGH: &str = "leo read enough of this recording";

struct Old<'a> {
    from: Cursor,
    passage: &'a mut dyn FnMut(u64, Passage) -> Result<bool>,
    point: &'a mut dyn FnMut(u64, Point) -> Result<bool>,
    shared: &'a Shared,
}

struct Each<'a, T> {
    from: Option<u64>,
    f: &'a mut dyn FnMut(u64, T) -> Result<bool>,
    shared: &'a Shared,
}

impl<'de> serde::de::DeserializeSeed<'de> for Old<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> serde::de::Visitor<'de> for Old<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a recording")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<std::borrow::Cow<str>>()? {
            match key.as_ref() {
                "passages" => map.next_value_seed(Each {
                    from: self.from.passages,
                    f: &mut *self.passage,
                    shared: self.shared,
                })?,
                "points" => map.next_value_seed(Each {
                    from: self.from.points,
                    f: &mut *self.point,
                    shared: self.shared,
                })?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

impl<'de, T: Deserialize<'de>> serde::de::DeserializeSeed<'de> for Each<'_, T> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Each<'_, T> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a list")
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> std::result::Result<(), A::Error> {
        let mut at = 0u64;
        if let Some(from) = self.from.filter(|&from| from > 1) {
            while at + 1 < from {
                if seq.next_element::<serde::de::IgnoredAny>()?.is_none() {
                    return Ok(());
                }
                at += 1;
            }
        }
        let mut live = self.from.is_some() && self.shared.failed.borrow().is_none();
        while live {
            let Some(row) = seq.next_element::<T>()? else {
                return Ok(());
            };
            at += 1;
            match (self.f)(at, row) {
                Ok(true) => {}
                Ok(false) => live = false,
                Err(e) => {
                    *self.shared.failed.borrow_mut() = Some(e);
                    live = false;
                }
            }
        }
        let open = self.shared.open.get().saturating_sub(1);
        self.shared.open.set(open);
        if open == 0 {
            return Err(serde::de::Error::custom(ENOUGH));
        }
        while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
        Ok(())
    }
}

fn stream_old(
    path: &Path,
    from: Cursor,
    passage: &mut dyn FnMut(u64, Passage) -> Result<bool>,
    point: &mut dyn FnMut(u64, Point) -> Result<bool>,
) -> Result<()> {
    use serde::de::DeserializeSeed;
    let shared = Shared {
        failed: Failed::default(),
        open: std::cell::Cell::new(2),
    };
    let mut de =
        serde_json::Deserializer::from_reader(BufReader::new(crate::paths::open_regular(path)?));
    let read = Old {
        from,
        passage,
        point,
        shared: &shared,
    }
    .deserialize(&mut de);
    if let Some(e) = shared.failed.into_inner() {
        return Err(e);
    }
    match read {
        Err(e) if shared.open.get() == 0 && e.to_string().starts_with(ENOUGH) => Ok(()),
        read => {
            read?;
            de.end()?;
            Ok(())
        }
    }
}

struct Window<T> {
    rows: Vec<(u64, T)>,
    bytes: usize,
    next: Option<u64>,
}

impl<T: Serialize> Window<T> {
    fn new() -> Window<T> {
        Window {
            rows: Vec::new(),
            bytes: 0,
            next: None,
        }
    }

    fn offer(&mut self, seq: u64, row: T) -> Result<bool> {
        let size = serde_json::to_vec(&row)?.len();
        if !self.rows.is_empty() && self.bytes + size > PAGE_BYTES {
            self.next = Some(seq);
            return Ok(false);
        }
        self.bytes += size;
        self.rows.push((seq, row));
        Ok(true)
    }
}

pub fn page(notes: &Path, note: &str, id: &str, at: Cursor) -> Result<Page> {
    let head = head(notes, note, id)?;
    let mut passages = Window::new();
    let mut points = Window::new();
    rows_from(
        notes,
        note,
        &head,
        at,
        |seq, p| passages.offer(seq, p),
        |seq, p| points.offer(seq, p),
    )?;
    let next = Cursor {
        passages: passages.next,
        points: points.next,
    };
    Ok(Page {
        head,
        passages: passages.rows,
        points: points.rows,
        at,
        next: (next.passages.is_some() || next.points.is_some()).then_some(next),
    })
}

pub fn each_passage(
    notes: &Path,
    note: &str,
    mut f: impl FnMut(&Head, u64, &Passage) -> Result<()>,
) -> Result<()> {
    for head in list(notes, note)? {
        rows(
            notes,
            note,
            &head,
            |seq, p| f(&head, seq, &p).map(|()| true),
            |_, _| Ok(false),
        )?;
    }
    Ok(())
}

pub fn each_point(
    notes: &Path,
    note: &str,
    mut f: impl FnMut(&Head, u64, &Point) -> Result<()>,
) -> Result<()> {
    for head in list(notes, note)? {
        rows(
            notes,
            note,
            &head,
            |_, _| Ok(false),
            |seq, p| f(&head, seq, &p).map(|()| true),
        )?;
    }
    Ok(())
}

pub fn edit(notes: &Path, note: &str, id: &str, revision: u64, changes: &[Change]) -> Result<Head> {
    let dir = folder(notes, note)?;
    let old = head(notes, note, id)?;
    if old.revision != revision {
        return Err(Refused::Stale.into());
    }
    let wanted = |kind| -> std::collections::HashMap<u64, &str> {
        changes
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| (c.seq, c.text.as_str()))
            .collect()
    };
    let (mut said, mut typed) = (wanted(Kind::Passage), wanted(Kind::Point));
    let mut new = old.clone();
    new.revision = old.revision + 1;
    commit(&dir, Some(&old), &mut new, true, |pout, qout| {
        rows(
            notes,
            note,
            &old,
            |seq, mut p| {
                if let Some(text) = said.remove(&seq) {
                    p.text = text.to_string();
                }
                pout.passage(seq, &p).map(|()| true)
            },
            |seq, mut p| {
                if let Some(text) = typed.remove(&seq) {
                    p.text = text.to_string();
                }
                qout.row(seq, &p).map(|()| true)
            },
        )?;
        if !said.is_empty() || !typed.is_empty() {
            return Err(Refused::NoRow.into());
        }
        Ok(())
    })?;
    Ok(new)
}

fn key(at: u64, text: &str) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    (at, hasher.finish())
}

pub fn merge_into(notes: &Path, note: &str, leftover: &Archive) -> Result<()> {
    let dir = folder(notes, note)?;
    let old = match head_at(&dir.join(head_name(checked(&leftover.id)?))) {
        Ok(old) if old.id == leftover.id => old,
        _ => return save(notes, note, leftover),
    };
    let mut said = std::collections::HashSet::new();
    let mut typed = std::collections::HashSet::new();
    let (mut last_passage, mut last_point) = (0u64, 0u64);
    rows(
        notes,
        note,
        &old,
        |seq, p| {
            said.insert(key(p.start_secs, &p.text));
            last_passage = last_passage.max(seq);
            Ok(true)
        },
        |seq, p| {
            typed.insert(key(p.at_secs, &p.text));
            last_point = last_point.max(seq);
            Ok(true)
        },
    )?;
    let mut passages: Vec<&Passage> = leftover
        .passages
        .iter()
        .filter(|p| !said.contains(&key(p.start_secs, &p.text)))
        .collect();
    passages.sort_by_key(|p| p.start_secs);
    let mut points: Vec<&Point> = leftover
        .points
        .iter()
        .filter(|p| !typed.contains(&key(p.at_secs, &p.text)))
        .collect();
    points.sort_by_key(|p| p.at_secs);
    let mut new = old.clone();
    for w in &leftover.warnings {
        if !new.warnings.contains(w) {
            new.warnings.push(w.clone());
        }
    }
    for t in &leftover.trace {
        if !new.trace.contains(t) {
            new.trace.push(t.clone());
        }
    }
    new.trace.sort_by_key(|t| t.at);
    let kept = |new: &String, old: &String| if new.is_empty() { old } else { new }.clone();
    new.context = kept(&leftover.context, &old.context);
    new.wants = kept(&leftover.wants, &old.wants);
    new.from = kept(&old.from, &leftover.from);
    if passages.is_empty() && points.is_empty() && new == old {
        return Ok(());
    }
    new.revision = old.revision + 1;
    let backup = old.revision == 0;
    commit(&dir, Some(&old), &mut new, backup, |pout, qout| {
        let mut adding = passages.into_iter().peekable();
        let mut seq = last_passage;
        let mut put_passages = |pout: &mut Out, before: Option<u64>| -> Result<()> {
            while let Some(p) = adding.next_if(|p| before.is_none_or(|at| p.start_secs < at)) {
                seq += 1;
                pout.passage(seq, p)?;
            }
            Ok(())
        };
        let mut jotting = points.into_iter().peekable();
        let mut jot = last_point;
        let mut put_points = |qout: &mut Out, before: Option<u64>| -> Result<()> {
            while let Some(p) = jotting.next_if(|p| before.is_none_or(|at| p.at_secs < at)) {
                jot += 1;
                qout.row(jot, p)?;
            }
            Ok(())
        };
        rows(
            notes,
            note,
            &old,
            |s, p| {
                put_passages(pout, Some(p.start_secs))?;
                pout.passage(s, &p).map(|()| true)
            },
            |s, p| {
                put_points(qout, Some(p.at_secs))?;
                qout.row(s, &p).map(|()| true)
            },
        )?;
        put_passages(pout, None)?;
        put_points(qout, None)
    })
}

pub fn copy_to(notes: &Path, from: &str, into: &str, label: &str) -> Result<Vec<String>> {
    let have: std::collections::HashSet<String> =
        list(notes, into)?.into_iter().map(|s| s.id).collect();
    let (source, target) = (folder(notes, from)?, folder(notes, into)?);
    let mut copied = Vec::new();
    for head in list(notes, from)? {
        if have.contains(&head.id) {
            continue;
        }
        let mut copy = head.clone();
        if copy.from.is_empty() {
            copy.from = label.chars().take(200).collect();
        }
        copy.copied_from = from.to_string();
        if head.revision == 0 {
            copy.revision = 1;
            commit(&target, None, &mut copy, false, |pout, qout| {
                rows(
                    notes,
                    from,
                    &head,
                    |s, p| pout.passage(s, &p).map(|()| true),
                    |s, p| qout.row(s, &p).map(|()| true),
                )
            })?;
        } else {
            prepare(&target)?;
            for kind in [Kind::Passage, Kind::Point] {
                let name = lines_name(&head.id, head.revision, kind);
                remove_quietly(&target.join(&name));
                keep_copy(&source.join(&name), &target.join(&name))?;
            }
            write_json(&target.join(head_name(&head.id)), &copy)?;
        }
        copied.push(head.id);
    }
    Ok(copied)
}

pub fn remove(notes: &Path, note: &str, ids: &[String]) -> Result<()> {
    for id in ids {
        checked(id)?;
    }
    let dir = folder(notes, note)?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    let ids: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some((id, rest)) = name.split_once('.') else {
            continue;
        };
        let ours = rest == "json"
            || rest == "original"
            || lines_of(&name).is_some()
            || rest == "original.passages.jsonl"
            || rest == "original.points.jsonl";
        if ours && ids.contains(id) {
            match std::fs::remove_file(entry.path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    match std::fs::remove_dir(&dir) {
        Err(e)
            if !matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            crate::diag::warn(format!(
                "could not remove the empty recording folder {}: {e}",
                dir.display()
            ))
        }
        _ => {}
    }
    Ok(())
}

pub fn list(notes: &Path, note: &str) -> Result<Vec<Head>> {
    let dir = folder(notes, note)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut heads = Vec::new();
    let mut lines = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with(".tmp") {
            if entry
                .metadata()
                .is_ok_and(|meta| crate::paths::stale_temp(&name, &meta))
            {
                if let Err(error) = std::fs::remove_file(entry.path()) {
                    crate::diag::warn(format!(
                        "could not remove the old temporary file {}: {error}",
                        entry.path().display()
                    ));
                }
            }
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        if name.ends_with(".json") {
            match head_at(&entry.path()) {
                Ok(head) if valid_id(&head.id) => heads.push(head),
                _ => crate::diag::warn(format!(
                    "skipped an unreadable recording source: {}",
                    entry.path().display()
                )),
            }
        } else if let Some((id, revision)) = lines_of(&name) {
            lines.push((id.to_string(), revision, entry.path()));
        }
    }
    let named: std::collections::HashMap<&str, u64> =
        heads.iter().map(|h| (h.id.as_str(), h.revision)).collect();
    let mut whole = std::collections::HashMap::new();
    for (id, revision, path) in &lines {
        match named.get(id.as_str()) {
            Some(&current) if *revision == current => *whole.entry(id.as_str()).or_insert(0) += 1,
            Some(&current) if *revision < current => remove_quietly(path),
            _ if older_than_an_hour(path) => remove_quietly(path),
            _ => {}
        }
    }
    heads.retain(|h| {
        let kept = h.revision == 0 || whole.get(h.id.as_str()) == Some(&2);
        if !kept {
            crate::diag::warn(format!(
                "skipped a recording source whose lines are missing: {}",
                dir.join(head_name(&h.id)).display()
            ));
        }
        kept
    });
    heads.sort_by_key(|s| s.started);
    Ok(heads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_labels_are_off_by_default_and_leave_old_profiles_byte_identical() {
        let old = serde_json::to_string(&Profile {
            context: "c".into(),
            wants: "w".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(old, r#"{"context":"c","wants":"w"}"#);
        let on: Profile = serde_json::from_str(r#"{"speakers":true}"#).unwrap();
        assert!(on.speakers && on.clone().checked().speakers);
        assert!(!serde_json::from_str::<Profile>("{}").unwrap().speakers);
    }

    #[test]
    fn what_the_user_wants_is_trimmed_and_kept_whole() {
        let p = Profile {
            context: " Design review ".into(),
            wants: " Just the decisions ".into(),
            ..Default::default()
        }
        .checked();
        assert_eq!(
            (p.context.as_str(), p.wants.as_str()),
            ("Design review", "Just the decisions")
        );
        let long = Profile {
            context: format!(" {} ", "c".repeat(30_000)),
            wants: format!(" {} ", "w".repeat(30_000)),
            ..Default::default()
        }
        .checked();
        assert_eq!(
            (long.context.len(), long.wants.len()),
            (30_000, 30_000),
            "a long brief and long meeting details are kept whole"
        );
        let old: Profile = serde_json::from_str(
            r#"{"template":"meeting","vocabulary":["Dijkstra"],"context":"c"}"#,
        )
        .unwrap();
        assert_eq!(
            old.context, "c",
            "manifests written before formats were removed still load"
        );
    }

    fn merged(old: &Archive, new: &Archive) -> Archive {
        let said: std::collections::HashSet<(u64, &str)> = old
            .passages
            .iter()
            .map(|p| (p.start_secs, p.text.as_str()))
            .collect();
        let mut passages = old.passages.clone();
        passages.extend(
            new.passages
                .iter()
                .filter(|p| !said.contains(&(p.start_secs, p.text.as_str())))
                .cloned(),
        );
        passages.sort_by_key(|p| p.start_secs);
        let mut points = old.points.clone();
        points.extend(
            new.points
                .iter()
                .filter(|p| !old.points.contains(p))
                .cloned(),
        );
        points.sort_by_key(|p| p.at_secs);
        let mut warnings = old.warnings.clone();
        warnings.extend(
            new.warnings
                .iter()
                .filter(|w| !old.warnings.contains(w))
                .cloned(),
        );
        let mut trace = old.trace.clone();
        trace.extend(new.trace.iter().filter(|t| !old.trace.contains(t)).cloned());
        trace.sort_by_key(|t| t.at);
        let kept = |new: &String, old: &String| if new.is_empty() { old } else { new }.clone();
        Archive {
            id: new.id.clone(),
            started: old.started,
            passages,
            points,
            context: kept(&new.context, &old.context),
            wants: kept(&new.wants, &old.wants),
            warnings,
            trace,
            from: kept(&old.from, &new.from),
        }
    }

    fn whole(notes: &Path, note: &str) -> Vec<Archive> {
        list(notes, note)
            .unwrap()
            .into_iter()
            .map(|head| {
                let (mut passages, mut points) = (Vec::new(), Vec::new());
                rows(
                    notes,
                    note,
                    &head,
                    |_, p| {
                        passages.push(p);
                        Ok(true)
                    },
                    |_, p| {
                        points.push(p);
                        Ok(true)
                    },
                )
                .unwrap();
                Archive {
                    id: head.id,
                    started: head.started,
                    passages,
                    points,
                    context: head.context,
                    wants: head.wants,
                    warnings: head.warnings,
                    trace: head.trace,
                    from: head.from,
                }
            })
            .collect()
    }

    type Paged = (Vec<(u64, Passage)>, Vec<(u64, Point)>, usize);

    fn every_page(notes: &Path, note: &str, id: &str) -> Paged {
        let (mut passages, mut points, mut pages) = (Vec::new(), Vec::new(), 0);
        let mut at = Cursor::START;
        loop {
            let page = page(notes, note, id, at).unwrap();
            pages += 1;
            for (_, p) in &page.passages {
                assert!(
                    page.passages.len() == 1 || serde_json::to_vec(p).unwrap().len() <= PAGE_BYTES
                );
            }
            passages.extend(page.passages);
            points.extend(page.points);
            match page.next {
                Some(next) => at = next,
                None => return (passages, points, pages),
            }
            assert!(pages < 100_000, "the pages never ended");
        }
    }

    fn numbered<T: Clone>(rows: &[T]) -> Vec<(u64, T)> {
        rows.iter()
            .enumerate()
            .map(|(n, r)| (n as u64 + 1, r.clone()))
            .collect()
    }

    fn spoken(id: &str, said: &[(u64, &str, &str)]) -> Archive {
        Archive {
            id: id.into(),
            started: Utc::now(),
            passages: said
                .iter()
                .map(|(at, speaker, text)| Passage {
                    start_secs: *at,
                    end_secs: at + 5,
                    text: (*text).into(),
                    speaker: (*speaker).into(),
                })
                .collect(),
            points: vec![],
            context: String::new(),
            wants: String::new(),
            warnings: vec![],
            trace: vec![],
            from: String::new(),
        }
    }

    fn typed(at: u64, text: &str) -> Point {
        Point {
            at_secs: at,
            text: text.into(),
        }
    }

    fn old_file(notes: &Path, note: &str, id: &str, passages: u64, text: &str) {
        let dir = folder(notes, note).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut out =
            std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{id}.json"))).unwrap());
        write!(
            out,
            "{{\"id\":\"{id}\",\"started\":\"2026-01-02T03:04:05Z\",\"passages\":["
        )
        .unwrap();
        for n in 0..passages {
            if n > 0 {
                out.write_all(b",").unwrap();
            }
            let p = Passage {
                start_secs: n,
                end_secs: n + 1,
                text: format!("{n} {text}"),
                speaker: if n % 2 == 0 { "A" } else { "B" }.into(),
            };
            serde_json::to_writer(&mut out, &p).unwrap();
        }
        out.write_all(b"],\"points\":[{\"at_secs\":3,\"text\":\"typed\"}],\"wants\":\"exam\"}")
            .unwrap();
        out.flush().unwrap();
    }

    fn old_layout(notes: &Path, note: &str, source: &Archive) {
        let dir = folder(notes, note).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{}.json", source.id)),
            serde_json::to_vec_pretty(source).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn sources_are_private_durable_and_repeatable() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let source = spoken("session-123", &[]);
        save(&notes, "note-1234", &source).unwrap();
        save(&notes, "note-1234", &source).unwrap();
        assert_eq!(list(&notes, "note-1234").unwrap().len(), 1);
        assert_eq!(
            std::fs::read_dir(folder(&notes, "note-1234").unwrap())
                .unwrap()
                .count(),
            3,
            "saving again leaves one header and one pair"
        );
        assert!(save(&notes, "../../outside", &source).is_err());
        let later = Archive {
            id: "session-456".into(),
            started: Utc::now(),
            ..source.clone()
        };
        save(&notes, "note-5678", &later).unwrap();
        assert_eq!(
            copy_to(&notes, "note-5678", "note-1234", "Lecture 2").unwrap(),
            ["session-456"]
        );
        assert!(copy_to(&notes, "note-5678", "note-1234", "Lecture 2")
            .unwrap()
            .is_empty());
        let both = list(&notes, "note-1234").unwrap();
        assert_eq!(
            both.iter()
                .map(|s| (s.id.as_str(), s.from.as_str()))
                .collect::<Vec<_>>(),
            [("session-123", ""), ("session-456", "Lecture 2")]
        );
        assert_eq!(
            list(&notes, "note-5678").unwrap().len(),
            1,
            "the original keeps its own copy"
        );
        remove(&notes, "note-1234", &["session-456".into()]).unwrap();
        remove(&notes, "note-1234", &["session-456".into()]).unwrap();
        assert_eq!(list(&notes, "note-1234").unwrap().len(), 1);
        assert!(remove(&notes, "note-1234", &["../x".into()]).is_err());
    }

    #[test]
    fn a_damaged_source_file_is_skipped_and_the_others_load() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(&notes, "note-0001", &spoken("good-0001", &[])).unwrap();
        let broken = folder(&notes, "note-0001")
            .unwrap()
            .join("broken-0001.json");
        std::fs::write(&broken, "{").unwrap();
        let loaded = list(&notes, "note-0001").unwrap();
        assert_eq!(
            loaded.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["good-0001"]
        );
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{");
    }

    #[test]
    fn stray_files_among_the_sources_are_left_out_and_old_temp_files_removed() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        for id in ["one-0001", "two-0001", "three-0001"] {
            save(&notes, "note-0001", &spoken(id, &[])).unwrap();
        }
        let dir = folder(&notes, "note-0001").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let stale = [dir.join(".tmpAbC123"), dir.join(".tmpXyZ789")];
        for path in &stale {
            std::fs::write(path, "{\"half\":").unwrap();
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        let kept = [
            dir.join(".tmpQrS456"),
            dir.join("one-0001.original"),
            dir.join("notes.txt"),
        ];
        for path in &kept {
            std::fs::write(path, "stray").unwrap();
        }
        assert_eq!(list(&notes, "note-0001").unwrap().len(), 3);
        for path in &stale {
            assert!(!path.exists(), "{} survived", path.display());
        }
        for path in &kept {
            assert!(path.exists(), "{} was removed", path.display());
        }
    }

    #[test]
    fn merged_keeps_the_other_side_points_and_context() {
        let mut you = spoken("call-0001", &[(0, "You", "my side"), (20, "You", "more")]);
        you.points = vec![typed(3, "ask about the budget")];
        you.context = "Weekly sync".into();
        you.warnings = vec!["Others: No sound was recorded.".into()];
        let others = spoken("call-0001", &[(10, "Others", "the other side")]);
        let both = merged(&you, &others);
        assert_eq!(
            both.passages
                .iter()
                .map(|p| (p.start_secs, p.speaker.as_str()))
                .collect::<Vec<_>>(),
            [(0, "You"), (10, "Others"), (20, "You")]
        );
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(&notes, "note-0001", &you).unwrap();
        merge_into(&notes, "note-0001", &others).unwrap();
        let revision = list(&notes, "note-0001").unwrap()[0].revision;
        merge_into(&notes, "note-0001", &others).unwrap();
        assert_eq!(
            list(&notes, "note-0001").unwrap()[0].revision,
            revision,
            "the same recovery twice writes nothing"
        );
        assert_eq!(
            serde_json::to_value(&whole(&notes, "note-0001")[0]).unwrap(),
            serde_json::to_value(&both).unwrap()
        );
        merge_into(&notes, "note-0002", &others).unwrap();
        assert_eq!(whole(&notes, "note-0002")[0].passages, others.passages);
    }

    #[test]
    fn the_same_words_under_two_labels_are_kept_once_with_the_stored_label() {
        let labelled = spoken(
            "solo-0001",
            &[(0, "Speaker 1", "hello"), (5, "Speaker 2", "hi there")],
        );
        let plain = spoken(
            "solo-0001",
            &[(0, "Microphone", "hello"), (5, "Microphone", "hi there")],
        );
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(&notes, "note-0001", &labelled).unwrap();
        merge_into(&notes, "note-0001", &plain).unwrap();
        assert_eq!(whole(&notes, "note-0001")[0].passages, labelled.passages);
    }

    #[test]
    fn merge_into_matches_merged() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut old = spoken(
            "call-0002",
            &[
                (0, "You", "a"),
                (10, "You", "b"),
                (10, "You", "c"),
                (30, "You", "d"),
            ],
        );
        old.points = vec![typed(1, "x"), typed(12, "y")];
        old.trace = vec![Trace {
            at: Utc::now(),
            stage: "saved".into(),
            detail: "one".into(),
        }];
        old.from = "Lecture".into();
        let mut new = spoken(
            "call-0002",
            &[
                (5, "Others", "e"),
                (10, "Others", "b"),
                (10, "Others", "f"),
                (40, "Others", "g"),
            ],
        );
        new.points = vec![typed(12, "y"), typed(0, "z"), typed(20, "w")];
        new.warnings = vec!["Others: quiet".into()];
        new.wants = "Decisions".into();
        for layout in ["new", "old"] {
            let note = format!("note-{layout}0");
            if layout == "new" {
                save(&notes, &note, &old).unwrap();
            } else {
                old_layout(&notes, &note, &old);
            }
            merge_into(&notes, &note, &new).unwrap();
            assert_eq!(
                serde_json::to_value(&whole(&notes, &note)[0]).unwrap(),
                serde_json::to_value(merged(&old, &new)).unwrap(),
                "{layout}"
            );
            let head = &list(&notes, &note).unwrap()[0];
            assert_eq!((head.passages, head.points), (7, 4));
        }
        let dir = folder(&notes, "note-old0").unwrap();
        assert_eq!(
            serde_json::from_slice::<Archive>(
                &std::fs::read(dir.join("call-0002.original")).unwrap()
            )
            .unwrap()
            .passages,
            old.passages,
            "converting an old file keeps it as the backup"
        );
    }

    #[test]
    fn copy_to_marks_copies_and_uncombine_removes_only_them() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(
            &notes,
            "note-into",
            &spoken("mine-0001", &[(0, "", "kept")]),
        )
        .unwrap();
        save(
            &notes,
            "note-from",
            &spoken("theirs-001", &[(0, "", "copied")]),
        )
        .unwrap();
        old_layout(
            &notes,
            "note-from",
            &spoken("theirs-002", &[(1, "", "old one")]),
        );
        assert_eq!(
            copy_to(&notes, "note-from", "note-into", "Other")
                .unwrap()
                .len(),
            2
        );
        let copies: Vec<String> = list(&notes, "note-into")
            .unwrap()
            .into_iter()
            .filter(|h| h.copied_from == "note-from")
            .map(|h| h.id)
            .collect();
        assert_eq!(copies.len(), 2);
        assert_eq!(whole(&notes, "note-into").len(), 3);
        edit(
            &notes,
            "note-into",
            "theirs-001",
            1,
            &[Change {
                kind: Kind::Passage,
                seq: 1,
                text: "fixed in the copy".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            whole(&notes, "note-from")[0].passages[0].text,
            "copied",
            "a fix to a copy leaves the other note's file alone"
        );
        remove(&notes, "note-into", &copies).unwrap();
        assert_eq!(
            list(&notes, "note-into")
                .unwrap()
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            ["mine-0001"]
        );
        assert_eq!(
            std::fs::read_dir(folder(&notes, "note-into").unwrap())
                .unwrap()
                .count(),
            3
        );
        assert_eq!(whole(&notes, "note-from").len(), 2);
    }

    #[test]
    fn removing_a_transcript_removes_its_saved_original_and_the_empty_folder() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(
            &notes,
            "note-gone",
            &spoken("rec-gone01", &[(0, "", "said")]),
        )
        .unwrap();
        edit(
            &notes,
            "note-gone",
            "rec-gone01",
            1,
            &[Change {
                kind: Kind::Passage,
                seq: 1,
                text: "fixed".into(),
            }],
        )
        .unwrap();
        let dir = folder(&notes, "note-gone").unwrap();
        std::fs::write(dir.join("rec-gone01.original"), "the first wording").unwrap();
        remove(&notes, "note-gone", &["rec-gone01".to_string()]).unwrap();
        assert!(!dir.exists());
        assert!(root(&notes).is_dir());
    }

    #[test]
    fn a_folder_with_other_transcripts_stays() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(
            &notes,
            "note-kept",
            &spoken("rec-kept01", &[(0, "", "one")]),
        )
        .unwrap();
        save(
            &notes,
            "note-kept",
            &spoken("rec-kept02", &[(0, "", "two")]),
        )
        .unwrap();
        let dir = folder(&notes, "note-kept").unwrap();
        std::fs::write(dir.join("rec-kept01.original"), "the first wording").unwrap();
        remove(&notes, "note-kept", &["rec-kept01".to_string()]).unwrap();
        assert!(!dir.join("rec-kept01.original").exists());
        assert_eq!(
            list(&notes, "note-kept")
                .unwrap()
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            ["rec-kept02"]
        );
        assert_eq!(whole(&notes, "note-kept")[0].passages[0].text, "two");
    }

    #[cfg(unix)]
    #[test]
    fn recording_roots_and_writers_reject_user_controlled_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root(&notes)).unwrap();
        assert!(folder(&notes, "note-1234").is_err());
        assert!(write_json(&root(&notes).join("test.json"), &serde_json::json!({})).is_err());
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn a_note_with_1001_recording_files_loads() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        for n in 0..1001 {
            let mut source = spoken(&format!("rec-{n:05}"), &[(0, "", "hello")]);
            source.started = Utc::now() + chrono::Duration::seconds(n);
            if n % 100 == 0 {
                save(&notes, "note-0001", &source).unwrap();
            } else {
                old_layout(&notes, "note-0001", &source);
            }
        }
        let heads = list(&notes, "note-0001").unwrap();
        assert_eq!(heads.len(), 1001);
        assert_eq!(
            page(&notes, "note-0001", &heads[1000].id, Cursor::START)
                .unwrap()
                .passages
                .len(),
            1
        );
    }

    #[test]
    fn a_note_whose_recordings_pass_64_mb_loads() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let text = "word ".repeat(20_000);
        for n in 0..3 {
            let said: Vec<(u64, &str, &str)> = (0..220).map(|i| (i, "", text.as_str())).collect();
            save(&notes, "note-0001", &spoken(&format!("big-{n:05}"), &said)).unwrap();
        }
        let heads = list(&notes, "note-0001").unwrap();
        assert_eq!(heads.len(), 3);
        let (passages, _, pages) = every_page(&notes, "note-0001", "big-00002");
        assert_eq!(passages.len(), 220);
        assert!(pages >= 40, "{pages}");
    }

    #[test]
    fn a_crash_at_any_step_of_a_change_leaves_the_old_or_the_new_recording_whole() {
        let mut first = spoken("rec-crash1", &[(0, "", "one"), (5, "", "two")]);
        first.points = vec![typed(1, "p")];
        let mut second = spoken("rec-crash1", &[(0, "", "uno")]);
        second.points = vec![typed(2, "q"), typed(3, "r")];
        let read = |notes: &Path| {
            let head = &list(notes, "note-0001").unwrap()[0];
            let (passages, points, _) = every_page(notes, "note-0001", "rec-crash1");
            (
                head.revision,
                passages
                    .into_iter()
                    .map(|(_, p)| p.text)
                    .collect::<Vec<_>>(),
                points.into_iter().map(|(_, p)| p.text).collect::<Vec<_>>(),
            )
        };
        let fix = [Change {
            kind: Kind::Passage,
            seq: 2,
            text: "TWO".into(),
        }];
        let more = spoken("rec-crash1", &[(9, "", "three")]);
        type Step = fn(&Path, &[Change], &Archive, &Archive) -> Result<()>;
        let changes: [(&str, bool, Step); 5] = [
            ("save over", false, |n, _, _, s| save(n, "note-0001", s)),
            ("edit", false, |n, f, _, _| {
                edit(n, "note-0001", "rec-crash1", 1, f).map(|_| ())
            }),
            ("merge", false, |n, _, m, _| merge_into(n, "note-0001", m)),
            ("convert by edit", true, |n, f, _, _| {
                edit(n, "note-0001", "rec-crash1", 0, f).map(|_| ())
            }),
            ("convert by merge", true, |n, _, m, _| {
                merge_into(n, "note-0001", m)
            }),
        ];
        for (what, old, change) in changes {
            for step in ["pair", "backup", "header"] {
                let temp = tempfile::tempdir().unwrap();
                let notes = temp.path().join("notes");
                if old {
                    old_layout(&notes, "note-0001", &first);
                } else {
                    save(&notes, "note-0001", &first).unwrap();
                }
                let before = read(&notes);
                let temp2 = tempfile::tempdir().unwrap();
                let clean = temp2.path().join("notes");
                if old {
                    old_layout(&clean, "note-0001", &first);
                } else {
                    save(&clean, "note-0001", &first).unwrap();
                }
                change(&clean, &fix, &more, &second).unwrap();
                let after_change = read(&clean);
                STOP_AFTER.with(|s| s.set(step));
                let result = change(&notes, &fix, &more, &second);
                STOP_AFTER.with(|s| s.set(""));
                let now = read(&notes);
                assert!(
                    now == before || now == after_change,
                    "{what} stopped after {step}: {now:?}"
                );
                if result.is_ok() {
                    assert_eq!(now, after_change, "{what} after {step}");
                }
                if step == "header" && result.is_err() {
                    assert_eq!(now, after_change, "{what}: the header is the commit");
                }
            }
        }
    }

    #[test]
    fn a_recording_with_only_typed_points_pages_through_all_of_them() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut source = spoken("rec-points", &[]);
        source.points = (0..3000).map(|n| typed(n, &"note ".repeat(100))).collect();
        save(&notes, "note-0001", &source).unwrap();
        let (passages, points, pages) = every_page(&notes, "note-0001", "rec-points");
        assert!(passages.is_empty());
        assert_eq!(points, numbered(&source.points));
        assert!(pages > 1);
    }

    #[test]
    fn a_dense_run_of_points_between_two_passages_pages_forward() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut source = spoken("rec-dense1", &[(0, "", "before"), (9000, "", "after")]);
        source.points = (1..5000).map(|n| typed(n, &"x".repeat(300))).collect();
        save(&notes, "note-0001", &source).unwrap();
        let first = page(&notes, "note-0001", "rec-dense1", Cursor::START).unwrap();
        assert_eq!(
            first.passages.len(),
            2,
            "the passages are not held back by the points"
        );
        let next = first.next.unwrap();
        assert_eq!(next.passages, None);
        let (passages, points, _) = every_page(&notes, "note-0001", "rec-dense1");
        assert_eq!(passages, numbered(&source.passages));
        assert_eq!(points, numbered(&source.points));
    }

    #[test]
    fn one_heavily_escaped_large_point_is_reachable_and_editable() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut source = spoken("rec-escape", &[(0, "", "hi")]);
        let large = "\u{1}\"".repeat(100_000);
        source.points = vec![typed(1, "small"), typed(2, &large), typed(3, "after")];
        save(&notes, "note-0001", &source).unwrap();
        let (_, points, pages) = every_page(&notes, "note-0001", "rec-escape");
        assert_eq!(points, numbered(&source.points));
        assert!(pages >= 2);
        let head = edit(
            &notes,
            "note-0001",
            "rec-escape",
            1,
            &[Change {
                kind: Kind::Point,
                seq: 2,
                text: format!("{large}!"),
            }],
        )
        .unwrap();
        assert_eq!(head.revision, 2);
        assert_eq!(
            whole(&notes, "note-0001")[0].points[1].text,
            format!("{large}!")
        );
    }

    #[test]
    fn a_merge_that_inserts_rows_moves_no_cursor_or_fix() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let text = "w".repeat(200_000);
        let said: Vec<(u64, &str, &str)> = (0..6).map(|n| (n * 10, "You", text.as_str())).collect();
        save(&notes, "note-0001", &spoken("call-0003", &said)).unwrap();
        let first = page(&notes, "note-0001", "call-0003", Cursor::START).unwrap();
        let cursor = first.next.unwrap();
        let named = cursor.passages.unwrap();
        let at = first.passages.len() as u64;
        merge_into(
            &notes,
            "note-0001",
            &spoken(
                "call-0003",
                &[(1, "Others", "early"), (45, "Others", "late")],
            ),
        )
        .unwrap();
        let again = page(&notes, "note-0001", "call-0003", cursor).unwrap();
        assert_eq!(again.passages[0].0, named);
        assert_eq!(again.passages[0].1.start_secs, at * 10);
        let revision = list(&notes, "note-0001").unwrap()[0].revision;
        edit(
            &notes,
            "note-0001",
            "call-0003",
            revision,
            &[Change {
                kind: Kind::Passage,
                seq: named,
                text: "fixed".into(),
            }],
        )
        .unwrap();
        let all = whole(&notes, "note-0001");
        let fixed = all[0].passages.iter().find(|p| p.text == "fixed").unwrap();
        assert_eq!(fixed.start_secs, at * 10);
        assert_eq!(all[0].passages.len(), 8);
    }

    #[test]
    fn an_old_recording_file_reads_in_pages_without_holding_it() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        old_file(
            &notes,
            "note-0001",
            "rec-old001",
            3000,
            &"word ".repeat(200),
        );
        let heads = list(&notes, "note-0001").unwrap();
        assert_eq!(
            (heads[0].passages, heads[0].points, heads[0].revision),
            (3000, 1, 0)
        );
        assert_eq!(heads[0].speakers, ["A", "B"]);
        assert_eq!(heads[0].wants, "exam");
        let (passages, points, pages) = every_page(&notes, "note-0001", "rec-old001");
        assert_eq!(passages.len(), 3000);
        assert_eq!(passages[2999].0, 3000);
        assert_eq!(points.len(), 1);
        assert!(pages > 4);
        assert_eq!(
            std::fs::read_dir(folder(&notes, "note-0001").unwrap())
                .unwrap()
                .count(),
            1,
            "reading an old file writes nothing"
        );
    }

    #[test]
    fn the_first_fix_rewrites_an_old_recording_in_the_new_layout_and_keeps_a_backup() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut source = spoken("rec-old002", &[(0, "A", "helo"), (5, "B", "world")]);
        source.points = vec![typed(2, "typed")];
        old_layout(&notes, "note-0001", &source);
        let before =
            std::fs::read(folder(&notes, "note-0001").unwrap().join("rec-old002.json")).unwrap();
        let head = edit(
            &notes,
            "note-0001",
            "rec-old002",
            0,
            &[Change {
                kind: Kind::Passage,
                seq: 1,
                text: "hello".into(),
            }],
        )
        .unwrap();
        assert_eq!((head.revision, head.passages, head.points), (1, 2, 1));
        let dir = folder(&notes, "note-0001").unwrap();
        assert_eq!(
            std::fs::read(dir.join("rec-old002.original")).unwrap(),
            before
        );
        assert!(dir.join("rec-old002.1.passages.jsonl").is_file());
        assert_eq!(whole(&notes, "note-0001")[0].passages[0].text, "hello");
        edit(
            &notes,
            "note-0001",
            "rec-old002",
            1,
            &[Change {
                kind: Kind::Point,
                seq: 1,
                text: "again".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            std::fs::read(dir.join("rec-old002.original")).unwrap(),
            before
        );
        assert!(!dir.join("rec-old002.original.passages.jsonl").exists());
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "rec-old002.2.passages.jsonl",
                "rec-old002.2.points.jsonl",
                "rec-old002.json",
                "rec-old002.original"
            ]
        );
        save(
            &notes,
            "note-0001",
            &spoken("rec-new003", &[(0, "", "one")]),
        )
        .unwrap();
        edit(
            &notes,
            "note-0001",
            "rec-new003",
            1,
            &[Change {
                kind: Kind::Passage,
                seq: 1,
                text: "uno".into(),
            }],
        )
        .unwrap();
        assert!(
            std::fs::read_to_string(dir.join("rec-new003.original.passages.jsonl"))
                .unwrap()
                .contains("\"one\"")
        );
    }

    #[test]
    fn converting_an_old_recording_fixed_before_keeps_its_first_backup_and_every_row() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut source = spoken("rec-old004", &[(0, "", "fixed once"), (5, "", "two")]);
        source.points = vec![typed(1, "p")];
        old_layout(&notes, "note-0001", &source);
        let dir = folder(&notes, "note-0001").unwrap();
        std::fs::write(dir.join("rec-old004.original"), "the first wording").unwrap();
        edit(
            &notes,
            "note-0001",
            "rec-old004",
            0,
            &[Change {
                kind: Kind::Passage,
                seq: 2,
                text: "TWO".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("rec-old004.original")).unwrap(),
            "the first wording"
        );
        let kept = &whole(&notes, "note-0001")[0];
        assert_eq!(
            kept.passages
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>(),
            ["fixed once", "TWO"]
        );
        assert_eq!(kept.points, source.points);
    }

    #[test]
    fn a_stale_revision_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(
            &notes,
            "note-0001",
            &spoken("rec-stale1", &[(0, "", "one")]),
        )
        .unwrap();
        let fix = |revision, seq| {
            edit(
                &notes,
                "note-0001",
                "rec-stale1",
                revision,
                &[Change {
                    kind: Kind::Passage,
                    seq,
                    text: "two".into(),
                }],
            )
        };
        assert_eq!(
            fix(0, 1).unwrap_err().downcast::<Refused>().unwrap(),
            Refused::Stale
        );
        assert_eq!(
            fix(1, 9).unwrap_err().downcast::<Refused>().unwrap(),
            Refused::NoRow
        );
        assert_eq!(list(&notes, "note-0001").unwrap()[0].revision, 1);
        assert_eq!(fix(1, 1).unwrap().revision, 2);
        assert_eq!(
            fix(1, 1).unwrap_err().downcast::<Refused>().unwrap(),
            Refused::Stale
        );
    }

    #[test]
    fn pages_cover_every_passage_exactly_once() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let mut seed = 7u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed >> 33
        };
        let mut source = spoken("rec-random", &[]);
        for n in 0..1000 {
            let size = match next() % 100 {
                0 => 520_000,
                1..=30 => 4_000,
                _ => (next() % 400) as usize,
            };
            source.passages.push(Passage {
                start_secs: n,
                end_secs: n + 1,
                text: "é".repeat(size / 2),
                speaker: String::new(),
            });
            if next() % 3 == 0 {
                source
                    .points
                    .push(typed(n, &"p".repeat((next() % 20_000) as usize)));
            }
        }
        save(&notes, "note-0001", &source).unwrap();
        old_layout(&notes, "note-0002", &source);
        for note in ["note-0001", "note-0002"] {
            let (passages, points, _) = every_page(&notes, note, "rec-random");
            assert_eq!(passages, numbered(&source.passages), "{note}");
            assert_eq!(points, numbered(&source.points), "{note}");
        }
    }

    #[test]
    fn lines_files_without_a_header_are_ignored_and_tidied() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        save(
            &notes,
            "note-0001",
            &spoken("rec-tidy01", &[(0, "", "one")]),
        )
        .unwrap();
        let dir = folder(&notes, "note-0001").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let age = |path: &Path| {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap()
        };
        let orphan_old = dir.join("rec-gone01.1.passages.jsonl");
        let orphan_new = dir.join("rec-gone02.1.points.jsonl");
        let older = dir.join("rec-tidy01.0.points.jsonl");
        let newer_old = dir.join("rec-tidy01.2.passages.jsonl");
        let newer_fresh = dir.join("rec-tidy01.3.passages.jsonl");
        let backup = dir.join("rec-tidy01.original.passages.jsonl");
        for path in [
            &orphan_old,
            &orphan_new,
            &older,
            &newer_old,
            &newer_fresh,
            &backup,
        ] {
            std::fs::write(path, "[1,{\"at_secs\":0,\"text\":\"x\"}]\n").unwrap();
        }
        age(&orphan_old);
        age(&newer_old);
        let heads = list(&notes, "note-0001").unwrap();
        assert_eq!(heads.len(), 1);
        assert_eq!(whole(&notes, "note-0001")[0].passages[0].text, "one");
        for gone in [&orphan_old, &older, &newer_old] {
            assert!(!gone.exists(), "{}", gone.display());
        }
        for kept in [&orphan_new, &newer_fresh, &backup] {
            assert!(kept.exists(), "{}", kept.display());
        }
        std::fs::remove_file(dir.join("rec-tidy01.1.points.jsonl")).unwrap();
        assert!(
            list(&notes, "note-0001").unwrap().is_empty(),
            "a header without its lines is skipped"
        );
    }

    #[test]
    #[ignore]
    fn p_c57_benchmark_recording_sources() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let case = std::env::var("P_C57_CASE").unwrap_or_default();
        let text = "a".repeat(1900);
        let small = |count: i64| {
            save(
                &notes,
                "note-many",
                &spoken("rec-00000", &[(0, "", text.as_str())]),
            )
            .unwrap();
            let dir = folder(&notes, "note-many").unwrap();
            let mut head = head(&notes, "note-many", "rec-00000").unwrap();
            for n in 1..count {
                head.id = format!("rec-{n:05}");
                head.started = Utc::now() + chrono::Duration::seconds(n);
                std::fs::write(
                    dir.join(head_name(&head.id)),
                    serde_json::to_vec_pretty(&head).unwrap(),
                )
                .unwrap();
                for kind in [Kind::Passage, Kind::Point] {
                    std::fs::copy(
                        dir.join(lines_name("rec-00000", 1, kind)),
                        dir.join(lines_name(&head.id, 1, kind)),
                    )
                    .unwrap();
                }
            }
        };
        let first_answer = |note: &str, query: &str| {
            let at = std::time::Instant::now();
            let heads = list(&notes, note).unwrap();
            let shown = page(&notes, note, &heads[0].id, Cursor::START).unwrap();
            let listed = at.elapsed();
            let mut found = 0;
            each_passage(&notes, note, |_, _, p| {
                found += usize::from(p.text.contains(query));
                Ok(())
            })
            .unwrap();
            eprintln!(
                "{note}: {} recordings, first page {} rows: list+page {listed:?}, search {found} in {:?}",
                heads.len(),
                shown.passages.len(),
                at.elapsed() - listed
            );
            heads
        };
        let fix = |note: &str, head: &Head| {
            let at = std::time::Instant::now();
            edit(
                &notes,
                note,
                &head.id,
                head.revision,
                &[Change {
                    kind: Kind::Passage,
                    seq: 2,
                    text: "fixed".into(),
                }],
            )
            .unwrap();
            eprintln!("{note}: fix of one passage {:?}", at.elapsed());
        };
        let at = std::time::Instant::now();
        match case.as_str() {
            "1000" | "5000" => {
                small(case.parse().unwrap());
                first_answer("note-many", "zzz");
            }
            "old200" => {
                old_file(
                    &notes,
                    "note-big0",
                    "huge-0001",
                    200_000,
                    &"word ".repeat(200),
                );
                let heads = first_answer("note-big0", "199999 ");
                let at = std::time::Instant::now();
                let second = page(&notes, "note-big0", "huge-0001", Cursor::START).unwrap();
                eprintln!(
                    "old 200 MB: a page again {:?} ({} rows)",
                    at.elapsed(),
                    second.passages.len()
                );
                fix("note-big0", &heads[0]);
                let at = std::time::Instant::now();
                page(&notes, "note-big0", "huge-0001", Cursor::START).unwrap();
                eprintln!("old 200 MB after its fix: first page {:?}", at.elapsed());
            }
            "new200" => {
                let big = "word ".repeat(200);
                let mut base = Head::of(&spoken("huge-0002", &[]));
                base.wants = "exam".into();
                save_from(
                    &notes,
                    "note-big1",
                    &base,
                    (0..200_000u64).map(|n| Passage {
                        start_secs: n,
                        end_secs: n + 1,
                        text: format!("{n} {big}"),
                        speaker: String::new(),
                    }),
                    std::iter::once(typed(3, "typed")),
                )
                .unwrap();
                let heads = first_answer("note-big1", "199999 ");
                fix("note-big1", &heads[0]);
            }
            "50x4" => {
                let big = "word ".repeat(200);
                for n in 0..50u64 {
                    let base = Head::of(&spoken(&format!("part-{n:05}"), &[]));
                    save_from(
                        &notes,
                        "note-big2",
                        &base,
                        (0..4_000u64).map(|i| Passage {
                            start_secs: i,
                            end_secs: i + 1,
                            text: format!("{i} {big}"),
                            speaker: String::new(),
                        }),
                        std::iter::empty(),
                    )
                    .unwrap();
                }
                let heads = first_answer("note-big2", "3999 ");
                fix("note-big2", &heads[0]);
            }
            _ => panic!("set P_C57_CASE"),
        }
        eprintln!("total {:?}", at.elapsed());
    }
}
