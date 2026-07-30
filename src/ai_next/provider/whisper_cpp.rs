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

    /// A cheap, local, no-subprocess PATH scan — portable to Windows (unlike
    /// shelling out to `which`, which does not exist there) and cheaper than
    /// spawning a process just to answer a yes/no question that's asked once
    /// per `available()`/`unavailable_reason()` call.
    fn binary_on_path(&self) -> bool {
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| {
            if dir.join(&self.bin).is_file() {
                return true;
            }
            // Windows resolves a bare name via PATHEXT; `.exe` covers the
            // common case without pulling in the full PATHEXT list.
            cfg!(windows) && dir.join(format!("{}.exe", self.bin)).is_file()
        })
    }
}

impl TranscribeProvider for WhisperCppTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let output = Command::new(&self.bin)
            .arg("-m")
            .arg(&self.model_path)
            .arg("-f")
            .arg(audio_path)
            .arg("-nt") // no timestamps — we want prose
            .arg("-np") // no progress chatter on stdout
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_bytes_is_none_no_size_limit() {
        let cfg = ProviderConfig::default();
        let provider = WhisperCppTranscribe::new("whisper_cpp".to_string(), &cfg);
        assert_eq!(provider.max_bytes(), None);
    }

    #[test]
    fn missing_binary_and_missing_model_are_unavailable() {
        let cfg = ProviderConfig {
            bin: Some("leo-definitely-not-a-real-binary".to_string()),
            model_path: Some("/nonexistent/model.bin".to_string()),
            ..Default::default()
        };
        let provider = WhisperCppTranscribe::new("whisper_cpp".to_string(), &cfg);
        assert!(!provider.available());
    }

    // `sh` is on PATH on every unix CI/dev box this crate targets (macOS,
    // Linux); this exercises the real "binary present" branch of the PATH
    // scan without shelling out to `which`.
    #[cfg(unix)]
    #[test]
    fn present_binary_and_model_file_are_available() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let cfg = ProviderConfig {
            bin: Some("sh".to_string()),
            model_path: Some(model.path().to_string_lossy().to_string()),
            ..Default::default()
        };
        let provider = WhisperCppTranscribe::new("whisper_cpp".to_string(), &cfg);
        assert!(provider.available());
    }
}
