pub mod provider;

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

/// Default chat chain: local first (free, private), cloud second.
const DEFAULT_CHAT_CHAIN: [&str; 2] = ["ollama", "openrouter"];
/// Default transcribe chain: local first, then the two free-tier cloud options.
const DEFAULT_TRANSCRIBE_CHAIN: [&str; 3] = ["whisper_cpp", "groq", "hf"];

impl Default for Config {
    fn default() -> Self {
        Config::parse(&Config::default_toml())
            .expect("built-in default config must parse")
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config> {
        toml::from_str(text).context("failed to parse leo config")
    }

    /// The config written on first run. Comments explain each knob, because
    /// this file is the primary UI for the model layer.
    pub fn default_toml() -> String {
        format!(
            r#"# leo model configuration.
# Secrets do NOT belong here — run `leo model login <provider>` to store an
# API key in your OS keychain instead.

# Providers are tried in order. Unavailable ones (no key, no binary, closed
# port) are skipped silently, so you can list more than you have installed.
[chat]
chain = [{chat}]

[transcribe]
chain = [{transcribe}]

# kind = "openai" speaks the OpenAI chat-completions protocol, so it covers
# OpenRouter, Ollama, LM Studio, llama.cpp server, vLLM, Groq chat, and more.
[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
# "openrouter/free" is a router across OpenRouter's zero-cost models. Naming
# the router rather than one model survives individual models being retired.
model = "openrouter/free"
key_env = "OPENROUTER_API_KEY"
max_tokens = 4096

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "~/.leo/models/ggml-base.en.bin"

[providers.groq]
kind = "groq"
model = "whisper-large-v3-turbo"
key_env = "GROQ_API_KEY"

[providers.hf]
kind = "hf"
model = "openai/whisper-large-v3-turbo"
key_env = "HF_API_KEY"
"#,
            chat = quoted_list(&DEFAULT_CHAT_CHAIN),
            transcribe = quoted_list(&DEFAULT_TRANSCRIBE_CHAIN),
        )
    }

    /// `~/.config/leo/config.toml`. Deliberately not beside the notes
    /// directory: notes are git-synced by sync.rs, and this file holds
    /// machine-local values (ports, model paths).
    pub fn config_path() -> Result<std::path::PathBuf> {
        let dir = dirs::config_dir()
            .context("could not determine a config directory for this platform")?;
        Ok(dir.join("leo").join("config.toml"))
    }

    /// Load from the standard path. Never fails: a missing or malformed file
    /// falls back to built-in defaults so the app always starts.
    pub fn load() -> Config {
        match Config::config_path() {
            Ok(path) => Config::load_from(&path),
            Err(e) => {
                eprintln!("  config: {e}; using defaults");
                let mut cfg = Config::default();
                cfg.apply_env_overrides();
                cfg
            }
        }
    }

    pub fn load_from(path: &std::path::Path) -> Config {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => match Config::parse(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("  config: {} is invalid ({e}); using defaults", path.display());
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        };
        cfg.apply_env_overrides();
        cfg
    }

    /// Write the commented default config if none exists yet. Returns the path
    /// and whether it was newly created.
    pub fn ensure_exists() -> Result<(std::path::PathBuf, bool)> {
        let path = Config::config_path()?;
        if path.exists() {
            return Ok((path, false));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        std::fs::write(&path, Config::default_toml())
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok((path, true))
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
                    eprintln!(
                        "  config: LEO_CHAT_PROVIDER names unknown provider \"{name}\"; ignoring"
                    );
                }
            }
        }
        if let Ok(name) = std::env::var("LEO_TRANSCRIBE_PROVIDER") {
            let name = name.trim();
            if !name.is_empty() {
                if self.providers.contains_key(name) {
                    self.transcribe.chain = vec![name.to_string()];
                } else {
                    eprintln!(
                        "  config: LEO_TRANSCRIBE_PROVIDER names unknown provider \"{name}\"; ignoring"
                    );
                }
            }
        }
        if let Ok(model) = std::env::var("LEO_CHAT_MODEL") {
            if let Some(first) = self.chat.chain.first().cloned() {
                if let Some(provider) = self.providers.get_mut(&first) {
                    provider.model = Some(model);
                } else {
                    eprintln!(
                        "  config: LEO_CHAT_MODEL set but chat provider \"{first}\" has no [providers] block; ignoring"
                    );
                }
            }
        }
        // Deprecated: honored for one release so existing .env files keep working.
        if let Ok(model) = std::env::var("OPENROUTER_CHAT_MODEL") {
            if let Some(provider) = self.providers.get_mut("openrouter") {
                provider.model = Some(model);
            } else {
                eprintln!(
                    "  config: OPENROUTER_CHAT_MODEL set but there is no \"openrouter\" [providers] block; ignoring"
                );
            }
        }
    }

    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }
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

    #[test]
    fn parses_a_full_config() {
        let toml = r#"
[chat]
chain = ["ollama", "openrouter"]

[transcribe]
chain = ["whisper_cpp", "groq"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "~/.leo/models/ggml-base.en.bin"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.chat.chain, vec!["ollama", "openrouter"]);
        assert_eq!(cfg.transcribe.chain, vec!["whisper_cpp", "groq"]);
        assert_eq!(
            cfg.providers["ollama"].kind,
            Some(ProviderKind::Openai)
        );
        assert_eq!(
            cfg.providers["ollama"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(
            cfg.providers["whisper_cpp"].kind,
            Some(ProviderKind::WhisperCpp)
        );
    }

    #[test]
    fn empty_config_yields_empty_chains() {
        let cfg = Config::parse("").unwrap();
        assert!(cfg.chat.chain.is_empty());
        assert!(cfg.providers.is_empty());
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
    fn defaults_use_only_free_providers() {
        let cfg = Config::default();
        assert_eq!(cfg.chat.chain, vec!["ollama", "openrouter"]);
        assert_eq!(cfg.transcribe.chain, vec!["whisper_cpp", "groq", "hf"]);
        assert_eq!(
            cfg.providers["openrouter"].model.as_deref(),
            Some("openrouter/free")
        );
        // The paid model must not reappear as a default.
        let models: Vec<_> = cfg
            .providers
            .values()
            .filter_map(|p| p.model.as_deref())
            .collect();
        assert!(!models.contains(&"google/gemini-2.5-flash"));
    }

    #[test]
    fn default_config_round_trips_through_toml() {
        let text = Config::default_toml();
        let parsed = Config::parse(&text).unwrap();
        assert_eq!(parsed.chat.chain, Config::default().chat.chain);
        assert_eq!(
            parsed.providers["openrouter"].model,
            Config::default().providers["openrouter"].model
        );
    }

    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        std::env::set_var("LEO_CHAT_PROVIDER", "openrouter");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_PROVIDER");

        assert_eq!(cfg.chat.chain, vec!["openrouter"]);
    }

    #[test]
    fn deprecated_openrouter_chat_model_still_works() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OPENROUTER_CHAT_MODEL", "legacy/model");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("OPENROUTER_CHAT_MODEL");

        assert_eq!(
            cfg.providers["openrouter"].model.as_deref(),
            Some("legacy/model")
        );
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
