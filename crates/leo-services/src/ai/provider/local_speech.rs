use super::TranscribeProvider;
use crate::ai::error::{ProviderError, ProviderResult};
use crate::config::provider::ProviderConfig;
use std::path::Path;

pub struct LocalSpeech {
    name: String,
}

impl LocalSpeech {
    pub fn new(name: String, _config: &ProviderConfig) -> Self {
        Self { name }
    }
}

impl TranscribeProvider for LocalSpeech {
    fn name(&self) -> &str {
        &self.name
    }
    fn available(&self) -> bool {
        crate::speech::library::library().is_ok_and(|l| l.selected().is_ok())
    }
    fn unavailable_reason(&self) -> String {
        "Download a model in Settings → Speech models. Your recorded audio is kept until one is ready.".into()
    }
    fn transcribe(&self, path: &Path) -> ProviderResult<String> {
        let run = || -> anyhow::Result<String> {
            let selected = crate::speech::library::library()?.selected()?;
            anyhow::ensure!(
                std::fs::metadata(path)?.len() <= 32 * 1024 * 1024,
                "This audio is too long. Record in shorter parts."
            );
            let pcm = {
                let bytes = std::fs::read(path)?;
                super::audio::read_wav(&bytes).map_err(anyhow::Error::msg)?
            };
            crate::speech::engine::engine().transcribe(
                selected,
                pcm,
                crate::speech::engine::Priority::Segment,
                Vec::new(),
            )
        };
        run().map_err(|e| ProviderError::Retryable(e.to_string()))
    }
}
