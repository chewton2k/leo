pub mod awake;
mod call_watch;
pub mod capture;
pub mod mic;
pub mod recorder;
pub mod speakers;
#[cfg(test)]
mod stress;
pub mod transcriber;
pub mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ai::chat::{clock, Jotted};
use leo_core::paths::write_atomic;

pub const SEGMENT_SECS: u64 = 300;
pub const OVERLAP_SECS: u64 = 1;
const LOCK_FRESH: Duration = Duration::from_secs(30);
const HEARTBEAT: Duration = Duration::from_secs(5);
const MOST_SAME_SECOND: usize = 1000;
const RECHECK: Duration = Duration::from_secs(60);
const RACY: Duration = Duration::from_secs(2);
pub const MOST_SESSION_BYTES: u64 = 8_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub at_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub profile: leo_core::recording::Profile,
    pub started: DateTime<Utc>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub append_to: Option<String>,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub screen: bool,
    pub segment_secs: u64,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub points: Vec<Point>,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub turns: bool,
    #[serde(default)]
    pub note: Option<String>,
}

impl Manifest {
    pub fn new(
        title: Option<String>,
        append_to: Option<String>,
        dir: &str,
        screen: bool,
    ) -> Manifest {
        Manifest {
            id: uuid::Uuid::new_v4().to_string(),
            profile: Default::default(),
            started: Utc::now(),
            title,
            append_to,
            dir: dir.to_string(),
            screen,
            segment_secs: SEGMENT_SECS,
            duration_secs: None,
            points: Vec::new(),
            stopped: false,
            saved: false,
            turns: false,
            note: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentState {
    Recording,
    Waiting,
    Done(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub index: u32,
    pub state: SegmentState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub index: u32,
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Assembled {
    pub parts: Vec<Part>,
    pub failed: Vec<(u64, u64)>,
    pub pending: usize,
}

impl Assembled {
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            out = crate::ai::live::stitch(&out, &part.text);
        }
        out
    }

    pub fn is_silent(&self) -> bool {
        self.failed.is_empty() && self.parts.iter().all(|p| p.text.trim().is_empty())
    }
}

pub fn assemble(segments: &[Segment], segment_secs: u64) -> Assembled {
    let mut out = Assembled::default();
    for seg in segments {
        let start = seg.index as u64 * segment_secs;
        let end = start + segment_secs;
        match &seg.state {
            SegmentState::Done(text) => out.parts.push(Part {
                index: seg.index,
                start_secs: start,
                end_secs: end,
                text: text.clone(),
            }),
            SegmentState::Failed(_) => out.failed.push((start, end)),
            SegmentState::Recording | SegmentState::Waiting => out.pending += 1,
        }
    }
    out
}

pub fn failure_notice(failed: &[(u64, u64)], dir: &Path) -> String {
    if failed.is_empty() {
        return String::new();
    }
    let spans: Vec<String> = failed
        .iter()
        .map(|(a, b)| format!("{}–{}", clock(*a), clock(*b)))
        .collect();
    format!(
        "> Parts of this recording could not be transcribed ({}), even after retrying. \
         Their audio is kept in `{}`.",
        spans.join(", "),
        dir.display()
    )
}

pub struct Session {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub struct Lock {
    path: PathBuf,
    alive: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

const MOST_NAMES_BYTES: u64 = 64 << 20;

pub(crate) fn trace_to(dir: &Path, stage: &str, detail: &str) {
    use std::io::Write;
    let row = leo_core::recording::Trace {
        at: Utc::now(),
        stage: stage.into(),
        detail: detail.into(),
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("trace.jsonl"))
    {
        if let Ok(text) = serde_json::to_string(&row) {
            let _ = writeln!(file, "{text}");
        }
    }
}

type Turns = Vec<(u64, Option<u64>, String)>;

#[derive(Deserialize)]
#[serde(untagged)]
enum TurnRow {
    Spoken(u64, u64, String),
    Started(u64, String),
}

pub fn turns_name(index: u32) -> String {
    format!("seg-{index:05}.turns.json")
}

fn index_of(name: &str) -> Option<(u32, &str)> {
    let rest = name.strip_prefix("seg-")?;
    let (digits, ext) = rest.split_once('.')?;
    Some((digits.parse().ok()?, ext))
}

pub fn root() -> Result<PathBuf> {
    leo_core::paths::contained_path(&leo_core::paths::data_dir()?, Path::new("recordings"))
}

fn process_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    if cfg!(unix) {
        leo_core::children::quiet_command("kill")
            .args(["-0", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else if cfg!(windows) {
        leo_core::children::quiet_command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
            .unwrap_or(true)
    } else {
        true
    }
}

pub fn locked(dir: &Path) -> bool {
    use std::io::Read;
    let path = dir.join("lock");
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        _ => return true,
    };
    if meta.len() > 64 {
        return true;
    }
    if !meta
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < LOCK_FRESH)
    {
        return false;
    }
    let mut text = String::new();
    match leo_core::paths::open_regular(&path)
        .and_then(|file| file.take(64).read_to_string(&mut text))
        .ok()
        .and_then(|_| text.trim().parse::<u32>().ok())
    {
        Some(pid) => process_alive(pid),
        None => true,
    }
}

pub fn sweep_temps(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|dir| !locked(dir))
        .map(|dir| leo_core::paths::sweep_stale_temps(&dir, 0))
        .sum()
}

fn settle_new(dir: PathBuf, manifest: Manifest) -> Result<Session> {
    let settled = (|| -> Result<Session> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let session = Session {
            dir: dir.clone(),
            manifest,
        };
        session.save()?;
        Ok(session)
    })();
    if settled.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    settled
}

#[derive(Debug, Clone)]
pub struct Brief {
    pub dir: PathBuf,
    pub id: String,
    pub saved: bool,
    pub started: DateTime<Utc>,
    pub note: Option<String>,
    pub traced: Option<String>,
}

impl Brief {
    pub fn owner(&self) -> Option<&str> {
        self.note.as_deref().or(self.traced.as_deref())
    }
}

#[derive(Clone)]
enum Seen {
    Parsed(Brief),
    Malformed,
    Unreadable,
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    file: (u64, u64),
}

type Held = std::collections::HashMap<PathBuf, (Option<Stamp>, std::time::Instant, Seen)>;

static SEEN: std::sync::LazyLock<std::sync::Mutex<Held>> =
    std::sync::LazyLock::new(Default::default);

fn held() -> std::sync::MutexGuard<'static, Held> {
    SEEN.lock().unwrap_or_else(|e| e.into_inner())
}

fn stamp_of(meta: &std::fs::Metadata) -> Option<Stamp> {
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok()?,
        #[cfg(unix)]
        changed: {
            use std::os::unix::fs::MetadataExt;
            (meta.ctime(), meta.ctime_nsec())
        },
        #[cfg(unix)]
        file: {
            use std::os::unix::fs::MetadataExt;
            (meta.ino(), meta.dev())
        },
    })
}

pub enum Walk {
    Newest { after: Option<String>, limit: usize },
    OlderThan(DateTime<Utc>),
    All,
}

#[derive(Default)]
pub struct Scan {
    pub sessions: Vec<Brief>,
    pub malformed: Vec<PathBuf>,
    pub unreadable: usize,
    pub after: Option<String>,
}

pub fn recording_id(id: &str) -> &str {
    id.strip_suffix("-you")
        .or_else(|| id.strip_suffix("-others"))
        .unwrap_or(id)
}

fn order_key(name: &str) -> (String, u32) {
    match name
        .get(15..)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|n| n.parse().ok())
    {
        Some(n) => (name.get(..15).unwrap_or(name).to_string(), n),
        None => (name.to_string(), 1),
    }
}

pub fn folder_name(name: &str) -> bool {
    started_of(name).is_some()
        && name.get(15..).is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn traced_note(dir: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    leo_core::paths::open_regular(&dir.join("trace.jsonl"))
        .ok()?
        .take(MOST_SESSION_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<leo_core::recording::Trace>(l).ok())
        .filter(|t| t.stage == "saved")
        .find_map(|t| {
            let note = t.detail.strip_prefix("Note ")?.strip_suffix(" saved")?;
            leo_core::recording::valid_id(note).then(|| note.to_owned())
        })
}

pub fn has_retry(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut done = std::collections::HashSet::new();
    let mut audio = std::collections::HashSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match index_of(&name) {
            Some((index, "txt")) => done.insert(index),
            Some((index, "wav")) => audio.insert(index),
            _ => false,
        };
    }
    audio.iter().any(|index| !done.contains(index))
}

pub fn kept_owned(root: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Session::open(&e.path()).ok())
        .filter(|s| s.manifest.saved)
        .filter_map(|s| Some((s.owner()?, s.dir)))
        .map(|(note, dir)| (dir, note))
        .collect()
}

fn started_of(name: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(name.get(..15)?, "%Y%m%d-%H%M%S")
        .ok()
        .map(|t| t.and_utc())
}

fn read_manifest(dir: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    leo_core::paths::open_regular(&dir.join("session.json"))?
        .take(MOST_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MOST_SESSION_BYTES {
        return Err(std::io::Error::other(
            "Recording metadata is too large. Try again later.",
        ));
    }
    Ok(bytes)
}

fn read_seen(dir: &Path) -> Seen {
    match read_manifest(dir) {
        Ok(bytes) => match serde_json::from_slice::<Manifest>(&bytes) {
            Ok(manifest) if manifest.id.is_empty() => Seen::Unreadable,
            Ok(manifest) => Seen::Parsed(Brief {
                dir: dir.to_path_buf(),
                id: manifest.id,
                saved: manifest.saved,
                started: manifest.started,
                traced: (manifest.saved && manifest.note.is_none())
                    .then(|| traced_note(dir))
                    .flatten(),
                note: manifest.note,
            }),
            Err(_) => Seen::Malformed,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Seen::Malformed,
        Err(_) => Seen::Unreadable,
    }
}

pub fn malformed(dir: &Path) -> bool {
    matches!(read_seen(dir), Seen::Malformed)
}

fn seen(dir: &Path) -> Seen {
    let stamp = match std::fs::symlink_metadata(dir.join("session.json")) {
        Ok(meta) => stamp_of(&meta),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Seen::Malformed,
        Err(_) => return Seen::Unreadable,
    };
    if let Some(stamp) = &stamp {
        if let Some((kept, at, seen)) = held().get(dir) {
            if kept.as_ref() == Some(stamp) && at.elapsed() < RECHECK {
                return seen.clone();
            }
        }
    }
    let read = read_seen(dir);
    let settled = stamp.filter(|stamp| {
        SystemTime::now()
            .duration_since(stamp.modified)
            .is_ok_and(|age| age >= RACY)
    });
    held().insert(
        dir.to_path_buf(),
        (settled, std::time::Instant::now(), read.clone()),
    );
    read
}

pub fn scan(root: &Path, walk: Walk) -> Scan {
    let mut found = Scan::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let mut names: Vec<((String, u32), String)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|name| (order_key(&name), name))
        .collect();
    names.sort_by(|a, b| b.0.cmp(&a.0));
    let present: std::collections::HashSet<PathBuf> =
        names.iter().map(|(_, name)| root.join(name)).collect();
    held().retain(|dir, _| dir.parent() != Some(root) || present.contains(dir));
    let visit: Vec<&String> = match &walk {
        Walk::Newest { after, .. } => {
            let after = after.as_deref().map(order_key);
            names
                .iter()
                .filter(|(key, _)| after.as_ref().is_none_or(|after| key < after))
                .map(|(_, name)| name)
                .collect()
        }
        Walk::OlderThan(cutoff) => names
            .iter()
            .rev()
            .filter(|(_, name)| started_of(name).is_none_or(|started| started < *cutoff))
            .map(|(_, name)| name)
            .collect(),
        Walk::All => names.iter().map(|(_, name)| name).collect(),
    };
    let mut recordings = std::collections::HashSet::new();
    for name in visit {
        let Ok(dir) = leo_core::paths::contained_path(root, Path::new(name)) else {
            continue;
        };
        match seen(&dir) {
            Seen::Parsed(brief) => {
                if let Walk::Newest { limit, .. } = walk {
                    if (brief.saved && !has_retry(&brief.dir)) || locked(&brief.dir) {
                        continue;
                    }
                    let recording = recording_id(&brief.id).to_string();
                    if recordings.len() >= limit && !recordings.contains(&recording) {
                        found.after = found
                            .sessions
                            .last()
                            .and_then(|b| b.dir.file_name())
                            .map(|n| n.to_string_lossy().into_owned());
                        break;
                    }
                    recordings.insert(recording);
                }
                found.sessions.push(brief);
            }
            Seen::Malformed => found.malformed.push(dir),
            Seen::Unreadable => found.unreadable += 1,
        }
    }
    if found.unreadable > 0 {
        leo_core::diag::warn(format!(
            "{} recording folders could not be read in {}",
            found.unreadable,
            root.display()
        ));
    }
    found
}

pub fn find(root: &Path, id: &str) -> Vec<Session> {
    let names = [id.to_string(), format!("{id}-you"), format!("{id}-others")];
    let open = |dirs: Vec<PathBuf>| -> Vec<Session> {
        dirs.iter()
            .filter_map(|dir| Session::open(dir).ok())
            .filter(|s| names.contains(&s.manifest.id))
            .collect()
    };
    let cached: Vec<PathBuf> = held()
        .iter()
        .filter(|(dir, _)| dir.parent() == Some(root))
        .filter(|(_, (_, _, seen))| matches!(seen, Seen::Parsed(b) if names.contains(&b.id)))
        .map(|(dir, _)| dir.clone())
        .collect();
    let found = open(cached);
    if found.len() > 1 || found.iter().any(|s| s.manifest.id == id) {
        return found;
    }
    open(
        scan(root, Walk::All)
            .sessions
            .into_iter()
            .filter(|b| names.contains(&b.id))
            .map(|b| b.dir)
            .collect(),
    )
}

fn whole_lines_end(file: &mut std::fs::File) -> Result<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut end = file.metadata()?.len();
    let mut last = [0u8];
    if end > 0 {
        file.seek(SeekFrom::Start(end - 1))?;
        file.read_exact(&mut last)?;
        if last[0] == b'\n' {
            return Ok(end);
        }
    }
    let mut block = vec![0u8; 64 * 1024];
    while end > 0 {
        let start = end.saturating_sub(block.len() as u64);
        let read = &mut block[..(end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(read)?;
        if let Some(at) = read.iter().rposition(|b| *b == b'\n') {
            return Ok(start + at as u64 + 1);
        }
        end = start;
    }
    Ok(0)
}

impl Session {
    pub fn create(root: &Path, manifest: Manifest) -> Result<Session> {
        if let Ok(meta) = std::fs::symlink_metadata(root) {
            anyhow::ensure!(
                !meta.file_type().is_symlink(),
                "Recording folders cannot use symbolic links"
            );
        }
        std::fs::create_dir_all(root)?;
        let stamp = manifest.started.format("%Y%m%d-%H%M%S").to_string();
        for n in 1..=MOST_SAME_SECOND {
            let dir = root.join(if n == 1 {
                stamp.clone()
            } else {
                format!("{stamp}-{n}")
            });
            match std::fs::create_dir(&dir) {
                Ok(()) => return settle_new(dir, manifest),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(e).context(
                        "Could not create a recording folder; check disk space and permissions",
                    )
                }
            }
        }
        anyhow::bail!(
            "Too many recording sessions started together; recover an earlier recording first"
        )
    }

    pub fn open(dir: &Path) -> Result<Session> {
        let bytes =
            read_manifest(dir).with_context(|| format!("no recording in {}", dir.display()))?;
        let manifest = serde_json::from_slice(&bytes)
            .with_context(|| format!("unreadable recording in {}", dir.display()))?;
        Ok(Session {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(
            &self.dir.join("session.json"),
            &serde_json::to_string_pretty(&self.manifest)?,
        )
    }

    pub fn add_points(&self, points: &[Point]) -> Result<u64> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join("points.jsonl"))
            .context("Could not save a typed point; check disk space and permissions")?;
        let whole = whole_lines_end(&mut file)?;
        if whole < file.metadata()?.len() {
            file.set_len(whole)?;
            file.sync_data()?;
        }
        let mut lines = Vec::new();
        for point in points {
            serde_json::to_writer(&mut lines, point)?;
            lines.push(b'\n');
        }
        let added = (|| -> Result<()> {
            file.seek(SeekFrom::Start(whole))?;
            file.write_all(&lines)?;
            file.sync_data()?;
            file.seek(SeekFrom::Start(whole))?;
            let mut back = Vec::with_capacity(lines.len());
            file.read_to_end(&mut back)?;
            let read: Vec<Point> = back
                .split_inclusive(|b| *b == b'\n')
                .map(serde_json::from_slice)
                .collect::<Result<_, _>>()?;
            anyhow::ensure!(read == points, "a typed point did not read back as written");
            Ok(())
        })();
        if let Err(e) = added {
            let _ = file.set_len(whole);
            return Err(e.context("Could not save a typed point; check disk space and permissions"));
        }
        Ok(points.len() as u64)
    }

    pub fn points(&self) -> Vec<Point> {
        let mut out = Vec::new();
        self.each_point(0, |_, point| out.push(point));
        out
    }

    pub fn jotted(&self) -> Vec<Jotted> {
        self.points()
            .into_iter()
            .map(|p| Jotted {
                at_secs: p.at_secs,
                text: p.text,
            })
            .collect()
    }

    pub fn each_point(&self, from: u64, mut f: impl FnMut(u64, Point)) {
        use std::io::BufRead;
        let mut at = 0u64;
        for point in &self.manifest.points {
            if at >= from {
                f(at, point.clone());
            }
            at += 1;
        }
        let Ok(file) = std::fs::File::open(self.dir.join("points.jsonl")) else {
            return;
        };
        let mut reader = std::io::BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) if line.last() != Some(&b'\n') => return,
                Ok(_) => {
                    if let Ok(point) = serde_json::from_slice::<Point>(&line) {
                        if at >= from {
                            f(at, point);
                        }
                        at += 1;
                    }
                }
            }
        }
    }

    pub fn lock(&self) -> Result<Lock> {
        let path = self.dir.join("lock");
        write_atomic(&path, std::process::id().to_string())?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&path)?;
        anyhow::ensure!(
            file.metadata()?.is_file(),
            "the recording lock must be a regular file"
        );
        let alive = Arc::new(AtomicBool::new(true));
        let beat = Arc::clone(&alive);
        let thread = std::thread::Builder::new()
            .name("leo-session-lock".into())
            .spawn(move || {
                while beat.load(Ordering::Relaxed) {
                    std::thread::park_timeout(HEARTBEAT);
                    if !beat.load(Ordering::Relaxed) {
                        break;
                    }
                    let _ = file.set_modified(SystemTime::now());
                }
            });
        let thread = match thread {
            Ok(thread) => thread,
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                return Err(error).context("the recording heartbeat could not start; close other applications and try again");
            }
        };
        Ok(Lock {
            path,
            alive,
            thread: Some(thread),
        })
    }

    pub fn unfinished_page(
        root: &Path,
        after: Option<String>,
        limit: usize,
    ) -> (Vec<Session>, Option<String>) {
        let found = scan(root, Walk::Newest { after, limit });
        let sessions = found
            .sessions
            .iter()
            .filter_map(|brief| Session::open(&brief.dir).ok())
            .filter(|s| !s.manifest.saved || has_retry(&s.dir))
            .collect();
        (sessions, found.after)
    }

    #[cfg(test)]
    pub fn unfinished(root: &Path) -> Vec<Session> {
        let mut out = Session::unfinished_page(root, None, usize::MAX).0;
        out.sort_by(|a, b| a.dir.cmp(&b.dir));
        out
    }

    pub fn settled(&self) -> (std::collections::BTreeSet<u32>, usize) {
        let mut all = std::collections::BTreeSet::new();
        let mut settled = std::collections::BTreeSet::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return (settled, 0);
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some((index, ext)) = index_of(&name) else {
                continue;
            };
            if !matches!(ext, "txt" | "err" | "wav" | "part.wav") {
                continue;
            }
            all.insert(index);
            if matches!(ext, "txt" | "err") {
                settled.insert(index);
            }
        }
        (settled, all.len())
    }

    pub fn segment(&self, index: u32) -> Option<Segment> {
        let path = |ext: &str| self.dir.join(format!("seg-{index:05}.{ext}"));
        let state = if let Ok(text) = std::fs::read_to_string(path("txt")) {
            SegmentState::Done(text)
        } else if path("err").exists() {
            SegmentState::Failed(std::fs::read_to_string(path("err")).unwrap_or_default())
        } else if path("wav").exists() {
            SegmentState::Waiting
        } else if path("part.wav").exists() {
            SegmentState::Recording
        } else {
            return None;
        };
        Some(Segment { index, state })
    }

    pub fn segments(&self) -> Vec<Segment> {
        let mut found: std::collections::BTreeMap<u32, SegmentState> = Default::default();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for name in names {
            let Some((index, ext)) = index_of(&name) else {
                continue;
            };
            let state = match ext {
                "txt" => std::fs::read_to_string(self.dir.join(&name))
                    .ok()
                    .map(SegmentState::Done),
                "err" => Some(SegmentState::Failed(
                    std::fs::read_to_string(self.dir.join(&name)).unwrap_or_default(),
                )),
                "wav" => Some(SegmentState::Waiting),
                "part.wav" => Some(SegmentState::Recording),
                _ => None,
            };
            let Some(state) = state else { continue };
            let entry = found.entry(index).or_insert(state.clone());
            let rank = |s: &SegmentState| match s {
                SegmentState::Done(_) => 3,
                SegmentState::Failed(_) => 2,
                SegmentState::Waiting => 1,
                SegmentState::Recording => 0,
            };
            if rank(&state) > rank(entry) {
                *entry = state;
            }
        }
        found
            .into_iter()
            .map(|(index, state)| Segment { index, state })
            .collect()
    }

    pub fn assemble(&self) -> Assembled {
        let mut assembled = assemble(&self.segments(), self.manifest.segment_secs);
        if let Some(spoken) = self.labels(&assembled) {
            let segment_ms = self.manifest.segment_secs * 1000;
            for part in &mut assembled.parts {
                part.text = spoken
                    .iter()
                    .filter(|s| speakers::part_of(s, segment_ms) == part.index as u64)
                    .map(|s| format!("[Speaker {}] {}", s.speaker, s.text))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        assembled
    }

    pub fn spoken(&self) -> Option<Vec<speakers::Spoken>> {
        self.labels(&assemble(&self.segments(), self.manifest.segment_secs))
    }

    fn turns(&self, assembled: &Assembled) -> Option<Vec<Turns>> {
        if !self.manifest.turns {
            return None;
        }
        assembled
            .parts
            .iter()
            .map(|p| {
                let bytes = std::fs::read(self.dir.join(turns_name(p.index))).ok()?;
                let rows: Vec<TurnRow> = serde_json::from_slice(&bytes).ok()?;
                Some(
                    rows.into_iter()
                        .map(|row| match row {
                            TurnRow::Spoken(from, to, text) => (from, Some(to), text),
                            TurnRow::Started(from, text) => (from, None, text),
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn labels(&self, assembled: &Assembled) -> Option<Vec<speakers::Spoken>> {
        (assembled.failed.is_empty() && assembled.pending == 0)
            .then(|| speakers::read(&self.dir))
            .flatten()
    }

    pub fn recover_parts(&self) -> Result<()> {
        for seg in self.segments() {
            if seg.state == SegmentState::Recording {
                let part = self.dir.join(wav::part_name(seg.index));
                wav::seal(&part)?;
                std::fs::rename(&part, self.dir.join(wav::done_name(seg.index)))?;
            }
        }
        for entry in std::fs::read_dir(&self.dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".err") {
                let wav = self.dir.join(name.replace(".err", ".wav"));
                if wav.exists() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self, note: &str) -> Result<()> {
        let assembled = self.assemble();
        let keep_audio = !assembled.failed.is_empty() || assembled.pending > 0;
        if keep_audio {
            for part in &assembled.parts {
                let _ = std::fs::remove_file(self.dir.join(wav::done_name(part.index)));
            }
            self.manifest.saved = true;
            self.manifest.note = Some(note.to_owned());
            self.save()
        } else {
            std::fs::remove_dir_all(&self.dir)
                .with_context(|| format!("could not remove {}", self.dir.display()))
        }
    }

    pub fn label(&self) -> &'static str {
        if self.manifest.screen {
            "Computer audio"
        } else {
            "Microphone"
        }
    }

    fn call_names(&self) -> Option<leo_core::call_names::Timeline> {
        let file =
            leo_core::paths::open_regular(&self.dir.join(leo_core::call_names::FILE)).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, MOST_NAMES_BYTES), &mut text)
            .ok()?;
        Some(leo_core::call_names::Timeline::parse(&text))
    }

    pub fn archive(&self) -> leo_core::recording::Archive {
        let assembled = assemble(&self.segments(), self.manifest.segment_secs);
        let passages = match self.labels(&assembled) {
            Some(spoken) => spoken
                .into_iter()
                .map(|s| leo_core::recording::Passage {
                    start_secs: s.start_ms / 1000,
                    end_secs: s.end_ms.div_ceil(1000),
                    text: s.text,
                    speaker: format!("Speaker {}", s.speaker),
                })
                .collect(),
            None => {
                let speaker = self.label();
                let end_of = |p: &Part| {
                    self.manifest
                        .duration_secs
                        .map_or(p.end_secs, |end| p.end_secs.min(end).max(p.start_secs))
                };
                let names = self.call_names();
                match self.turns(&assembled) {
                    Some(turns) => assembled
                        .parts
                        .iter()
                        .zip(turns)
                        .flat_map(|(p, entries)| {
                            let end = end_of(p);
                            let base = p.start_secs * 1000;
                            let lead = if p.index > 0 { OVERLAP_SECS * 1000 } else { 0 };
                            let starts: Vec<u64> =
                                entries.iter().map(|(ms, _, _)| base + ms).collect();
                            let names = &names;
                            entries
                                .into_iter()
                                .enumerate()
                                .map(move |(i, (ms, spoken_to, text))| {
                                    let from = starts[i];
                                    let said = (base + ms).saturating_sub(lead);
                                    let said_to = match spoken_to {
                                        Some(to) => (base + to).saturating_sub(lead),
                                        None => starts
                                            .get(i + 1)
                                            .map_or(end * 1000, |next| next.saturating_sub(lead)),
                                    }
                                    .max(said);
                                    leo_core::recording::Passage {
                                        start_secs: from / 1000,
                                        end_secs: starts
                                            .get(i + 1)
                                            .map(|ms| ms / 1000)
                                            .unwrap_or(end)
                                            .max(from / 1000),
                                        text,
                                        speaker: names
                                            .as_ref()
                                            .and_then(|t| {
                                                t.name(
                                                    said as f64 / 1000.0,
                                                    said_to as f64 / 1000.0,
                                                )
                                            })
                                            .unwrap_or_else(|| speaker.into()),
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect(),
                    None => assembled
                        .parts
                        .iter()
                        .map(|p| leo_core::recording::Passage {
                            start_secs: p.start_secs,
                            end_secs: end_of(p),
                            text: p.text.clone(),
                            speaker: speaker.into(),
                        })
                        .collect(),
                }
            }
        };
        leo_core::recording::Archive {
            id: self.manifest.id.clone(),
            started: self.manifest.started,
            passages,
            points: self
                .points()
                .into_iter()
                .map(|p| leo_core::recording::Point {
                    at_secs: p.at_secs,
                    text: p.text,
                })
                .collect(),
            context: self.manifest.profile.context.clone(),
            wants: self.manifest.profile.wants.clone(),
            warnings: assembled
                .failed
                .iter()
                .map(|(a, b)| {
                    format!(
                        "{}–{} could not be transcribed; audio is kept for retry",
                        clock(*a),
                        clock(*b)
                    )
                })
                .collect(),
            trace: self.traces(),
            from: String::new(),
        }
    }

    pub fn trace(&self, stage: &str, detail: &str) {
        trace_to(&self.dir, stage, detail);
    }

    pub fn traces(&self) -> Vec<leo_core::recording::Trace> {
        std::fs::read_to_string(self.dir.join("trace.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn commit(self, notes: &Path, note: &str) -> Result<()> {
        self.trace("saved", &format!("Note {note} saved"));
        leo_core::recording::save(notes, note, &self.archive())?;
        self.finish(note)
    }

    pub fn owner(&self) -> Option<String> {
        self.manifest
            .note
            .clone()
            .or_else(|| traced_note(&self.dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(unix)]
    #[test]
    fn a_fifo_manifest_does_not_block_opening_a_session() {
        let (session, _tmp) = session();
        let path = session.dir.join("session.json");
        std::fs::remove_file(&path).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let (done, result) = std::sync::mpsc::channel();
        let worker =
            std::thread::spawn(move || done.send(Session::open(&session.dir).is_err()).unwrap());
        let received = result.recv_timeout(std::time::Duration::from_secs(2));
        if received.is_err() {
            let _ = std::fs::OpenOptions::new().write(true).open(&path);
        }
        worker.join().unwrap();
        assert!(received.unwrap());
    }

    fn session() -> (Session, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        (s, tmp)
    }

    fn point(at_secs: u64, text: &str) -> Point {
        Point {
            at_secs,
            text: text.into(),
        }
    }

    #[test]
    fn points_are_added_one_line_at_a_time() {
        let (s, _t) = session();
        let manifest = std::fs::read(s.dir.join("session.json")).unwrap();
        assert_eq!(s.add_points(&[point(1, "first")]).unwrap(), 1);
        assert_eq!(s.add_points(&[point(2, "second\nline")]).unwrap(), 1);
        let file = std::fs::read_to_string(s.dir.join("points.jsonl")).unwrap();
        assert_eq!(file.lines().count(), 2);
        std::fs::OpenOptions::new()
            .append(true)
            .open(s.dir.join("points.jsonl"))
            .unwrap()
            .write_all(b"{\"at_secs\":3,\"te")
            .unwrap();
        assert_eq!(s.points(), [point(1, "first"), point(2, "second\nline")]);
        let mut seen = Vec::new();
        s.each_point(1, |at, p| seen.push((at, p.text)));
        assert_eq!(seen, [(1, "second\nline".to_string())]);
        assert_eq!(
            std::fs::read(s.dir.join("session.json")).unwrap(),
            manifest,
            "a point never rewrites the manifest"
        );
    }

    #[test]
    fn a_point_after_a_torn_tail_is_kept() {
        let (s, _t) = session();
        s.add_points(&[point(1, "kept")]).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(s.dir.join("points.jsonl"))
            .unwrap()
            .write_all(b"{\"at_secs\":2,\"text\":\"torn")
            .unwrap();
        let again = Session::open(&s.dir).unwrap();
        again.add_points(&[point(3, "after")]).unwrap();
        let fresh = Session::open(&s.dir).unwrap();
        assert_eq!(fresh.points(), [point(1, "kept"), point(3, "after")]);
        let file = std::fs::read_to_string(s.dir.join("points.jsonl")).unwrap();
        assert!(!file.contains("torn"));
        assert!(file.ends_with('\n'));
    }

    #[test]
    fn an_old_session_keeps_its_manifest_points() {
        let (mut s, _t) = session();
        s.manifest.points = vec![point(5, "from the manifest")];
        s.save().unwrap();
        let old = Session::open(&s.dir).unwrap();
        assert!(!old.dir.join("points.jsonl").exists());
        assert_eq!(old.points(), [point(5, "from the manifest")]);
        assert_eq!(old.jotted()[0].text, "from the manifest");
        assert_eq!(old.archive().points[0].text, "from the manifest");
        old.add_points(&[point(9, "new")]).unwrap();
        assert_eq!(
            Session::open(&s.dir).unwrap().points(),
            [point(5, "from the manifest"), point(9, "new")]
        );
    }

    #[test]
    fn a_session_that_cannot_save_its_manifest_removes_its_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("fresh");
        std::fs::create_dir(&dir).unwrap();
        std::fs::create_dir(dir.join("session.json")).unwrap();
        assert!(settle_new(dir.clone(), Manifest::new(None, None, "", false)).is_err());
        assert!(!dir.exists());
    }

    #[test]
    fn settled_counts_parts_from_names_only() {
        let (recording, _dir) = session();
        for name in ["seg-00000.txt", "seg-00001.txt", "seg-00002.txt"] {
            std::fs::create_dir(recording.dir.join(name)).unwrap();
        }
        for name in [
            "seg-00003.err",
            "seg-00004.wav",
            "seg-00005.wav",
            "seg-00006.part.wav",
            "notes.txt",
        ] {
            std::fs::write(recording.dir.join(name), "").unwrap();
        }
        let (settled, total) = recording.settled();
        assert_eq!(settled.into_iter().collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(total, 7);
    }

    #[test]
    fn dropping_a_recording_lock_stops_its_heartbeat_and_removes_the_file() {
        let (recording, _dir) = session();
        let held = recording.lock().unwrap();
        assert!(recording.dir.join("lock").exists());
        let start = std::time::Instant::now();
        drop(held);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!recording.dir.join("lock").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_recording_lock_does_not_truncate_a_preexisting_symlink_target() {
        let (recording, dir) = session();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, "keep").unwrap();
        std::os::unix::fs::symlink(&outside, recording.dir.join("lock")).unwrap();
        let held = recording.lock().unwrap();
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
        drop(held);
    }

    #[test]
    fn a_session_kept_for_retry_drops_audio_already_transcribed_and_keeps_the_failed_part() {
        let (s, _t) = session();
        let dir = s.dir.clone();
        std::fs::write(dir.join("seg-00000.txt"), "done").unwrap();
        wav::write(&dir.join(wav::done_name(0)), &[1, 2, 3]).unwrap();
        std::fs::write(dir.join("seg-00001.err"), "failed").unwrap();
        wav::write(&dir.join(wav::done_name(1)), &[1, 2, 3]).unwrap();
        s.finish("n1").unwrap();
        assert!(!dir.join(wav::done_name(0)).exists());
        assert!(dir.join(wav::done_name(1)).exists());
    }

    #[test]
    fn saved_speaker_labels_prefix_the_note_text_and_split_the_sources() {
        let (s, _t) = session();
        std::fs::write(s.dir.join("seg-00000.txt"), "so friday then fine").unwrap();
        std::fs::write(s.dir.join("seg-00001.txt"), "okay").unwrap();
        let plain = s.archive();
        assert_eq!(plain.passages.len(), 2);
        assert_eq!(plain.passages[0].speaker, "Microphone");
        speakers::write(
            &s.dir,
            &[
                speakers::Spoken {
                    start_ms: 0,
                    end_ms: 4_000,
                    speaker: 1,
                    text: "So Friday then?".into(),
                },
                speakers::Spoken {
                    start_ms: 4_000,
                    end_ms: 9_000,
                    speaker: 2,
                    text: "Fine.".into(),
                },
                speakers::Spoken {
                    start_ms: 300_500,
                    end_ms: 302_000,
                    speaker: 1,
                    text: "Okay.".into(),
                },
            ],
        )
        .unwrap();
        let parts = s.assemble().parts;
        assert_eq!(
            parts[0].text,
            "[Speaker 1] So Friday then?\n[Speaker 2] Fine."
        );
        assert_eq!(parts[1].text, "[Speaker 1] Okay.");
        let archive = s.archive();
        assert_eq!(
            archive
                .passages
                .iter()
                .map(|p| (p.speaker.as_str(), p.text.as_str(), p.start_secs))
                .collect::<Vec<_>>(),
            vec![
                ("Speaker 1", "So Friday then?", 0),
                ("Speaker 2", "Fine.", 4),
                ("Speaker 1", "Okay.", 300)
            ]
        );
        std::fs::write(s.dir.join("seg-00002.err"), "failed").unwrap();
        assert_eq!(
            s.archive().passages[0].speaker,
            "Microphone",
            "a failed part keeps today's text"
        );
    }

    #[test]
    fn a_session_survives_being_reopened() {
        let (mut s, _t) = session();
        s.manifest.points.push(Point {
            at_secs: 61,
            text: "exam".into(),
        });
        s.save().unwrap();
        let again = Session::open(&s.dir).unwrap();
        assert_eq!(again.manifest, s.manifest);
    }

    #[test]
    fn segment_states_come_from_the_files_and_text_wins() {
        let (s, _t) = session();
        std::fs::write(s.dir.join("seg-00000.txt"), "hello").unwrap();
        std::fs::write(s.dir.join("seg-00001.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00001.err"), "429").unwrap();
        std::fs::write(s.dir.join("seg-00002.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00003.part.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00004.txt"), "").unwrap();
        std::fs::write(s.dir.join("seg-00004.wav"), "x").unwrap();
        let states: Vec<SegmentState> = s.segments().into_iter().map(|s| s.state).collect();
        assert_eq!(
            states,
            vec![
                SegmentState::Done("hello".into()),
                SegmentState::Failed("429".into()),
                SegmentState::Waiting,
                SegmentState::Recording,
                SegmentState::Done(String::new()),
            ]
        );
    }

    #[test]
    fn assembly_keeps_order_marks_failures_and_joins_overlaps() {
        let segments = vec![
            Segment {
                index: 0,
                state: SegmentState::Done("the cat sat on the mat".into()),
            },
            Segment {
                index: 1,
                state: SegmentState::Done("on the mat and slept".into()),
            },
            Segment {
                index: 2,
                state: SegmentState::Failed("timeout".into()),
            },
            Segment {
                index: 3,
                state: SegmentState::Waiting,
            },
        ];
        let a = assemble(&segments, 300);
        assert_eq!(a.text(), "the cat sat on the mat and slept");
        assert_eq!(a.failed, vec![(600, 900)]);
        assert_eq!(a.pending, 1);
        assert_eq!(a.parts[1].start_secs, 300);
        let notice = failure_notice(&a.failed, Path::new("/x"));
        assert!(notice.contains("10:00–15:00"), "{notice}");
    }

    #[test]
    fn crash_leftovers_are_sealed_and_retried() {
        let (s, _t) = session();
        wav::write(&s.dir.join("seg-00000.part.wav"), &[1, 2, 3]).unwrap();
        std::fs::write(s.dir.join("seg-00001.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00001.err"), "gave up").unwrap();
        s.recover_parts().unwrap();
        let states: Vec<SegmentState> = s.segments().into_iter().map(|s| s.state).collect();
        assert_eq!(states, vec![SegmentState::Waiting, SegmentState::Waiting]);
    }

    fn leave_old_temp(dir: &Path) -> PathBuf {
        let path = dir.join(".tmpAbC123");
        std::fs::write(&path, "half a segment").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - std::time::Duration::from_secs(7200))
            .unwrap();
        path
    }

    #[test]
    fn a_locked_session_keeps_its_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let held = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let free = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        std::fs::write(held.dir.join("lock"), std::process::id().to_string()).unwrap();
        let kept = leave_old_temp(&held.dir);
        let gone = leave_old_temp(&free.dir);
        let segment = free.dir.join("seg-00000.wav");
        std::fs::write(&segment, "sealed").unwrap();
        assert_eq!(sweep_temps(tmp.path()), 1);
        assert!(kept.exists());
        assert!(!gone.exists());
        assert!(segment.exists());
        assert_eq!(sweep_temps(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn a_lock_left_by_a_process_that_died_does_not_count() {
        let (s, _t) = session();
        std::fs::write(s.dir.join("lock"), "999999").unwrap();
        assert!(!locked(&s.dir), "a dead process still holds the recording");
        std::fs::write(s.dir.join("lock"), std::process::id().to_string()).unwrap();
        assert!(locked(&s.dir));
        std::fs::write(s.dir.join("lock"), "not a pid").unwrap();
        assert!(
            locked(&s.dir),
            "an unreadable lock is treated as held while it is fresh"
        );
    }

    #[test]
    fn unfinished_sessions_are_found_unless_saved_or_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let a = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let mut b = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        b.manifest.saved = true;
        b.save().unwrap();
        let c = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let _held = c.lock().unwrap();
        let older = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let mut expected = vec![a.dir.clone(), older.dir.clone()];
        expected.sort();
        let found: Vec<PathBuf> = Session::unfinished(tmp.path())
            .into_iter()
            .map(|s| s.dir)
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn finish_now_audio_is_offered_for_retry() {
        use super::recorder::{finish, Event, Live};
        use super::transcriber::{Policy, Transcriber};
        let root = tempfile::tempdir().unwrap();
        let notes = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new(None, None, "", false);
        manifest.id = "verify-rec01".into();
        let session = Session::create(root.path(), manifest).unwrap();
        let dir = session.dir.clone();
        std::fs::write(dir.join("seg-00000.txt"), "the first part").unwrap();
        wav::write(&dir.join(wav::done_name(1)), &vec![3000i16; 16_000]).unwrap();
        let (tx, updates) = std::sync::mpsc::channel();
        let transcriber = Transcriber::start(
            &dir,
            Arc::new(|_| Err("boom".into())),
            Policy {
                first_wait: Duration::from_millis(5),
                idle: Duration::from_millis(5),
                finish_limit: Duration::from_millis(40),
                ..Policy::default()
            },
            false,
            tx,
        );
        let events = std::sync::Mutex::new(Vec::new());
        finish(
            session,
            transcriber,
            &updates,
            &mut Live::new(crate::ai::live::ROLL_INTERVAL),
            &|event| events.lock().unwrap().push(event),
            &AtomicBool::new(true),
        );
        let events = events.into_inner().unwrap();
        assert!(
            matches!(events.last(), Some(Event::Finished { .. })),
            "{:?}",
            events.last()
        );
        Session::open(&dir)
            .unwrap()
            .commit(notes.path(), "verify-rec01")
            .unwrap();
        let kept = Session::open(&dir).unwrap();
        assert!(kept.manifest.saved && dir.join(wav::done_name(1)).exists());
        let offered: Vec<PathBuf> = Session::unfinished(root.path())
            .into_iter()
            .map(|s| s.dir)
            .collect();
        assert_eq!(offered, [dir]);
    }

    #[test]
    fn a_saved_recording_is_offered_while_any_part_has_audio_and_no_text() {
        let (mut s, _t) = session();
        s.manifest.saved = true;
        s.save().unwrap();
        assert!(!has_retry(&s.dir));
        std::fs::write(s.dir.join("seg-00000.txt"), "done").unwrap();
        wav::write(&s.dir.join(wav::done_name(0)), &[1, 2, 3]).unwrap();
        std::fs::write(s.dir.join("seg-00001.err"), "failed").unwrap();
        assert!(!has_retry(&s.dir));
        wav::write(&s.dir.join(wav::done_name(2)), &[1, 2, 3]).unwrap();
        assert!(has_retry(&s.dir));
    }

    fn settled_folder(root: &Path, manifest: &Manifest) -> PathBuf {
        let dir = root.join(manifest.started.format("%Y%m%d-%H%M%S").to_string());
        std::fs::create_dir(&dir).unwrap();
        write_settled(&dir, manifest);
        dir
    }

    fn write_settled(dir: &Path, manifest: &Manifest) {
        let path = dir.join("session.json");
        std::fs::write(&path, serde_json::to_vec(manifest).unwrap()).unwrap();
        set_old(&path);
    }

    fn set_old(path: &Path) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    #[test]
    fn every_recording_is_reachable_a_page_at_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let base = chrono::Utc::now() - chrono::Duration::days(400);
        let mut dirs = Vec::new();
        for n in 0..5000 {
            let mut manifest = Manifest::new(None, None, "", false);
            manifest.started = base + chrono::Duration::seconds(n * 7);
            dirs.push(settled_folder(tmp.path(), &manifest));
        }
        dirs.reverse();
        let (first, mut after) = Session::unfinished_page(tmp.path(), None, 50);
        let first: Vec<PathBuf> = first.into_iter().map(|s| s.dir).collect();
        assert_eq!(first, dirs[..50]);
        let mut seen = first;
        while let Some(cursor) = after.take() {
            let (page, next) = Session::unfinished_page(tmp.path(), Some(cursor), 50);
            assert!(page.len() <= 50);
            seen.extend(page.into_iter().map(|s| s.dir));
            after = next;
        }
        assert_eq!(seen, dirs);
        let junk = tempfile::tempdir().unwrap();
        for n in 0..1000 {
            std::fs::write(junk.path().join(format!("junk-{n}.txt")), "").unwrap();
        }
        let only = Session::create(junk.path(), Manifest::new(None, None, "", false))
            .unwrap()
            .dir;
        let found: Vec<PathBuf> = Session::unfinished(junk.path())
            .into_iter()
            .map(|s| s.dir)
            .collect();
        assert_eq!(found, [only]);
    }

    #[test]
    fn a_call_is_never_split_across_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let base = chrono::Utc::now() - chrono::Duration::days(10);
        for (n, id) in ["single-a", "call-b-you", "call-b-others", "single-c"]
            .into_iter()
            .enumerate()
        {
            let mut manifest = Manifest::new(None, None, "", false);
            manifest.id = id.into();
            manifest.started = base + chrono::Duration::seconds([30, 20, 20, 10][n]);
            Session::create(tmp.path(), manifest).unwrap();
        }
        let ids = |sessions: Vec<Session>| -> Vec<String> {
            sessions.into_iter().map(|s| s.manifest.id).collect()
        };
        let (first, after) = Session::unfinished_page(tmp.path(), None, 1);
        assert_eq!(ids(first), ["single-a"]);
        let (second, after) = Session::unfinished_page(tmp.path(), after, 1);
        let mut second = ids(second);
        second.sort();
        assert_eq!(second, ["call-b-others", "call-b-you"]);
        let (third, after) = Session::unfinished_page(tmp.path(), after, 1);
        assert_eq!(ids(third), ["single-c"]);
        assert!(after.is_none());
    }

    #[test]
    fn a_call_found_by_id_has_both_sides_when_one_is_remembered() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dirs = Vec::new();
        for (side, secs) in [("you", 0), ("others", 1)] {
            let mut manifest = Manifest::new(None, None, "", side == "others");
            manifest.id = format!("call-1-{side}");
            manifest.started += chrono::Duration::seconds(secs);
            dirs.push(Session::create(tmp.path(), manifest).unwrap().dir);
        }
        assert_eq!(find(tmp.path(), "call-1").len(), 2);
        held().remove(&dirs[1]);
        assert_eq!(find(tmp.path(), "call-1").len(), 2);
    }

    #[test]
    fn a_recording_without_an_id_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new(None, None, "", false);
        manifest.id.clear();
        manifest.started = chrono::Utc::now() - chrono::Duration::days(40);
        let dir = settled_folder(tmp.path(), &manifest);
        let path = dir.join("session.json");
        let bytes = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(Session::unfinished(tmp.path()).is_empty());
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        let found = scan(tmp.path(), Walk::OlderThan(cutoff));
        assert!(found.sessions.is_empty() && found.malformed.is_empty());
        assert_eq!(found.unreadable, 1);
        assert!(!malformed(&dir));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn the_folder_name_order_puts_ten_after_two() {
        let mut names = ["20250101-120000-10", "20250101-120000", "20250101-120000-2"];
        names.sort_by_key(|n| order_key(n));
        assert_eq!(
            names,
            ["20250101-120000", "20250101-120000-2", "20250101-120000-10"]
        );
        assert!(order_key("20250101-120001") > order_key("20250101-120000-10"));
    }

    #[test]
    fn a_same_stamp_manifest_edit_is_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let saved = || scan(tmp.path(), Walk::All).sessions[0].saved;
        let mut manifest = Manifest::new(None, None, "", false);
        manifest.started = chrono::Utc::now() - chrono::Duration::days(1);
        let dir = settled_folder(tmp.path(), &manifest);
        assert!(!saved());
        let stale = held().get(&dir).unwrap().2.clone();
        manifest.saved = true;
        let replaced = dir.join("session.json.new");
        std::fs::write(&replaced, serde_json::to_vec(&manifest).unwrap()).unwrap();
        set_old(&replaced);
        std::fs::rename(&replaced, dir.join("session.json")).unwrap();
        if cfg!(unix) {
            assert!(saved());
        }
        let stamp =
            stamp_of(&std::fs::symlink_metadata(dir.join("session.json")).unwrap()).unwrap();
        held().insert(dir.clone(), (Some(stamp), std::time::Instant::now(), stale));
        assert!(!saved());
        if let Some(entry) = held().get_mut(&dir) {
            entry.1 = std::time::Instant::now()
                .checked_sub(RECHECK + Duration::from_secs(1))
                .unwrap();
        }
        assert!(saved());
    }

    #[test]
    fn a_manifest_rewritten_inside_two_seconds_is_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        assert_eq!(Session::unfinished(tmp.path()).len(), 1);
        assert!(held()
            .get(&s.dir)
            .is_some_and(|(stamp, _, _)| stamp.is_none()));
        s.manifest.saved = true;
        s.save().unwrap();
        assert!(Session::unfinished(tmp.path()).is_empty());
    }

    #[test]
    fn an_unchanged_manifest_is_not_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = settled_folder(tmp.path(), &Manifest::new(None, None, "", false));
        let held_at = |dir: &Path| held().get(dir).map(|(_, at, _)| *at);
        assert_eq!(scan(tmp.path(), Walk::All).sessions.len(), 1);
        let first = held_at(&dir).unwrap();
        assert_eq!(scan(tmp.path(), Walk::All).sessions.len(), 1);
        assert_eq!(held_at(&dir), Some(first));
        std::fs::remove_dir_all(&dir).unwrap();
        scan(tmp.path(), Walk::All);
        assert!(held_at(&dir).is_none());
    }

    #[test]
    #[ignore]
    fn benchmark_recording_listing() {
        for n in [500usize, 5_000, 50_000] {
            let tmp = tempfile::tempdir().unwrap();
            let base = chrono::Utc::now() - chrono::Duration::days(400);
            for i in 0..n {
                let mut manifest = Manifest::new(None, None, "", false);
                manifest.started = base + chrono::Duration::seconds(i as i64 * 7);
                settled_folder(tmp.path(), &manifest);
            }
            for view in ["first", "second"] {
                let start = std::time::Instant::now();
                let listed = Session::unfinished_page(tmp.path(), None, 50).0.len();
                println!(
                    "folders={n} view={view} listed={listed} ms={:.2}",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn scan_tells_malformed_from_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let folder = |name: &str| {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            dir
        };
        let broken = folder("20200101-000001");
        std::fs::write(broken.join("session.json"), "{").unwrap();
        let empty = folder("20200101-000002");
        let manifest = serde_json::to_string(&Manifest::new(None, None, "", false)).unwrap();
        let shut = folder("20200101-000003");
        std::fs::write(shut.join("session.json"), &manifest).unwrap();
        std::fs::set_permissions(
            shut.join("session.json"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let huge = folder("20200101-000004");
        std::fs::write(
            huge.join("session.json"),
            " ".repeat(MOST_SESSION_BYTES as usize + 1),
        )
        .unwrap();
        let good = folder("20200101-000005");
        std::fs::write(good.join("session.json"), &manifest).unwrap();
        std::fs::write(tmp.path().join("not-a-folder"), "").unwrap();
        let found = scan(tmp.path(), Walk::All);
        std::fs::set_permissions(
            shut.join("session.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let sessions: Vec<PathBuf> = found.sessions.into_iter().map(|s| s.dir).collect();
        assert_eq!(sessions, [good]);
        assert_eq!(found.malformed, [empty, broken]);
        assert_eq!(found.unreadable, 2);
        assert!(Session::open(&huge).is_err());
    }

    #[test]
    fn simultaneous_tracks_get_independent_session_directories() {
        let temp = tempfile::tempdir().unwrap();
        let barrier = std::sync::Barrier::new(32);
        let manifest = Manifest::new(None, None, "", false);
        let dirs = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..32)
                .map(|_| {
                    let barrier = &barrier;
                    let root = temp.path();
                    let manifest = manifest.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        Session::create(root, manifest).unwrap().dir
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<std::collections::HashSet<_>>()
        });
        assert_eq!(dirs.len(), 32);
    }

    #[test]
    fn finishing_removes_the_audio_unless_something_failed() {
        let (s, t) = session();
        std::fs::write(s.dir.join("seg-00000.txt"), "ok").unwrap();
        let dir = s.dir.clone();
        s.finish("n1").unwrap();
        assert!(!dir.exists());

        let s2 = Session::create(t.path(), Manifest::new(None, None, "", false)).unwrap();
        std::fs::write(s2.dir.join("seg-00000.err"), "no").unwrap();
        std::fs::write(s2.dir.join("seg-00000.wav"), "x").unwrap();
        let dir2 = s2.dir.clone();
        s2.finish("n1").unwrap();
        assert!(dir2.exists());
        let kept = Session::open(&dir2).unwrap().manifest;
        assert!(kept.saved);
        assert_eq!(kept.note.as_deref(), Some("n1"));
    }

    #[test]
    fn a_kept_recording_names_its_note() {
        let (s, _t) = session();
        let notes = tempfile::tempdir().unwrap();
        let dir = s.dir.clone();
        std::fs::write(dir.join("seg-00000.txt"), "the first part").unwrap();
        std::fs::write(dir.join("seg-00001.err"), "failed").unwrap();
        wav::write(&dir.join(wav::done_name(1)), &[1, 2, 3]).unwrap();
        s.commit(notes.path(), "note-n01").unwrap();
        let kept = Session::open(&dir).unwrap();
        assert!(kept.manifest.saved);
        assert_eq!(kept.manifest.note.as_deref(), Some("note-n01"));
        assert_eq!(kept.owner().as_deref(), Some("note-n01"));
        assert!(dir.join(wav::done_name(1)).exists());
    }

    #[test]
    fn kept_owned_lists_folders_with_their_note() {
        let tmp = tempfile::tempdir().unwrap();
        let base = chrono::Utc::now() - chrono::Duration::days(3);
        let folder = |n: i64, saved: bool, note: Option<&str>| {
            let mut manifest = Manifest::new(None, None, "", false);
            manifest.id = format!("kept-{n}");
            manifest.started = base + chrono::Duration::minutes(n);
            manifest.saved = saved;
            manifest.note = note.map(String::from);
            settled_folder(tmp.path(), &manifest)
        };
        let named = folder(0, true, Some("note-a01"));
        let old = folder(1, true, None);
        Session::open(&old)
            .unwrap()
            .trace("saved", "Note note-b01 saved");
        let moved = folder(2, true, Some("note-c01"));
        Session::open(&moved)
            .unwrap()
            .trace("saved", "Note note-a01 saved");
        let ownerless = folder(3, true, None);
        let unfinished = folder(4, false, Some("note-d01"));
        let mut owned = kept_owned(tmp.path());
        owned.sort();
        assert_eq!(
            owned,
            [
                (named, "note-a01".to_string()),
                (old, "note-b01".to_string()),
                (moved, "note-c01".to_string())
            ]
        );
        assert!(ownerless.exists() && unfinished.exists());
    }

    fn call_track(parts: &[&str]) -> (Session, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new(None, None, "", false);
        manifest.turns = true;
        manifest.duration_secs = Some(600);
        let s = Session::create(tmp.path(), manifest).unwrap();
        for (i, text) in parts.iter().enumerate() {
            std::fs::write(s.dir.join(format!("seg-{i:05}.txt")), text).unwrap();
        }
        (s, tmp)
    }

    #[test]
    fn speech_crossing_a_part_boundary_is_kept() {
        let (s, _tmp) = call_track(&["Before the turn.", "A long answer that runs on. Then more."]);
        std::fs::write(
            s.dir.join(turns_name(0)),
            r#"[[290000,"Before the turn."]]"#,
        )
        .unwrap();
        std::fs::write(
            s.dir.join(turns_name(1)),
            r#"[[500,"A long answer that runs on."],[20500,"Then more."]]"#,
        )
        .unwrap();
        let passages = s.archive().passages;
        let spans: Vec<(u64, u64, &str)> = passages
            .iter()
            .map(|p| (p.start_secs, p.end_secs, p.text.as_str()))
            .collect();
        assert_eq!(
            spans,
            [
                (290, 300, "Before the turn."),
                (300, 320, "A long answer that runs on."),
                (320, 600, "Then more."),
            ]
        );
    }

    #[test]
    fn call_lines_take_the_names_the_call_app_showed() {
        let (s, _tmp) = call_track(&["First. Second. Third."]);
        std::fs::write(
            s.dir.join(turns_name(0)),
            r#"[[1200,"First."],[4600,"Second."],[9100,"Third."]]"#,
        )
        .unwrap();
        std::fs::write(
            s.dir.join(leo_core::call_names::FILE),
            concat!(
                "{\"from\":1.0,\"to\":4.4,\"who\":\"Rowan Ellery\"}\n",
                "{\"from\":4.4,\"to\":6.0,\"who\":\"Tamsin Okafor\"}\n",
                "{\"me\":\"Juno Halvorsen\"}\n",
            ),
        )
        .unwrap();
        let speakers: Vec<String> = s
            .archive()
            .passages
            .into_iter()
            .map(|p| p.speaker)
            .collect();
        assert_eq!(speakers, ["Rowan Ellery", "Microphone", "Microphone"]);
    }

    #[test]
    fn a_short_line_before_a_long_silence_keeps_its_name_and_later_parts_skip_their_lead() {
        let (s, _tmp) = call_track(&["", "Hi. Later."]);
        std::fs::write(s.dir.join(turns_name(0)), "[]").unwrap();
        std::fs::write(
            s.dir.join(turns_name(1)),
            r#"[[1500,2500,"Hi."],[22500,23500,"Later."]]"#,
        )
        .unwrap();
        std::fs::write(
            s.dir.join(leo_core::call_names::FILE),
            concat!(
                "{\"from\":300.4,\"to\":301.6,\"who\":\"Rowan Ellery\"}\n",
                "{\"from\":321.4,\"to\":322.6,\"who\":\"Tamsin Okafor\"}\n",
            ),
        )
        .unwrap();
        let lines: Vec<(u64, String)> = s
            .archive()
            .passages
            .into_iter()
            .map(|p| (p.start_secs, p.speaker))
            .collect();
        assert_eq!(
            lines,
            [
                (301, "Rowan Ellery".to_string()),
                (322, "Tamsin Okafor".to_string())
            ]
        );
    }

    #[test]
    fn a_call_without_turns_files_keeps_one_passage_per_part() {
        let (s, _tmp) = call_track(&["First part.", "Second part."]);
        std::fs::write(s.dir.join(turns_name(0)), r#"[[0,"First part."]]"#).unwrap();
        let passages = s.archive().passages;
        let spans: Vec<(u64, &str)> = passages
            .iter()
            .map(|p| (p.start_secs, p.text.as_str()))
            .collect();
        assert_eq!(spans, [(0, "First part."), (300, "Second part.")]);
    }
}
