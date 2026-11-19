pub mod choice;
pub mod edit;
pub mod file_store;
pub mod provider;
pub mod secret;
pub mod tidy;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use provider::{ProviderConfig, TaskChain};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub chat: TaskChain,
    #[serde(default)]
    pub transcribe: TaskChain,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
}

static BUILT_IN: std::sync::LazyLock<Result<Config, toml::de::Error>> =
    std::sync::LazyLock::new(|| toml::from_str(&Config::built_in_toml()));

pub const DEFAULT_CHAT_CHAIN: [&str; 1] = ["ollama"];
pub const DEFAULT_TRANSCRIBE_CHAIN: [&str; 1] = ["parakeet"];

impl Default for Config {
    fn default() -> Self {
        Config::parse_with_built_ins(&Config::default_toml())
            .expect("built-in default config must parse")
    }
}

impl Config {
    /// Parse exactly what the text says, with no built-ins added. `load_from`
    /// is what a user's config goes through; this stays literal so a caller can
    /// reason about one file in isolation.
    pub fn parse(text: &str) -> Result<Config> {
        toml::from_str(text).context("failed to parse leo config")
    }

    /// Parse a user's config and add the providers leo ships with.
    pub(crate) fn parse_with_built_ins(text: &str) -> Result<Config> {
        let mut config = Config::parse(text)?;
        config.merge_built_in_providers();
        Ok(config)
    }

    /// Add every provider leo ships with that the file does not already define.
    ///
    /// The file wins on a name collision, so overriding a built-in is a matter
    /// of writing a block with the same name — no need to copy the other
    /// seventeen to keep them.
    fn merge_built_in_providers(&mut self) {
        let built_in = match &*BUILT_IN {
            Ok(config) => config,
            // Unreachable in a shipped binary: a test asserts it parses.
            Err(e) => {
                leo_core::diag::warn(format!("built-in providers are unparsable: {e}"));
                return;
            }
        };
        for (name, provider) in built_in.providers.clone() {
            match self.providers.get_mut(&name) {
                Some(own) => own.fill_from(provider),
                None => {
                    self.providers.insert(name, provider);
                }
            }
        }
    }

    /// The file written on first run: the two lines a user actually tunes, and
    /// a pointer to where everything else lives.
    ///
    /// Short on purpose. Every provider leo supports is available whether or not
    /// it appears here, so the file holds decisions rather than an inventory.
    pub fn default_toml() -> String {
        format!(
            r#"# leo configuration.
#
# Keys do NOT belong in this file. Add them in Settings inside leo.
#
# Open Settings in leo to choose writing: Ollama on this computer, or your signed-in
# Codex or Claude Code CLI.
# Speech uses the local model chosen in Settings → Speech models.
# Settings writes these two lines for you.

[chat]
chain = [{chat}]

[transcribe]
chain = [{transcribe}]

#
# To change the Ollama server or writing model, override only those fields:
#
#   [providers.ollama]
#   base_url = "http://localhost:11434/v1"
#   model = "qwen3:8b"
"#,
            chat = quoted_list(&DEFAULT_CHAT_CHAIN),
            transcribe = quoted_list(&DEFAULT_TRANSCRIBE_CHAIN),
        )
    }

    fn built_in_toml() -> String {
        r#"
[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.openai]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-6-luna"
key_env = "OPENAI_API_KEY"
max_tokens = 32000
reasoning = true

[providers.anthropic]
kind = "openai"
base_url = "https://api.anthropic.com/v1"
model = "claude-sonnet-5-5"
key_env = "ANTHROPIC_API_KEY"
max_tokens = 32000
reasoning = true

[providers.gemini]
kind = "openai"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
model = "gemini-3.8-flash"
key_env = "GEMINI_API_KEY"
max_tokens = 32000

[providers.xai]
kind = "openai"
base_url = "https://api.x.ai/v1"
model = "grok-4.7"
key_env = "XAI_API_KEY"
max_tokens = 32000
reasoning = true

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "OPENROUTER_API_KEY"
max_tokens = 8192

[providers.claude_code]
kind = "claude_code"
model = "claude-sonnet-5-5"

[providers.codex]
kind = "codex"
model = "gpt-6-luna"

[providers.parakeet]
kind = "parakeet"

"#
        .to_string()
    }

    /// `~/.config/leo/config.toml`. Deliberately not beside the notes
    /// directory: this file holds machine-local values (ports, model paths).
    pub fn config_path() -> Result<std::path::PathBuf> {
        Ok(leo_core::paths::config_dir()?.join(leo_core::paths::own::CONFIG))
    }

    /// Load from the standard path. Never fails: a missing or malformed file
    /// falls back to built-in defaults so the app always starts.
    pub fn load() -> Config {
        match Config::config_path() {
            Ok(path) => Config::load_from(&path),
            Err(e) => {
                leo_core::diag::warn(format!("config: {e}; using defaults"));
                let mut cfg = Config::default();
                cfg.apply_env_overrides();
                cfg
            }
        }
    }

    pub fn load_from(path: &std::path::Path) -> Config {
        load_at(path, std::time::Instant::now())
    }

    /// Env vars win over the file, so a one-off override needs no file edit.
    ///
    /// An override that names a provider with no `[providers]` block is left
    /// unapplied (with a warning) rather than fabricating a bare, kind-less
    /// entry: such an entry is unconstructable and gets silently skipped
    /// downstream, turning a typo into a silent no-op instead of a visible one.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(name) = std::env::var("LEO_CHAT_PROVIDER") {
            let name = name.trim();
            if !name.is_empty() {
                if self.providers.contains_key(name) {
                    self.chat.chain = vec![name.to_string()];
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_CHAT_PROVIDER names unknown provider \"{name}\"; ignoring"
                    ));
                }
            }
        }
        if let Ok(name) = std::env::var("LEO_TRANSCRIBE_PROVIDER") {
            let name = name.trim();
            if !name.is_empty() {
                if self.providers.contains_key(name) {
                    self.transcribe.chain = vec![name.to_string()];
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_TRANSCRIBE_PROVIDER names unknown provider \"{name}\"; ignoring"
                    ));
                }
            }
        }
        if let Ok(model) = std::env::var("LEO_CHAT_MODEL") {
            if let Some(first) = self.chat.chain.first().cloned() {
                if let Some(provider) = self.providers.get_mut(&first) {
                    provider.model = Some(model);
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_CHAT_MODEL set but chat provider \"{first}\" has no [providers] block; ignoring"
                    ));
                }
            }
        }
    }

    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    pub fn built_in_provider(name: &str) -> Option<ProviderConfig> {
        BUILT_IN.as_ref().ok()?.providers.get(name).cloned()
    }
}

const RECHECK: std::time::Duration = std::time::Duration::from_secs(10);
const RACY: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    file: (u64, u64),
}

type Held = std::collections::HashMap<std::path::PathBuf, (Stamp, std::time::Instant, Config)>;

static LOADED: std::sync::LazyLock<std::sync::Mutex<Held>> =
    std::sync::LazyLock::new(Default::default);

fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
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

fn load_at(path: &std::path::Path, now: std::time::Instant) -> Config {
    let seen = stamp(path);
    if let Some(seen) = &seen {
        let held = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((kept, at, cfg)) = held.get(path) {
            if kept == seen && now.saturating_duration_since(*at) < RECHECK {
                let mut cfg = cfg.clone();
                cfg.apply_env_overrides();
                return cfg;
            }
        }
    }
    let mut cfg = read_from(path).unwrap_or_default();
    let settled = stamp(path)
        .filter(|after| seen.is_some() && after.modified.elapsed().is_ok_and(|age| age > RACY));
    let mut held = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    match settled {
        Some(after) => {
            held.insert(path.to_path_buf(), (after, now, cfg.clone()));
        }
        None => {
            held.remove(path);
        }
    }
    drop(held);
    cfg.apply_env_overrides();
    cfg
}

pub(crate) fn forget_loaded(path: &std::path::Path) {
    LOADED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
}

fn read_from(path: &std::path::Path) -> Option<Config> {
    match std::fs::read_to_string(path).map(|text| tidied(path, text)) {
        Ok(text) => Some(match Config::parse_with_built_ins(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                leo_core::diag::warn(format!(
                    "config: {} is invalid ({e}); using defaults",
                    path.display()
                ));
                Config::default()
            }
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            leo_core::diag::warn(format!(
                "config: {} could not be read ({e}); using defaults",
                path.display()
            ));
            None
        }
    }
}

fn tidied(path: &std::path::Path, text: String) -> String {
    let Ok(mut doc) = text.parse::<toml_edit::DocumentMut>() else {
        return text;
    };
    if !tidy::tidy(&mut doc) {
        return text;
    }
    let Some(_saving) = edit::saving() else {
        return doc.to_string();
    };
    if std::fs::read_to_string(path).is_ok_and(|now| now == text) {
        let _ = edit::save_document(path, &doc);
    }
    doc.to_string()
}

fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::ProviderKind;

    fn model_text(model: &str) -> String {
        format!("[providers.claude_code]\nmodel = \"{model}\"\n")
    }

    fn model_of(cfg: &Config) -> String {
        cfg.provider("claude_code").unwrap().model.clone().unwrap()
    }

    fn set_age(path: &std::path::Path, seconds: u64) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(seconds))
            .unwrap();
    }

    fn aged_config(dir: &tempfile::TempDir, model: &str) -> std::path::PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, model_text(model)).unwrap();
        set_age(&path, 10);
        path
    }

    #[test]
    fn a_same_stamp_outside_edit_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let path = aged_config(&dir, "model-aaaa");
        assert_eq!(model_of(&Config::load_from(&path)), "model-aaaa");

        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, model_text("model-bbbb")).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        set_age(&path, 10);
        #[cfg(unix)]
        assert_eq!(model_of(&Config::load_from(&path)), "model-bbbb");

        let loaded_before = Config::load_from(&path);
        std::fs::write(&path, model_text("model-cccc")).unwrap();
        set_age(&path, 10);
        {
            let mut held = LOADED.lock().unwrap();
            let entry = held.get_mut(&path).unwrap();
            entry.0 = stamp(&path).unwrap();
        }
        assert_eq!(
            model_of(&Config::load_from(&path)),
            model_of(&loaded_before)
        );
        let later = std::time::Instant::now() + RECHECK * 2;
        assert_eq!(model_of(&load_at(&path, later)), "model-cccc");
    }

    #[test]
    fn a_config_written_inside_two_seconds_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, model_text("model-aaaa")).unwrap();
        assert_eq!(model_of(&Config::load_from(&path)), "model-aaaa");
        std::fs::write(&path, model_text("model-bbbb")).unwrap();
        assert_eq!(model_of(&Config::load_from(&path)), "model-bbbb");
    }

    #[test]
    fn a_config_saved_by_settings_is_seen_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = aged_config(&dir, "model-aaaa");
        assert_eq!(model_of(&Config::load_from(&path)), "model-aaaa");
        let doc = model_text("model-bbbb")
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        edit::save_document(&path, &doc).unwrap();
        set_age(&path, 10);
        assert_eq!(model_of(&Config::load_from(&path)), "model-bbbb");
    }

    #[test]
    fn two_config_paths_do_not_share_an_entry() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let one = aged_config(&first, "model-aaaa");
        let two = aged_config(&second, "model-bbbb");
        assert_eq!(model_of(&Config::load_from(&one)), "model-aaaa");
        assert_eq!(model_of(&Config::load_from(&two)), "model-bbbb");
        assert_eq!(model_of(&Config::load_from(&one)), "model-aaaa");
    }

    #[test]
    fn parses_a_full_config() {
        let toml = r#"
[chat]
chain = ["ollama", "anthropic"]

[transcribe]
chain = ["parakeet"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.parakeet]
kind = "parakeet"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.chat.chain, vec!["ollama", "anthropic"]);
        assert_eq!(cfg.transcribe.chain, vec!["parakeet"]);
        assert_eq!(cfg.providers["ollama"].kind, Some(ProviderKind::Openai));
        assert_eq!(
            cfg.providers["ollama"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(cfg.providers["parakeet"].kind, Some(ProviderKind::Parakeet));
    }

    /// An empty file means "no opinion about chains", not "no providers": every
    /// provider leo ships with is still known, which is what lets the shipped
    /// file stay short.
    #[test]
    fn an_empty_config_has_no_chains_but_still_knows_every_provider() {
        // `load_from` applies env overrides, and other tests set them; without
        // this the chain can arrive non-empty depending on interleaving.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.chat.chain.is_empty());
        assert_eq!(cfg.providers.len(), 9, "{:?}", cfg.providers.keys());
        assert!(cfg.providers.contains_key("ollama"));
        assert!(cfg.providers.contains_key("parakeet"));
        assert!(!cfg.providers.contains_key("agy"));
        assert!(!cfg.providers.contains_key("copilot"));
    }

    /// The file the user gets must be short enough to read. The inventory of
    /// providers lives in code precisely so this stays small.
    #[test]
    fn the_shipped_config_is_short() {
        let text = Config::default_toml();
        let lines = text.lines().count();
        assert!(lines < 40, "the shipped config grew to {lines} lines");
        // And it must not have turned back into an inventory. The commented
        // example of how to add one is fine; a real block is not.
        let real_blocks = text
            .lines()
            .filter(|l| l.trim_start().starts_with("[providers."))
            .count();
        assert_eq!(
            real_blocks, 0,
            "provider blocks are back in the shipped file"
        );
        // It still has to be a working config.
        let cfg = Config::parse(&text).unwrap();
        assert_eq!(cfg.chat.chain, DEFAULT_CHAT_CHAIN);
        assert_eq!(cfg.transcribe.chain, DEFAULT_TRANSCRIBE_CHAIN);
    }

    /// A user's own block must win over the built-in of the same name, so
    /// overriding one provider does not mean copying the other seventeen.
    #[test]
    fn a_user_block_overrides_the_built_in_of_the_same_name() {
        let cfg = Config::parse_with_built_ins(
            r#"
[chat]
chain = ["ollama"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:9999/v1"
model = "my-own-model"
"#,
        )
        .unwrap();
        let ollama = cfg.provider("ollama").expect("ollama");
        assert_eq!(ollama.model.as_deref(), Some("my-own-model"));
        assert_eq!(
            ollama.base_url.as_deref(),
            Some("http://localhost:9999/v1"),
            "the built-in overwrote the user's block"
        );
        assert!(cfg.providers.contains_key("anthropic"));
    }

    #[test]
    fn a_block_naming_only_a_model_keeps_the_rest_of_the_built_in() {
        let cfg =
            Config::parse_with_built_ins("[providers.openai]\nmodel = \"gpt-6-luna\"\n").unwrap();
        let openai = cfg.provider("openai").unwrap();
        assert_eq!(openai.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(openai.kind, Some(ProviderKind::Openai));
        assert_eq!(
            openai.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(openai.key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(openai.reasoning, Some(true));
    }

    #[test]
    fn a_block_of_another_kind_under_a_built_in_name_takes_nothing_from_it() {
        let cfg = Config::parse_with_built_ins(
            "[providers.gemini]\nkind = \"codex\"\nbase_url = \"http://localhost:8000/v1\"\n",
        )
        .unwrap();
        let gemini = cfg.provider("gemini").unwrap();
        assert_eq!(gemini.kind, Some(ProviderKind::Codex));
        assert_eq!(gemini.key_env, None);
        assert_eq!(gemini.model, None);
    }

    /// The built-in table has to parse, since every load path merges it.
    #[test]
    fn the_built_in_provider_table_parses() {
        let built_in: Config = toml::from_str(&Config::built_in_toml()).unwrap();
        let mut names: Vec<&String> = built_in.providers.keys().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "anthropic",
                "claude_code",
                "codex",
                "gemini",
                "ollama",
                "openai",
                "openrouter",
                "parakeet",
                "xai",
            ]
        );
    }

    #[test]
    fn an_old_theme_table_is_ignored() {
        let cfg =
            Config::parse("[chat]\nchain = [\"ollama\"]\n[theme]\npreset = \"mono\"\n").unwrap();
        assert_eq!(cfg.chat.chain, ["ollama"]);
    }

    #[test]
    fn malformed_toml_is_an_error_naming_the_problem() {
        let err = Config::parse("[chat\nchain = ]").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("config"), "unhelpful error: {msg}");
    }

    #[test]
    fn unknown_provider_kind_is_an_error() {
        let toml = r#"
[providers.weird]
kind = "telepathy"
"#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn defaults_run_on_this_computer() {
        let cfg = Config::default();
        assert_eq!(cfg.chat.chain, vec!["ollama"]);
        assert_eq!(cfg.transcribe.chain, vec!["parakeet"]);
        assert_eq!(cfg.providers["parakeet"].bin, None);
    }

    #[test]
    fn a_file_naming_removed_kinds_still_loads_without_them() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[chat]
chain = ["codex"]
[transcribe]
chain = ["mine", "w"]
[providers.mine]
kind = "groq"
[providers.w]
kind = "whisper_cpp"
model_path = "/m.bin"
[providers.a]
kind = "chat_audio"
[providers.t]
kind = "openai_transcribe"
[theme]
preset = "mono"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, ["codex"]);
        assert_eq!(cfg.transcribe.chain, ["parakeet"]);
        for name in ["mine", "w", "a", "t"] {
            assert!(cfg.provider(name).is_none(), "{name}");
        }
    }

    /// The shipped config is the primary UI for the model layer, so every
    /// provider in it must be usable as written, not just parseable.
    #[test]
    fn every_shipped_provider_is_complete_enough_to_build() {
        let cfg = Config::default();
        for (name, p) in &cfg.providers {
            let kind = p.kind.unwrap_or_else(|| panic!("{name} has no kind"));
            match kind {
                ProviderKind::Openai => {
                    assert!(p.base_url.is_some(), "{name} has no base_url");
                    assert!(p.model.is_some(), "{name} has no model");
                    // A remote endpoint without a key_env could never
                    // authenticate; a local one must not demand a key.
                    let local = p
                        .base_url
                        .as_deref()
                        .map(|u| u.contains("localhost") || u.contains("127.0.0.1"))
                        .unwrap_or(false);
                    assert_eq!(
                        p.key_env.is_none(),
                        local,
                        "{name}: key_env presence should match whether it is local"
                    );
                }
                ProviderKind::ClaudeCode | ProviderKind::Codex => {
                    assert!(
                        p.key_env.is_none(),
                        "{name} signs in through its own program"
                    );
                    assert!(p.base_url.is_none(), "{name} is a program, not a server");
                }
                ProviderKind::Parakeet => {
                    assert!(p.key_env.is_none(), "{name} runs on this computer");
                }
            }
        }
    }

    /// Every provider named in a chain must exist, or the chain silently
    /// shortens and the user gets a mysterious "no provider available".
    #[test]
    fn every_chain_entry_names_a_defined_provider() {
        let cfg = Config::default();
        for name in cfg.chat.chain.iter().chain(cfg.transcribe.chain.iter()) {
            assert!(
                cfg.providers.contains_key(name),
                "chain names unknown {name}"
            );
        }
    }

    /// The defaults must stay free. A paid provider is offered in the file but
    /// never wired into a chain without the user asking.
    #[test]
    fn no_paid_provider_is_enabled_by_default() {
        let cfg = Config::default();
        for paid in ["openai", "anthropic", "gemini", "xai"] {
            assert!(
                cfg.providers.contains_key(paid),
                "{paid} should be offered in the file"
            );
            assert!(
                !cfg.chat.chain.contains(&paid.to_string())
                    && !cfg.transcribe.chain.contains(&paid.to_string()),
                "{paid} bills, so it must not be in a default chain"
            );
        }
    }

    /// Two providers sharing one key_env is fine and intentional (groq chat and
    /// groq transcription), but a typo'd variable name is not detectable later,
    /// so pin the spellings.
    #[test]
    fn key_env_names_follow_the_provider_convention() {
        let cfg = Config::default();
        for (name, p) in &cfg.providers {
            if let Some(var) = &p.key_env {
                assert!(
                    var.chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                    "{name}: {var} is not a conventional env var name"
                );
                assert!(
                    var.ends_with("_API_KEY"),
                    "{name}: {var} should end in _API_KEY"
                );
            }
        }
    }

    /// An hour of lecture makes a long note; cloud models can write one, and
    /// a 4096-token cap cut it off mid-sentence. Local servers keep the lower
    /// cap, since small local models have small contexts.
    #[test]
    fn cloud_chat_providers_allow_long_notes_and_local_ones_stay_modest() {
        let cfg = Config::default();
        for thinking in ["openai", "anthropic", "gemini", "xai"] {
            assert_eq!(
                cfg.provider(thinking).and_then(|p| p.max_tokens),
                Some(32000),
                "{thinking}"
            );
        }
        assert_eq!(
            cfg.provider("ollama").and_then(|p| p.max_tokens),
            Some(4096)
        );
    }

    #[test]
    fn the_shipped_file_explains_local_speech_and_writing_choices() {
        let text = Config::default_toml();
        // The file is the documentation, so these have to be present.
        assert!(text.contains("Settings inside leo"));
        assert!(text.contains("[providers.ollama]"));
        assert!(!text.contains("Antigravity"));
        assert!(!text.contains("Copilot"));
        assert!(text.contains("Speech uses the local model chosen in Settings → Speech models"));
        assert!(text.contains("[chat]"));
        assert!(text.contains("[transcribe]"));
        // And it must warn that keys do not belong in it.
        assert!(text.to_lowercase().contains("keys do not belong"));
    }

    #[test]
    fn default_config_round_trips_through_toml() {
        let text = Config::default_toml();
        let parsed = Config::parse_with_built_ins(&text).unwrap();
        assert_eq!(parsed.chat.chain, Config::default().chat.chain);
        assert_eq!(
            parsed.providers["anthropic"].model,
            Config::default().providers["anthropic"].model
        );
    }

    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    pub(super) static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn load_from_missing_path_returns_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml"));
        assert_eq!(cfg.chat.chain, Config::default().chat.chain);
    }

    #[test]
    fn load_from_malformed_file_falls_back_to_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat\nbroken").unwrap();
        // Must not panic and must not refuse to start.
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, Config::default().chat.chain);
    }

    #[test]
    fn file_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat]\nchain = [\"only_this\"]\n").unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, vec!["only_this"]);
    }

    #[test]
    fn leo_chat_model_overrides_first_chain_provider() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_MODEL", "some/other-model");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_MODEL");

        let first = &cfg.chat.chain[0];
        assert_eq!(
            cfg.providers[first].model.as_deref(),
            Some("some/other-model")
        );
    }

    #[test]
    fn leo_chat_provider_replaces_the_chain() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_PROVIDER", "anthropic");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_PROVIDER");

        assert_eq!(cfg.chat.chain, vec!["anthropic"]);
    }

    #[test]
    fn leo_chat_provider_naming_unknown_provider_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_PROVIDER", "typo_provider");
        let mut cfg = Config::default();
        let original_chain = cfg.chat.chain.clone();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_PROVIDER");

        assert_eq!(cfg.chat.chain, original_chain);
        assert!(!cfg.providers.contains_key("typo_provider"));
    }

    #[test]
    fn leo_chat_model_for_provider_without_block_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut cfg = Config::default();
        cfg.chat.chain = vec!["no_such_provider".to_string()];
        std::env::set_var("LEO_CHAT_MODEL", "some/other-model");
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_MODEL");

        assert!(!cfg.providers.contains_key("no_such_provider"));
    }
}
