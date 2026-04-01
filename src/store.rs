use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::notes::Note;

const DATA_FILE: &str = "notes.json";

#[derive(Serialize, Deserialize)]
struct StoreData {
    notes: Vec<Note>,
    #[serde(default)]
    directories: Vec<String>,
}

/// Persistent store backed by a single JSON file in the user's data directory.
pub struct Store {
    pub notes: Vec<Note>,
    pub directories: Vec<String>,
    path: PathBuf,
}

impl Store {
    /// Load notes from disk (creates an empty store if the file doesn't exist yet).
    pub fn load() -> Result<Self> {
        let path = data_path()?;
        let (notes, directories) = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            if raw.trim_start().starts_with('[') {
                // Legacy format: plain array of notes
                let notes: Vec<Note> = serde_json::from_str(&raw)
                    .with_context(|| format!("Failed to parse {}", path.display()))?;
                (notes, Vec::new())
            } else {
                let data: StoreData = serde_json::from_str(&raw)
                    .with_context(|| format!("Failed to parse {}", path.display()))?;
                (data.notes, data.directories)
            }
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(Store {
            notes,
            directories,
            path,
        })
    }

    /// Persist notes to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = StoreData {
            notes: self.notes.clone(),
            directories: self.directories.clone(),
        };
        let json = serde_json::to_string_pretty(&data)?;
        fs::write(&self.path, json)
            .with_context(|| format!("Failed to write {}", self.path.display()))?;
        Ok(())
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

    /// Find a note by numeric index (1-based into the default list), full id, or unique prefix.
    pub fn find_by_index_or_prefix(&self, input: &str) -> Option<&Note> {
        // Try as 1-based numeric index into the default sorted list
        if let Ok(n) = input.parse::<usize>() {
            let list = self.list_notes(None, 20);
            if n >= 1 && n <= list.len() {
                return Some(list[n - 1]);
            }
        }
        // Fall back to ID prefix
        self.find_note(input)
    }

    /// Mutable version of find_by_index_or_prefix.
    pub fn find_by_index_or_prefix_mut(&mut self, input: &str) -> Option<&mut Note> {
        // Try as 1-based numeric index into the default sorted list
        if let Ok(n) = input.parse::<usize>() {
            let list = self.list_notes(None, 20);
            if n >= 1 && n <= list.len() {
                let id = list[n - 1].id.clone();
                return self.notes.iter_mut().find(|note| note.id == id);
            }
        }
        // Fall back to ID prefix
        self.find_note_mut(input)
    }

    /// Find a note by full id or unique prefix.
    pub fn find_note(&self, id_prefix: &str) -> Option<&Note> {
        let matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.id.starts_with(id_prefix))
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    /// Find a mutable note by full id or unique prefix.
    pub fn find_note_mut(&mut self, id_prefix: &str) -> Option<&mut Note> {
        let mut found = self
            .notes
            .iter_mut()
            .filter(|n| n.id.starts_with(id_prefix));
        let first = found.next()?;
        if found.next().is_none() {
            Some(first)
        } else {
            None
        }
    }

    /// Delete a note by id prefix; returns true if a note was removed.
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
                if full_text {
                    n.matches_full_text(query)
                } else {
                    n.matches_title(query)
                }
            })
            .collect()
    }

    /// Return all tags with their usage count, sorted by most-used first.
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

    /// Create a directory and all parent directories. Returns true if any were created.
    pub fn create_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return false;
        }
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

    /// Check if a directory exists.
    pub fn dir_exists(&self, path: &str) -> bool {
        if path.is_empty() {
            return true; // root always exists
        }
        self.directories.contains(&path.to_string())
    }

    /// Return immediate subdirectory names for a parent directory.
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
                    if !d.contains('/') {
                        Some(d.clone())
                    } else {
                        None
                    }
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

    /// Delete an empty directory. Returns true if deleted.
    pub fn delete_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        let prefix = format!("{path}/");
        let has_notes = self
            .notes
            .iter()
            .any(|n| n.directory == path || n.directory.starts_with(&prefix));
        let has_subdirs = self
            .directories
            .iter()
            .any(|d| d.starts_with(&prefix));

        if has_notes || has_subdirs {
            return false;
        }

        let before = self.directories.len();
        self.directories.retain(|d| d != path);
        self.directories.len() < before
    }

    /// Move a note to a different directory.
    pub fn move_note(&mut self, id_prefix: &str, new_dir: &str) -> Option<String> {
        let note = self.find_note_mut(id_prefix)?;
        note.directory = new_dir.to_string();
        note.updated_at = Utc::now();
        Some(note.title.clone())
    }
}

/// Returns the path to the notes JSON file:
///   macOS:   ~/Library/Application Support/leo/notes.json
///   Linux:   ~/.local/share/leo/notes.json
///   Windows: %APPDATA%\leo\notes.json
fn data_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Could not determine user data directory")?;
    Ok(base.join("leo").join(DATA_FILE))
}
