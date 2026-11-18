//! Editing `config.toml` in place, without throwing away the user's file.
//!
//! The shipped config is mostly comments — which provider is free, how to add
//! your own, where to get a key. Serializing the parsed `Config` back out would
//! silently delete all of it, so edits go through `toml_edit`, which preserves
//! comments, ordering, and formatting and touches only the value being changed.

use anyhow::{Context, Result};
use toml_edit::{Array, DocumentMut, Item, Value};

/// Which chain an edit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Chat,
    Transcribe,
}

impl Task {
    pub fn table(self) -> &'static str {
        match self {
            Task::Chat => "chat",
            Task::Transcribe => "transcribe",
        }
    }
}

/// Read the chain as it appears in the file.
pub fn read_chain(doc: &DocumentMut, task: Task) -> Vec<String> {
    doc.get(task.table())
        .and_then(|t| t.get("chain"))
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Replace one chain, creating the table if the user deleted it.
pub fn write_chain(doc: &mut DocumentMut, task: Task, chain: &[String]) {
    let mut array = Array::new();
    for name in chain {
        array.push(name.as_str());
    }
    // Keep it on one line, the way the shipped file has it.
    array.set_trailing_comma(false);

    let table = doc
        .entry(task.table())
        .or_insert(Item::Table(toml_edit::Table::new()));
    if let Some(table) = table.as_table_like_mut() {
        table.insert("chain", Item::Value(Value::Array(array)));
    }
}

static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    static HOLDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct Saving {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Drop for Saving {
    fn drop(&mut self) {
        HOLDING.set(false);
    }
}

pub(crate) fn saving() -> Option<Saving> {
    if HOLDING.get() {
        return None;
    }
    let guard = SAVING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    HOLDING.set(true);
    Some(Saving { _guard: guard })
}

/// Write a document back, atomically enough that a crash mid-write cannot leave
/// a truncated config: the new text lands in a sibling file first, then
/// replaces the original in one rename.
pub fn save_document(path: &std::path::Path, doc: &DocumentMut) -> Result<()> {
    leo_core::paths::write_atomic(path, doc.to_string())
        .with_context(|| format!("could not replace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a config to a temp file so it can be read back through the real
    /// load path, which is what merges in the providers leo ships with.
    fn write_temp(text: &str) -> std::path::PathBuf {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn doc() -> DocumentMut {
        crate::config::Config::default_toml()
            .parse::<DocumentMut>()
            .unwrap()
    }

    #[test]
    fn the_shipped_config_is_valid_toml_for_editing() {
        let d = doc();
        assert_eq!(read_chain(&d, Task::Chat), vec!["ollama"]);
        assert_eq!(read_chain(&d, Task::Transcribe), vec!["parakeet"]);
    }

    #[test]
    fn rewriting_an_inline_chain_preserves_its_other_fields() {
        let mut doc: DocumentMut = r#"chat = { chain = ["agy"], custom = "keep" }
transcribe = { chain = ["openai_whisper"] }
"#
        .parse()
        .unwrap();
        write_chain(&mut doc, Task::Chat, &["codex".to_string()]);
        assert_eq!(read_chain(&doc, Task::Chat), ["codex"]);
        assert_eq!(read_chain(&doc, Task::Transcribe), ["openai_whisper"]);
        assert_eq!(doc["chat"]["custom"].as_str(), Some("keep"));
    }

    /// The whole reason for `toml_edit`: an edit must not cost the user the
    /// comments that explain the file.
    #[test]
    fn rewriting_a_chain_preserves_comments_and_the_rest_of_the_file() {
        let mut d = doc();
        let before = d.to_string();
        assert!(before.contains("Keys do NOT belong in this file"));

        write_chain(&mut d, Task::Chat, &["xai".to_string()]);
        let after = d.to_string();

        assert!(after.contains("chain = [\"xai\"]"), "{after}");
        // Comments survive.
        assert!(after.contains("Keys do NOT belong in this file"));
        assert!(after.contains("Open Settings in leo"), "{after}");
        // And the transcribe chain is untouched.
        assert_eq!(read_chain(&d, Task::Transcribe), vec!["parakeet"]);
    }

    /// A chain may name a provider that has no block in the file, because most
    /// providers are built in. Writing such a chain must still produce a config
    /// that resolves it.
    #[test]
    fn a_chain_can_name_a_provider_absent_from_the_file() {
        let mut d = doc();
        write_chain(&mut d, Task::Chat, &["claude_code".to_string()]);
        let text = d.to_string();
        assert!(
            !text.contains("[providers.claude_code]"),
            "claude_code should not need a block"
        );

        let cfg = crate::config::Config::load_from(&write_temp(&text));
        assert_eq!(cfg.chat.chain, vec!["claude_code"]);
        assert!(
            cfg.provider("claude_code").is_some(),
            "a chained built-in did not resolve"
        );
    }

    #[test]
    fn a_rewritten_chain_reads_back_and_still_parses_as_config() {
        let mut d = doc();
        write_chain(
            &mut d,
            Task::Chat,
            &["openrouter".to_string(), "ollama".to_string()],
        );
        assert_eq!(read_chain(&d, Task::Chat), vec!["openrouter", "ollama"]);

        let cfg = crate::config::Config::parse(&d.to_string()).unwrap();
        assert_eq!(cfg.chat.chain, vec!["openrouter", "ollama"]);
    }

    #[test]
    fn an_empty_chain_is_writable_so_a_user_can_disable_a_task() {
        let mut d = doc();
        write_chain(&mut d, Task::Chat, &[]);
        assert!(read_chain(&d, Task::Chat).is_empty());
        assert!(crate::config::Config::parse(&d.to_string()).is_ok());
    }

    #[test]
    fn a_missing_table_is_created_rather_than_erroring() {
        let mut d = "# just a comment\n".parse::<DocumentMut>().unwrap();
        assert!(read_chain(&d, Task::Chat).is_empty());
        write_chain(&mut d, Task::Chat, &["ollama".to_string()]);
        assert_eq!(read_chain(&d, Task::Chat), vec!["ollama"]);
        assert!(d.to_string().contains("# just a comment"));
    }

    #[test]
    fn saving_replaces_the_file_without_leaving_the_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat]\nchain = [\"old\"]\n").unwrap();

        let mut d = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        write_chain(&mut d, Task::Chat, &["new".to_string()]);
        save_document(&path, &d).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"new\""));
        assert!(!text.contains("\"old\""));
        assert!(
            !path.with_extension("toml.new").exists(),
            "temp file left behind"
        );
    }
}
