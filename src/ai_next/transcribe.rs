use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::ai_next::chain::{run_transcribe_chain, ChainOutcome};
use crate::ai_next::error::ProviderError;
use crate::ai_next::provider::{build_transcribe_chain, TranscribeProvider};
use crate::config::secret::SecretStore;
use crate::config::Config;

/// One slice of a long recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSpec {
    pub start_secs: u64,
    pub duration_secs: u64,
}

/// Never produce a chunk shorter than this — a cap so small it would generate
/// thousands of requests means something is wrong with the size estimate.
const MIN_CHUNK_SECS: u64 = 30;

/// Split a recording into slices that each fit under a provider's request cap.
///
/// Chunk length is derived from the file's actual byte rate rather than a
/// hardcoded constant, so it is correct for both 16-bit and 32-bit float WAVs
/// (`rec` on macOS writes 32-bit float, at double the byte rate).
pub fn plan_chunks(file_size: u64, duration_secs: u64, max_bytes: Option<u64>) -> Vec<ChunkSpec> {
    let whole = vec![ChunkSpec {
        start_secs: 0,
        duration_secs,
    }];

    let Some(max_bytes) = max_bytes else {
        return whole; // No request-size limit: local provider reads the file.
    };
    if file_size <= max_bytes || duration_secs == 0 {
        return whole;
    }

    let byte_rate = (file_size / duration_secs).max(1);
    let chunk_secs = (max_bytes / byte_rate).max(MIN_CHUNK_SECS);

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < duration_secs {
        let remaining = duration_secs - start;
        let duration = remaining.min(chunk_secs);
        chunks.push(ChunkSpec {
            start_secs: start,
            duration_secs: duration,
        });
        start += duration;
    }
    chunks
}

/// Read a WAV's true duration via sox. Returns None if sox is absent or the
/// header's DataSize was never finalized (sox reports 0 in that case).
fn wav_duration_secs(path: &Path) -> Option<u64> {
    let output = Command::new("sox")
        .args(["--i", "-D", path.to_str()?])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .map(|d| d as u64)
        .filter(|&d| d > 0)
}

/// Cut one slice out of a WAV with sox. Returns the temp file's path.
fn cut_chunk(
    source: &Path,
    spec: &ChunkSpec,
    index: usize,
) -> Result<std::path::PathBuf, ProviderError> {
    let out = std::env::temp_dir().join(format!("leo-chunk-{index}.wav"));
    let status = Command::new("sox")
        .arg(source)
        .arg(&out)
        .arg("trim")
        .arg(spec.start_secs.to_string())
        .arg(spec.duration_secs.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| ProviderError::Fatal(format!("sox not available: {e}")))?;

    if !status.success() || !out.exists() {
        return Err(ProviderError::Fatal(format!(
            "sox trim failed at {}s",
            spec.start_secs
        )));
    }
    Ok(out)
}

/// Transcribe one file with one provider, chunking only if that provider has a
/// size limit the file exceeds.
fn transcribe_with(
    provider: &dyn TranscribeProvider,
    audio_path: &Path,
) -> Result<String, ProviderError> {
    let file_size = std::fs::metadata(audio_path)
        .map_err(|e| ProviderError::Fatal(format!("cannot stat audio: {e}")))?
        .len();

    // Fall back to a byte-rate estimate if sox cannot read the header.
    let duration = wav_duration_secs(audio_path)
        .unwrap_or_else(|| file_size.saturating_sub(44) / 32000);

    let chunks = plan_chunks(file_size, duration, provider.max_bytes());

    if chunks.len() == 1 {
        return provider.transcribe(audio_path);
    }

    eprintln!(
        "  Long recording (~{}min), splitting into {} chunks for {}...",
        duration / 60,
        chunks.len(),
        provider.name()
    );

    let mut full = String::new();
    for (i, spec) in chunks.iter().enumerate() {
        let path = cut_chunk(audio_path, spec, i)?;

        // A chunk that is header-only means the recording ended early.
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size < 100 {
            let _ = std::fs::remove_file(&path);
            break;
        }

        eprintln!("  Transcribing chunk {}/{}...", i + 1, chunks.len());
        let result = provider.transcribe(&path);
        let _ = std::fs::remove_file(&path);

        match result {
            Ok(text) => {
                if !full.is_empty() {
                    full.push(' ');
                }
                full.push_str(&text);
            }
            // Surface the failure so the chain can move to the next provider
            // rather than returning a half-finished transcript.
            Err(e) => return Err(e),
        }
    }

    if full.trim().is_empty() {
        return Err(ProviderError::Retryable(format!(
            "{}: no speech detected",
            provider.name()
        )));
    }
    Ok(full)
}

/// Transcribe through the configured chain.
pub fn run(
    cfg: &Config,
    store: &dyn SecretStore,
    audio_path: &Path,
) -> Result<ChainOutcome<String>> {
    let providers = build_transcribe_chain(cfg, store);
    run_transcribe_chain(providers, audio_path, transcribe_with)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn no_limit_means_a_single_chunk() {
        // The local-binary case: reading the file directly, so never split.
        let chunks = plan_chunks(500 * MB, 3600, None);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_secs, 0);
        assert_eq!(chunks[0].duration_secs, 3600);
    }

    #[test]
    fn a_file_under_the_limit_is_a_single_chunk() {
        let chunks = plan_chunks(5 * MB, 300, Some(20 * MB));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].duration_secs, 300);
    }

    #[test]
    fn a_long_recording_is_split_by_actual_byte_rate() {
        // 60 minutes at 64000 bytes/sec (32-bit float, what `rec` writes on
        // macOS) = ~220MB. At a 20MB cap that is ~12 chunks.
        let duration = 3600;
        let size = duration * 64000;
        let chunks = plan_chunks(size, duration, Some(20 * MB));
        assert!(chunks.len() >= 10, "got {} chunks", chunks.len());
        // Every chunk must fit under the cap.
        for c in &chunks {
            assert!(c.duration_secs * 64000 <= 20 * MB, "chunk too big: {c:?}");
        }
    }

    #[test]
    fn chunks_are_contiguous_and_cover_the_whole_recording() {
        let duration = 1800;
        let size = duration * 64000;
        let chunks = plan_chunks(size, duration, Some(20 * MB));

        assert_eq!(chunks[0].start_secs, 0);
        for pair in chunks.windows(2) {
            assert_eq!(
                pair[0].start_secs + pair[0].duration_secs,
                pair[1].start_secs,
                "gap or overlap between {:?} and {:?}",
                pair[0],
                pair[1]
            );
        }
        let last = chunks.last().unwrap();
        assert_eq!(last.start_secs + last.duration_secs, duration);
    }

    #[test]
    fn a_16_bit_recording_gets_longer_chunks_than_a_32_bit_one() {
        // Chunk length must follow the real byte rate, not a hardcoded guess.
        let duration = 3600;
        let narrow = plan_chunks(duration * 64000, duration, Some(20 * MB));
        let wide = plan_chunks(duration * 32000, duration, Some(20 * MB));
        assert!(
            wide[0].duration_secs > narrow[0].duration_secs,
            "16-bit chunk {} should exceed 32-bit chunk {}",
            wide[0].duration_secs,
            narrow[0].duration_secs
        );
    }

    #[test]
    fn zero_duration_does_not_divide_by_zero() {
        let chunks = plan_chunks(1000, 0, Some(20 * MB));
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn an_empty_file_yields_one_chunk_not_a_panic() {
        let chunks = plan_chunks(0, 0, Some(20 * MB));
        assert_eq!(chunks.len(), 1);
    }
}
