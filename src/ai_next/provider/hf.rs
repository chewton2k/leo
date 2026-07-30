use std::path::Path;

use crate::ai_next::error::{classify_reqwest, classify_status, ProviderError, ProviderResult};
use crate::ai_next::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

/// Per-request size cap, with headroom under the API's real limit.
const MAX_BYTES: u64 = 20 * 1024 * 1024;

pub struct HfTranscribe {
    name: String,
    model: String,
    key: Option<Secret>,
}

impl HfTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        HfTranscribe {
            name,
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| "openai/whisper-large-v3-turbo".to_string()),
            key,
        }
    }
}

impl TranscribeProvider for HfTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| ProviderError::Fatal(format!("{}: no API key", self.name)))?;

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;

        let url = format!(
            "https://router.huggingface.co/hf-inference/models/{}",
            self.model
        );

        let resp = client
            .post(&url)
            .header("Content-Type", "audio/wav")
            .header("Authorization", format!("Bearer {}", key.as_str()))
            .body(bytes)
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
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
