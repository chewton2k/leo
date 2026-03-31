use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use colored::Colorize;

const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const GROQ_TRANSCRIPTION_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const HF_WHISPER_URL: &str =
    "https://router.huggingface.co/hf-inference/models/openai/whisper-large-v3-turbo";
const DEFAULT_CHAT_MODEL: &str = "google/gemini-2.5-flash";
const DEFAULT_TRANSCRIPTION_MODEL: &str = "whisper-large-v3-turbo";

/// Max file size per chunk for transcription APIs (~20MB, with headroom).
const MAX_CHUNK_BYTES: u64 = 20 * 1024 * 1024;
/// Chunk duration in seconds (~10 min of 16kHz mono 16-bit WAV ≈ 19MB).
const CHUNK_SECONDS: u64 = 600;

fn openrouter_key() -> Result<String> {
    std::env::var("OPENROUTER_API_KEY").context(
        "OPENROUTER_API_KEY not set. Add it to your .env file:\n  OPENROUTER_API_KEY=sk-or-...",
    )
}

fn chat_model() -> String {
    std::env::var("OPENROUTER_CHAT_MODEL").unwrap_or_else(|_| DEFAULT_CHAT_MODEL.to_string())
}

fn transcription_model() -> String {
    std::env::var("GROQ_TRANSCRIPTION_MODEL")
        .unwrap_or_else(|_| DEFAULT_TRANSCRIPTION_MODEL.to_string())
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Transcribe an audio file of any length. Automatically chunks long recordings.
pub fn transcribe(audio_path: &Path) -> Result<String> {
    let file_size = std::fs::metadata(audio_path)?.len();

    if file_size <= MAX_CHUNK_BYTES {
        return transcribe_single(audio_path);
    }

    // Split into chunks and transcribe each
    // WAV 16kHz mono 16-bit ≈ 32000 bytes/sec
    let approx_duration = file_size / 32000;
    let num_chunks = (approx_duration / CHUNK_SECONDS) + 1;

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
    let mut groq_failed = false;

    for i in 0..num_chunks {
        let start = i * CHUNK_SECONDS;
        let chunk_path = std::env::temp_dir().join(format!("leo-chunk-{i}.wav"));

        let status = Command::new("sox")
            .args([
                audio_path.to_str().unwrap(),
                chunk_path.to_str().unwrap(),
                "trim",
                &start.to_string(),
                &CHUNK_SECONDS.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if status.is_err() || !chunk_path.exists() {
            break;
        }

        // Skip empty/tiny chunks (end of file)
        if std::fs::metadata(&chunk_path).map(|m| m.len()).unwrap_or(0) < 1000 {
            let _ = std::fs::remove_file(&chunk_path);
            break;
        }

        eprintln!(
            "  {}",
            format!("Transcribing chunk {}/{}...", i + 1, num_chunks).cyan()
        );

        let result = if groq_failed {
            // Skip Groq if it already failed (rate limit is per-day)
            transcribe_single_hf(&chunk_path)
        } else {
            match transcribe_single_groq(&chunk_path) {
                Ok(text) => Ok(text),
                Err(e) => {
                    let msg = format!("{e}");
                    if msg.contains("413") || msg.contains("429") || msg.contains("rate_limit") {
                        eprintln!(
                            "  {}",
                            "Groq rate-limited, using Hugging Face for remaining chunks..."
                                .yellow()
                        );
                        groq_failed = true;
                        transcribe_single_hf(&chunk_path)
                    } else {
                        Err(e)
                    }
                }
            }
        };

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
         - Group related points under ## headings\n\
         - Keep it concise but don't lose important details\n\n\
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

// ── Single-file transcription with fallback ────────────────────────────────

/// Transcribe a single file (must be under MAX_CHUNK_BYTES).
fn transcribe_single(audio_path: &Path) -> Result<String> {
    // Try Groq first if key is set
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        if !key.is_empty() && key != "your-groq-key-here" {
            match transcribe_groq(audio_path, &key) {
                Ok(text) => return Ok(text),
                Err(e) => {
                    let msg = format!("{e}");
                    if msg.contains("413") || msg.contains("429") || msg.contains("rate_limit") {
                        eprintln!(
                            "  {}",
                            "Groq rate-limited, falling back to Hugging Face...".yellow(),
                        );
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    transcribe_huggingface(audio_path)
}

fn transcribe_single_groq(audio_path: &Path) -> Result<String> {
    let key = std::env::var("GROQ_API_KEY").context("GROQ_API_KEY not set")?;
    transcribe_groq(audio_path, &key)
}

fn transcribe_single_hf(audio_path: &Path) -> Result<String> {
    transcribe_huggingface(audio_path)
}

// ── Provider implementations ───────────────────────────────────────────────

fn transcribe_groq(audio_path: &Path, key: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let file_bytes = std::fs::read(audio_path)?;
    let file_name = file_name_str(audio_path);

    let form = reqwest::blocking::multipart::Form::new()
        .text("model", transcription_model())
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(file_bytes)
                .file_name(file_name)
                .mime_str("audio/wav")?,
        );

    let resp = client
        .post(GROQ_TRANSCRIPTION_URL)
        .header("Authorization", format!("Bearer {key}"))
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
        .map(|s| s.to_string())
        .context("Unexpected Groq API response format")
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

fn file_name_str(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}
