use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::notes::Note;

const DATA_FILE: &str = "notes.json";

/// Persistent store backed by a single JSON file in the user's data directory.
pub struct Store {
    pub notes: Vec<Note>,
    path: PathBuf,
}

impl Store {
    /// Load notes from disk (creates an empty store if the file doesn't exist yet).
    pub fn load() -> Result<Self> {
        let path = data_path()?;
        let notes = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?
        } else {
            Vec::new()
        };
        Ok(Store { notes, path })
    }

    /// Persist notes to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.notes)?;
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
    ) -> Result<&Note> {
        let note = Note::new(title, body, tags);
        self.notes.push(note);
        Ok(self.notes.last().unwrap())
    }

    /// Return notes sorted newest-first, optionally filtered by tag.
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

    /// Find a note by full id or unique prefix.
    pub fn find_note(&self, id_prefix: &str) -> Option<&Note> {
        let matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.id.starts_with(id_prefix))
            .collect();
        if matches.len() == 1 { Some(matches[0]) } else { None }
    }

    /// Find a mutable note by full id or unique prefix.
    pub fn find_note_mut(&mut self, id_prefix: &str) -> Option<&mut Note> {
        let mut found = self
            .notes
            .iter_mut()
            .filter(|n| n.id.starts_with(id_prefix));
        let first = found.next()?;
        if found.next().is_none() { Some(first) } else { None }
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

    /// Update a note's body and bump its updated_at timestamp.
    pub fn update_body(&mut self, id_prefix: &str, new_body: String) -> bool {
        match self.find_note_mut(id_prefix) {
            Some(note) => {
                note.body = new_body;
                note.updated_at = Utc::now();
                true
            }
            None => false,
        }
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
