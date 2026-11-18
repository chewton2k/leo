pub mod budget;
pub mod chain;
pub mod chat;
pub mod error;
pub mod live;
pub mod long;
pub mod provider;
pub mod session;
pub mod spend;
pub mod transcribe;

use std::path::Path;

use anyhow::Result;

use crate::config::Config;

/// Token budget for full note structuring.
/// Room for a long lecture's notes, for a provider with no cap of its own.
pub const STRUCTURE_MAX_TOKENS: u32 = 8192;

pub fn tidy_rooms() {
    tidy_rooms_in(&std::env::temp_dir());
}

fn tidy_rooms_in(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with("leo-writing-") || name.starts_with("leo-usage-"))
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && crate::providers::older_than(&entry, std::time::Duration::from_secs(86_400))
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

pub fn stop_writing() {
    provider::process::stop_writing();
}

pub fn writing_budget() -> budget::Budget {
    budget::writing_budget(&crate::config::Config::load())
}

pub fn open_session(
    systems: &session::Systems,
    tools: &[session::ToolDef],
    light: bool,
) -> Option<Box<dyn session::Session>> {
    let (cfg, store) = context();
    let cfg = if light { quick_config(&cfg) } else { cfg };
    session::open(&cfg, store.as_ref(), systems, tools)
}

pub fn felix_room() -> usize {
    budget::felix_room(&crate::config::Config::load())
}

pub fn felix_context() -> usize {
    budget::felix_context(&crate::config::Config::load())
}
fn context() -> (Config, Box<dyn crate::config::secret::SecretStore>) {
    (Config::load(), crate::config::secret::default_store())
}

/// Resolve every credential the AI chains might need, discarding the values.
///
/// Called before live transcription starts. Reading a credential is not free:
/// the OS keychain can take a long time to answer — over a hundred seconds when
/// macOS decides to ask permission — and paying that inside the rolling loop
/// stalls it completely, with nothing on screen to say why. Paying it up front
/// costs nothing the user notices, because the recorder is already running and
/// no audio is lost, and the read is cached for the rest of the process.
pub fn warm_credentials() {
    warm(
        &crate::config::Config::load(),
        crate::config::secret::default_store().as_ref(),
    );
}

fn warm(config: &Config, store: &dyn crate::config::secret::SecretStore) {
    let names = config
        .transcribe
        .chain
        .iter()
        .chain(config.chat.chain.iter());
    for name in names {
        if let Some(provider) = config.provider(name).filter(|p| p.key_env.is_some()) {
            let _ = crate::config::secret::resolve(provider.account(name), store);
        }
    }
}

pub fn transcribe_outcome(audio_path: &Path) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    transcribe::run(&cfg, &store, audio_path)
}

/// One chat completion through the configured chain, without printing.
pub fn chat_outcome(prompt: chat::Prompt, max_tokens: u32) -> Result<chain::ChainOutcome<String>> {
    outcome_with(Config::clone, prompt, max_tokens)
}

pub fn chat_outcome_until(
    prompt: chat::Prompt,
    max_tokens: u32,
    stopped: &dyn Fn() -> bool,
) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    chat::complete_until(&cfg, &store, prompt, max_tokens, stopped)
}

fn outcome_with(
    derive: fn(&Config) -> Config,
    prompt: chat::Prompt,
    max_tokens: u32,
) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    chat::complete(&derive(&cfg), &store, prompt, max_tokens)
}

pub fn quick_config(cfg: &Config) -> Config {
    let mut quick = cfg.clone();
    for name in &cfg.chat.chain {
        let Some(choice) = crate::config::choice::WRITING
            .iter()
            .find(|c| c.provider == name.as_str())
        else {
            continue;
        };
        let Some(model) = choice.models.first() else {
            continue;
        };
        let mut provider = cfg
            .provider(name)
            .cloned()
            .or_else(|| Config::built_in_provider(name));
        if let Some(provider) = provider.as_mut() {
            provider.model = Some(model.id.to_string());
            provider.effort = crate::config::choice::efforts(name, model.id)
                .first()
                .map(|e| e.to_string());
            quick.providers.insert(name.clone(), provider.clone());
        }
    }
    quick
}

fn lighter_in(cfg: &Config) -> bool {
    let quick = quick_config(cfg);
    let tier = |c: &Config, name: &str| {
        c.provider(name)
            .map(|p| (p.model.clone(), p.effort.clone()))
    };
    cfg.chat
        .chain
        .first()
        .is_some_and(|name| tier(cfg, name) != tier(&quick, name))
}

pub fn lighter_model_exists() -> bool {
    lighter_in(&Config::load())
}

pub fn quick_outcome(prompt: chat::Prompt, max_tokens: u32) -> Result<chain::ChainOutcome<String>> {
    outcome_with(quick_config, prompt, max_tokens)
}

pub fn toolless_outcome(
    prompt: chat::Prompt,
    max_tokens: u32,
) -> Result<chain::ChainOutcome<String>> {
    outcome_with(toolless_config, prompt, max_tokens)
}

pub fn toolless_streaming(
    prompt: chat::Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
    stopped: &dyn Fn() -> bool,
) -> Result<(String, Option<spend::Answered>)> {
    streaming_with(
        |cfg| without_tools(cfg.clone(), cfg),
        prompt,
        max_tokens,
        on_fragment,
        on_restart,
        stopped,
    )
}

fn without_tools(mut chosen: Config, cfg: &Config) -> Config {
    for name in &cfg.chat.chain {
        let provider = chosen
            .provider(name)
            .cloned()
            .or_else(|| Config::built_in_provider(name));
        if let Some(mut provider) = provider {
            provider.no_tools = true;
            chosen.providers.insert(name.clone(), provider);
        }
    }
    chosen
}

fn toolless_config(cfg: &Config) -> Config {
    let mut quick = without_tools(quick_config(cfg), cfg);
    quick.chat.chain.truncate(1);
    quick
}

pub(crate) fn loopback(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback() || address.is_unspecified())
}

pub fn reads_stay_here(cfg: &Config) -> bool {
    let quick = toolless_config(cfg);
    let Some(provider) = quick.chat.chain.first().and_then(|n| quick.provider(n)) else {
        return false;
    };
    let cloud = provider
        .model
        .as_deref()
        .is_some_and(|model| model.to_ascii_lowercase().contains("cloud"));
    provider.kind == Some(crate::config::provider::ProviderKind::Openai)
        && provider.base_url.as_deref().is_some_and(loopback)
        && !cloud
}

fn dictation_config(cfg: Config, quick: bool) -> Config {
    let chosen = if quick {
        quick_config(&cfg)
    } else {
        cfg.clone()
    };
    without_tools(chosen, &cfg)
}

pub fn dictation_outcome(
    prompt: chat::Prompt,
    max_tokens: u32,
    quick: bool,
    stopped: &dyn Fn() -> bool,
) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    let cfg = dictation_config(cfg, quick);
    chat::complete_until(&cfg, &*store, prompt, max_tokens, stopped)
}

pub fn writing_ready() -> bool {
    let (cfg, store) = context();
    crate::health::writing(&cfg, &*store, None).state == crate::health::WritingState::Ready
}

pub use leo_core::NO_WRITING_AI;

pub fn writing_error(err: &anyhow::Error) -> String {
    let (cfg, store) = context();
    let writing = crate::health::writing(&cfg, &*store, None);
    let agent = writing
        .provider
        .as_deref()
        .and_then(|name| cfg.provider(name))
        .and_then(provider::agent_cli::Agent::of)
        .map(|agent| agent.kind());
    why(writing.state, agent, &err.to_string())
}

fn why(state: crate::health::WritingState, agent: Option<&str>, text: &str) -> String {
    use crate::health::WritingState::*;
    let name = agent.unwrap_or("your writing AI");
    let reason = match state {
        NotChosen => "no writing AI is chosen.".to_string(),
        NotRunning => "no writing AI is ready (Ollama isn't running).".to_string(),
        NoModel => "no writing AI is ready (Ollama has no models yet).".to_string(),
        NotInstalled => format!("{name} is not installed on this computer."),
        SignInNeeded => format!("{name} needs you to sign in."),
        Ready => {
            let lower = text.to_lowercase();
            if ["not signed in", "not logged in", "/login", "codex login"]
                .iter()
                .any(|marker| lower.contains(marker))
            {
                format!("{name} needs you to sign in.")
            } else {
                return leo_core::writing_said(text);
            }
        }
    };
    format!("{NO_WRITING_AI}{reason}")
}

pub fn see(prompt: chat::Prompt, images: &[provider::Image], max_tokens: u32) -> Result<String> {
    let (cfg, store) = context();
    let req = prompt.request(0.2, max_tokens);
    let providers = provider::build_chat_chain(&cfg, &store);
    Ok(chain::run_vision_chain(providers, &req, images)?.value)
}

pub fn quick_streaming(
    prompt: chat::Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
    stopped: &dyn Fn() -> bool,
) -> Result<(String, Option<spend::Answered>)> {
    streaming_with(
        quick_config,
        prompt,
        max_tokens,
        on_fragment,
        on_restart,
        stopped,
    )
}

fn streaming_with(
    derive: fn(&Config) -> Config,
    prompt: chat::Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
    stopped: &dyn Fn() -> bool,
) -> Result<(String, Option<spend::Answered>)> {
    let (cfg, store) = context();
    let outcome = chat::complete_streaming(
        &derive(&cfg),
        &store,
        prompt,
        max_tokens,
        on_fragment,
        on_restart,
        stopped,
    )?;
    let answered = outcome
        .spent
        .map(|spent| spend::answered(&cfg, &outcome.provider, spent));
    Ok((outcome.value, answered))
}

pub fn chat_streaming(
    prompt: chat::Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
    stopped: &dyn Fn() -> bool,
) -> Result<(String, Option<spend::Answered>)> {
    streaming_with(
        Config::clone,
        prompt,
        max_tokens,
        on_fragment,
        on_restart,
        stopped,
    )
}

#[cfg(test)]
mod tests {
    use crate::config::provider::{ProviderConfig, ProviderKind};

    #[derive(Default)]
    struct Reads(std::cell::RefCell<Vec<String>>);

    impl crate::config::secret::SecretStore for Reads {
        fn get(&self, account: &str) -> anyhow::Result<Option<crate::config::secret::Secret>> {
            self.0.borrow_mut().push(account.to_string());
            Ok(None)
        }
        fn set(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn available(&self) -> bool {
            true
        }
    }

    #[test]
    fn warming_reads_only_credentials_a_chain_provider_uses() {
        let mut config =
            crate::config::Config::parse_with_built_ins("[transcribe]\nchain = [\"parakeet\"]\n")
                .unwrap();
        config.chat.chain = ["claude_code", "ollama", "keyed"]
            .map(String::from)
            .to_vec();
        config.providers.insert(
            "keyed".into(),
            ProviderConfig {
                kind: Some(ProviderKind::Openai),
                key_env: Some("KEYED_API_KEY".into()),
                ..ProviderConfig::default()
            },
        );
        let store = Reads::default();
        super::warm(&config, &store);
        assert_eq!(*store.0.borrow(), ["keyed"]);
    }

    #[test]
    fn only_old_private_work_folders_are_swept_from_the_temp_folder() {
        let temp = tempfile::tempdir().unwrap();
        let aged = |name: &str, old: bool| {
            let path = temp.path().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("prompt.txt"), "kept private").unwrap();
            if old {
                let mut open = std::fs::File::options();
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    open.write(true).custom_flags(0x0200_0000);
                }
                #[cfg(not(windows))]
                open.read(true);
                open.open(&path)
                    .unwrap()
                    .set_modified(
                        std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86_400),
                    )
                    .unwrap();
            }
            path
        };
        let stale = [aged("leo-writing-old", true), aged("leo-usage-old", true)];
        let kept = [aged("leo-writing-new", false), aged("someone-else", true)];
        super::tidy_rooms_in(temp.path());
        assert!(stale.iter().all(|path| !path.exists()));
        assert!(kept.iter().all(|path| path.exists()));
    }

    fn chained(first: &str) -> crate::config::Config {
        let mut cfg = crate::config::Config::default();
        cfg.chat.chain = vec![
            first.to_string(),
            "claude_code".to_string(),
            "codex".to_string(),
        ];
        cfg
    }

    #[test]
    fn a_writing_failure_names_one_plain_reason_after_the_prefix() {
        use crate::health::WritingState::*;
        assert_eq!(
            super::why(NotRunning, None, "x"),
            "leo can't write yet: no writing AI is ready (Ollama isn't running)."
        );
        assert_eq!(
            super::why(
                Ready,
                Some("Claude Code"),
                "claude ended. If it is not signed in, run it"
            ),
            "leo can't write yet: Claude Code needs you to sign in."
        );
        assert_eq!(
            super::why(SignInNeeded, Some("Codex"), "x"),
            "leo can't write yet: Codex needs you to sign in."
        );
        assert_eq!(
            super::why(Ready, None, "openai returned 429: slow down\nsecond line"),
            "Your writing AI didn't answer just now (busy). Try again in a moment."
        );
        assert_eq!(
            super::why(Ready, None, "claude: request timed out"),
            "Your writing AI didn't answer just now (timed out). Try again in a moment."
        );
        assert_eq!(
            super::why(Ready, None, "openai rejected the key stored in leo (401)."),
            "leo can't write yet: openai rejected the key stored in leo (401)."
        );
    }

    #[test]
    fn a_read_never_reaches_a_second_provider_and_has_no_tools() {
        let quick = super::toolless_config(&chained("ollama"));
        assert_eq!(quick.chat.chain, ["ollama"]);
        assert!(quick.provider("ollama").unwrap().no_tools);
        let full = super::without_tools(chained("codex").clone(), &chained("codex"));
        assert_eq!(full.chat.chain.len(), 3);
        for name in &full.chat.chain {
            assert!(full.provider(name).unwrap().no_tools, "{name}");
        }
        let quick = super::toolless_config(&chained("claude_code"));
        assert!(quick.provider("claude_code").unwrap().no_tools);
        assert!(quick.provider("claude_code").unwrap().model.is_some());
    }

    #[test]
    fn the_dictation_chain_has_no_tools() {
        for quick in [false, true] {
            let cfg = super::dictation_config(chained("codex"), quick);
            assert_eq!(cfg.chat.chain.len(), 3);
            for name in &cfg.chat.chain {
                assert!(cfg.provider(name).unwrap().no_tools, "{name} quick={quick}");
            }
        }
    }

    #[test]
    fn this_computer_is_loopback_or_unspecified_and_nothing_that_merely_contains_it() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:1/v1",
            "http://0.0.0.0:11434/v1",
            "http://[::]:11434/v1",
        ] {
            assert!(super::loopback(url), "{url}");
        }
        for url in [
            "https://openrouter.ai/api/v1",
            "http://127.0.0.1.evil.example/v1",
            "http://evil.example/localhost",
        ] {
            assert!(!super::loopback(url), "{url}");
        }
    }

    #[test]
    fn only_a_loopback_ollama_with_a_local_model_keeps_text_here() {
        let with = |url: &str, model: &str| {
            let mut cfg = chained("ollama");
            cfg.providers.insert(
                "ollama".into(),
                ProviderConfig {
                    kind: Some(ProviderKind::Openai),
                    base_url: Some(url.into()),
                    model: Some(model.into()),
                    ..Default::default()
                },
            );
            super::reads_stay_here(&cfg)
        };
        assert!(with("http://127.0.0.1:11434/v1", "llama3.2"));
        assert!(with("http://localhost:11434/v1", "llama3.2"));
        assert!(with("http://[::1]:11434/v1", "llama3.2"));
        assert!(!with("http://127.0.0.1:11434/v1", "gpt-oss:120b-cloud"));
        assert!(!with("http://ollama.example.com/v1", "llama3.2"));
        assert!(!with("http://127.0.0.1.evil.example/v1", "llama3.2"));
        assert!(!with("http://localhost.evil.example/v1", "llama3.2"));
        assert!(!with("http://evil.example/127.0.0.1", "llama3.2"));
        assert!(with("http://0.0.0.0:11434/v1", "llama3.2"));
        assert!(with("http://[::]:11434/v1", "llama3.2"));
        assert!(!super::reads_stay_here(&chained("claude_code")));
    }

    use super::*;

    #[test]
    fn a_lighter_model_exists_only_when_the_quick_tier_differs() {
        let mut cfg = Config::default();
        cfg.chat.chain = vec!["claude_code".into()];
        let mut chosen = Config::built_in_provider("claude_code").unwrap();
        chosen.model = Some("claude-opus-5-5".into());
        chosen.effort = Some("high".into());
        cfg.providers.insert("claude_code".into(), chosen.clone());
        assert!(lighter_in(&cfg));
        chosen.model = Some("claude-haiku-5-5".into());
        chosen.effort = Some("low".into());
        cfg.providers.insert("claude_code".into(), chosen);
        assert!(!lighter_in(&cfg));
        cfg.chat.chain = vec!["ollama".into()];
        assert!(!lighter_in(&cfg));
        cfg.chat.chain = vec!["ollama".into(), "codex".into()];
        cfg.providers
            .insert("codex".into(), Config::built_in_provider("codex").unwrap());
        assert!(!lighter_in(&cfg));
    }

    #[test]
    fn the_quick_model_is_the_cheapest_model_of_the_same_ai_at_low_effort() {
        let mut cfg = Config::default();
        cfg.chat.chain = vec!["claude_code".into(), "ollama".into()];
        let mut chosen = Config::built_in_provider("claude_code").unwrap();
        chosen.model = Some("claude-opus-5-5".into());
        chosen.effort = Some("high".into());
        cfg.providers.insert("claude_code".into(), chosen);
        let quick = quick_config(&cfg);
        let claude = quick.provider("claude_code").unwrap();
        assert_eq!(claude.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(claude.effort.as_deref(), Some("low"));
        assert_eq!(quick.chat.chain, cfg.chat.chain);
        assert_eq!(
            quick.provider("ollama").and_then(|p| p.model.clone()),
            cfg.provider("ollama").and_then(|p| p.model.clone())
        );
    }
}
