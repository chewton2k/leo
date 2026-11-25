//! What works on this machine, and what to run if it doesn't.
//!
//! greeting, and the pre-flight check before recording all agree — and so a
//! missing dependency is reported with the command that installs it rather than
//! as a failure after the user has already tried to use it.

use crate::config::provider::ProviderConfig;
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Whether a capability is usable, and what to do when it is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Works right now.
    Ready,
    /// Not usable, with the one command that fixes it.
    Missing { fix: String },
    /// Usable, but worth mentioning.
    Warn { note: String },
}

impl State {
    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        matches!(self, State::Ready)
    }
}

/// One checked thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What it is, in the user's words rather than the binary's name.
    pub what: String,
    /// What it is needed for, so the user can judge whether to care.
    pub needed_for: String,
    pub state: State,
    /// Extra detail: a version, a path, a model name.
    pub detail: Option<String>,
}

impl Check {
    pub(crate) fn ready(what: &str, needed_for: &str, detail: Option<String>) -> Self {
        Self {
            what: what.to_string(),
            needed_for: needed_for.to_string(),
            state: State::Ready,
            detail,
        }
    }

    pub(crate) fn missing(what: &str, needed_for: &str, fix: &str) -> Self {
        Self {
            what: what.to_string(),
            needed_for: needed_for.to_string(),
            state: State::Missing {
                fix: fix.to_string(),
            },
            detail: None,
        }
    }
}

pub use leo_core::paths::on_path;

/// Whether a local HTTP server is listening, used for Ollama and friends.
///
/// Resolves the address itself rather than going through `to_socket_addrs`,
/// which performs a DNS lookup with no timeout — a bogus hostname there can
/// block for minutes, and this runs on startup.
pub(crate) fn port_open(base_url: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    let trimmed = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let authority = trimmed.split('/').next().unwrap_or(trimmed);

    // Split host and port, keeping bracketed IPv6 literals intact.
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h, p.parse::<u16>().unwrap_or(0))
        }
        _ => (
            authority,
            if base_url.starts_with("https://") {
                443
            } else {
                80
            },
        ),
    };
    if port == 0 {
        return false;
    }

    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ip: IpAddr = match host {
        "localhost" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        other => match other.parse::<Ipv4Addr>() {
            Ok(v4) => IpAddr::V4(v4),
            Err(_) => match other.parse::<Ipv6Addr>() {
                Ok(v6) => IpAddr::V6(v6),
                // Not a literal address. Only loopback reaches this function,
                // so anything else is not something to probe.
                Err(_) => return false,
            },
        },
    };

    TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(300)).is_ok()
}

/// Whether the microphone is actually heard, by recording a fraction of a
/// second and looking at the samples.
///
/// Worth the half second because on macOS a denied microphone permission is not
/// an error: `rec` succeeds and every sample is zero. Nothing downstream can
/// tell that from a quiet room, so a user gets a transcript of invented text
/// instead of a reason. Probing also makes macOS raise its permission prompt,
/// which is the fix.
pub fn microphone_device() -> Check {
    match crate::session::mic::microphone_name() {
        Some(name) => Check::ready("microphone device", "recording audio", Some(name)),
        None => Check::missing(
            "microphone device",
            "recording audio",
            "plug in a microphone, or pick an input in your sound settings",
        ),
    }
}

/// Which chain a check refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Chat,
    Transcribe,
}

impl Chain {
    fn label(self) -> &'static str {
        match self {
            Chain::Chat => "a chat model",
            Chain::Transcribe => "a transcription model",
        }
    }

    fn needed_for(self) -> &'static str {
        match self {
            Chain::Chat => "ask, and structuring recordings into notes",
            Chain::Transcribe => "turning speech into text",
        }
    }
}

/// Whether at least one provider in a chain is usable, and which one leo would
/// reach for.
pub(crate) fn chain_check(config: &Config, chain: Chain, store: &dyn SecretStore) -> Check {
    let names = match chain {
        Chain::Chat => &config.chat.chain,
        Chain::Transcribe => &config.transcribe.chain,
    };

    let (usable, fix) = match chain {
        Chain::Chat => {
            let writing = writing(
                config,
                store,
                Some(&crate::config::choice::local_models(config)),
            );
            let ready = writing.state == WritingState::Ready;
            (
                writing.provider.filter(|_| ready),
                writing.fix.unwrap_or_default(),
            )
        }
        Chain::Transcribe => (
            names
                .iter()
                .find(|name| provider_usable(config, name, store))
                .cloned(),
            "Choose and download a speech model in Settings → Speech models.".to_string(),
        ),
    };

    match usable {
        Some(first) => Check::ready(
            chain.label(),
            chain.needed_for(),
            Some(format!("using {first}")),
        ),
        None => Check {
            what: chain.label().to_string(),
            needed_for: chain.needed_for().to_string(),
            state: State::Missing { fix },
            detail: if names.is_empty() {
                Some("no providers configured".to_string())
            } else {
                Some(format!("tried {}", names.join(", ")))
            },
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingState {
    Ready,
    NotInstalled,
    NotRunning,
    NoModel,
    NotChosen,
    SignInNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Writing {
    pub provider: Option<String>,
    pub state: WritingState,
    pub fix: Option<String>,
}

pub fn writing(
    config: &Config,
    store: &dyn SecretStore,
    models: Option<&crate::config::choice::Local>,
) -> Writing {
    let names = &config.chat.chain;
    let usable = names
        .iter()
        .find(|name| provider_usable(config, name, store));
    let provider = usable.or(names.first()).cloned();
    let pc = provider.as_deref().and_then(|name| config.provider(name));
    let agent = pc.and_then(crate::ai::provider::agent_cli::Agent::of);
    let state = match (pc, usable) {
        (None, _) => WritingState::NotChosen,
        (Some(pc), Some(name))
            if agent.is_some_and(|agent| {
                crate::ai::provider::agent_cli::last_signed_in(
                    crate::ai::provider::agent_cli::AgentCli::new(name.clone(), agent, pc).bin(),
                ) == Some(false)
            }) =>
        {
            WritingState::SignInNeeded
        }
        (Some(_), Some(name)) => match models.filter(|_| name == "ollama") {
            Some(local) if !local.ollama_running => WritingState::NotRunning,
            Some(local) if local.ollama.is_empty() => WritingState::NoModel,
            _ => WritingState::Ready,
        },
        (Some(_), None) if agent.is_some() => WritingState::NotInstalled,
        (Some(pc), None)
            if pc.key_env.is_none() && pc.base_url.as_deref().is_some_and(crate::ai::loopback) =>
        {
            WritingState::NotRunning
        }
        (Some(_), None) => WritingState::NotChosen,
    };
    let fix = fix_text(state, agent, on_path("ollama"), cfg!(windows));
    Writing {
        provider,
        state,
        fix,
    }
}

fn fix_text(
    state: WritingState,
    agent: Option<crate::ai::provider::agent_cli::Agent>,
    ollama_installed: bool,
    windows: bool,
) -> Option<String> {
    let name = agent.map_or("it", |agent| agent.kind());
    Some(match state {
        WritingState::Ready => return None,
        WritingState::NotInstalled => format!(
            "{name} is not installed on this computer. Choose another provider in Writing AI, or install {name}."
        ),
        WritingState::NotChosen | WritingState::NotRunning if !ollama_installed => {
            "Ollama is not installed on this computer. Choose Claude Code or Codex in Writing AI, or install Ollama.".to_string()
        }
        WritingState::NotRunning if windows => {
            "Ollama is installed but not running. Open Ollama from the Start menu.".to_string()
        }
        WritingState::NotRunning => {
            "Ollama is installed but not running. Open the Ollama app.".to_string()
        }
        WritingState::NoModel => "Ollama has no models yet. Choose one in Writing AI.".to_string(),
        WritingState::SignInNeeded => format!("{name} needs you to sign in. Open Writing AI."),
        WritingState::NotChosen => "No writing AI is ready. Choose one in Writing AI.".to_string(),
    })
}

/// Whether one named provider could serve a request right now.
///
/// Deliberately cheap and offline: a key check, a PATH check, or a port check.
/// Never a real request, so this stays fast enough to run on startup.
pub fn provider_usable(config: &Config, name: &str, store: &dyn SecretStore) -> bool {
    let Some(provider) = config.provider(name) else {
        return false;
    };
    let _: &ProviderConfig = provider;

    if provider.kind == Some(crate::config::provider::ProviderKind::Parakeet) {
        return crate::speech::library::library().is_ok_and(|l| l.selected().is_ok());
    }
    if let Some(agent) = crate::ai::provider::agent_cli::Agent::of(provider) {
        let cli = crate::ai::provider::agent_cli::AgentCli::new(name.to_string(), agent, provider);
        return crate::ai::provider::agent_cli::locate(cli.bin()).is_some();
    }
    // A key-based provider: only the key leo stored, and `has` rather than a
    // read so this cannot cost a permission prompt.
    if provider.key_env.is_some() {
        return store.has(provider.account(name));
    }

    // A local server with no key: is anything listening?
    match &provider.base_url {
        Some(url) if crate::ai::loopback(url) => port_open(url),
        // Remote and keyless: assume reachable; a request will say otherwise.
        Some(_) => true,
        None => false,
    }
}

/// Where credentials are kept, and whether the file is readable by anyone else.
///
/// Worth reporting because the choice of a file over the keychain trades
/// encryption at rest for filesystem permissions — so those permissions are the
/// protection, and an unchecked assumption is not one.
pub(crate) fn credentials_check() -> Check {
    use crate::config::file_store::FileStore;

    if std::env::var("LEO_USE_KEYCHAIN").is_ok_and(|v| v != "0" && !v.is_empty()) {
        return Check::ready(
            "credentials",
            "storing API keys",
            Some("OS keychain (LEO_USE_KEYCHAIN is set)".to_string()),
        );
    }

    match FileStore::new() {
        // Tighten it rather than only complaining: leo created this file, and a
        // loose mode is a bug in an older leo, not a decision the user made.
        Ok(file) if !file.is_private() && !file.make_private() => Check {
            what: "credentials".to_string(),
            needed_for: "storing API keys".to_string(),
            state: State::Warn {
                note: format!(
                    "{} is readable by other accounts on this machine",
                    file.path().display()
                ),
            },
            detail: Some(format!("chmod 600 {}", file.path().display())),
        },
        Ok(file) => Check::ready(
            "credentials",
            "storing API keys",
            Some(format!("{} — only you can read it", file.path().display())),
        ),
        Err(e) => Check {
            what: "credentials".to_string(),
            needed_for: "storing API keys".to_string(),
            state: State::Warn {
                note: format!("no location for a credentials file: {e}"),
            },
            detail: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::provider::ProviderConfig;
    use crate::config::secret::MemoryStore;

    /// Never the real keychain: an unsigned test binary reading it can block on
    /// a permission dialog with nobody to answer it.
    fn store() -> MemoryStore {
        MemoryStore::default()
    }

    fn config_with(providers: Vec<(&str, ProviderConfig)>, chat: Vec<&str>) -> Config {
        Config {
            providers: providers
                .into_iter()
                .map(|(n, p)| (n.to_string(), p))
                .collect(),
            chat: crate::config::provider::TaskChain {
                chain: chat.into_iter().map(String::from).collect(),
            },
            transcribe: crate::config::provider::TaskChain { chain: vec![] },
        }
    }

    #[test]
    fn a_binary_on_path_is_found_and_a_missing_one_is_not() {
        // `sh` exists on every unix box this targets.
        #[cfg(unix)]
        assert!(on_path("sh"));
        assert!(!on_path("leo-definitely-not-a-real-binary"));
    }

    fn cli(bin: &str) -> ProviderConfig {
        ProviderConfig {
            kind: Some(crate::config::provider::ProviderKind::ClaudeCode),
            bin: Some(bin.to_string()),
            ..Default::default()
        }
    }

    fn installed() -> ProviderConfig {
        cli(if cfg!(windows) { "cmd.exe" } else { "sh" })
    }

    fn missing() -> ProviderConfig {
        cli("leo-definitely-not-a-real-binary")
    }

    #[test]
    fn a_named_program_must_be_installed() {
        let config = config_with(
            vec![("present", installed()), ("absent", missing())],
            vec!["present"],
        );
        assert!(provider_usable(&config, "present", &store()));
        assert!(!provider_usable(&config, "absent", &store()));
    }

    #[test]
    fn an_empty_program_setting_means_the_usual_program() {
        for kind in [
            crate::config::provider::ProviderKind::ClaudeCode,
            crate::config::provider::ProviderKind::Codex,
        ] {
            let usual = ProviderConfig {
                kind: Some(kind),
                ..Default::default()
            };
            let empty = ProviderConfig {
                bin: Some(String::new()),
                ..usual.clone()
            };
            let config = config_with(vec![("usual", usual), ("empty", empty)], vec![]);
            assert_eq!(
                provider_usable(&config, "empty", &store()),
                provider_usable(&config, "usual", &store())
            );
        }
    }

    #[test]
    fn an_unknown_provider_is_never_usable() {
        let config = config_with(vec![], vec![]);
        assert!(!provider_usable(&config, "nope", &store()));
    }

    /// An empty chain is a real gap, not a pass.
    #[test]
    fn a_chain_with_no_usable_provider_is_missing() {
        let config = config_with(vec![], vec![]);
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(matches!(check.state, State::Missing { .. }));
        assert_eq!(check.detail.as_deref(), Some("no providers configured"));
    }

    /// The chain reports which provider would actually serve the request, since
    /// "chat works" is less useful than "chat works, using ollama".
    #[test]
    fn a_usable_chain_names_the_provider_it_would_use() {
        let config = config_with(vec![("localbin", installed())], vec!["localbin"]);
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(check.state.is_ready());
        assert_eq!(check.detail.as_deref(), Some("using localbin"));
    }

    /// A chain skips ahead: an unusable first provider must not mask a working
    /// second one, because that is exactly how the fallback behaves at runtime.
    #[test]
    fn a_chain_falls_past_an_unusable_provider() {
        let config = config_with(
            vec![("broken", missing()), ("working", installed())],
            vec!["broken", "working"],
        );
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(check.state.is_ready());
        assert_eq!(check.detail.as_deref(), Some("using working"));
    }

    fn server(url: &str) -> ProviderConfig {
        ProviderConfig {
            kind: Some(crate::config::provider::ProviderKind::Openai),
            base_url: Some(url.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn writing_says_what_is_missing_for_each_kind_of_provider() {
        for (providers, chain, state) in [
            (vec![], vec![], WritingState::NotChosen),
            (vec![], vec!["gone"], WritingState::NotChosen),
            (
                vec![("cli", missing())],
                vec!["cli"],
                WritingState::NotInstalled,
            ),
            (vec![("cli", installed())], vec!["cli"], WritingState::Ready),
            (
                vec![("local", server("http://127.0.0.1:1/v1"))],
                vec!["local"],
                WritingState::NotRunning,
            ),
            (
                vec![("remote", server("https://example.invalid/v1"))],
                vec!["remote"],
                WritingState::Ready,
            ),
        ] {
            let config = config_with(providers, chain);
            let local = crate::config::choice::Local::default();
            for models in [None, Some(&local)] {
                let writing = writing(&config, &store(), models);
                assert_eq!(writing.state, state, "{writing:?}");
                assert_eq!(writing.fix.is_none(), state == WritingState::Ready);
                assert_eq!(
                    chain_check(&config, Chain::Chat, &store()).state.is_ready(),
                    state == WritingState::Ready
                );
            }
        }
        assert_eq!(serde_json::json!(WritingState::NoModel), "no_model");
    }

    #[test]
    fn writing_fixes_speak_to_the_app_on_every_platform() {
        use crate::ai::provider::agent_cli::Agent;
        let mut seen = std::collections::HashSet::new();
        for windows in [false, true] {
            for installed in [false, true] {
                for state in [
                    WritingState::NotInstalled,
                    WritingState::NotRunning,
                    WritingState::NoModel,
                    WritingState::NotChosen,
                ] {
                    for agent in [None, Some(Agent::Codex), Some(Agent::ClaudeCode)] {
                        let fix = fix_text(state, agent, installed, windows).unwrap();
                        for banned in ["brew", "leo update", "OpenAI, Gemini or xAI", "ollama pull"]
                        {
                            assert!(!fix.contains(banned), "{state:?}: {fix}");
                        }
                        if state == WritingState::NotRunning {
                            seen.insert((installed, fix));
                        }
                    }
                }
            }
        }
        assert_eq!(fix_text(WritingState::Ready, None, true, false), None);
        assert_eq!(seen.len(), 3);
        assert!(
            fix_text(WritingState::NotInstalled, Some(Agent::Codex), true, true)
                .unwrap()
                .starts_with("Codex is not installed")
        );
        let chain = chain_check(&config_with(vec![], vec![]), Chain::Transcribe, &store());
        match chain.state {
            State::Missing { fix } => {
                assert!(!fix.contains("OpenAI") && !fix.contains("leo update"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ollama_without_models_is_no_model_only_when_its_models_were_listed() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let config = config_with(vec![("ollama", server(&url))], vec!["ollama"]);
        let local = |running: bool, models: &[&str]| crate::config::choice::Local {
            ollama_running: running,
            ollama: models.iter().map(|m| m.to_string()).collect(),
            speech_ready: false,
        };

        assert_eq!(writing(&config, &store(), None).state, WritingState::Ready);
        listener.set_nonblocking(true).unwrap();
        while let Ok((mut stream, _)) = listener.accept() {
            stream.set_nonblocking(false).unwrap();
            let mut sent = Vec::new();
            stream.read_to_end(&mut sent).unwrap();
            assert!(sent.is_empty(), "the cheap path sent a request");
        }

        for (models, state) in [
            (local(true, &[]), WritingState::NoModel),
            (local(false, &[]), WritingState::NotRunning),
            (local(true, &["qwen3:8b"]), WritingState::Ready),
        ] {
            assert_eq!(writing(&config, &store(), Some(&models)).state, state);
        }
    }

    fn empty_ollama() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0u8; 8192];
                if stream.read(&mut buffer).unwrap_or(0) > 0 {
                    let body = "{\"models\":[]}";
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            }
        });
        url
    }

    #[test]
    fn the_chat_check_is_not_ready_for_ollama_without_a_model() {
        let config = config_with(vec![("ollama", server(&empty_ollama()))], vec!["ollama"]);
        assert_eq!(writing(&config, &store(), None).state, WritingState::Ready);
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(!check.state.is_ready(), "{check:?}");
        assert!(matches!(
            check.state,
            State::Missing { ref fix } if fix.contains("no models")
        ));
    }

    #[test]
    #[cfg(unix)]
    fn a_signed_out_cli_reads_only_the_last_sign_in_check() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let bin = dir.path().join("claude");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf '{{\"loggedIn\": false}}'\n",
                ran.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = config_with(vec![("cli", cli(bin.to_str().unwrap()))], vec!["cli"]);

        assert_eq!(writing(&config, &store(), None).state, WritingState::Ready);
        assert!(!ran.exists(), "the cheap path ran the program");

        assert_eq!(
            crate::ai::provider::agent_cli::signed_in(
                crate::ai::provider::agent_cli::Agent::ClaudeCode,
                bin.to_str().unwrap()
            ),
            Some(false)
        );
        std::fs::remove_file(&ran).unwrap();

        let found = writing(&config, &store(), None);
        assert_eq!(found.state, WritingState::SignInNeeded);
        assert_eq!(
            found.fix.as_deref(),
            Some("Claude Code needs you to sign in. Open Writing AI.")
        );
        assert!(!chain_check(&config, Chain::Chat, &store()).state.is_ready());
        assert!(!ran.exists(), "the cheap path ran the program");
    }

    /// A port check must not hang, panic, or resolve DNS on nonsense input.
    /// It ran for fifteen minutes before it stopped going through
    /// `to_socket_addrs`, which does an unbounded lookup.
    #[test]
    fn a_closed_or_malformed_url_is_simply_not_open() {
        let started = std::time::Instant::now();
        assert!(!port_open("http://127.0.0.1:1"));
        assert!(!port_open("not a url at all"));
        assert!(!port_open(
            "http://a-hostname-that-does-not-resolve.invalid:80"
        ));
        assert!(!port_open(""));
        assert!(!port_open("http://127.0.0.1:0"));
        // Generous, but far below what a DNS timeout costs.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?} — is it resolving DNS?",
            started.elapsed()
        );
    }

    #[test]
    fn a_loopback_port_check_parses_host_and_port() {
        // Nothing is listening on port 1, but the address must parse and the
        // attempt must be made rather than short-circuiting on a parse failure.
        assert!(!port_open("http://localhost:1"));
        assert!(!port_open("http://[::1]:1/v1"));
        assert!(!port_open("http://127.0.0.1:1/v1/chat"));
    }
}
