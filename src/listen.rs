use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use colored::Colorize;

/// Record audio from microphone. Returns path to the recorded WAV file.
/// Requires `sox` to be installed (provides the `rec` command).
pub fn record_audio() -> Result<PathBuf> {
    // Check if sox/rec is available
    if Command::new("rec")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        bail!(
            "Audio recording requires SoX. Install it:\n  \
             macOS:   brew install sox\n  \
             Linux:   sudo apt install sox\n  \
             Windows: choco install sox"
        );
    }

    let tmp_path = std::env::temp_dir().join("leo-recording.wav");

    // Remove stale recording if it exists
    let _ = std::fs::remove_file(&tmp_path);

    println!(
        "  {} {}",
        "Recording...".cyan().bold(),
        "press Enter to stop".dimmed()
    );

    // Start recording in background: 16kHz mono WAV (optimal for speech recognition)
    let mut child = Command::new("rec")
        .args([
            tmp_path.to_str().unwrap(),
            "rate",
            "16000",
            "channels",
            "1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start recording")?;

    // Wait for user to press Enter
    let mut buf = String::new();
    io::stdout().flush()?;
    io::stdin().read_line(&mut buf)?;

    // Stop recording
    child.kill().ok();
    child.wait().ok();

    if !tmp_path.exists() || std::fs::metadata(&tmp_path)?.len() == 0 {
        bail!("Recording failed — no audio captured.");
    }

    let size = std::fs::metadata(&tmp_path)?.len();
    let secs = size / (16000 * 2); // 16kHz, 16-bit mono
    println!(
        "  {} (~{}s, {:.1}MB)",
        "Recording stopped.".dimmed(),
        secs,
        size as f64 / (1024.0 * 1024.0)
    );

    Ok(tmp_path)
}
