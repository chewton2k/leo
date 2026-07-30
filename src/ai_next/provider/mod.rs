use std::path::Path;

use crate::ai_next::error::ProviderResult;

/// One chat completion request, provider-independent.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub prompt: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub trait ChatProvider {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String>;
    /// A cheap, local precondition check: key present, binary on PATH, port
    /// open. Performs no inference and makes no billable call.
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    /// Why this provider is unavailable, for the error shown when a whole
    /// chain is exhausted.
    fn unavailable_reason(&self) -> String {
        format!("{} is not configured", self.name())
    }
}

pub trait TranscribeProvider {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String>;
    /// Largest file this provider accepts in one request. `None` means no
    /// limit, which skips chunking entirely — the local-binary case.
    fn max_bytes(&self) -> Option<u64>;
    fn available(&self) -> bool;
    fn name(&self) -> &str;
    fn unavailable_reason(&self) -> String {
        format!("{} is not configured", self.name())
    }
}
