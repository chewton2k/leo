use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use colored::Colorize;

/// Record audio from microphone or system audio device.
/// Returns path to the recorded WAV file.
/// Requires `sox` to be installed (provides the `rec` command).
/// For screen audio, also requires BlackHole: `brew install blackhole-2ch`.
pub fn record_audio(screen: bool) -> Result<PathBuf> {
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

    // Resolve audio device for screen mode
    let device = if screen {
        Some(
            std::env::var("LEO_SCREEN_DEVICE")
                .unwrap_or_else(|_| "BlackHole 2ch".to_string()),
        )
    } else {
        None
    };

    let path_str = tmp_path.to_str().context("temp path is not valid UTF-8")?;

    // Build rec args: <output> rate 16000 channels 1
    // Device selection uses AUDIODEV env var (not -d flag, which means --default-device on macOS)
    let rec_args = [path_str, "rate", "16000", "channels", "1"];

    // Start recording in background
    let mut cmd = Command::new("rec");
    if let Some(ref dev) = device {
        cmd.env("AUDIODEV", dev);
    }
    let mut child = cmd
        .args(rec_args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            if let Some(ref dev) = device {
                format!(
                    "Failed to start recording from screen audio device '{dev}'.\n  \
                     Set up system audio capture:\n  \
                     1. brew install blackhole-2ch\n  \
                     2. Open Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                     3. Set that Multi-Output Device as System Output in Sound Settings\n  \
                     To use a different device: add LEO_SCREEN_DEVICE=<name> to your .env"
                )
            } else {
                "Failed to start recording".to_string()
            }
        })?;

    // Live stopwatch display
    let label = if screen { "Recording screen" } else { "Recording" };
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
                label.cyan().bold(),
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
        if let Some(ref dev) = device {
            bail!(
                "Recording failed — no audio captured from screen audio device '{dev}'.\n  \
                 Check your setup:\n  \
                 1. brew install blackhole-2ch\n  \
                 2. Open Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                 3. Set that Multi-Output Device as System Output in Sound Settings\n  \
                 To use a different device: add LEO_SCREEN_DEVICE=<name> to your .env"
            );
        }
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

/// Where the in-progress recording is written. One fixed path, since only one
/// recording can run at a time.
pub fn recording_path() -> PathBuf {
    std::env::temp_dir().join("leo-recording.wav")
}

/// Check that SoX is installed, with the install command in the error.
fn require_sox() -> Result<()> {
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
    Ok(())
}

/// A recording in progress, owned by whoever started it.
///
/// The blocking [`record_audio`] waits for Enter; this exists for the TUI,
/// where recording has to run alongside a live event loop and the same growing
/// file has to be readable for rolling transcription.
pub struct Recorder {
    child: std::process::Child,
    path: PathBuf,
    started: Instant,
}

impl Recorder {
    /// Start `rec` writing a 16kHz mono WAV, and return immediately.
    pub fn start(screen: bool) -> Result<Recorder> {
        require_sox()?;

        let path = recording_path();
        let _ = std::fs::remove_file(&path);

        let device = if screen {
            Some(std::env::var("LEO_SCREEN_DEVICE").unwrap_or_else(|_| "BlackHole 2ch".to_string()))
        } else {
            None
        };

        let path_str = path.to_str().context("temp path is not valid UTF-8")?;
        let mut cmd = Command::new("rec");
        if let Some(dev) = &device {
            cmd.env("AUDIODEV", dev);
        }
        let child = cmd
            .args([path_str, "rate", "16000", "channels", "1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| match &device {
                Some(dev) => format!(
                    "Failed to start recording from screen audio device '{dev}'.\n  \
                     Set up system audio capture:\n  \
                     1. brew install blackhole-2ch\n  \
                     2. Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                     3. Set that Multi-Output Device as System Output\n  \
                     To use a different device: set LEO_SCREEN_DEVICE=<name>"
                ),
                None => "Failed to start recording".to_string(),
            })?;

        Ok(Recorder { child, path, started: Instant::now() })
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// The file being written. Safe to read while recording continues, which is
    /// what the rolling transcription loop does.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Stop recording and finalize the file. Returns the finished WAV.
    pub fn stop(mut self) -> Result<PathBuf> {
        self.child.kill().ok();
        self.child.wait().ok();
        repair_wav_header(&self.path);

        if !self.path.exists() || std::fs::metadata(&self.path)?.len() < 100 {
            bail!("Recording failed — no audio captured.");
        }
        Ok(self.path)
    }
}

/// `rec` is killed before it can write the final DataSize field, leaving it
/// zero. `sox --ignore-length` reads to EOF and writes a correct header.
///
/// Used for the finished file and, during live transcription, for a copy of the
/// growing one — a slice cut from a header that claims zero length yields
/// nothing.
pub fn repair_wav_header(path: &std::path::Path) {
    let fixed = path.with_extension("fixed.wav");
    let ok = Command::new("sox")
        .args([
            "--ignore-length",
            &path.to_string_lossy(),
            &fixed.to_string_lossy(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && fixed.exists())
        .unwrap_or(false);
    if ok {
        let _ = std::fs::rename(&fixed, path);
    } else {
        let _ = std::fs::remove_file(&fixed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_audio_accepts_screen_bool() {
        // Compile-time proof the signature is correct.
        let _f: fn(bool) -> anyhow::Result<std::path::PathBuf> = record_audio;
    }

    #[test]
    fn the_recorder_reports_a_path_before_any_audio_arrives() {
        // Nothing here starts `rec`; this pins the naming contract the live
        // transcription worker relies on.
        let path = recording_path();
        assert!(path.to_string_lossy().ends_with(".wav"));
        assert!(path.to_string_lossy().contains("leo-recording"));
    }
}
