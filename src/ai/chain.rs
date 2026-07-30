use std::path::Path;

use anyhow::{bail, Result};

use crate::ai::error::ProviderError;
use crate::ai::provider::{ChatProvider, ChatRequest, TranscribeProvider};

/// One degradation step, surfaced to the UI so a silent downgrade is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ChainOutcome<T> {
    pub value: T,
    /// Which provider actually served the request.
    pub provider: String,
    pub fallbacks: Vec<Fallback>,
}

/// Walk providers in order:
/// - unavailable  -> skip silently, record the reason for the exhaustion error
/// - retryable    -> record a fallback, try the next
/// - fatal        -> stop and surface the real cause
pub fn run_chat_chain(
    providers: Vec<Box<dyn ChatProvider>>,
    req: &ChatRequest,
) -> Result<ChainOutcome<String>> {
    if providers.is_empty() {
        bail!("no chat providers configured — check the [chat] chain in your leo config");
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut fallbacks: Vec<Fallback> = Vec::new();
    let mut pending: Option<(String, String)> = None; // (from, reason)
    let mut last_error: Option<String> = None;

    for p in &providers {
        if !p.available() {
            skipped.push(p.unavailable_reason());
            continue;
        }

        if let Some((from, reason)) = pending.take() {
            fallbacks.push(Fallback {
                from,
                to: p.name().to_string(),
                reason,
            });
        }

        // Provider config wins over the caller's request: when a provider
        // has its own configured `max_tokens`, build a per-provider request
        // rather than mutating the caller's, since different providers in
        // the same chain may disagree.
        let mut effective_req = req.clone();
        if let Some(max_tokens) = p.max_tokens() {
            effective_req.max_tokens = max_tokens;
        }

        match p.complete(&effective_req) {
            Ok(value) => {
                return Ok(ChainOutcome {
                    value,
                    provider: p.name().to_string(),
                    fallbacks,
                })
            }
            Err(ProviderError::Fatal(msg)) => bail!("{msg}"),
            Err(ProviderError::Retryable(msg)) => {
                pending = Some((p.name().to_string(), msg.clone()));
                last_error = Some(msg);
            }
        }
    }

    match last_error {
        Some(msg) if skipped.is_empty() => bail!("every chat provider failed. Last error: {msg}"),
        Some(msg) => bail!(
            "chat chain exhausted — some providers were unavailable and the rest failed:\n  {}\nLast error: {msg}",
            skipped.join("\n  ")
        ),
        None => bail!(
            "no chat provider is available:\n  {}",
            skipped.join("\n  ")
        ),
    }
}

/// Same policy as `run_chat_chain`, for transcription.
pub fn run_transcribe_chain(
    providers: Vec<Box<dyn TranscribeProvider>>,
    audio_path: &Path,
    transcribe_with: impl Fn(&dyn TranscribeProvider, &Path) -> Result<String, ProviderError>,
) -> Result<ChainOutcome<String>> {
    if providers.is_empty() {
        bail!("no transcription providers configured — check the [transcribe] chain in your leo config");
    }

    let mut skipped: Vec<String> = Vec::new();
    let mut fallbacks: Vec<Fallback> = Vec::new();
    let mut pending: Option<(String, String)> = None;
    let mut last_error: Option<String> = None;

    for p in &providers {
        if !p.available() {
            skipped.push(p.unavailable_reason());
            continue;
        }

        if let Some((from, reason)) = pending.take() {
            fallbacks.push(Fallback {
                from,
                to: p.name().to_string(),
                reason,
            });
        }

        match transcribe_with(p.as_ref(), audio_path) {
            Ok(value) => {
                return Ok(ChainOutcome {
                    value,
                    provider: p.name().to_string(),
                    fallbacks,
                })
            }
            Err(ProviderError::Fatal(msg)) => bail!("{msg}"),
            Err(ProviderError::Retryable(msg)) => {
                pending = Some((p.name().to_string(), msg.clone()));
                last_error = Some(msg);
            }
        }
    }

    match last_error {
        Some(msg) if skipped.is_empty() => {
            bail!("every transcription provider failed. Last error: {msg}")
        }
        Some(msg) => bail!(
            "transcription chain exhausted — some providers were unavailable and the rest failed:\n  {}\nLast error: {msg}",
            skipped.join("\n  ")
        ),
        None => bail!(
            "no transcription provider is available:\n  {}",
            skipped.join("\n  ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeChat {
        name: &'static str,
        available: bool,
        result: Option<Result<String, ProviderError>>,
        max_tokens: Option<u32>,
        /// Captures the `max_tokens` the runner actually passed to
        /// `complete`, so tests can assert on the override behavior.
        observed_max_tokens: std::rc::Rc<std::cell::RefCell<Option<u32>>>,
    }

    impl FakeChat {
        fn ok(name: &'static str, out: &str) -> Box<dyn ChatProvider> {
            Box::new(FakeChat {
                name,
                available: true,
                result: Some(Ok(out.to_string())),
                max_tokens: None,
                observed_max_tokens: Default::default(),
            })
        }
        fn retryable(name: &'static str) -> Box<dyn ChatProvider> {
            Box::new(FakeChat {
                name,
                available: true,
                result: Some(Err(ProviderError::Retryable(format!("{name} 429")))),
                max_tokens: None,
                observed_max_tokens: Default::default(),
            })
        }
        fn fatal(name: &'static str) -> Box<dyn ChatProvider> {
            Box::new(FakeChat {
                name,
                available: true,
                result: Some(Err(ProviderError::Fatal(format!("{name} 400")))),
                max_tokens: None,
                observed_max_tokens: Default::default(),
            })
        }
        fn unavailable(name: &'static str) -> Box<dyn ChatProvider> {
            Box::new(FakeChat {
                name,
                available: false,
                result: None,
                max_tokens: None,
                observed_max_tokens: Default::default(),
            })
        }
        /// A provider that records the `max_tokens` it actually observed in
        /// `complete`, with or without an opinion of its own. Returns the box
        /// plus a shared handle to the observed value, since the runner
        /// consumes the `Vec<Box<dyn ChatProvider>>` and the box isn't
        /// reachable again after `run_chat_chain` returns.
        fn capturing(
            name: &'static str,
            max_tokens: Option<u32>,
            out: &str,
        ) -> (Box<dyn ChatProvider>, std::rc::Rc<std::cell::RefCell<Option<u32>>>) {
            let observed: std::rc::Rc<std::cell::RefCell<Option<u32>>> = Default::default();
            let provider = Box::new(FakeChat {
                name,
                available: true,
                result: Some(Ok(out.to_string())),
                max_tokens,
                observed_max_tokens: observed.clone(),
            });
            (provider, observed)
        }
    }

    impl ChatProvider for FakeChat {
        fn complete(&self, req: &ChatRequest) -> Result<String, ProviderError> {
            *self.observed_max_tokens.borrow_mut() = Some(req.max_tokens);
            self.result
                .clone()
                .expect("complete called on an unavailable provider")
        }
        fn available(&self) -> bool {
            self.available
        }
        fn name(&self) -> &str {
            self.name
        }
        fn unavailable_reason(&self) -> String {
            format!("{} has no key", self.name)
        }
        fn max_tokens(&self) -> Option<u32> {
            self.max_tokens
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            prompt: "hi".to_string(),
            temperature: 0.3,
            max_tokens: 100,
        }
    }

    #[test]
    fn first_available_provider_serves_the_request() {
        let out = run_chat_chain(vec![FakeChat::ok("ollama", "answer")], &req()).unwrap();
        assert_eq!(out.value, "answer");
        assert_eq!(out.provider, "ollama");
        assert!(out.fallbacks.is_empty());
    }

    #[test]
    fn provider_max_tokens_overrides_the_callers_request_value() {
        let (provider, observed) = FakeChat::capturing("ollama", Some(55), "answer");
        let out = run_chat_chain(vec![provider], &req()).unwrap();
        assert_eq!(out.value, "answer");
        assert_eq!(*observed.borrow(), Some(55));
    }

    #[test]
    fn provider_with_no_max_tokens_opinion_receives_the_callers_value_unchanged() {
        let (provider, observed) = FakeChat::capturing("ollama", None, "answer");
        let out = run_chat_chain(vec![provider], &req()).unwrap();
        assert_eq!(out.value, "answer");
        // req().max_tokens == 100 — see the `req()` helper above.
        assert_eq!(*observed.borrow(), Some(100));
    }

    #[test]
    fn unavailable_providers_are_skipped_silently() {
        let out = run_chat_chain(
            vec![
                FakeChat::unavailable("ollama"),
                FakeChat::ok("openrouter", "answer"),
            ],
            &req(),
        )
        .unwrap();
        assert_eq!(out.provider, "openrouter");
        // Skipping an unconfigured provider is not a degradation worth
        // reporting — only an actual failure is.
        assert!(out.fallbacks.is_empty());
    }

    #[test]
    fn retryable_error_advances_and_records_the_fallback() {
        let out = run_chat_chain(
            vec![
                FakeChat::retryable("ollama"),
                FakeChat::ok("openrouter", "answer"),
            ],
            &req(),
        )
        .unwrap();
        assert_eq!(out.provider, "openrouter");
        assert_eq!(out.fallbacks.len(), 1);
        assert_eq!(out.fallbacks[0].from, "ollama");
        assert_eq!(out.fallbacks[0].to, "openrouter");
        assert!(out.fallbacks[0].reason.contains("429"));
    }

    #[test]
    fn fatal_error_aborts_without_trying_the_next_provider() {
        let err = run_chat_chain(
            vec![
                FakeChat::fatal("openrouter"),
                FakeChat::ok("ollama", "should never run"),
            ],
            &req(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("400"), "got: {err}");
    }

    #[test]
    fn exhausted_chain_names_every_provider_and_its_reason() {
        let err = run_chat_chain(
            vec![
                FakeChat::unavailable("ollama"),
                FakeChat::unavailable("openrouter"),
            ],
            &req(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ollama has no key"), "got: {msg}");
        assert!(msg.contains("openrouter has no key"), "got: {msg}");
    }

    #[test]
    fn empty_chain_is_an_error_not_a_panic() {
        assert!(run_chat_chain(vec![], &req()).is_err());
    }

    #[test]
    fn all_retryable_reports_the_last_failure() {
        let err = run_chat_chain(
            vec![FakeChat::retryable("a"), FakeChat::retryable("b")],
            &req(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("b 429"), "got: {msg}");
    }

    #[test]
    fn mixed_chain_reports_both_skipped_and_failed() {
        let err = run_chat_chain(
            vec![FakeChat::unavailable("ollama"), FakeChat::retryable("openrouter")],
            &req(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ollama has no key"), "got: {msg}");
        assert!(msg.contains("openrouter 429"), "got: {msg}");
    }

    #[test]
    fn mixed_chain_reports_every_skipped_provider_not_just_the_first() {
        let err = run_chat_chain(
            vec![
                FakeChat::unavailable("ollama"),
                FakeChat::retryable("openrouter"),
                FakeChat::unavailable("claude"),
            ],
            &req(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ollama has no key"), "got: {msg}");
        assert!(msg.contains("claude has no key"), "got: {msg}");
        assert!(msg.contains("openrouter 429"), "got: {msg}");
    }

    #[test]
    fn mixed_chain_does_not_claim_every_provider_failed() {
        let err = run_chat_chain(
            vec![FakeChat::unavailable("ollama"), FakeChat::retryable("openrouter")],
            &req(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("every chat provider failed"), "got: {msg}");
    }

    struct FakeTranscribe {
        name: &'static str,
        available: bool,
        result: Option<Result<String, ProviderError>>,
    }

    impl FakeTranscribe {
        fn retryable(name: &'static str) -> Box<dyn TranscribeProvider> {
            Box::new(FakeTranscribe {
                name,
                available: true,
                result: Some(Err(ProviderError::Retryable(format!("{name} 429")))),
            })
        }
        fn unavailable(name: &'static str) -> Box<dyn TranscribeProvider> {
            Box::new(FakeTranscribe {
                name,
                available: false,
                result: None,
            })
        }
    }

    impl TranscribeProvider for FakeTranscribe {
        fn transcribe(&self, _audio_path: &Path) -> Result<String, ProviderError> {
            self.result
                .clone()
                .expect("transcribe called on an unavailable provider")
        }
        fn max_bytes(&self) -> Option<u64> {
            None
        }
        fn available(&self) -> bool {
            self.available
        }
        fn name(&self) -> &str {
            self.name
        }
        fn unavailable_reason(&self) -> String {
            format!("{} has no key", self.name)
        }
    }

    fn transcribe_with(p: &dyn TranscribeProvider, path: &Path) -> Result<String, ProviderError> {
        p.transcribe(path)
    }

    #[test]
    fn transcribe_mixed_chain_reports_both_skipped_and_failed() {
        let err = run_transcribe_chain(
            vec![
                FakeTranscribe::unavailable("whisper-local"),
                FakeTranscribe::retryable("hf-whisper"),
            ],
            Path::new("/tmp/does-not-matter.wav"),
            transcribe_with,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("whisper-local has no key"), "got: {msg}");
        assert!(msg.contains("hf-whisper 429"), "got: {msg}");
    }

    #[test]
    fn transcribe_mixed_chain_reports_every_skipped_provider_not_just_the_first() {
        let err = run_transcribe_chain(
            vec![
                FakeTranscribe::unavailable("whisper-local"),
                FakeTranscribe::retryable("hf-whisper"),
                FakeTranscribe::unavailable("openai-whisper"),
            ],
            Path::new("/tmp/does-not-matter.wav"),
            transcribe_with,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("whisper-local has no key"), "got: {msg}");
        assert!(msg.contains("openai-whisper has no key"), "got: {msg}");
        assert!(msg.contains("hf-whisper 429"), "got: {msg}");
    }

    #[test]
    fn transcribe_mixed_chain_does_not_claim_every_provider_failed() {
        let err = run_transcribe_chain(
            vec![
                FakeTranscribe::unavailable("whisper-local"),
                FakeTranscribe::retryable("hf-whisper"),
            ],
            Path::new("/tmp/does-not-matter.wav"),
            transcribe_with,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("every transcription provider failed"),
            "got: {msg}"
        );
    }
}
