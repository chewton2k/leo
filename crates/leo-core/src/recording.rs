use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MOST_SOURCES: usize = 1000;
pub const MOST_WANTS_CHARS: usize = 4000;
pub const MOST_CONTEXT_CHARS: usize = 8000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Profile {
    pub context: String,
    pub wants: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub speakers: bool,
}

impl Profile {
    pub fn checked(self) -> Result<Profile> {
        anyhow::ensure!(
            self.wants.chars().count() <= MOST_WANTS_CHARS,
            "Keep what you want from the notes under 4,000 characters"
        );
        anyhow::ensure!(
            self.context.chars().count() <= MOST_CONTEXT_CHARS,
            "Keep the meeting details under 8,000 characters"
        );
        Ok(Profile {
            context: self.context.trim().to_string(),
            wants: self.wants.trim().to_string(),
            speakers: self.speakers,
        })
    }
}
const MOST_SOURCE_BYTES: u64 = 64_000_000;

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

pub fn valid_id(id: &str) -> bool {
    (8..=80).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn root(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("recording-sources")
}

fn folder(notes: &Path, note: &str) -> Result<PathBuf> {
    if !valid_id(note) {
        bail!("Invalid recording note id");
    }
    crate::paths::contained_path(
        notes.parent().unwrap_or(notes),
        &Path::new("recording-sources").join(note),
    )
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing parent folder"))?;
    if let Ok(meta) = std::fs::symlink_metadata(parent) {
        anyhow::ensure!(
            !meta.file_type().is_symlink(),
            "Recording files cannot use symbolic links"
        );
    }
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    crate::paths::write_atomic(path, serde_json::to_vec_pretty(value)?)
}

pub fn save(notes: &Path, note: &str, source: &Archive) -> Result<()> {
    if !valid_id(&source.id) {
        bail!("Invalid recording id");
    }
    let dir = folder(notes, note)?;
    let path = crate::paths::contained_path(&dir, Path::new(&format!("{}.json", source.id)))?;
    write_json(&path, source)
}

pub fn copy_to(notes: &Path, from: &str, into: &str, label: &str) -> Result<Vec<String>> {
    let have: std::collections::HashSet<String> =
        load(notes, into)?.into_iter().map(|s| s.id).collect();
    let mut copied = Vec::new();
    for mut source in load(notes, from)? {
        if have.contains(&source.id) {
            continue;
        }
        if source.from.is_empty() {
            source.from = label.chars().take(200).collect();
        }
        save(notes, into, &source)?;
        copied.push(source.id);
    }
    Ok(copied)
}

pub fn remove(notes: &Path, note: &str, ids: &[String]) -> Result<()> {
    let dir = folder(notes, note)?;
    for id in ids {
        if !valid_id(id) {
            bail!("Invalid recording id");
        }
        let path = crate::paths::contained_path(&dir, Path::new(&format!("{id}.json")))?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

pub fn load(notes: &Path, note: &str) -> Result<Vec<Archive>> {
    let dir = folder(notes, note)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut bytes = 0u64;
    for (index, entry) in std::fs::read_dir(&dir)?.take(MOST_SOURCES + 1).enumerate() {
        anyhow::ensure!(
            index < MOST_SOURCES,
            "Too many recording sources in this note"
        );
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json") {
            bytes = bytes.saturating_add(entry.metadata()?.len());
            anyhow::ensure!(
                bytes <= MOST_SOURCE_BYTES,
                "This note's recording sources exceed the size limit"
            );
            match std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Archive>(&bytes).ok())
            {
                Some(source) if valid_id(&source.id) => out.push(source),
                _ => crate::diag::warn(format!(
                    "skipped an unreadable recording source: {}",
                    entry.path().display()
                )),
            }
        }
    }
    out.sort_by_key(|s| s.started);
    Ok(out)
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
        assert!(on.speakers && on.clone().checked().unwrap().speakers);
        assert!(!serde_json::from_str::<Profile>("{}").unwrap().speakers);
    }

    #[test]
    fn what_the_user_wants_is_bounded_and_trimmed() {
        let p = Profile {
            context: " Design review ".into(),
            wants: " Just the decisions ".into(),
            ..Default::default()
        }
        .checked()
        .unwrap();
        assert_eq!(
            (p.context.as_str(), p.wants.as_str()),
            ("Design review", "Just the decisions")
        );
        assert!(Profile {
            wants: "x".repeat(MOST_WANTS_CHARS + 1),
            ..Default::default()
        }
        .checked()
        .is_err());
        let old: Profile = serde_json::from_str(
            r#"{"template":"meeting","vocabulary":["Dijkstra"],"context":"c"}"#,
        )
        .unwrap();
        assert_eq!(
            old.context, "c",
            "manifests written before formats were removed still load"
        );
    }

    #[test]
    fn sources_are_private_durable_and_repeatable() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let source = Archive {
            id: "session-123".into(),
            started: Utc::now(),
            passages: vec![],
            points: vec![],
            context: String::new(),
            wants: String::new(),
            warnings: vec![],
            trace: vec![],
            from: String::new(),
        };
        save(&notes, "note-1234", &source).unwrap();
        save(&notes, "note-1234", &source).unwrap();
        assert_eq!(load(&notes, "note-1234").unwrap().len(), 1);
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
        let both = load(&notes, "note-1234").unwrap();
        assert_eq!(
            both.iter()
                .map(|s| (s.id.as_str(), s.from.as_str()))
                .collect::<Vec<_>>(),
            [("session-123", ""), ("session-456", "Lecture 2")]
        );
        assert_eq!(
            load(&notes, "note-5678").unwrap().len(),
            1,
            "the original keeps its own copy"
        );
        remove(&notes, "note-1234", &["session-456".into()]).unwrap();
        remove(&notes, "note-1234", &["session-456".into()]).unwrap();
        assert_eq!(load(&notes, "note-1234").unwrap().len(), 1);
        assert!(remove(&notes, "note-1234", &["../x".into()]).is_err());
    }
    #[test]
    fn a_damaged_source_file_is_skipped_and_the_others_load() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let good = Archive {
            id: "good-0001".into(),
            started: Utc::now(),
            passages: vec![],
            points: vec![],
            context: String::new(),
            wants: String::new(),
            warnings: vec![],
            trace: vec![],
            from: String::new(),
        };
        save(&notes, "note-0001", &good).unwrap();
        let broken = folder(&notes, "note-0001")
            .unwrap()
            .join("broken-0001.json");
        std::fs::write(&broken, "{").unwrap();
        let loaded = load(&notes, "note-0001").unwrap();
        assert_eq!(
            loaded.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["good-0001"]
        );
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{");
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
}
