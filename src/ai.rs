use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use colored::Colorize;

const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const HF_WHISPER_URL: &str =
    "https://router.huggingface.co/hf-inference/models/openai/whisper-large-v3-turbo";
const GROQ_WHISPER_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const DEFAULT_CHAT_MODEL: &str = "google/gemini-2.5-flash";

/// Max file size per chunk for transcription APIs (~20MB, with headroom).
const MAX_CHUNK_BYTES: u64 = 20 * 1024 * 1024;

/// Get the actual duration of a WAV file in seconds using sox.
fn wav_duration_secs(path: &Path) -> Option<u64> {
    let output = Command::new("sox")
        .args(["--i", "-D", path.to_str()?])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse::<f64>().ok().map(|d| d as u64)
}

fn openrouter_key() -> Result<String> {
    std::env::var("OPENROUTER_API_KEY").context(
        "OPENROUTER_API_KEY not set. Add it to your .env file:\n  OPENROUTER_API_KEY=sk-or-...",
    )
}

fn chat_model() -> String {
    std::env::var("OPENROUTER_CHAT_MODEL").unwrap_or_else(|_| DEFAULT_CHAT_MODEL.to_string())
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Transcribe an audio file of any length. Automatically chunks long recordings.
pub fn transcribe(audio_path: &Path) -> Result<String> {
    let file_size = std::fs::metadata(audio_path)?.len();

    if file_size <= MAX_CHUNK_BYTES {
        return transcribe_single(audio_path);
    }

    // Get actual duration from WAV header; fall back to byte-rate estimate.
    // Filter out 0: sox reports 0 when the WAV header DataSize field wasn't finalized.
    let approx_duration = wav_duration_secs(audio_path)
        .filter(|&d| d > 0)
        .unwrap_or_else(|| (file_size.saturating_sub(44)) / 32000);

    // Derive chunk duration from actual byte rate so it works for both 16-bit and
    // 32-bit audio (rec on macOS defaults to 32-bit float = 64000 bytes/sec).
    let byte_rate = file_size / approx_duration.max(1);
    let chunk_secs = (MAX_CHUNK_BYTES / byte_rate.max(1)).max(30);
    let num_chunks = (approx_duration / chunk_secs) + 1;

    eprintln!(
        "  {}",
        format!(
            "Long recording (~{}min), splitting into {} chunks...",
            approx_duration / 60,
            num_chunks
        )
        .cyan()
    );

    let mut full_transcript = String::new();

    for i in 0..num_chunks {
        let start = i * chunk_secs;
        let remaining = approx_duration.saturating_sub(start);
        if remaining == 0 {
            break;
        }
        let duration = remaining.min(chunk_secs);
        let chunk_path = std::env::temp_dir().join(format!("leo-chunk-{i}.wav"));

        let status = Command::new("sox")
            .args([
                audio_path.to_str().unwrap(),
                chunk_path.to_str().unwrap(),
                "trim",
                &start.to_string(),
                &duration.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        let ok = match &status {
            Ok(s) => s.success() && chunk_path.exists(),
            Err(_) => false,
        };
        if !ok {
            eprintln!(
                "  {}",
                format!("Chunk {}/{} — sox trim failed, stopping.", i + 1, num_chunks)
                    .yellow()
            );
            break;
        }

        // Skip empty chunks (WAV header alone is 44 bytes)
        let chunk_size = std::fs::metadata(&chunk_path).map(|m| m.len()).unwrap_or(0);
        if chunk_size < 100 {
            let _ = std::fs::remove_file(&chunk_path);
            eprintln!(
                "  {}",
                format!(
                    "Chunk {}/{} — empty ({}B), no more audio. Stopping.",
                    i + 1,
                    num_chunks,
                    chunk_size
                )
                .yellow()
            );
            break;
        }

        eprintln!(
            "  {}",
            format!("Transcribing chunk {}/{}...", i + 1, num_chunks).cyan()
        );

        let result = transcribe_with_fallback(&chunk_path);
        let _ = std::fs::remove_file(&chunk_path);

        match result {
            Ok(text) => {
                if !full_transcript.is_empty() {
                    full_transcript.push(' ');
                }
                full_transcript.push_str(&text);
            }
            Err(e) => return Err(e),
        }
    }

    if full_transcript.is_empty() {
        bail!("No speech detected in recording.");
    }

    Ok(full_transcript)
}

/// Use an LLM via OpenRouter to structure a raw transcript into organized notes.
/// Returns (title, body) for the note.
pub fn structure_notes(transcript: &str) -> Result<(String, String)> {
    let key = openrouter_key()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let prompt = format!(
        "You are a note-taking assistant. Given the following transcript from a lecture or meeting, \
         create well-structured notes in Markdown format.\n\n\
         Rules:\n\
         - The FIRST line must be ONLY a concise title (no # prefix, no formatting, just plain text)\n\
         - Follow it with a blank line, then the structured body\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Make tables when grouping like ideas\n\
         - Group related points under ## headings\n\
         - There will sometimes be noise in the transcription so make sure to filter out any extraneous information not related to the main topic \n\
         - Interweave your own notes with the structured output where you deem helpful \n\
         - Don't lose important details and capture notes that are meaningful\n\n\
         Transcript:\n{transcript}"
    );

    let body = serde_json::json!({
        "model": chat_model(),
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.3,
        "max_tokens": 10000
    });

    let url = format!("{OPENROUTER_BASE}/chat/completions");

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/leo-cli")
        .header("X-Title", "leo")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Failed to reach OpenRouter API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        bail!("OpenRouter API error ({status}): {body}");
    }

    let json: serde_json::Value = resp.json()?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .context("Unexpected API response format")?;

    // Split first line as title, rest as body
    let mut lines = content.lines();
    let title = lines
        .next()
        .unwrap_or("Untitled Notes")
        .trim_start_matches('#')
        .trim()
        .to_string();

    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_string();

    Ok((title, body))
}

/// Structure a new transcript as an addition to an existing note.
/// Returns only the new body content to append.
pub fn structure_notes_append(transcript: &str, existing_body: &str) -> Result<String> {
    let key = openrouter_key()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let prompt = format!(
        "You are a note-taking assistant. You are adding to an EXISTING note. \
         Given the existing notes and a new transcript, create well-structured notes \
         for ONLY the new content in Markdown format.\n\n\
         Rules:\n\
         - Do NOT include a title — this will be appended to an existing note\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Group related points under ## headings\n\
         - Filter out noise from transcription\n\
         - Keep it concise but don't lose important details\n\
         - Avoid duplicating information already in the existing notes\n\
         - Use the same style and structure as the existing notes\n\n\
         Existing notes:\n{existing_body}\n\n\
         New transcript:\n{transcript}"
    );

    let body = serde_json::json!({
        "model": chat_model(),
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.3,
        "max_tokens": 10000
    });

    let url = format!("{OPENROUTER_BASE}/chat/completions");

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/leo-cli")
        .header("X-Title", "leo")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Failed to reach OpenRouter API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        bail!("OpenRouter API error ({status}): {body}");
    }

    let json: serde_json::Value = resp.json()?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .context("Unexpected API response format")?;

    Ok(content.trim().to_string())
}

// ── Transcription ─────────────────────────────────────────────────────────

/// Try HF first; on 402 (credits exhausted) fall back to Groq automatically.
fn transcribe_with_fallback(audio_path: &Path) -> Result<String> {
    match transcribe_huggingface(audio_path) {
        Ok(text) => Ok(text),
        Err(e) if e.to_string().contains("402") => {
            eprintln!(
                "  {}",
                "HF credits exhausted, switching to Groq...".yellow()
            );
            transcribe_groq(audio_path)
        }
        Err(e) => Err(e),
    }
}

fn transcribe_single(audio_path: &Path) -> Result<String> {
    transcribe_with_fallback(audio_path)
}

fn transcribe_huggingface(audio_path: &Path) -> Result<String> {
    let token = std::env::var("HF_API_KEY").context(
        "HF_API_KEY not set (free at https://huggingface.co/settings/tokens).\n  Add it to your .env file:\n  HF_API_KEY=hf_...",
    )?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let file_bytes = std::fs::read(audio_path)?;

    let resp = client
        .post(HF_WHISPER_URL)
        .header("Content-Type", "audio/wav")
        .header("Authorization", format!("Bearer {token}"))
        .body(file_bytes)
        .send()
        .context("Failed to reach Hugging Face API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        bail!("Hugging Face transcription error ({status}): {body}");
    }

    let json: serde_json::Value = resp.json()?;
    json["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .context("Unexpected Hugging Face API response format")
}

fn transcribe_groq(audio_path: &Path) -> Result<String> {
    let token = std::env::var("GROQ_API_KEY").context(
        "GROQ_API_KEY not set. Add it to your .env file:\n  GROQ_API_KEY=gsk_...",
    )?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let file_name = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();
    let file_bytes = std::fs::read(audio_path)?;

    let form = reqwest::blocking::multipart::Form::new()
        .text("model", "whisper-large-v3-turbo")
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(file_bytes)
                .file_name(file_name)
                .mime_str("audio/wav")?,
        );

    let resp = client
        .post(GROQ_WHISPER_URL)
        .header("Authorization", format!("Bearer {token}"))
        .multipart(form)
        .send()
        .context("Failed to reach Groq API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        bail!("Groq transcription error ({status}): {body}");
    }

    let json: serde_json::Value = resp.json()?;
    json["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .context("Unexpected Groq API response format")
}

/// Expand a single `@leo` prompt in-place, using the full note as background context
/// and nearby lines as local context.
pub fn expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> Result<String> {
    let key = openrouter_key()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let prompt = format!(
        "You are a note-taking assistant helping expand a specific section of lecture notes.\n\n\
         Topic: {note_title}\n\n\
         Full lecture notes (for background):\n{full_body}\n\n\
         Local context around the question:\n{local_context}\n\n\
         Question to expand on:\n{question}\n\n\
         Rules:\n\
         - Answer concisely in markdown (bullet points, short paragraphs)\n\
         - Tie your answer back to the lecture context where relevant\n\
         - Do not repeat what's already in the notes\n\
         - Return only the expanded content, no preamble"
    );

    let body = serde_json::json!({
        "model": chat_model(),
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
        "max_tokens": 2000
    });

    let url = format!("{OPENROUTER_BASE}/chat/completions");

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/leo-cli")
        .header("X-Title", "leo")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("Failed to reach OpenRouter API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        bail!("OpenRouter API error ({status}): {body}");
    }

    let json: serde_json::Value = resp.json()?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .context("Unexpected API response format")?;

    Ok(content.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke-test: function exists and accepts the right types.
    // Full integration test requires a live API key — run manually with `cargo run`.
    #[test]
    fn expand_prompt_signature_compiles() {
        // If this compiles, the function signature is correct.
        let _f: fn(&str, &str, &str, &str) -> anyhow::Result<String> = expand_prompt;
    }
}

