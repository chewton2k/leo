use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

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

    // Live stopwatch display
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);
    let start = Instant::now();

    let stopwatch = std::thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            let elapsed = start.elapsed().as_secs();
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            print!(
                "\r  {} {} {}",
                "Recording".cyan().bold(),
                format!("{:02}:{:02}", mins, secs).cyan().bold(),
                "press Enter to stop".dimmed()
            );
            io::stdout().flush().ok();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    // Wait for user to press Enter
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;

    // Stop stopwatch and capture final elapsed time
    running.store(false, Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs();
    stopwatch.join().ok();

    // Stop recording
    child.kill().ok();
    child.wait().ok();

    // Repair WAV header: `rec` is killed before it can write the final DataSize field,
    // leaving it as 0. sox --ignore-length reads to EOF and writes a correct header.
    let fixed = std::env::temp_dir().join("leo-recording-fixed.wav");
    let repaired = Command::new("sox")
        .args([
            "--ignore-length",
            tmp_path.to_str().unwrap(),
            fixed.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && fixed.exists())
        .unwrap_or(false);
    if repaired {
        let _ = std::fs::rename(&fixed, &tmp_path);
    }

    if !tmp_path.exists() || std::fs::metadata(&tmp_path)?.len() == 0 {
        bail!("Recording failed — no audio captured.");
    }

    let size = std::fs::metadata(&tmp_path)?.len();
    // Get actual duration from WAV header via sox; fall back to byte-rate estimate
    let file_secs = Command::new("sox")
        .args(["--i", "-D", tmp_path.to_str().unwrap()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .map(|d| d as u64)
        .unwrap_or_else(|| size / (16000 * 2));
    let file_mins = file_secs / 60;
    let duration = if file_mins > 0 {
        format!("~{}m{}s", file_mins, file_secs % 60)
    } else {
        format!("~{}s", file_secs)
    };

    // Overwrite stopwatch line with final summary
    let e_mins = elapsed / 60;
    let e_secs = elapsed % 60;
    print!("\r\x1b[2K\x1b[1A\x1b[2K");
    println!(
        "  {} {} ({}, {:.1}MB)",
        "Recorded".green(),
        format!("{:02}:{:02}", e_mins, e_secs).dimmed(),
        duration,
        size as f64 / (1024.0 * 1024.0)
    );

    Ok(tmp_path)
}
