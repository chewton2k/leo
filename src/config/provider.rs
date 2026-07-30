use serde::{Deserialize, Serialize};

/// Which wire protocol a provider speaks. Adding a provider that speaks an
/// existing protocol is a config-file change, not a code change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI-compatible chat completions: OpenRouter, Ollama, LM Studio, vLLM.
    Openai,
    /// Hugging Face inference, raw audio body.
    Hf,
    /// Groq transcription, multipart form.
    Groq,
    /// Local whisper.cpp binary.
    WhisperCpp,
}

/// One named provider from `[providers.<name>]`.
///
/// Fields are optional because they are kind-specific; validation happens when
/// the provider is constructed, so an unrelated missing field never blocks load.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub kind: Option<ProviderKind>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Name of the env var holding this provider's key. Never the key itself.
    #[serde(default)]
    pub key_env: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// whisper_cpp only: binary name or path.
    #[serde(default)]
    pub bin: Option<String>,
    /// whisper_cpp only: path to the ggml model file.
    #[serde(default)]
    pub model_path: Option<String>,
}

/// An ordered fallback chain for one task, from `[chat]` or `[transcribe]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaskChain {
    #[serde(default)]
    pub chain: Vec<String>,
}
