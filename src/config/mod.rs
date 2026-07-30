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
}
