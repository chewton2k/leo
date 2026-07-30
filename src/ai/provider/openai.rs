use crate::ai::error::{
    classify_reqwest, classify_status, scrub_secret, ProviderError, ProviderResult,
};
use crate::ai::provider::{ChatProvider, ChatRequest};
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Any endpoint speaking the OpenAI chat-completions protocol: OpenRouter,
/// Ollama, LM Studio, llama.cpp server, vLLM, Groq chat.
pub struct OpenAiChat {
    name: String,
    base_url: String,
    model: String,
    key: Option<Secret>,
    /// Whether this endpoint needs a key at all. Local servers do not.
    needs_key: bool,
    max_tokens: u32,
}

impl OpenAiChat {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        OpenAiChat {
            name,
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            model: cfg.model.clone().unwrap_or_else(|| "openrouter/free".to_string()),
            key,
            // A provider that names no key_env is a local server needing none.
            needs_key: cfg.key_env.is_some(),
            max_tokens: cfg.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        }
    }
}

impl ChatProvider for OpenAiChat {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": req.prompt}],
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            // OpenRouter attribution headers; harmless elsewhere.
            .header("HTTP-Referer", "https://github.com/leo-cli")
            .header("X-Title", "leo")
            .json(&body);

        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Bearer {}", key.as_str()));
        }

        let resp = request
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            // Only the response body is quoted — never our request headers,
            // so our own Authorization header cannot leak this way. Scrub
            // defensively in case a misbehaving gateway reflects the key
            // back inside the body itself.
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(Secret::as_str));
            return Err(classify_status(status, &self.name, &text));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| ProviderError::Fatal(format!("{}: unreadable response: {e}", self.name)))?;

        let message = &json["choices"][0]["message"];
        let finish = json["choices"][0]["finish_reason"].as_str().unwrap_or("");

        // Prefer `content`. Reasoning models on OpenRouter's free tier often
        // return an empty or null `content` while putting text in `reasoning`,
        // especially when the token budget ran out mid-thought, so fall back to
        // that rather than discarding a usable answer.
        let text = message["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                message["reasoning"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
            });

        match text {
            Some(t) => Ok(t.to_string()),
            // Retryable, not fatal: an empty completion is this provider
            // failing to answer, and the next one in the chain may well do
            // better. Treating it as fatal would abort the whole chain over a
            // truncated reasoning trace.
            None if finish == "length" => Err(ProviderError::Retryable(format!(
                "{}: hit the token limit before producing any content",
                self.name
            ))),
            None => Err(ProviderError::Retryable(format!(
                "{}: returned no content",
                self.name
            ))),
        }
    }

    fn available(&self) -> bool {
        !self.needs_key || self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: no API key (run `leo model login {}`)",
            self.name, self.name
        )
    }

    fn max_tokens(&self) -> Option<u32> {
        Some(self.max_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::ChatProvider;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn max_bytes_is_not_applicable_but_max_tokens_reflects_config() {
        let cfg = ProviderConfig {
            max_tokens: Some(777),
            ..Default::default()
        };
        let provider = OpenAiChat::new("ollama".to_string(), &cfg, None);
        assert_eq!(ChatProvider::max_tokens(&provider), Some(777));
    }

    #[test]
    fn unavailable_reason_never_contains_the_key_value() {
        let store = MemoryStore::default();
        store.set("openrouter", "sk-super-secret-value").unwrap();
        let key = resolve("openrouter", None, &store);
        let cfg = ProviderConfig {
            key_env: Some("LEO_TEST_UNUSED_KEY_ENV".to_string()),
            ..Default::default()
        };
        let provider = OpenAiChat::new("openrouter".to_string(), &cfg, key);
        assert!(!provider
            .unavailable_reason()
            .contains("sk-super-secret-value"));
    }
}
