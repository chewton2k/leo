use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::ai::chain::{run_transcribe_chain, ChainOutcome};
use crate::ai::error::ProviderError;
use crate::ai::provider::{build_transcribe_chain, TranscribeProvider};
use crate::config::secret::SecretStore;
use crate::config::Config;

/// One slice of a long recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSpec {
    pub start_secs: u64,
    pub duration_secs: u64,
}

/// Split a recording into slices that each fit under a provider's request cap.
///
/// Chunk length is derived from the file's actual byte rate rather than a
/// hardcoded constant, so it is correct for both 16-bit and 32-bit float WAVs
/// (`rec` on macOS writes 32-bit float, at double the byte rate). The size cap
/// always wins over chunk count: there is deliberately no floor on chunk
/// length, since a high enough byte rate (multi-channel, high sample rate)
/// would otherwise push chunks back over `max_bytes`. A pathological file
/// just yields many small chunks — slow but correct, and the per-chunk
/// progress output makes that visible.
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
    let chunk_secs = (max_bytes / byte_rate).max(1);

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

/// Parse a canonical WAV header's byte rate directly, needing no external
/// process. Used only as a fallback for when sox is absent or cannot report
/// a duration (an interrupted recording leaves DataSize unfinalized) — sox
/// remains the primary source since it also handles non-canonical layouts
/// where the fmt chunk isn't at this fixed offset.
///
/// Field offsets in the first 44 bytes of a canonical WAV:
///   22-23 (u16 LE): channels
///   24-27 (u32 LE): sample rate
///   34-35 (u16 LE): bits per sample
/// byte rate = sample_rate * channels * (bits_per_sample / 8)
fn wav_byte_rate(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 {
        return None;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]) as u64;
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]) as u64;
    let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]) as u64;

    if channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
        return None;
    }
    Some(sample_rate * channels * (bits_per_sample / 8).max(1))
}

/// Cut one slice out of a WAV with sox. Returns the temp file's path.
fn cut_chunk(
    source: &Path,
    spec: &ChunkSpec,
    index: usize,
) -> Result<std::path::PathBuf, ProviderError> {
    // Include the process id so two concurrent `leo` sessions transcribing
    // long recordings don't overwrite each other's chunks mid-flight.
    let pid = std::process::id();
    let out = std::env::temp_dir().join(format!("leo-chunk-{pid}-{index}.wav"));
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
        // sox may have partially written `out` before failing — don't leak it.
        let _ = std::fs::remove_file(&out);
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

    // sox is the primary duration source — it handles non-canonical WAV
    // layouts too. Fall back to parsing the header's byte rate directly only
    // when sox is absent or cannot report a duration (e.g. an interrupted
    // recording left DataSize unfinalized). If both fail, don't guess: a
    // wrong guess here is what caused a prior chunk-sizing bug (see fix
    // round 1, C2) — surface a clear error naming the file instead.
    let duration = match wav_duration_secs(audio_path) {
        Some(d) => d,
        None => {
            let byte_rate = wav_byte_rate(audio_path).ok_or_else(|| {
                ProviderError::Fatal(format!(
                    "cannot determine duration of {}: sox unavailable and WAV header unreadable",
                    audio_path.display()
                ))
            })?;
            file_size.saturating_sub(44) / byte_rate
        }
    };

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

        // A failed chunk discards everything transcribed so far and surfaces
        // the failure so the chain can move to the next provider. This is
        // deliberate: the provider contract is one text per file, and
        // returning a transcript silently missing its back half would be
        // worse than a clear failure. The cost is that the next provider in
        // the chain re-transcribes the whole file from chunk 0, not just the
        // failed chunk onward.
        let text = result?;
        if !full.is_empty() {
            full.push(' ');
        }
        full.push_str(&text);
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
    fn a_high_byte_rate_never_exceeds_the_cap_even_for_short_chunks() {
        // 96kHz stereo 32-bit float = 768,000 B/s. A floor on chunk length
        // (e.g. "never under 30s") would push chunks over a 20MB cap here;
        // the cap must always win over chunk count.
        let byte_rate = 768_000;
        let duration = 3600;
        let size = duration * byte_rate;
        let chunks = plan_chunks(size, duration, Some(20 * MB));
        for c in &chunks {
            assert!(
                c.duration_secs * byte_rate <= 20 * MB,
                "chunk too big: {c:?} at {byte_rate} B/s"
            );
        }
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

    /// Build a synthetic 44-byte canonical WAV header with the given
    /// sample rate, channel count, and bits per sample.
    fn synthetic_wav_header(sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
        let mut h = vec![0u8; 44];
        h[0..4].copy_from_slice(b"RIFF");
        h[8..12].copy_from_slice(b"WAVE");
        h[22..24].copy_from_slice(&channels.to_le_bytes());
        h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
        h[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());
        h
    }

    fn write_temp_wav(bytes: &[u8]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn wav_byte_rate_reads_16khz_mono_16bit() {
        let f = write_temp_wav(&synthetic_wav_header(16000, 1, 16));
        assert_eq!(wav_byte_rate(f.path()), Some(32000));
    }

    #[test]
    fn wav_byte_rate_reads_16khz_mono_32bit_float() {
        let f = write_temp_wav(&synthetic_wav_header(16000, 1, 32));
        assert_eq!(wav_byte_rate(f.path()), Some(64000));
    }

    #[test]
    fn wav_byte_rate_reads_44100hz_stereo_16bit() {
        let f = write_temp_wav(&synthetic_wav_header(44100, 2, 16));
        assert_eq!(wav_byte_rate(f.path()), Some(176400));
    }

    #[test]
    fn wav_byte_rate_returns_none_for_a_truncated_file() {
        let f = write_temp_wav(&synthetic_wav_header(16000, 1, 16)[..30]);
        assert_eq!(wav_byte_rate(f.path()), None);
    }

    #[test]
    fn wav_byte_rate_returns_none_for_zero_sample_rate_not_a_panic() {
        let f = write_temp_wav(&synthetic_wav_header(0, 1, 16));
        assert_eq!(wav_byte_rate(f.path()), None);
    }
}
