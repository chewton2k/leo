use std::fmt;

/// Whether a provider failure should advance the chain or stop it.
///
/// Retryable means "this provider cannot serve the request right now" — try
/// the next one. Fatal means the request itself is wrong, so trying another
/// provider would fail identically and hide the real cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Retryable(String),
    Fatal(String),
}

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Retryable(m) | ProviderError::Fatal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ProviderError::Retryable(_))
    }

    pub fn message(&self) -> &str {
        match self {
            ProviderError::Retryable(m) | ProviderError::Fatal(m) => m,
        }
    }
}

/// Cap on how much of a provider's response body is quoted in an error.
const MAX_BODY_CHARS: usize = 400;

fn truncate(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_BODY_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(MAX_BODY_CHARS).collect();
    format!("{head}…")
}

/// Classify an HTTP status into retryable or fatal.
///
/// Only the response body is quoted — never a request header, so *our own*
/// Authorization header can never reach an error message this way. That is
/// narrower than "a key cannot leak": a gateway that reflects request
/// headers into a debug body could still put a key in the text passed here.
/// Callers holding a key scrub it from the body via `scrub_secret` before
/// calling this function, as defense in depth against that case.
pub fn classify_status(status: u16, provider: &str, body: &str) -> ProviderError {
    let msg = format!("{provider} returned {status}: {}", truncate(body));
    match status {
        402 | 408 | 429 => ProviderError::Retryable(msg),
        500..=599 => ProviderError::Retryable(msg),
        _ => ProviderError::Fatal(msg),
    }
}

/// A transport-level failure. A refused connection is the normal signal that a
/// local provider (Ollama, LM Studio) simply is not running, so it is
/// retryable rather than fatal.
pub fn classify_reqwest(provider: &str, e: &reqwest::Error) -> ProviderError {
    let msg = format!("{provider}: {e}");
    if e.is_timeout() || e.is_connect() || e.is_request() {
        ProviderError::Retryable(msg)
    } else {
        ProviderError::Fatal(msg)
    }
}

/// Defense in depth for `classify_status`: replace every occurrence of
/// `secret` in `body` with `…` before it is quoted in an error. Our own
/// request headers are never echoed back as `body` (see `classify_status`),
/// but a misbehaving gateway that reflects request headers into a 4xx debug
/// payload could still put the key text in the response body itself — this
/// is the guard against that. A pure string op; makes no network call.
pub fn scrub_secret(body: &str, secret: Option<&str>) -> String {
    match secret {
        Some(s) if !s.is_empty() => body.replace(s, "…"),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_and_rate_limit_errors_are_retryable() {
        // 402 is the HF credits-exhausted case that today's hardcoded
        // fallback special-cases.
        assert!(classify_status(402, "hf", "no credits").is_retryable());
        assert!(classify_status(429, "groq", "slow down").is_retryable());
        assert!(classify_status(408, "hf", "timeout").is_retryable());
    }

    #[test]
    fn server_errors_are_retryable() {
        for status in [500, 502, 503, 504] {
            assert!(
                classify_status(status, "openrouter", "oops").is_retryable(),
                "{status} should be retryable"
            );
        }
    }

    #[test]
    fn client_errors_are_fatal() {
        // A malformed request fails identically on every provider, so
        // advancing the chain would only hide the cause.
        assert!(!classify_status(400, "openrouter", "bad request").is_retryable());
        assert!(!classify_status(401, "openrouter", "bad key").is_retryable());
        assert!(!classify_status(403, "openrouter", "forbidden").is_retryable());
        assert!(!classify_status(404, "openrouter", "no model").is_retryable());
    }

    #[test]
    fn error_message_names_the_provider_and_status() {
        let e = classify_status(401, "openrouter", "invalid api key");
        let msg = e.message();
        assert!(msg.contains("openrouter"), "missing provider: {msg}");
        assert!(msg.contains("401"), "missing status: {msg}");
    }

    #[test]
    fn long_bodies_are_truncated() {
        let body = "x".repeat(5000);
        let e = classify_status(500, "groq", &body);
        assert!(
            e.message().len() < 1000,
            "body should be truncated, got {} chars",
            e.message().len()
        );
    }

    #[test]
    fn scrub_secret_removes_the_key_from_a_reflected_body() {
        let body = "debug: header Authorization: Bearer sk-secret-value was sent upstream";
        let scrubbed = scrub_secret(body, Some("sk-secret-value"));
        assert!(!scrubbed.contains("sk-secret-value"), "got: {scrubbed}");
        assert!(scrubbed.contains('…'));
    }

    #[test]
    fn scrub_secret_is_a_no_op_without_a_key() {
        assert_eq!(scrub_secret("plain body", None), "plain body");
        assert_eq!(scrub_secret("plain body", Some("")), "plain body");
    }
}
