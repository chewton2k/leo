use std::path::Path;

use crate::ai_next::error::ProviderResult;

/// One chat completion request, provider-independent.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub prompt: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub trait ChatProvider {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String>;
    /// A cheap, local precondition check: key present, binary on PATH, port
    /// open. Performs no inference and makes no billable call.
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    /// Why this provider is unavailable, for the error shown when a whole
    /// chain is exhausted.
    fn unavailable_reason(&self) -> String {
        format!("{} is not configured", self.name())
    }
    /// This provider's own configured token cap, if it has an opinion.
    /// `None` means "no opinion — use the caller's request value unchanged".
    /// When `Some`, provider config wins: the chain runner overrides the
    /// request's `max_tokens` with this value before calling `complete`, so
    /// e.g. `[providers.ollama] max_tokens = 4096` in leo's config actually
    /// takes effect instead of being silently ignored.
    fn max_tokens(&self) -> Option<u32> {
        None
    }
}

pub trait TranscribeProvider {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String>;
    /// Largest file this provider accepts in one request. `None` means no
    /// limit, which skips chunking entirely — the local-binary case.
    fn max_bytes(&self) -> Option<u64>;
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    fn unavailable_reason(&self) -> String {
        format!("{} is not configured", self.name())
    }
}

pub mod groq;
pub mod hf;
pub mod openai;
pub mod whisper_cpp;

use crate::config::provider::ProviderKind;
use crate::config::secret::{resolve, SecretStore};
use crate::config::Config;

/// Build the chat chain in config order. Entries naming a missing provider
/// block, or a provider whose kind cannot chat, are dropped — a config typo
/// degrades the chain rather than killing the program.
pub fn build_chat_chain(cfg: &Config, store: &dyn SecretStore) -> Vec<Box<dyn ChatProvider>> {
    let mut out: Vec<Box<dyn ChatProvider>> = Vec::new();
    for name in &cfg.chat.chain {
        let Some(pc) = cfg.provider(name) else {
            continue;
        };
        if pc.kind != Some(ProviderKind::Openai) {
            continue;
        }
        let key = resolve(name, pc.key_env.as_deref(), store);
        out.push(Box::new(openai::OpenAiChat::new(name.clone(), pc, key)));
    }
    out
}

/// Build the transcription chain in config order, same dropping policy.
pub fn build_transcribe_chain(
    cfg: &Config,
    store: &dyn SecretStore,
) -> Vec<Box<dyn TranscribeProvider>> {
    let mut out: Vec<Box<dyn TranscribeProvider>> = Vec::new();
    for name in &cfg.transcribe.chain {
        let Some(pc) = cfg.provider(name) else {
            continue;
        };
        let key = resolve(name, pc.key_env.as_deref(), store);
        match pc.kind {
            Some(ProviderKind::Hf) => {
                out.push(Box::new(hf::HfTranscribe::new(name.clone(), pc, key)))
            }
            Some(ProviderKind::Groq) => {
                out.push(Box::new(groq::GroqTranscribe::new(name.clone(), pc, key)))
            }
            Some(ProviderKind::WhisperCpp) => out.push(Box::new(
                whisper_cpp::WhisperCppTranscribe::new(name.clone(), pc),
            )),
            _ => continue,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;

    #[test]
    fn chat_chain_is_built_in_config_order() {
        let cfg = Config::default();
        let store = MemoryStore::default();
        let chain = build_chat_chain(&cfg, &store);
        let names: Vec<_> = chain.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(names, vec!["ollama", "openrouter"]);
    }

    #[test]
    fn transcribe_chain_is_built_in_config_order() {
        let cfg = Config::default();
        let store = MemoryStore::default();
        let chain = build_transcribe_chain(&cfg, &store);
        let names: Vec<_> = chain.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(names, vec!["whisper_cpp", "groq", "hf"]);
    }

    #[test]
    fn a_chain_entry_with_no_provider_block_is_dropped() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["ghost", "real"]

[providers.real]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "m"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        let chain = build_chat_chain(&cfg, &store);
        let names: Vec<_> = chain.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn a_transcribe_kind_in_the_chat_chain_is_dropped() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["groq_whisper"]

[providers.groq_whisper]
kind = "groq"
model = "whisper-large-v3-turbo"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        assert!(build_chat_chain(&cfg, &store).is_empty());
    }

    #[test]
    fn keyless_local_provider_is_available() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["ollama"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        let chain = build_chat_chain(&cfg, &store);
        // No key_env means a local server that needs no credential.
        assert!(chain[0].available());
    }

    #[test]
    fn keyed_provider_becomes_available_once_the_store_has_a_key() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["openrouter"]

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "LEO_TEST_ABSENT_KEY"
"#,
        )
        .unwrap();
        std::env::remove_var("LEO_TEST_ABSENT_KEY");

        let empty = MemoryStore::default();
        assert!(!build_chat_chain(&cfg, &empty)[0].available());

        let stocked = MemoryStore::default();
        stocked.set("openrouter", "a-key").unwrap();
        assert!(build_chat_chain(&cfg, &stocked)[0].available());
    }

    #[test]
    fn local_transcriber_reports_no_size_limit() {
        let cfg = Config::parse(
            r#"
[transcribe]
chain = ["whisper_cpp"]

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "/nonexistent/model.bin"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        let chain = build_transcribe_chain(&cfg, &store);
        assert_eq!(chain[0].max_bytes(), None);
    }
}
