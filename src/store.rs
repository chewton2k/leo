use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::notes::Note;

// ── Frontmatter serialization ───────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct NoteFrontmatter {
    id: String,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn note_to_markdown(note: &Note) -> Result<String> {
    let fm = NoteFrontmatter {
        id: note.id.clone(),
        title: note.title.clone(),
        tags: note.tags.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
    };
    let yaml = serde_yaml::to_string(&fm)?;
    Ok(format!("---\n{}---\n\n{}", yaml, note.body))
}

fn parse_note_from_markdown(content: &str, relative_path: &Path) -> Result<Note> {
    let rest = content
        .strip_prefix("---\n")
        .context("note file missing opening ---")?;
    let end = rest.find("\n---\n").context("note file missing closing ---")?;
    let yaml_str = &rest[..end];
    let body = rest[end + 5..].trim_start_matches('\n').to_string();

    let fm: NoteFrontmatter =
        serde_yaml::from_str(yaml_str).context("failed to parse frontmatter")?;

    let directory = relative_path
        .parent()
        .and_then(|p| if p == Path::new("") { None } else { p.to_str() })
        .unwrap_or("")
        .to_string();

    Ok(Note {
        id: fm.id,
        title: fm.title,
        body,
        tags: fm.tags,
        directory,
        created_at: fm.created_at,
        updated_at: fm.updated_at,
    })
}

// ── Filesystem helpers ──────────────────────────────────────────────────────

/// notes directory: ~/Library/Application Support/leo/notes  (macOS)
fn notes_dir_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Could not determine user data directory")?;
    Ok(base.join("leo").join("notes"))
}

/// Legacy notes.json path — used only for migration detection.
fn old_data_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Could not determine user data directory")?;
    Ok(base.join("leo").join("notes.json"))
}

fn load_directories(notes_dir: &Path) -> Result<Vec<String>> {
    let path = notes_dir.join("directories.json");
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    } else {
        Ok(Vec::new())
    }
}

fn save_directories(notes_dir: &Path, directories: &[String]) -> Result<()> {
    fs::write(
        notes_dir.join("directories.json"),
        serde_json::to_string_pretty(directories)?,
    )?;
    Ok(())
}

/// Collect absolute paths of all .md files under `dir`, skipping hidden dirs.
fn collect_md_paths(dir: &Path, result: &mut HashSet<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') {
                collect_md_paths(&path, result)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            result.insert(path);
        }
    }
    Ok(())
}

/// Recursively parse all .md files under `dir` into `notes`.
fn collect_notes(notes_dir: &Path, dir: &Path, notes: &mut Vec<Note>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') {
                collect_notes(notes_dir, &path, notes)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let content = fs::read_to_string(&path)?;
            let relative = path.strip_prefix(notes_dir).context("path outside notes_dir")?;
            match parse_note_from_markdown(&content, relative) {
                Ok(note) => notes.push(note),
                Err(e) => eprintln!("warn: skipping {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

// ── Store ───────────────────────────────────────────────────────────────────

/// Persistent store backed by per-note .md files in a directory.
pub struct Store {
    pub notes: Vec<Note>,
    pub directories: Vec<String>,
    pub notes_dir: PathBuf,
}

impl Store {
    /// Load notes from the platform data directory.
    /// Automatically migrates from legacy notes.json on first run.
    pub fn load() -> Result<Self> {
        let notes_dir = notes_dir_path()?;
        let old_path = old_data_path()?;
        // Treat notes_dir as empty if it doesn't exist, or if it exists but
        // contains no .md files (handles the case where sync init ran first
        // and created notes_dir with .git/ before migration could fire).
        let notes_dir_empty = !notes_dir.exists() || {
            let mut md_paths = std::collections::HashSet::new();
            collect_md_paths(&notes_dir, &mut md_paths).unwrap_or(());
            md_paths.is_empty()
        };
        if old_path.exists() && notes_dir_empty {
            migrate_from_json(&old_path, &notes_dir)?;
        }
        Self::load_from(&notes_dir)
    }

    /// Load notes from a specific directory. Used directly in tests.
    pub fn load_from(notes_dir: &Path) -> Result<Self> {
        fs::create_dir_all(notes_dir)?;
        let directories = load_directories(notes_dir)?;
        let mut notes = Vec::new();
        collect_notes(notes_dir, notes_dir, &mut notes)?;
        Ok(Store {
            notes,
            directories,
            notes_dir: notes_dir.to_path_buf(),
        })
    }

    /// Persist notes to disk with full reconcile (writes new, deletes removed).
    /// All `.md` files in `notes_dir` are owned by the store — any file not
    /// corresponding to a current note will be deleted.
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.notes_dir)?;

        // Snapshot existing .md paths before writing
        let mut old_paths: HashSet<PathBuf> = HashSet::new();
        collect_md_paths(&self.notes_dir, &mut old_paths)?;

        // Write all current notes
        let mut new_paths: HashSet<PathBuf> = HashSet::new();
        for note in &self.notes {
            let file_path = self.note_path(note);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, note_to_markdown(note)?)?;
            new_paths.insert(file_path);
        }

        // Delete files no longer in the notes vec
        for old_path in &old_paths {
            if !new_paths.contains(old_path) {
                fs::remove_file(old_path)?;
            }
        }

        save_directories(&self.notes_dir, &self.directories)?;

        // Auto-commit if git repo is initialized (non-fatal — notes are saved regardless)
        if crate::sync::is_initialized(&self.notes_dir) {
            if let Err(e) = crate::sync::auto_commit(&self.notes_dir) {
                eprintln!("warn: sync auto-commit failed: {e}");
            }
        }

        Ok(())
    }

    /// Returns the expected .md file path for a note.
    /// Assumes `note.directory` is a clean relative path with no `..` components.
    fn note_path(&self, note: &Note) -> PathBuf {
        if note.directory.is_empty() {
            self.notes_dir.join(format!("{}.md", note.id))
        } else {
            self.notes_dir
                .join(&note.directory)
                .join(format!("{}.md", note.id))
        }
    }

    /// Create and store a new note, returning a reference to it.
    pub fn create_note(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        tags: Vec<String>,
        directory: &str,
    ) -> Result<&Note> {
        let note = Note::new(title, body, tags, directory);
        self.notes.push(note);
        Ok(self.notes.last().unwrap())
    }

    /// Return notes sorted newest-first, optionally filtered by tag.
    /// Searches across ALL directories.
    pub fn list_notes(&self, tag: Option<&str>, limit: usize) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| match tag {
                Some(t) => n.tags.iter().any(|tag| tag == t),
                None => true,
            })
            .collect();
        notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        notes.truncate(limit);
        notes
    }

    /// Return notes in a specific directory, sorted newest-first.
    pub fn list_notes_in_dir(&self, dir: &str, tag: Option<&str>, limit: usize) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.directory == dir)
            .filter(|n| match tag {
                Some(t) => n.tags.iter().any(|tag| tag == t),
                None => true,
            })
            .collect();
        notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        notes.truncate(limit);
        notes
    }

    /// Find a note by numeric index, full ID, unique prefix, or title.
    pub fn find_by_index_or_prefix(&self, input: &str) -> Option<&Note> {
        if let Ok(n) = input.parse::<usize>() {
            let list = self.list_notes(None, 20);
            if n >= 1 && n <= list.len() {
                return Some(list[n - 1]);
            }
        }
        if let Some(note) = self.find_note(input) {
            return Some(note);
        }
        let title_matches = self.find_by_title(input);
        if title_matches.len() == 1 {
            return Some(title_matches[0]);
        }
        None
    }

    /// Mutable version of find_by_index_or_prefix.
    pub fn find_by_index_or_prefix_mut(&mut self, input: &str) -> Option<&mut Note> {
        let id = self.find_by_index_or_prefix(input)?.id.clone();
        self.notes.iter_mut().find(|note| note.id == id)
    }

    /// Find notes whose title contains the query (case-insensitive), newest first.
    pub fn find_by_title(&self, query: &str) -> Vec<&Note> {
        let q = query.to_lowercase();
        let mut matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.title.to_lowercase().contains(&q))
            .collect();
        matches.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        matches
    }

    /// Find a note by full ID or unique prefix.
    pub fn find_note(&self, id_prefix: &str) -> Option<&Note> {
        let matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.id.starts_with(id_prefix))
            .collect();
        if matches.len() == 1 { Some(matches[0]) } else { None }
    }

    /// Find a mutable note by full ID or unique prefix.
    pub fn find_note_mut(&mut self, id_prefix: &str) -> Option<&mut Note> {
        let mut found = self.notes.iter_mut().filter(|n| n.id.starts_with(id_prefix));
        let first = found.next()?;
        if found.next().is_none() { Some(first) } else { None }
    }

    /// Delete a note by ID prefix; returns true if removed.
    pub fn delete_note(&mut self, id_prefix: &str) -> bool {
        let before = self.notes.len();
        self.notes.retain(|n| !n.id.starts_with(id_prefix));
        self.notes.len() < before
    }

    /// Search notes. If `full_text` is false, only title is searched.
    pub fn search(&self, query: &str, full_text: bool) -> Vec<&Note> {
        self.notes
            .iter()
            .filter(|n| {
                if full_text { n.matches_full_text(query) } else { n.matches_title(query) }
            })
            .collect()
    }

    /// Return all tags with usage counts, sorted most-used first.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for note in &self.notes {
            for tag in &note.tags {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1));
        tags
    }

    /// Find the most recently updated note with a given tag (mutable).
    pub fn find_by_tag_mut(&mut self, tag: &str) -> Option<&mut Note> {
        let idx = self
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.tags.iter().any(|t| t == tag))
            .max_by_key(|(_, n)| n.updated_at)
            .map(|(i, _)| i)?;
        Some(&mut self.notes[idx])
    }

    /// Toggle the Nth checkbox in a note. Returns the new state text.
    pub fn toggle_checkbox(&mut self, id_prefix: &str, n: usize) -> Option<String> {
        let note = self.find_note_mut(id_prefix)?;
        note.toggle_checkbox(n)
    }

    // ── Directory operations ───────────────────────────────────────────────

    pub fn create_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        if path.is_empty() { return false; }
        let mut created = false;
        let parts: Vec<&str> = path.split('/').collect();
        for i in 0..parts.len() {
            let dir = parts[..=i].join("/");
            if !self.directories.contains(&dir) {
                self.directories.push(dir);
                created = true;
            }
        }
        created
    }

    pub fn dir_exists(&self, path: &str) -> bool {
        if path.is_empty() { return true; }
        self.directories.contains(&path.to_string())
    }

    pub fn subdirs(&self, parent: &str) -> Vec<String> {
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let mut dirs: Vec<String> = self
            .directories
            .iter()
            .filter_map(|d| {
                if parent.is_empty() {
                    if !d.contains('/') { Some(d.clone()) } else { None }
                } else if let Some(rest) = d.strip_prefix(&prefix) {
                    if !rest.is_empty() && !rest.contains('/') {
                        Some(rest.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    pub fn delete_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        let prefix = format!("{path}/");
        let has_notes = self
            .notes
            .iter()
            .any(|n| n.directory == path || n.directory.starts_with(&prefix));
        let has_subdirs = self.directories.iter().any(|d| d.starts_with(&prefix));
        if has_notes || has_subdirs { return false; }
        let before = self.directories.len();
        self.directories.retain(|d| d != path);
        self.directories.len() < before
    }

    pub fn move_note(&mut self, id_prefix: &str, new_dir: &str) -> Option<String> {
        let note = self.find_note_mut(id_prefix)?;
        note.directory = new_dir.to_string();
        note.updated_at = Utc::now();
        Some(note.title.clone())
    }
}

// ── Migration ───────────────────────────────────────────────────────────────

fn migrate_from_json(old_path: &Path, notes_dir: &Path) -> Result<()> {
    let raw = fs::read_to_string(old_path)
        .with_context(|| format!("failed to read {}", old_path.display()))?;

    #[derive(serde::Deserialize)]
    struct LegacyStore {
        notes: Vec<Note>,
        #[serde(default)]
        directories: Vec<String>,
    }

    let (notes, directories): (Vec<Note>, Vec<String>) = if raw.trim_start().starts_with('[') {
        let notes: Vec<Note> = serde_json::from_str(&raw)?;
        (notes, Vec::new())
    } else {
        let data: LegacyStore = serde_json::from_str(&raw)?;
        (data.notes, data.directories)
    };

    let count = notes.len();
    fs::create_dir_all(notes_dir)?;

    for note in &notes {
        let file_path = if note.directory.is_empty() {
            notes_dir.join(format!("{}.md", note.id))
        } else {
            let dir = notes_dir.join(&note.directory);
            fs::create_dir_all(&dir)?;
            dir.join(format!("{}.md", note.id))
        };
        fs::write(file_path, note_to_markdown(note)?)?;
    }

    save_directories(notes_dir, &directories)?;

    fs::rename(old_path, old_path.with_extension("json.bak"))?;

    println!("Migrated {count} notes to {}", notes_dir.display());
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;

    fn make_note() -> Note {
        Note {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            title: "Test Note".to_string(),
            body: "Hello **world**".to_string(),
            tags: vec!["rust".to_string(), "test".to_string()],
            directory: "".to_string(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }

    #[test]
    fn test_note_roundtrip() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed =
            parse_note_from_markdown(&md, std::path::Path::new("550e8400.md")).unwrap();
        assert_eq!(parsed.id, note.id);
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.body, note.body);
        assert_eq!(parsed.tags, note.tags);
        assert_eq!(parsed.directory, "");
    }

    #[test]
    fn test_directory_derived_from_path() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed = parse_note_from_markdown(
            &md,
            std::path::Path::new("cs162/lec/550e8400.md"),
        )
        .unwrap();
        assert_eq!(parsed.directory, "cs162/lec");
    }

    #[test]
    fn test_note_path_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        let store = Store { notes: vec![], directories: vec![], notes_dir: notes_dir.clone() };
        let note = make_note();
        assert_eq!(
            store.note_path(&note),
            notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md")
        );
    }

    #[test]
    fn test_note_path_subdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        let store = Store { notes: vec![], directories: vec![], notes_dir: notes_dir.clone() };
        let mut note = make_note();
        note.directory = "cs162/lec".to_string();
        assert_eq!(
            store.note_path(&note),
            notes_dir.join("cs162/lec/550e8400-e29b-41d4-a716-446655440000.md")
        );
    }

    #[test]
    fn test_load_from_reads_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let note = make_note();
        std::fs::write(
            notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md"),
            note_to_markdown(&note).unwrap(),
        )
        .unwrap();

        let mut note2 = make_note();
        note2.id = "aaaabbbb-0000-0000-0000-000000000000".to_string();
        note2.title = "Subdir Note".to_string();
        note2.directory = "cs162".to_string();
        std::fs::create_dir_all(notes_dir.join("cs162")).unwrap();
        std::fs::write(
            notes_dir.join("cs162/aaaabbbb-0000-0000-0000-000000000000.md"),
            note_to_markdown(&note2).unwrap(),
        )
        .unwrap();

        let store = Store::load_from(&notes_dir).unwrap();
        assert_eq!(store.notes.len(), 2);
        let loaded = store.notes.iter().find(|n| n.id == note.id).unwrap();
        assert_eq!(loaded.directory, "");
        assert_eq!(loaded.title, "Test Note");
        assert_eq!(loaded.body, "Hello **world**");
        let loaded2 = store.notes.iter().find(|n| n.id == note2.id).unwrap();
        assert_eq!(loaded2.directory, "cs162");
        assert_eq!(loaded2.title, "Subdir Note");
    }

    #[test]
    fn test_save_writes_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let store = Store {
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
    }

    #[test]
    fn test_save_deletes_orphaned_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let orphan = notes_dir.join("deadbeef-0000-0000-0000-000000000000.md");
        std::fs::write(&orphan, "---\nid: deadbeef-0000-0000-0000-000000000000\ntitle: Old\ntags: []\ncreated_at: '2026-01-01T00:00:00Z'\nupdated_at: '2026-01-01T00:00:00Z'\n---\n\nbody").unwrap();

        let store = Store {
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();

        assert!(!orphan.exists(), "orphaned file should be deleted");
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
    }

    #[test]
    fn test_migrate_from_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("notes.json");
        let json = serde_json::json!({
            "notes": [{
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "title": "Migrated Note",
                "body": "content",
                "tags": ["rust"],
                "directory": "",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z"
            }],
            "directories": []
        });
        std::fs::write(&old_path, serde_json::to_string(&json).unwrap()).unwrap();

        let notes_dir = tmp.path().join("notes");
        assert!(!notes_dir.exists());

        migrate_from_json(&old_path, &notes_dir).unwrap();

        assert!(notes_dir.exists());
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
        assert!(!old_path.exists(), "notes.json should be renamed");
        assert!(tmp.path().join("notes.json.bak").exists());
    }

    #[test]
    fn test_save_handles_directory_move() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let note = make_note();
        let store = Store {
            notes: vec![note.clone()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());

        let mut moved = note.clone();
        moved.directory = "ideas".to_string();
        let store2 = Store {
            notes: vec![moved],
            directories: vec!["ideas".to_string()],
            notes_dir: notes_dir.clone(),
        };
        store2.save().unwrap();

        assert!(notes_dir.join("ideas/550e8400-e29b-41d4-a716-446655440000.md").exists());
        assert!(
            !notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists(),
            "old location should be removed after move"
        );
    }

    #[test]
    fn test_save_auto_commits_when_initialized() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        crate::sync::init(&notes_dir).unwrap();

        let store = Store {
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();

        let log = std::process::Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&notes_dir)
            .output()
            .unwrap();
        let log_str = String::from_utf8(log.stdout).unwrap();
        assert!(
            log_str.contains("update notes"),
            "expected auto-commit, got: {log_str}"
        );
    }
}
