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
        read(path)
            .and_then(|(selected, pcm)| {
                crate::speech::engine::engine().transcribe(
                    selected,
                    pcm,
                    crate::speech::engine::Priority::Segment,
                    Vec::new(),
                )
            })
            .map_err(|e| ProviderError::Retryable(e.to_string()))
    }
}

impl LocalSpeech {
    pub fn transcribe_timed(&self, path: &Path) -> ProviderResult<Vec<(u64, u64, String)>> {
        read(path)
            .and_then(|(selected, pcm)| {
                crate::speech::engine::engine().transcribe_timed(
                    selected,
                    pcm,
                    crate::speech::engine::Priority::Segment,
                    Vec::new(),
                )
            })
            .map_err(|e| ProviderError::Retryable(e.to_string()))
    }
}

fn read(path: &Path) -> anyhow::Result<(crate::speech::library::Selected, Vec<f32>)> {
    let selected = crate::speech::library::library()?.selected()?;
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 32 * 1024 * 1024,
        "This audio is too long. Record in shorter parts."
    );
    let bytes = std::fs::read(path)?;
    let pcm = super::audio::read_wav(&bytes).map_err(anyhow::Error::msg)?;
    Ok((selected, pcm))
}
