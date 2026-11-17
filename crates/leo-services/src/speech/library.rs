use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::catalog::{catalog, Catalog, DEFAULT_MODEL, DIARIZER};

const MOST_DOWNLOADS: usize = 2;
const PARTIAL: &str = ".leo-download-";
const LOCK_WAIT: Duration = Duration::from_secs(5);
const CANCEL_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Selection {
    pub id: String,
    pub language: String,
}

#[derive(Clone, Debug)]
pub struct Selected {
    pub id: String,
    pub language: String,
    pub path: PathBuf,
    pub streaming: bool,
    pub live: bool,
}

#[derive(Clone)]
struct Download {
    cancel: Arc<AtomicBool>,
    bytes: u64,
    total: u64,
    state: &'static str,
    error: Option<String>,
}

type Verified = BTreeMap<PathBuf, (u64, SystemTime)>;
const VERIFIED_FILE: &str = "speech-verified.json";
const MOST_VERIFIED_BYTES: u64 = 64 * 1024;

fn remembered(config: &Path) -> Verified {
    let path = config.join(VERIFIED_FILE);
    std::fs::symlink_metadata(&path)
        .ok()
        .filter(|meta| meta.is_file() && meta.len() <= MOST_VERIFIED_BYTES)
        .and_then(|_| std::fs::File::open(&path).ok())
        .and_then(|file| serde_json::from_reader(file.take(MOST_VERIFIED_BYTES + 1)).ok())
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct Library {
    models: PathBuf,
    config: PathBuf,
    catalog: Arc<Catalog>,
    jobs: Arc<Mutex<BTreeMap<String, Download>>>,
    verified: Arc<Mutex<Verified>>,
    hashing: Arc<Mutex<()>>,
}

pub fn library() -> Result<&'static Library> {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    if let Some(library) = LIBRARY.get() {
        return Ok(library);
    }
    let value = Library::new(
        crate::config::choice::models_dir().join("speech"),
        leo_core::paths::config_dir()?,
    );
    Ok(LIBRARY.get_or_init(|| value))
}

fn safe_root(path: &Path) -> Result<()> {
    let home = dirs::home_dir();
    let temporary = std::env::temp_dir();
    for ancestor in path.ancestors().take(64) {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            ensure!(
                !meta.file_type().is_symlink() && meta.is_dir(),
                "The speech model and settings folders cannot use symbolic links."
            );
        }
        if home.as_deref() == Some(ancestor) || ancestor == temporary {
            break;
        }
    }
    Ok(())
}

impl Library {
    pub fn new(models: PathBuf, config: PathBuf) -> Self {
        Self::with_catalog(models, config, Arc::new(catalog().clone()))
    }

    pub(crate) fn with_catalog(models: PathBuf, config: PathBuf, catalog: Arc<Catalog>) -> Self {
        let library = Self {
            verified: Arc::new(Mutex::new(remembered(&config))),
            models,
            config,
            catalog,
            jobs: Default::default(),
            hashing: Default::default(),
        };
        library.drop_partials();
        library
    }

    pub fn models_dir(&self) -> &Path {
        &self.models
    }

    fn folders(&self) -> Result<()> {
        safe_root(&self.models)?;
        safe_root(&self.config)?;
        crate::private_file::directory(&self.models)?;
        crate::private_file::directory(&self.config)?;
        Ok(())
    }

    pub fn model_path(&self, id: &str) -> Result<PathBuf> {
        safe_root(&self.models)?;
        let model = self.catalog.known(id)?;
        if let Ok(meta) = std::fs::symlink_metadata(&self.models) {
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "The speech model folder cannot be a symbolic link."
            );
        }
        leo_core::paths::contained_path(&self.models, Path::new(&model.file().filename))
    }

    fn selection_path(&self) -> Result<PathBuf> {
        safe_root(&self.config)?;
        leo_core::paths::contained_path(&self.config, Path::new("speech-model.json"))
    }

    pub fn selection(&self) -> Result<Selection> {
        let path = self.selection_path()?;
        if !path.exists() {
            return Ok(Selection {
                id: DEFAULT_MODEL.into(),
                language: "auto".into(),
            });
        }
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_file() && meta.len() <= 4096,
            "The speech model choice could not be read. Choose a model again in Settings."
        );
        let selected: Selection = serde_json::from_reader(std::fs::File::open(&path)?.take(4097))
            .context(
            "The speech model choice could not be read. Choose a model again in Settings.",
        )?;
        if selected.language == "auto" {
            if let Some(model) = self
                .catalog
                .get(&selected.id)
                .ok()
                .filter(|m| !m.language("auto"))
            {
                if let Some(language) = model.default_language() {
                    return Ok(Selection {
                        id: selected.id,
                        language,
                    });
                }
            }
        }
        if selected.id.is_empty()
            || self
                .catalog
                .get(&selected.id)
                .is_ok_and(|model| model.language(&selected.language))
        {
            return Ok(selected);
        }
        Ok(Selection {
            id: String::new(),
            language: "auto".into(),
        })
    }

    fn verify(&self, id: &str) -> Result<PathBuf> {
        let model = self.catalog.known(id)?;
        let path = self.model_path(id)?;
        let meta = std::fs::symlink_metadata(&path)
            .context("Download a model in Settings → Speech models. Your recorded audio is kept until one is ready.")?;
        ensure!(meta.is_file() && meta.len() == model.file().size,
            "This speech model is incomplete. Delete it and download it again in Settings → Speech models.");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                meta.nlink() == 1,
                "The speech model cannot have hard links. Download it again."
            );
        }
        let stamp = (meta.len(), meta.modified()?);
        let known = || {
            self.verified
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&path)
                == Some(&stamp)
        };
        if known() {
            return Ok(path);
        }
        let _hashing = self.hashing.lock().unwrap_or_else(|e| e.into_inner());
        if known() {
            return Ok(path);
        }
        let actual = crate::providers::sha256_of(&path)?;
        ensure!(actual.eq_ignore_ascii_case(&model.file().sha256), "This speech model is damaged. Delete it and download it again in Settings → Speech models.");
        self.change_verified(|verified| {
            verified.insert(path.clone(), stamp);
        });
        Ok(path)
    }

    fn change_verified(&self, change: impl FnOnce(&mut Verified)) {
        let mut verified = self.verified.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut verified);
        let saved = serde_json::to_vec(&*verified)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| {
                leo_core::paths::write_atomic(&self.config.join(VERIFIED_FILE), bytes)
            });
        if let Err(error) = saved {
            log::warn!("Could not remember the verified speech models: {error}");
        }
    }

    /// The chosen model's catalog name, marked when its file is not on disk.
    /// It never hashes the file (the menu bar reads this every few seconds;
    /// `selected` verifies the model). None when no model is chosen.
    pub fn selected_name(&self) -> Option<String> {
        let selection = self.selection().ok()?;
        if selection.id.is_empty() {
            return None;
        }
        let name = self.catalog.get(&selection.id).ok()?.name.clone();
        let there = self
            .model_path(&selection.id)
            .ok()
            .and_then(|path| std::fs::symlink_metadata(path).ok())
            .is_some_and(|meta| meta.is_file());
        Some(if there {
            name
        } else {
            format!("{name} (not downloaded)")
        })
    }

    pub fn selected(&self) -> Result<Selected> {
        let selection = self.selection()?;
        ensure!(!selection.id.is_empty(), "Choose and download a model in Settings → Speech models. Your audio is kept until one is ready.");
        let model = self.catalog.get(&selection.id)?;
        Ok(Selected {
            path: self.verify(&selection.id)?,
            live: self.catalog.live(&selection.id),
            streaming: model.capabilities["streaming"].as_bool().unwrap_or(false),
            id: selection.id,
            language: selection.language,
        })
    }

    pub fn use_model(&self, id: &str, language: &str) -> Result<()> {
        let model = self.catalog.get(id)?;
        let language = if language == "auto" && !model.language("auto") {
            model.default_language().unwrap_or_else(|| language.into())
        } else {
            language.into()
        };
        let language = language.as_str();
        ensure!(
            model.language(language),
            "Choose a language supported by this speech model."
        );
        self.folders()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        self.verify(id)?;
        let selection = Selection {
            id: id.into(),
            language: language.into(),
        };
        leo_core::paths::write_atomic(&self.selection_path()?, serde_json::to_vec(&selection)?)
    }

    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        self.catalog.known(id)?;
        self.folders()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        ensure!(
            !self
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .is_some_and(|j| j.state == "downloading"),
            "Cancel this model's download before deleting it."
        );
        let path = self.model_path(id)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.change_verified(|verified| {
            verified.remove(&path);
        });
        if self.selection()?.id == id {
            leo_core::paths::write_atomic(
                &self.selection_path()?,
                serde_json::to_vec(&Selection {
                    id: String::new(),
                    language: "auto".into(),
                })?,
            )?;
        }
        Ok(())
    }

    pub fn downloading(&self, id: &str) -> bool {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .is_some_and(|j| j.state == "downloading")
    }

    pub fn speaker_model(&self) -> Result<PathBuf> {
        self.verify(DIARIZER)
            .context("the speaker model is not downloaded yet (Download it beside Label speakers on the Record page)")
    }

    pub fn view(&self) -> Result<Value> {
        let selection = self.selection()?;
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut disk_bytes = 0;
        let mut models = Vec::new();
        for model in self.catalog.curated() {
            let path = self.model_path(&model.id)?;
            let metadata = std::fs::symlink_metadata(&path)
                .ok()
                .filter(|m| m.is_file());
            let bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            disk_bytes += bytes;
            let downloaded = metadata.is_some();
            let verified = downloaded && self.verify(&model.id).is_ok();
            let job = jobs.get(&model.id);
            let accuracy = self.catalog.accuracy_for(&model.id, &selection.language);
            models.push(json!({ "id": model.id, "name": model.name, "description": model.description,
                "languages": model.languages, "size": model.file().size, "bytes": bytes, "downloaded": downloaded, "verified": verified,
                "in_use": verified && selection.id == model.id, "streaming": model.capabilities["streaming"], "detects_language": model.detects_language(),
                "live": self.catalog.live(&model.id), "recommended": model.recommended(), "license": model.license,
                "speed": self.catalog.speed(&model.id), "accuracy": accuracy.unwrap_or(0.0), "accuracy_measured": accuracy.is_some(), "benchmark_language": selection.language,
                "progress": job.map(|j| json!({ "bytes": j.bytes, "total": j.total, "state": j.state, "error": j.error })) }));
        }
        let mut extra_files = Vec::new();
        if self.models.exists() {
            safe_root(&self.models)?;
            for (index, entry) in std::fs::read_dir(&self.models)?.enumerate() {
                ensure!(index<256,"There are too many files in the speech model folder. Move older files elsewhere, then reopen Speech models.");
                let entry = entry?;
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !Self::extra_name(&name)
                    || self.catalog.curated().any(|m| m.file().filename == name)
                    || self
                        .catalog
                        .known(DIARIZER)
                        .is_ok_and(|m| m.file().filename == name)
                {
                    continue;
                }
                let metadata = std::fs::symlink_metadata(entry.path())?;
                if !metadata.is_file() {
                    continue;
                }
                disk_bytes += metadata.len();
                extra_files.push(json!({"name":name,"bytes":metadata.len()}));
            }
            extra_files.sort_by_key(|v| v["name"].as_str().unwrap_or_default().to_owned());
        }
        let speakers = match (self.catalog.known(DIARIZER), self.model_path(DIARIZER)) {
            (Ok(model), Ok(path)) => {
                let bytes = std::fs::symlink_metadata(path)
                    .ok()
                    .filter(|m| m.is_file())
                    .map_or(0, |m| m.len());
                disk_bytes += bytes;
                json!({ "id": DIARIZER, "size": model.file().size, "bytes": bytes,
                    "verified": bytes > 0 && self.verify(DIARIZER).is_ok(),
                    "progress": jobs.get(DIARIZER).map(|j| json!({ "bytes": j.bytes, "total": j.total, "state": j.state, "error": j.error })) })
            }
            _ => Value::Null,
        };
        Ok(
            json!({ "selected": selection, "ready": self.selected().is_ok(), "models": models, "extra_files":extra_files,"disk_bytes": disk_bytes, "speakers": speakers }),
        )
    }

    pub fn ensure_speaker_model(
        &self,
        start: &dyn Fn() -> Result<()>,
        stop: &dyn Fn() -> bool,
        progress: &dyn Fn(u64, u64),
    ) -> Result<PathBuf> {
        if let Ok(path) = self.speaker_model() {
            return Ok(path);
        }
        while !self.downloading(DIARIZER) {
            let Err(e) = start() else { break };
            if self.downloading(DIARIZER) {
                break;
            }
            let full = self
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|j| j.state == "downloading")
                .count()
                >= MOST_DOWNLOADS;
            if !full {
                return Err(e);
            }
            ensure!(!stop(), "Stopped while the speaker model was downloading");
            std::thread::sleep(CANCEL_POLL);
        }
        let failed = loop {
            let job = self
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(DIARIZER)
                .cloned();
            match job {
                Some(job) if job.state == "downloading" => {
                    ensure!(!stop(), "Stopped while the speaker model was downloading");
                    progress(job.bytes, job.total);
                    std::thread::sleep(CANCEL_POLL);
                }
                job => {
                    break job.and_then(|j| match j.state {
                        "failed" => j.error,
                        "cancelled" => Some("The speaker model download was cancelled".into()),
                        _ => None,
                    });
                }
            }
        };
        let path = self
            .speaker_model()
            .map_err(|e| failed.map_or(e, anyhow::Error::msg))?;
        let size = self.catalog.known(DIARIZER)?.file().size;
        progress(size, size);
        Ok(path)
    }

    #[cfg(test)]
    pub(crate) fn seed_job(&self, id: &str, state: &'static str, error: Option<&str>) {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.into(),
            Download {
                cancel: Default::default(),
                bytes: 0,
                total: 0,
                state,
                error: error.map(Into::into),
            },
        );
    }

    pub fn download(&self, id: &str) -> Result<()> {
        self.download_from(id, &self.catalog.known(id)?.url())
    }

    pub(crate) fn download_from(&self, id: &str, url: &str) -> Result<()> {
        let model = self.catalog.known(id)?;
        let url = url.to_owned();
        let total = model.file().size;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !jobs.get(id).is_some_and(|j| j.state == "downloading"),
                "This model is already downloading."
            );
            let downloading = jobs.values().filter(|j| j.state == "downloading").count();
            ensure!(
                downloading < MOST_DOWNLOADS,
                "Let a model download finish or cancel one first."
            );
            if downloading == 0 {
                self.drop_partials();
            }
            jobs.insert(
                id.into(),
                Download {
                    cancel: cancel.clone(),
                    bytes: 0,
                    total,
                    state: "downloading",
                    error: None,
                },
            );
        }
        let library = self.clone();
        let id = id.to_owned();
        std::thread::spawn(move || {
            let result = library.fetch(&id, &url, &cancel, &|bytes, _| {
                if let Some(job) = library
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&id)
                {
                    job.bytes = bytes;
                }
            });
            let mut jobs = library.jobs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(job) = jobs.get_mut(&id) {
                job.state = if result.is_ok() {
                    "ready"
                } else if cancel.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "failed"
                };
                job.error = result.err().map(|e| e.to_string());
            }
        });
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        self.catalog.known(id)?;
        if let Some(job) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            job.cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) fn fetch(
        &self,
        id: &str,
        url: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, u64),
    ) -> Result<()> {
        let model = self.catalog.known(id)?;
        self.folders()?;
        let target = self.model_path(id)?;
        let mut tmp = tempfile::Builder::new()
            .prefix(PARTIAL)
            .tempfile_in(&self.models)?;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
            #[cfg(not(test))]
            let client = crate::web::model_client()?;
            #[cfg(test)]
            let client = reqwest::Client::builder().no_proxy().build()?;
            let transfer = async {
                let mut response = client.get(url).send().await.context("The model download could not start. Check your connection and try again.")?
                    .error_for_status().context("The model download failed. Check your connection and try again.")?;
                ensure!(response.content_length().is_none_or(|n| n == model.file().size), "The model download has the wrong size. Try again.");
                let mut hash = Sha256::new();
                let mut bytes = 0u64;
                while let Some(chunk) = response.chunk().await.context("The model download was interrupted. Try again.")? {
                    ensure!(!cancel.load(Ordering::Acquire), "The model download was cancelled.");
                    bytes += chunk.len() as u64;
                    ensure!(bytes <= model.file().size, "The model download is too large. Try again.");
                    tmp.write_all(&chunk)?;
                    hash.update(&chunk);
                    progress(bytes, model.file().size);
                }
                ensure!(bytes == model.file().size && crate::providers::hex(&hash.finalize()).eq_ignore_ascii_case(&model.file().sha256),
                    "The model download was damaged. Try downloading it again.");
                Ok::<(), anyhow::Error>(())
            };
            tokio::select! {
                result = transfer => result,
                _ = async { loop { if cancel.load(Ordering::Acquire) { break; } tokio::time::sleep(CANCEL_POLL).await; } } => bail!("The model download was cancelled."),
            }
        })?;
        tmp.as_file().sync_all()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        ensure!(
            !cancel.load(Ordering::Acquire),
            "The model download was cancelled."
        );
        ensure!(
            target == self.model_path(id)?,
            "The model download location changed. Try again."
        );
        tmp.persist(&target).map_err(|e| e.error)?;
        let stamp = std::fs::symlink_metadata(&target).and_then(|m| Ok((m.len(), m.modified()?)));
        self.change_verified(|verified| match stamp {
            Ok(stamp) => {
                verified.insert(target.clone(), stamp);
            }
            Err(_) => {
                verified.remove(&target);
            }
        });
        Ok(())
    }

    pub(crate) fn drop_partials(&self) {
        if safe_root(&self.models).is_err() {
            return;
        }
        crate::providers::drop_stale_downloads(&self.models);
    }

    fn extra_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 255
            && !name.contains(['/', '\\'])
            && !name.chars().any(char::is_control)
            && name.to_ascii_lowercase().ends_with(".gguf")
            && std::path::Path::new(name).components().count() == 1
    }
    fn delete_extra(&self, name: &str) -> Result<()> {
        ensure!(
            Self::extra_name(name),
            "Choose a downloaded model file from the list."
        );
        ensure!(
            !self.catalog.curated().any(|m| m.file().filename == name)
                && !self
                    .catalog
                    .known(DIARIZER)
                    .is_ok_and(|m| m.file().filename == name),
            "Use the model’s Delete button instead."
        );
        self.folders()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        let path = leo_core::paths::contained_path(&self.models, Path::new(name))?;
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file(),
            "This is not a regular model file. Remove the link from the model folder instead."
        );
        std::fs::remove_file(path)?;
        Ok(())
    }

    pub fn change(&self, change: &Value) -> Result<Value> {
        let id = change["id"].as_str().context("Choose a speech model.")?;
        match change["action"].as_str() {
            Some("delete_file") => {
                ensure!(
                    Self::extra_name(id),
                    "Choose a downloaded model file from the list."
                );
                let path = leo_core::paths::contained_path(&self.models, Path::new(id))?;
                let library = self.clone();
                let name = id.to_owned();
                return super::engine::engine().edit_model(path, false, move || {
                    library.delete_extra(&name)?;
                    library.view()
                });
            }
            Some("download") => self.download(id)?,
            Some("cancel") => self.cancel(id)?,
            Some("use") => self.use_model(id, change["language"].as_str().unwrap_or("auto"))?,
            Some("delete") => {
                let library = self.clone();
                let id = id.to_owned();
                let path = self.model_path(&id)?;
                return super::engine::engine().edit_model(
                    path,
                    self.selection()?.id == id,
                    move || {
                        library.delete(&id)?;
                        library.view()
                    },
                );
            }
            _ => bail!("Choose Download, Cancel, Use or Delete."),
        }
        self.view()
    }
}
