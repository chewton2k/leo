use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, OnceLock};

use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

use crate::providers::{download_verified, model_state, worst_state, ModelState};

pub const MODEL_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a";
pub const MODEL_MANIFEST: &str = "config.json=094f8e891b932f2000c92cfc663bac4c62069f5d8af5b5278c4306aef3084750 tokenizer.json=d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 model.safetensors=3c9f31665447c8911517620762200d2245a2518d6e7208acc78cd9db317e21ad";
pub const MODEL_MB: u32 = 134;
pub const QUERY: &str = "Represent this sentence for searching relevant passages: ";
const MOST_TOKENS: usize = 512;
const BATCH: usize = 16;

pub fn manifest() -> Vec<(String, String)> {
    MODEL_MANIFEST
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, sha)| (name.to_string(), sha.to_string()))
        .collect()
}

pub fn dir() -> PathBuf {
    crate::config::choice::models_dir().join("bge-small-en-v1.5")
}

type Stamp = (u64, std::time::SystemTime);

static VERIFIED: LazyLock<Mutex<HashMap<(PathBuf, String), Stamp>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

fn checked(path: &Path, sha: &str) -> ModelState {
    let stamp = stamp_of(path);
    let key = (path.to_path_buf(), sha.to_string());
    if stamp.is_some()
        && VERIFIED.lock().unwrap_or_else(|e| e.into_inner()).get(&key) == stamp.as_ref()
    {
        return ModelState::Ready;
    }
    let state = model_state(path, sha);
    if let (ModelState::Ready, Some(stamp)) = (state, stamp) {
        VERIFIED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, stamp);
    }
    state
}

fn remember(path: &Path, sha: &str) {
    if let Some(stamp) = stamp_of(path) {
        VERIFIED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((path.to_path_buf(), sha.to_string()), stamp);
    }
}

pub fn state() -> ModelState {
    state_in(&dir(), &manifest())
}

pub(crate) fn state_in(dir: &Path, manifest: &[(String, String)]) -> ModelState {
    worst_state(
        manifest
            .iter()
            .map(|(name, sha)| checked(&dir.join(name), sha)),
    )
}

pub fn wanted() -> bool {
    std::env::var_os("LEO_INSTALL_NO_MODEL").is_none()
        && std::env::var_os("LEO_NO_MEANING_MODEL").is_none()
        && state() != ModelState::Ready
}

pub fn fetch() -> Result<PathBuf> {
    fetch_from(MODEL_URL, &dir(), &manifest())
}

pub(crate) fn fetch_from(base: &str, dir: &Path, manifest: &[(String, String)]) -> Result<PathBuf> {
    for (name, sha) in manifest {
        if checked(&dir.join(name), sha) == ModelState::Ready {
            continue;
        }
        let path = download_verified(&format!("{base}/{name}"), sha, dir, name)?;
        remember(&path, sha);
    }
    Ok(dir.to_path_buf())
}

pub struct Model {
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl Model {
    pub fn load(dir: &Path) -> Result<Model> {
        let device = Device::Cpu;
        let config: Config = serde_json::from_str(
            &std::fs::read_to_string(dir.join("config.json"))
                .context("the meaning model has no config.json")?,
        )?;
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("the meaning model's tokenizer could not be read: {e}"))?;
        tokenizer.with_padding(Some(PaddingParams::default()));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MOST_TOKENS,
                ..TruncationParams::default()
            }))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let weights = std::fs::read(dir.join("model.safetensors"))
            .context("the meaning model has no weights")?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DTYPE, &device)?;
        let bert = BertModel::load(vb, &config)?;
        Ok(Model {
            bert,
            tokenizer,
            device,
        })
    }

    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            let encodings = self
                .tokenizer
                .encode_batch(batch.to_vec(), true)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let ids: Vec<Tensor> = encodings
                .iter()
                .map(|e| Tensor::new(e.get_ids(), &self.device))
                .collect::<candle_core::Result<_>>()?;
            let masks: Vec<Tensor> = encodings
                .iter()
                .map(|e| Tensor::new(e.get_attention_mask(), &self.device))
                .collect::<candle_core::Result<_>>()?;
            let ids = Tensor::stack(&ids, 0)?;
            let masks = Tensor::stack(&masks, 0)?;
            let types = ids.zeros_like()?;
            let hidden = self.bert.forward(&ids, &types, Some(&masks))?;
            let first = hidden.narrow(1, 0, 1)?.squeeze(1)?;
            let norms = first.sqr()?.sum_keepdim(1)?.sqrt()?;
            let unit = first.broadcast_div(&norms)?;
            out.extend(unit.to_vec2::<f32>()?);
        }
        Ok(out)
    }
}

static LOADED: OnceLock<Mutex<Loaded>> = OnceLock::new();
static READY: Condvar = Condvar::new();
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(60);
const IDLE: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Default)]
struct Loaded {
    model: Option<Arc<Model>>,
    last_used: Option<std::time::Instant>,
    missed: Option<std::time::Instant>,
    watching: bool,
    loading: bool,
}

struct Loading<'a> {
    slot: &'a Mutex<Loaded>,
    ready: &'a Condvar,
}

impl Drop for Loading<'_> {
    fn drop(&mut self) {
        if let Ok(mut held) = self.slot.lock() {
            held.loading = false;
        }
        self.ready.notify_all();
    }
}

fn reserve<'a>(
    slot: &'a Mutex<Loaded>,
    ready: &'a Condvar,
) -> Option<std::sync::MutexGuard<'a, Loaded>> {
    let held = slot.lock().ok()?;
    let (held, _) = ready
        .wait_timeout_while(held, std::time::Duration::from_secs(30), |held| {
            held.loading
        })
        .ok()?;
    (!held.loading).then_some(held)
}

fn watch_idle(slot: &'static Mutex<Loaded>, idle: std::time::Duration) {
    loop {
        let sleep_for = {
            let Ok(mut held) = slot.lock() else {
                return;
            };
            let Some(model) = held.model.as_ref() else {
                held.watching = false;
                return;
            };
            let elapsed = held.last_used.map(|at| at.elapsed()).unwrap_or(idle);
            if elapsed >= idle && Arc::strong_count(model) == 1 {
                held.model = None;
                held.last_used = None;
                held.watching = false;
                return;
            }
            idle.saturating_sub(elapsed)
                .max(std::time::Duration::from_secs(1))
        };
        std::thread::sleep(sleep_for);
    }
}

pub fn model() -> Option<Arc<Model>> {
    let slot = LOADED.get_or_init(|| Mutex::new(Loaded::default()));
    {
        let mut held = reserve(slot, &READY)?;
        if let Some(model) = held.model.as_ref().cloned() {
            held.last_used = Some(std::time::Instant::now());
            return Some(model);
        }
        if held.missed.is_some_and(|at| at.elapsed() < LOOK_AGAIN) {
            return None;
        }
        held.loading = true;
    }
    let _loading = Loading {
        slot,
        ready: &READY,
    };
    let loaded = (state() == ModelState::Ready)
        .then(|| Model::load(&dir()).ok())
        .flatten()
        .map(Arc::new);
    let mut held = slot.lock().ok()?;
    if let Some(existing) = held.model.as_ref().cloned() {
        held.last_used = Some(std::time::Instant::now());
        return Some(existing);
    }
    match loaded {
        Some(model) => {
            held.model = Some(Arc::clone(&model));
            held.last_used = Some(std::time::Instant::now());
            if !held.watching {
                held.watching = true;
                if std::thread::Builder::new()
                    .name("leo-meaning".into())
                    .spawn(move || watch_idle(slot, IDLE))
                    .is_err()
                {
                    held.watching = false;
                    held.model = None;
                    held.last_used = None;
                }
            }
            Some(model)
        }
        None => {
            held.missed = Some(std::time::Instant::now());
            None
        }
    }
}

fn present(dir: &Path) -> bool {
    manifest().iter().all(|(name, _)| dir.join(name).is_file())
}

pub fn embed(texts: &[String], query: bool) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        anyhow::ensure!(present(&dir()), "the meaning model is not downloaded yet");
        return Ok(Vec::new());
    }
    let model = model().context("the meaning model is not downloaded yet")?;
    if query {
        let asked: Vec<String> = texts.iter().map(|t| format!("{QUERY}{t}")).collect();
        model.embed(&asked)
    } else {
        model.embed(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_whether_the_model_is_there_looks_for_its_files_without_loading_it() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!present(dir.path()));
        for (name, _) in manifest() {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        assert!(present(dir.path()));
    }

    #[test]
    fn concurrent_model_loads_wait_for_the_first_and_panics_release_the_reservation() {
        let slot = Arc::new(Mutex::new(Loaded::default()));
        let ready = Arc::new(Condvar::new());
        reserve(&slot, &ready).unwrap().loading = true;
        let (started, start) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let (other, notification) = (Arc::clone(&slot), Arc::clone(&ready));
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            let held = reserve(&other, &notification).unwrap();
            done.send(held.loading).unwrap();
        });
        start
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(finished
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_err());
        assert!(std::panic::catch_unwind(|| {
            let _loading = Loading {
                slot: &slot,
                ready: &ready,
            };
            panic!("fake model load failure");
        })
        .is_err());
        assert!(!finished
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn the_model_is_pinned_to_one_commit_and_every_file_has_a_checksum() {
        assert!(MODEL_URL.contains("/resolve/5c38ec7c"));
        let files: Vec<String> = manifest()
            .into_iter()
            .map(|(name, sha)| {
                assert_eq!(sha.len(), 64);
                name
            })
            .collect();
        assert_eq!(
            files,
            ["config.json", "tokenizer.json", "model.safetensors"]
        );
    }

    #[test]
    fn the_meaning_model_is_fetched_once_and_then_kept() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("config.json"), b"{}").unwrap();
        let manifest = vec![(
            "config.json".to_string(),
            crate::providers::sha256_of(&source.join("config.json")).unwrap(),
        )];
        let dest = scratch.path().join("models");
        let base = format!("file://{}", source.display());
        assert_eq!(fetch_from(&base, &dest, &manifest).unwrap(), dest);
        assert_eq!(std::fs::read(dest.join("config.json")).unwrap(), b"{}");
        std::fs::remove_dir_all(&source).unwrap();
        assert_eq!(fetch_from(&base, &dest, &manifest).unwrap(), dest);
        assert_eq!(std::fs::read(dest.join("config.json")).unwrap(), b"{}");
    }

    fn two_files(scratch: &Path) -> (String, Vec<(String, String)>, PathBuf) {
        let source = scratch.join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("one.json"), b"{\"one\":1}").unwrap();
        std::fs::write(source.join("two.json"), b"{\"two\":2}").unwrap();
        let manifest = ["one.json", "two.json"]
            .map(|name| {
                (
                    name.to_string(),
                    crate::providers::sha256_of(&source.join(name)).unwrap(),
                )
            })
            .to_vec();
        (
            format!("file://{}", source.display()),
            manifest,
            scratch.join("models"),
        )
    }

    fn overwrite_keeping_stamp(path: &Path, bytes: &[u8]) {
        let before = std::fs::metadata(path).unwrap();
        assert_eq!(before.len() as usize, bytes.len());
        std::fs::write(path, bytes).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(before.modified().unwrap())
            .unwrap();
    }

    #[test]
    fn the_fetch_trusts_the_check_it_follows() {
        let scratch = tempfile::tempdir().unwrap();
        let (base, manifest, dest) = two_files(scratch.path());
        std::fs::create_dir(&dest).unwrap();
        let present = dest.join("one.json");
        std::fs::copy(scratch.path().join("source/one.json"), &present).unwrap();
        assert_eq!(state_in(&dest, &manifest), ModelState::Missing);
        overwrite_keeping_stamp(&present, b"{\"one\":9}");
        fetch_from(&base, &dest, &manifest).unwrap();
        assert_eq!(std::fs::read(&present).unwrap(), b"{\"one\":9}");
        assert_eq!(
            std::fs::read(dest.join("two.json")).unwrap(),
            b"{\"two\":2}"
        );
    }

    #[test]
    fn a_downloaded_file_is_remembered() {
        let scratch = tempfile::tempdir().unwrap();
        let (base, manifest, dest) = two_files(scratch.path());
        fetch_from(&base, &dest, &manifest).unwrap();
        overwrite_keeping_stamp(&dest.join("one.json"), b"{\"one\":9}");
        overwrite_keeping_stamp(&dest.join("two.json"), b"{\"two\":9}");
        assert_eq!(state_in(&dest, &manifest), ModelState::Ready);
    }

    #[test]
    #[ignore = "times a 134 MB file; run by hand with --release"]
    fn benchmark_meaning_fetch_after_check() {
        use std::io::Write;
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("source");
        let dest = scratch.path().join("models");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(source.join("config.json"), vec![b'c'; 743]).unwrap();
        let mut weights = std::fs::File::create(dest.join("model.safetensors")).unwrap();
        let chunk: Vec<u8> = (0..1 << 20).map(|i| (i % 251) as u8).collect();
        for _ in 0..134 {
            weights.write_all(&chunk).unwrap();
        }
        drop(weights);
        let manifest: Vec<(String, String)> = [
            (source.join("config.json"), "config.json"),
            (dest.join("model.safetensors"), "model.safetensors"),
        ]
        .iter()
        .map(|(path, name)| (name.to_string(), crate::providers::sha256_of(path).unwrap()))
        .collect();
        let base = format!("file://{}", source.display());
        let time = |what: &str, run: &mut dyn FnMut()| {
            let at = std::time::Instant::now();
            run();
            eprintln!("{what}: {:?}", at.elapsed());
        };
        time("check, one file missing", &mut || {
            assert_eq!(state_in(&dest, &manifest), ModelState::Missing);
        });
        time("fetch after the check", &mut || {
            fetch_from(&base, &dest, &manifest).unwrap();
        });
        time("check after the fetch", &mut || {
            assert_eq!(state_in(&dest, &manifest), ModelState::Ready);
        });
    }

    #[test]
    #[ignore = "needs the downloaded model: the app fetches it when it opens"]
    fn sentences_that_mean_the_same_are_closer_than_ones_that_do_not() {
        let model = Model::load(&dir()).unwrap();
        let v = model
            .embed(&[
                format!("{QUERY}how does breadth first search pick the next vertex?"),
                "BFS takes the oldest discovered node from a FIFO queue.".into(),
                "Photosynthesis turns light into chemical energy in plants.".into(),
            ])
            .unwrap();
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        assert_eq!(v[0].len(), 384);
        assert!((dot(&v[1], &v[1]) - 1.0).abs() < 1e-4);
        assert!(
            dot(&v[0], &v[1]) > dot(&v[0], &v[2]) + 0.1,
            "{} vs {}",
            dot(&v[0], &v[1]),
            dot(&v[0], &v[2])
        );
    }
}
