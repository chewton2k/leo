use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use transcribe_cpp::{Backend, BackendMask, Model, ModelOptions, VadSession, VadSessionOptions};

use super::{check_whole_frames, constants, SpeechScorer, VadFrame, VoiceActivityDetector};

pub const SILERO_VAD_THRESHOLD: f32 = 0.3;
pub(super) const MODEL: &[u8] = include_bytes!("silero-vad-v6.2-F32.gguf");
const MODEL_FILE: &str = "silero-vad-v6.2-F32.gguf";

pub fn dir() -> PathBuf {
    if cfg!(test) {
        std::env::temp_dir().join("leo-test-vad")
    } else {
        crate::config::choice::models_dir().join("vad")
    }
}

fn store(folder: &Path, path: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    let mut file = tempfile::NamedTempFile::new_in(folder)?;
    file.write_all(MODEL)?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn model_file() -> Result<PathBuf> {
    static READY: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = READY.get() {
        return Ok(path.clone());
    }
    let folder = dir();
    let path = folder.join(MODEL_FILE);
    let unchanged = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
        && std::fs::read(&path).is_ok_and(|bytes| bytes == MODEL);
    if !unchanged {
        store(&folder, &path)?;
    }
    Ok(READY.get_or_init(|| path).clone())
}

fn cpu_backend() -> Result<()> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    READY
        .get_or_init(|| {
            transcribe_cpp::init_logging();
            transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|e| anyhow!("transcribe.cpp CPU backend: {e}"))
}

pub struct SileroVad {
    session: Mutex<VadSession>,
    frame_samples: usize,
    threshold: f32,
}

impl SileroVad {
    pub fn new(threshold: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&threshold) {
            bail!("threshold must be between 0.0 and 1.0");
        }
        #[cfg(all(windows, target_arch = "x86_64"))]
        if !(is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("fma")
            && is_x86_feature_detected!("f16c")
            && is_x86_feature_detected!("bmi2"))
        {
            bail!("this processor lacks the AVX2 instructions Silero VAD is built for");
        }
        cpu_backend()?;
        let model = Model::load_with(
            model_file()?,
            &ModelOptions {
                backend: Backend::Cpu,
                device: None,
                no_mmap: false,
            },
        )
        .context("Silero VAD model")?;
        let info = model.vad_info().context("Silero VAD model")?;
        if info.sample_rate != constants::WHISPER_SAMPLE_RATE as i32 || info.frame_samples <= 0 {
            bail!(
                "unsupported VAD model: {} Hz, {} samples per frame",
                info.sample_rate,
                info.frame_samples
            );
        }
        let session = model
            .vad_session_with(&VadSessionOptions { n_threads: 1 })
            .context("Silero VAD session")?;
        Ok(Self {
            session: Mutex::new(session),
            frame_samples: info.frame_samples as usize,
            threshold,
        })
    }
}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        if frame.len() != self.frame_samples {
            bail!(
                "expected {} samples, got {}",
                self.frame_samples,
                frame.len()
            );
        }
        if self.score(frame)?[0] > self.threshold {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    fn reset(&mut self) {
        self.session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .stream_reset();
    }
}

impl SpeechScorer for SileroVad {
    fn score(&mut self, frames: &[f32]) -> Result<Vec<f32>> {
        check_whole_frames(frames, self.frame_samples)?;
        let result = self
            .session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .stream_feed(frames)
            .context("Silero VAD")?;
        let expected = frames.len() / self.frame_samples;
        if result.probs.len() != expected {
            bail!(
                "Silero VAD returned {} probabilities for {expected} frames",
                result.probs.len()
            );
        }
        Ok(result.probs)
    }
}
