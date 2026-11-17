//! Everything that touches AI providers from outside the chains: testing one
//! provider and reporting the state of the downloaded models. Settings and the
//! doctor both call these, so there is one implementation of each.

use anyhow::Result;

use crate::ai;
use crate::config::{self, Config};

/// Build the single transcription provider named by `name`, regardless of
/// whether it appears in the `[transcribe]` chain, so Settings can test a
/// provider a user is still setting up.
fn build_one_transcriber(
    name: &str,
    pc: &config::provider::ProviderConfig,
) -> Option<Box<dyn ai::provider::TranscribeProvider>> {
    use config::provider::ProviderKind;
    match pc.kind {
        Some(ProviderKind::Parakeet) => Some(Box::new(
            ai::provider::local_speech::LocalSpeech::new(name.to_string(), pc),
        )),
        _ => None,
    }
}

/// Send one minimal request to a provider and report what happened.
///
/// Returns the report rather than printing it, so the CLI can print it and the
/// provider screen can show it in its status line. Transcription providers are
/// only checked for reachability: exercising one needs audio, and a test that
/// records from the microphone is not a test anyone wants to run twice.
pub fn test_provider(name: &str) -> Result<String> {
    let cfg = Config::load();
    let store = config::secret::default_store();
    let Some(pc) = cfg.provider(name) else {
        anyhow::bail!("no provider named '{name}' in your config");
    };

    let started = std::time::Instant::now();
    if let Some(p) = ai::provider::build_one_chat(name, pc, &store) {
        if !p.available() {
            anyhow::bail!("{}", p.unavailable_reason());
        }
        let reply = p
            .complete(&ai::provider::ChatRequest {
                system: None,
                prompt: "Reply with the single word: ok".to_string(),
                temperature: 0.0,
                max_tokens: 16,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let reply = reply.trim().replace('\n', " ");
        // A reasoning model can answer at length; one line is enough here.
        let reply: String = reply.chars().take(60).collect();
        return Ok(format!(
            "{name} responded in {:?}: {reply}",
            started.elapsed()
        ));
    }
    match pc.kind {
        Some(_) => match build_one_transcriber(name, pc) {
            Some(p) if p.available() => Ok(format!("{name} is reachable")),
            Some(p) => anyhow::bail!("{}", p.unavailable_reason()),
            None => anyhow::bail!("provider '{name}' could not be built"),
        },
        None => anyhow::bail!("provider '{name}' has no `kind`"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelState {
    Ready,
    Missing,
    Damaged,
}

pub fn model_state(path: &std::path::Path, sha256: &str) -> ModelState {
    if !path.is_file() {
        return ModelState::Missing;
    }
    match sha256_of(path) {
        Ok(actual) if actual.eq_ignore_ascii_case(sha256) => ModelState::Ready,
        _ => ModelState::Damaged,
    }
}

pub fn files_state(dir: &std::path::Path, manifest: &[(String, String)]) -> ModelState {
    worst_state(
        manifest
            .iter()
            .map(|(name, sha)| model_state(&dir.join(name), sha)),
    )
}

pub fn worst_state(states: impl IntoIterator<Item = ModelState>) -> ModelState {
    let states: Vec<ModelState> = states.into_iter().collect();
    if states.contains(&ModelState::Damaged) {
        ModelState::Damaged
    } else if states.contains(&ModelState::Missing) {
        ModelState::Missing
    } else {
        ModelState::Ready
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_of(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn download_verified(
    url: &str,
    sha256: &str,
    dir: &std::path::Path,
    name: &str,
) -> Result<std::path::PathBuf> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 200
            && !name.contains(['/', '\\', ':'])
            && name != "."
            && name != ".."
            && !name.chars().any(char::is_control),
        "the model download needs a safe file name"
    );
    std::fs::create_dir_all(dir)?;
    anyhow::ensure!(
        !std::fs::symlink_metadata(dir)?.file_type().is_symlink(),
        "the model folder cannot be a symbolic link"
    );
    let target = leo_core::paths::contained_path(dir, std::path::Path::new(name))?;
    drop_stale_downloads(dir);
    let partial = tempfile::Builder::new()
        .prefix(".leo-download-")
        .suffix(".part")
        .tempfile_in(dir)?;
    let mut curl = leo_core::children::quiet_command("curl");
    curl.args([
        "-L",
        "--fail",
        "--connect-timeout",
        "15",
        "--max-time",
        "600",
        "-o",
    ])
    .arg(partial.path())
    .arg(url);
    if !url.starts_with("file://") {
        curl.args(["--proto", "=https", "--proto-redir", "=https"]);
    }
    curl.args(["--max-filesize", "1073741824", "-sS"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let curl = ai::provider::process::Process::new(
        curl.spawn()
            .map_err(|e| anyhow::anyhow!("could not run curl: {e}"))?,
    );
    let status = loop {
        let ended = match curl.0.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            Some(running) => running.try_wait()?,
            None => None,
        };
        if let Some(status) = ended {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if !status.success() {
        anyhow::bail!("the download failed; try again when online");
    }
    let actual = sha256_of(partial.path())?;
    if !actual.eq_ignore_ascii_case(sha256) {
        anyhow::bail!("the download was damaged (checksum mismatch); try again");
    }
    partial.persist(&target).map_err(|e| e.error)?;
    Ok(target)
}

pub(crate) fn drop_stale_downloads(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".leo-download-")
            && entry.file_type().is_ok_and(|t| t.is_file())
            && older_than(&entry, STALE_DOWNLOAD)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

const STALE_DOWNLOAD: std::time::Duration = std::time::Duration::from_secs(15 * 60);

pub(crate) fn older_than(entry: &std::fs::DirEntry, age: std::time::Duration) -> bool {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|since| since >= age)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_url(path: &std::path::Path) -> String {
        format!("file://{}", path.display())
    }

    #[test]
    fn model_download_names_stay_inside_the_model_folder() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../outside", "/outside", "a/b", "a\\b", ".", "..", "a:b"] {
            assert!(download_verified("file:///unused", "00", dir.path(), name).is_err());
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn model_downloads_do_not_follow_existing_target_or_partial_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, "keep").unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir(&models).unwrap();
        let good = sha256_of(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, models.join("weights.part")).unwrap();
        let saved = download_verified(&file_url(&outside), &good, &models, "weights").unwrap();
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
        std::os::unix::fs::symlink(&outside, models.join("linked")).unwrap();
        assert!(download_verified(&file_url(&outside), &good, &models, "linked").is_err());
    }

    #[test]
    fn a_download_is_kept_only_when_its_checksum_matches() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        std::fs::write(&source, b"pretend model").unwrap();
        let good = sha256_of(&source).unwrap();
        assert_eq!(good.len(), 64);

        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        let stale = models.join(".leo-download-old.part");
        let fresh = models.join(".leo-download-new.part");
        std::fs::write(&stale, b"left by a crash").unwrap();
        std::fs::write(&fresh, b"another download").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - STALE_DOWNLOAD * 2)
            .unwrap();
        let saved = download_verified(&file_url(&source), &good, &models, "ggml-x.bin").unwrap();
        assert_eq!(std::fs::read(&saved).unwrap(), b"pretend model");
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(!models.join("ggml-x.bin.part").exists());

        let err = download_verified(&file_url(&source), &"0".repeat(64), &models, "ggml-y.bin")
            .unwrap_err();
        assert!(err.to_string().contains("damaged"), "{err}");
        assert!(!models.join("ggml-y.bin").exists());
        assert!(!models.join("ggml-y.bin.part").exists());

        let err = download_verified(
            &file_url(&dir.path().join("missing.bin")),
            &good,
            &models,
            "ggml-z.bin",
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed"), "{err}");
        assert!(!models.join("ggml-z.bin.part").exists());
    }

    #[test]
    fn a_model_is_ready_only_when_its_checksum_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-base.en.bin");
        assert_eq!(model_state(&path, "00"), ModelState::Missing);
        std::fs::write(&path, b"whole model").unwrap();
        let good = sha256_of(&path).unwrap();
        assert_eq!(model_state(&path, &good), ModelState::Ready);
        assert_eq!(model_state(&path, &good.to_uppercase()), ModelState::Ready);
        std::fs::write(&path, b"whole mo").unwrap();
        assert_eq!(model_state(&path, &good), ModelState::Damaged);
    }

    #[test]
    fn a_model_of_several_files_is_ready_only_when_every_one_is() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.onnx");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"aaa").unwrap();
        std::fs::write(&b, b"bbb").unwrap();
        let manifest = vec![
            ("a.onnx".to_string(), sha256_of(&a).unwrap()),
            ("b.txt".to_string(), sha256_of(&b).unwrap()),
        ];
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Ready);
        std::fs::remove_file(&b).unwrap();
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Missing);
        std::fs::write(&a, b"aa").unwrap();
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Damaged);
    }
}
