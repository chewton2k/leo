pub mod awake;
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
const MOST_SESSIONS: usize = 1000;
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
        }
    }

    pub fn jotted(&self) -> Vec<Jotted> {
        self.points
            .iter()
            .map(|p| Jotted {
                at_secs: p.at_secs,
                text: p.text.clone(),
            })
            .collect()
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
    let path = dir.join("lock");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < LOCK_FRESH);
    if !fresh {
        return false;
    }
    match std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    {
        Some(pid) => process_alive(pid),
        None => true,
    }
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

pub struct Scan {
    pub sessions: Vec<Session>,
    pub malformed: Vec<PathBuf>,
    pub unreadable: usize,
}

fn read_manifest(dir: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(dir.join("session.json"))?
        .take(MOST_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MOST_SESSION_BYTES {
        return Err(std::io::Error::other(
            "Recording metadata is too large. Try again later.",
        ));
    }
    Ok(bytes)
}

pub fn scan(root: &Path) -> Scan {
    let mut found = Scan {
        sessions: Vec::new(),
        malformed: Vec::new(),
        unreadable: 0,
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let mut dirs: Vec<PathBuf> = entries
        .take(10 * MOST_SESSIONS)
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| leo_core::paths::contained_path(root, Path::new(&e.file_name())).ok())
        .collect();
    dirs.sort_by(|a, b| b.cmp(a));
    for dir in dirs {
        match read_manifest(&dir) {
            Ok(bytes) => match serde_json::from_slice::<Manifest>(&bytes) {
                Ok(manifest) => match Session::with_manifest(&dir, manifest) {
                    Ok(session) => found.sessions.push(session),
                    Err(_) => found.unreadable += 1,
                },
                Err(_) => found.malformed.push(dir),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => found.malformed.push(dir),
            Err(_) => found.unreadable += 1,
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
        for n in 1..=MOST_SESSIONS {
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
        Session::with_manifest(dir, manifest)
    }

    fn with_manifest(dir: &Path, mut manifest: Manifest) -> Result<Session> {
        if manifest.id.is_empty() {
            manifest.id = uuid::Uuid::new_v4().to_string();
            write_atomic(
                &dir.join("session.json"),
                &serde_json::to_string_pretty(&manifest)?,
            )?;
        }
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

    pub fn unfinished(root: &Path) -> Vec<Session> {
        let mut out: Vec<Session> = scan(root)
            .sessions
            .into_iter()
            .filter(|s| !s.manifest.saved && !locked(&s.dir))
            .take(MOST_SESSIONS)
            .collect();
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

    fn turns(&self, assembled: &Assembled) -> Option<Vec<Vec<(u64, String)>>> {
        if !self.manifest.turns {
            return None;
        }
        assembled
            .parts
            .iter()
            .map(|p| {
                let bytes = std::fs::read(self.dir.join(turns_name(p.index))).ok()?;
                serde_json::from_slice(&bytes).ok()
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

    pub fn finish(mut self) -> Result<()> {
        let assembled = self.assemble();
        let keep_audio = !assembled.failed.is_empty() || assembled.pending > 0;
        if keep_audio {
            for part in &assembled.parts {
                let _ = std::fs::remove_file(self.dir.join(wav::done_name(part.index)));
            }
            self.manifest.saved = true;
            self.save()
        } else {
            std::fs::remove_dir_all(&self.dir)
                .with_context(|| format!("could not remove {}", self.dir.display()))
        }
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
                let speaker = if self.manifest.screen {
                    "Computer audio"
                } else {
                    "Microphone"
                };
                let end_of = |p: &Part| {
                    self.manifest
                        .duration_secs
                        .map_or(p.end_secs, |end| p.end_secs.min(end).max(p.start_secs))
                };
                match self.turns(&assembled) {
                    Some(turns) => assembled
                        .parts
                        .iter()
                        .zip(turns)
                        .flat_map(|(p, entries)| {
                            let end = end_of(p);
                            let starts: Vec<u64> = entries
                                .iter()
                                .map(|(ms, _)| p.start_secs + ms / 1000)
                                .collect();
                            entries
                                .into_iter()
                                .enumerate()
                                .map(|(i, (_, text))| leo_core::recording::Passage {
                                    start_secs: starts[i],
                                    end_secs: starts
                                        .get(i + 1)
                                        .copied()
                                        .unwrap_or(end)
                                        .max(starts[i]),
                                    text,
                                    speaker: speaker.into(),
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
                .manifest
                .points
                .iter()
                .map(|p| leo_core::recording::Point {
                    at_secs: p.at_secs,
                    text: p.text.clone(),
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
        use std::io::Write;
        let row = leo_core::recording::Trace {
            at: Utc::now(),
            stage: stage.into(),
            detail: detail.into(),
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("trace.jsonl"))
        {
            if let Ok(text) = serde_json::to_string(&row) {
                let _ = writeln!(file, "{text}");
            }
        }
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
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (Session, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        (s, tmp)
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
        s.finish().unwrap();
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
    fn past_a_thousand_folders_the_newest_are_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let base = chrono::Utc::now() - chrono::Duration::days(2000);
        let mut dirs = Vec::new();
        for n in 0..1001 {
            let mut manifest = Manifest::new(None, None, "", false);
            manifest.started = base + chrono::Duration::days(n);
            dirs.push(Session::create(tmp.path(), manifest).unwrap().dir);
        }
        let listed: Vec<PathBuf> = Session::unfinished(tmp.path())
            .into_iter()
            .map(|s| s.dir)
            .collect();
        assert_eq!(
            listed,
            dirs[1..].to_vec(),
            "the newest 1000 recordings are listed"
        );
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
        let found = scan(tmp.path());
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
        s.finish().unwrap();
        assert!(!dir.exists());

        let s2 = Session::create(t.path(), Manifest::new(None, None, "", false)).unwrap();
        std::fs::write(s2.dir.join("seg-00000.err"), "no").unwrap();
        std::fs::write(s2.dir.join("seg-00000.wav"), "x").unwrap();
        let dir2 = s2.dir.clone();
        s2.finish().unwrap();
        assert!(dir2.exists());
        assert!(Session::open(&dir2).unwrap().manifest.saved);
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
