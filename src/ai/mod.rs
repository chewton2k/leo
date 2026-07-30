pub mod chain;
pub mod chat;
pub mod error;
pub mod provider;
pub mod transcribe;

use std::path::Path;

use anyhow::Result;

use crate::config::secret::KeyringStore;
use crate::config::Config;

/// Token budget for full note structuring.
const STRUCTURE_MAX_TOKENS: u32 = 4096;
/// Token budget for expanding one inline @leo prompt.
const EXPAND_MAX_TOKENS: u32 = 2000;

/// Print each degradation so a silent downgrade is never invisible.
fn report(outcome: &chain::ChainOutcome<String>) {
    for f in &outcome.fallbacks {
        eprintln!("  {} unavailable, using {} ({})", f.from, f.to, f.reason);
    }
}

fn context() -> (Config, KeyringStore) {
    (Config::load(), KeyringStore)
}

/// Transcribe an audio file of any length through the configured chain.
pub fn transcribe(audio_path: &Path) -> Result<String> {
    let (cfg, store) = context();
    let outcome = transcribe::run(&cfg, &store, audio_path)?;
    report(&outcome);
    Ok(outcome.value)
}

/// Structure a raw transcript into organized notes. Returns (title, body).
pub fn structure_notes(transcript: &str) -> Result<(String, String)> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_structure_prompt(transcript),
        STRUCTURE_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(chat::split_title_body(&outcome.value))
}

/// Structure a new transcript as an addition to an existing note.
pub fn structure_notes_append(transcript: &str, existing_body: &str) -> Result<String> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_append_prompt(transcript, existing_body),
        STRUCTURE_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(outcome.value.trim().to_string())
}

/// Expand a single `@leo` prompt in place.
pub fn expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> Result<String> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_expand_prompt(question, local_context, note_title, full_body),
        EXPAND_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(outcome.value.trim().to_string())
}
