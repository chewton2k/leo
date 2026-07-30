use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ai_next::error::{ProviderError, ProviderResult};
use crate::ai_next::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;

/// Local whisper.cpp. Free, offline, and — because it reads the file directly
/// — subject to no request-size limit, so `max_bytes()` is None and the
/// chunking path is skipped entirely.
pub struct WhisperCppTranscribe {
    name: String,
    bin: String,
    model_path: PathBuf,
}

/// Expand a leading `~` so config files can use it.
fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

impl WhisperCppTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig) -> Self {
        WhisperCppTranscribe {
            name,
            bin: cfg.bin.clone().unwrap_or_else(|| "whisper-cli".to_string()),
            model_path: expand_tilde(cfg.model_path.as_deref().unwrap_or("")),
        }
    }

    fn binary_on_path(&self) -> bool {
        Command::new("which")
            .arg(&self.bin)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl TranscribeProvider for WhisperCppTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let output = Command::new(&self.bin)
            .args([
                "-m",
                &self.model_path.to_string_lossy(),
                "-f",
                &audio_path.to_string_lossy(),
                "-nt", // no timestamps — we want prose
                "-np", // no progress chatter on stdout
            ])
            .output()
            .map_err(|e| {
                // Treat a failure to launch as retryable: the next provider in
                // the chain may well work.
                ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Retryable(format!(
                "{}: {} exited with {}: {}",
                self.name,
                self.bin,
                output.status,
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(ProviderError::Retryable(format!(
                "{}: produced no output",
                self.name
            )));
        }
        Ok(text)
    }

    fn max_bytes(&self) -> Option<u64> {
        None
    }

    fn available(&self) -> bool {
        self.binary_on_path() && self.model_path.is_file()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        if !self.binary_on_path() {
            format!(
                "{}: `{}` not on PATH (brew install whisper-cpp)",
                self.name, self.bin
            )
        } else {
            format!(
                "{}: model file not found at {}",
                self.name,
                self.model_path.display()
            )
        }
    }
}
