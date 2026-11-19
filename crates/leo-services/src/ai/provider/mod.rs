use std::path::Path;

use crate::ai::error::ProviderResult;

/// One chat completion request, provider-independent.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Standing instructions, sent as a system message when present.
    pub system: Option<String>,
    /// The material to work on, sent as the user's message.
    pub prompt: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// Somewhere to send text as it arrives.
///
/// A callback rather than a channel so a provider stays independent of how the
/// caller wants to display things — the TUI feeds a worker channel, the CLI
/// prints, and tests collect into a string.
pub type Sink<'a> = &'a mut dyn FnMut(&str);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spent {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input: u64,
    pub output: u64,
    pub estimated: bool,
    pub cached: u64,
    pub cache_written: u64,
}

pub(crate) const NO_MORE_TOOLS: &str = "No more tools are available for this answer. Use the results already returned and answer the user now. Use interpretable language.";

pub(crate) fn tokens_of(chars: usize) -> u64 {
    chars.div_ceil(4) as u64
}

impl Spent {
    pub fn guessed(model: Option<String>, req: &ChatRequest, answer: &str) -> Spent {
        let asked =
            req.system.as_deref().map_or(0, |s| s.chars().count()) + req.prompt.chars().count();
        Spent {
            model,
            effort: None,
            input: tokens_of(asked),
            output: tokens_of(answer.chars().count()),
            estimated: true,
            ..Spent::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl Image {
    pub fn data_url(&self) -> String {
        use base64::Engine;
        format!(
            "data:{};base64,{}",
            self.mime,
            base64::engine::general_purpose::STANDARD.encode(&self.bytes)
        )
    }

    pub fn extension(&self) -> &'static str {
        match self.mime.as_str() {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "jpg",
        }
    }
}

pub trait ChatProvider {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String>;

    /// Complete, calling `sink` with each fragment as it arrives.
    ///
    /// The default answers in one piece once the whole response is in, so a
    /// provider that cannot stream still works everywhere streaming is used —
    /// a local binary, for instance, has nothing to stream.
    fn complete_streaming(&self, req: &ChatRequest, sink: Sink<'_>) -> ProviderResult<String> {
        let text = self.complete(req)?;
        sink(&text);
        Ok(text)
    }
    fn complete_until(
        &self,
        req: &ChatRequest,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        if stopped() {
            return Err(crate::ai::error::ProviderError::Fatal(
                "The answer was stopped.".into(),
            ));
        }
        self.complete(req)
    }
    fn complete_streaming_until(
        &self,
        req: &ChatRequest,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        if stopped() {
            return Err(crate::ai::error::ProviderError::Fatal(
                "The answer was stopped.".into(),
            ));
        }
        self.complete_streaming(req, sink)
    }
    /// A cheap, local precondition check: key present, binary on PATH, port
    /// open. Performs no inference and makes no billable call.
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    fn spent(&self) -> Option<Spent> {
        None
    }
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
    fn complete_with_images(
        &self,
        _req: &ChatRequest,
        _images: &[Image],
    ) -> ProviderResult<String> {
        Err(crate::ai::error::ProviderError::Fatal(format!(
            "{} cannot read images; for photos and scans choose Claude Code or Codex for writing",
            self.name()
        )))
    }
}

pub trait TranscribeProvider {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String>;
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    fn unavailable_reason(&self) -> String {
        format!("{} is not configured", self.name())
    }
}

pub mod agent_cli;
pub mod audio;
pub mod codex_app;
pub mod local_speech;
pub mod mcp;
pub mod openai;
pub(crate) mod process;
pub mod scoped_cli;

use crate::config::provider::ProviderKind;
use crate::config::secret::{resolve, SecretStore};
use crate::config::Config;

/// Build the chat chain in config order. Entries naming a missing provider
/// block, or a provider whose kind cannot chat, are dropped — a config typo
/// degrades the chain rather than killing the program.
///
/// `resolve` (a keychain read) runs only for providers that name a `key_env`.
/// A local server like Ollama needs no credential, so reading the store for it
/// is pure cost: with an unusable backend it prints a warning per entry per
/// invocation for a value that would be discarded anyway. Mirrors the same
/// rule in `build_transcribe_chain`.
pub fn build_chat_chain(cfg: &Config, store: &dyn SecretStore) -> Vec<Box<dyn ChatProvider>> {
    cfg.chat
        .chain
        .iter()
        .filter_map(|name| build_one_chat(name, cfg.provider(name)?, store))
        .collect()
}

pub fn build_one_chat(
    name: &str,
    pc: &crate::config::provider::ProviderConfig,
    store: &dyn SecretStore,
) -> Option<Box<dyn ChatProvider>> {
    match pc.kind {
        Some(ProviderKind::Openai) => {
            let key = match pc.key_env.as_deref() {
                Some(_) => resolve(pc.account(name), store),
                None => None,
            };
            Some(Box::new(openai::OpenAiChat::new(name.to_string(), pc, key)))
        }
        Some(ProviderKind::ClaudeCode | ProviderKind::Codex) => {
            let agent = agent_cli::Agent::of(pc)?;
            Some(Box::new(agent_cli::AgentCli::new(
                name.to_string(),
                agent,
                pc,
            )))
        }
        _ => None,
    }
}

/// Build the transcription chain in config order, same dropping policy. Only
/// local speech remains, so no credential is read.
pub fn build_transcribe_chain(
    cfg: &Config,
    _store: &dyn SecretStore,
) -> Vec<Box<dyn TranscribeProvider>> {
    let mut out: Vec<Box<dyn TranscribeProvider>> = Vec::new();
    for name in &cfg.transcribe.chain {
        let Some(pc) = cfg.provider(name) else {
            continue;
        };
        match pc.kind {
            Some(ProviderKind::Parakeet) => {
                out.push(Box::new(local_speech::LocalSpeech::new(name.clone(), pc)))
            }
            _ => continue,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{MemoryStore, Secret};
    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them, the
    /// same pattern `config::secret`'s own tests use.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A `SecretStore` double that counts `get` calls, to prove a provider
    /// that needs no credential never triggers a keychain read.
    #[derive(Default)]
    struct CountingStore {
        gets: std::cell::Cell<u32>,
    }

    impl SecretStore for CountingStore {
        fn get(&self, _account: &str) -> anyhow::Result<Option<Secret>> {
            self.gets.set(self.gets.get() + 1);
            Ok(None)
        }
        fn set(&self, _account: &str, _secret: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete(&self, _account: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn available(&self) -> bool {
            true
        }
    }

    #[test]
    fn chat_chain_is_built_in_config_order() {
        // A chain order that disagrees with alphabetical order, so this test
        // actually exercises "config order" rather than incidentally passing
        // because it happens to match `BTreeMap`'s key order too.
        let cfg = Config::parse(
            r#"
[chat]
chain = ["zzz_provider", "aaa_provider"]

[providers.zzz_provider]
kind = "openai"
base_url = "http://localhost:1/v1"
model = "m"

[providers.aaa_provider]
kind = "openai"
base_url = "http://localhost:2/v1"
model = "m"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        let chain = build_chat_chain(&cfg, &store);
        let names: Vec<_> = chain.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(names, vec!["zzz_provider", "aaa_provider"]);
    }

    #[test]
    fn transcribe_chain_is_built_in_config_order() {
        let cfg = Config::default();
        let store = MemoryStore::default();
        let chain = build_transcribe_chain(&cfg, &store);
        let names: Vec<_> = chain.iter().map(|p| p.name().to_string()).collect();
        assert_eq!(names, vec!["parakeet"]);
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
    fn a_speech_kind_in_the_chat_chain_is_dropped() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["local"]

[providers.local]
kind = "parakeet"
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
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_ABSENT_KEY");

        let empty = MemoryStore::default();
        assert!(!build_chat_chain(&cfg, &empty)[0].available());

        let stocked = MemoryStore::default();
        stocked.set("openrouter", "a-key").unwrap();
        assert!(build_chat_chain(&cfg, &stocked)[0].available());
    }

    /// The chat side of the same rule. A keyless local server in the chat
    /// chain must not trigger a keychain read either — on a machine with an
    /// unusable backend that read only produces a warning about a credential
    /// the provider would never use.
    #[test]
    fn keyless_chat_provider_never_touches_the_secret_store() {
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
        let store = CountingStore::default();
        let chain = build_chat_chain(&cfg, &store);
        assert_eq!(chain.len(), 1);
        assert_eq!(
            store.gets.get(),
            0,
            "a chat provider with no key_env must never read the secret store"
        );
    }

    /// ...and a chat provider that *does* name a key_env must still read it.
    #[test]
    fn keyed_chat_provider_does_read_the_secret_store() {
        let cfg = Config::parse(
            r#"
[chat]
chain = ["openrouter"]

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "LEO_TEST_UNSET_KEY_ENV"
"#,
        )
        .unwrap();
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_UNSET_KEY_ENV");
        let store = CountingStore::default();
        let chain = build_chat_chain(&cfg, &store);
        assert_eq!(chain.len(), 1);
        assert_eq!(store.gets.get(), 1);
    }
}
