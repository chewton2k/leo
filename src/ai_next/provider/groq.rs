use std::path::Path;

use crate::ai_next::error::{
    classify_reqwest, classify_status, scrub_secret, ProviderError, ProviderResult,
};
use crate::ai_next::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const MAX_BYTES: u64 = 20 * 1024 * 1024;
const URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

pub struct GroqTranscribe {
    name: String,
    model: String,
    key: Option<Secret>,
}

impl GroqTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        GroqTranscribe {
            name,
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| "whisper-large-v3-turbo".to_string()),
            key,
        }
    }
}

impl TranscribeProvider for GroqTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| ProviderError::Fatal(format!("{}: no API key", self.name)))?;

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let file_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();
        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;

        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str("audio/wav")
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let form = reqwest::blocking::multipart::Form::new()
            .text("model", self.model.clone())
            .part("file", part);

        let resp = client
            .post(URL)
            .header("Authorization", format!("Bearer {}", key.as_str()))
            .multipart(form)
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            // Scrub defensively in case a misbehaving gateway reflects the
            // key back inside the body itself.
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, Some(key.as_str()));
            return Err(classify_status(status, &self.name, &text));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| ProviderError::Fatal(format!("{}: unreadable response: {e}", self.name)))?;

        json["text"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| ProviderError::Fatal(format!("{}: unexpected response shape", self.name)))
    }

    fn max_bytes(&self) -> Option<u64> {
        Some(MAX_BYTES)
    }

    fn available(&self) -> bool {
        self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!("{}: no API key (run `leo model login {}`)", self.name, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn max_bytes_is_twenty_megabytes() {
        let cfg = ProviderConfig::default();
        let provider = GroqTranscribe::new("groq".to_string(), &cfg, None);
        assert_eq!(provider.max_bytes(), Some(20 * 1024 * 1024));
    }

    #[test]
    fn available_requires_a_key() {
        let cfg = ProviderConfig::default();
        assert!(!GroqTranscribe::new("groq".to_string(), &cfg, None).available());

        let store = MemoryStore::default();
        store.set("groq", "a-key").unwrap();
        let key = resolve("groq", None, &store);
        assert!(GroqTranscribe::new("groq".to_string(), &cfg, key).available());
    }

    #[test]
    fn unavailable_reason_never_contains_the_key_value() {
        let store = MemoryStore::default();
        store.set("groq", "sk-super-secret-value").unwrap();
        let key = resolve("groq", None, &store);
        let cfg = ProviderConfig::default();
        let provider = GroqTranscribe::new("groq".to_string(), &cfg, key);
        assert!(!provider
            .unavailable_reason()
            .contains("sk-super-secret-value"));
    }
}
