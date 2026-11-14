use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub open_at_login: bool,
    pub show_tray: bool,
    pub automatic_updates: bool,
    pub install_on_quit: bool,
    pub onboarding: [bool; 3],
    pub writing_done: Option<bool>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            open_at_login: true,
            show_tray: true,
            automatic_updates: true,
            install_on_quit: true,
            onboarding: [false; 3],
            writing_done: None,
        }
    }
}
impl Preferences {
    pub fn writing_done(&self) -> bool {
        self.writing_done
            .unwrap_or_else(|| self.onboarding.iter().all(|done| *done))
    }
    pub fn finish_step(&mut self, step: usize) -> Result<()> {
        ensure!(step < 4, "Choose a setup step.");
        let done = self.writing_done();
        self.writing_done = Some(step == 3 || done);
        if step < 3 {
            self.onboarding[step] = true;
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let meta = std::fs::symlink_metadata(path)?;
        ensure!(
            meta.is_file() && meta.len() <= 16_384,
            "App preferences could not be opened. Restore app-settings.json from a backup."
        );
        serde_json::from_slice(&std::fs::read(path)?)
            .context("App preferences could not be read. Review and save your settings again.")
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("App settings need a folder.")?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}
pub trait UpdateApi: Send + Sync {
    fn view(&self) -> Value;
    fn change(&self, action: &str) -> Result<()>;
}
enum PreferenceChange {
    Set(String, bool),
    Finish(usize),
}
type NativePreferences = Arc<dyn Fn(&Preferences, &Preferences) -> Result<()> + Send + Sync>;
struct PreferenceStore {
    tx: std::sync::mpsc::SyncSender<(PreferenceChange, std::sync::mpsc::SyncSender<Result<()>>)>,
    cached: Arc<Mutex<Preferences>>,
    problem: Arc<Mutex<Option<String>>>,
}
impl PreferenceStore {
    fn new(path: PathBuf, native: NativePreferences) -> Self {
        let (initial, problem) = match Preferences::load(&path) {
            Ok(preferences) => (preferences, None),
            Err(_) => {
                let preferences = Preferences {
                    automatic_updates: false,
                    install_on_quit: false,
                    onboarding: [true; 3],
                    ..Default::default()
                };
                (preferences,Some("App preferences could not be read. Review your settings or restore app-settings.json from a backup.".into()))
            }
        };
        let cached = Arc::new(Mutex::new(initial.clone()));
        let problem = Arc::new(Mutex::new(problem));
        let cache = cached.clone();
        let issue = problem.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel::<(
            PreferenceChange,
            std::sync::mpsc::SyncSender<Result<()>>,
        )>(8);
        std::thread::spawn(move || {
            let mut preferences = initial;
            while let Ok((change, done)) = rx.recv() {
                let result = (|| -> Result<()> {
                    let mut next = preferences.clone();
                    match change {
                        PreferenceChange::Finish(step) => next.finish_step(step)?,
                        PreferenceChange::Set(field, enabled) => match field.as_str() {
                            "open_at_login" => next.open_at_login = enabled,
                            "show_tray" => next.show_tray = enabled,
                            "automatic_updates" => next.automatic_updates = enabled,
                            "install_on_quit" => next.install_on_quit = enabled,
                            _ => anyhow::bail!(
                                "Choose an app setting from General or Updates & about."
                            ),
                        },
                    }
                    if let Ok(meta) = std::fs::symlink_metadata(&path) {
                        ensure!(meta.is_file() && meta.len()<=16_384,"Restore app-settings.json from a backup before saving these preferences.");
                    }
                    let original = match std::fs::read(&path) {
                        Ok(bytes) => Some(bytes),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                        Err(e) => return Err(e.into()),
                    };
                    next.save(&path)?;
                    if let Err(error) = native(&preferences, &next) {
                        if let Some(bytes) = original {
                            let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                            file.write_all(&bytes)?;
                            file.as_file().sync_all()?;
                            file.persist(&path).map_err(|e| e.error)?;
                        } else {
                            std::fs::remove_file(&path)?;
                        }
                        let _ = native(&next, &preferences);
                        return Err(error.context(
                            "This app setting could not be changed. Try again or reopen Leo.",
                        ));
                    }
                    preferences = next;
                    *cache.lock().unwrap_or_else(|e| e.into_inner()) = preferences.clone();
                    *issue.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    Ok(())
                })();
                let _ = done.send(result);
            }
        });
        Self {
            tx,
            cached,
            problem,
        }
    }
    fn view(&self) -> Preferences {
        self.cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn problem(&self) -> Option<String> {
        self.problem
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn record(&self, apply: impl FnOnce() -> Result<()>) {
        if let Err(error) = apply() {
            self.problem
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| format!("Leo could not apply a startup setting: {error:#}"));
        }
    }
    fn change(&self, change: PreferenceChange) -> Result<()> {
        let (done, answer) = std::sync::mpsc::sync_channel(1);
        self.tx
            .try_send((change, done))
            .map_err(|_| anyhow::anyhow!("App settings are busy. Try again in a moment."))?;
        answer
            .recv_timeout(std::time::Duration::from_secs(10))
            .context("The app setting is taking too long. Reopen Settings to check it.")?
    }
}

pub struct AppSettings {
    pub app: AppHandle,
    preferences: PreferenceStore,
    path: PathBuf,
    dictation: Arc<crate::dictation::Dictation>,
    pub updates: OnceLock<Arc<dyn UpdateApi>>,
}
impl AppSettings {
    pub fn new(app: AppHandle, dictation: Arc<crate::dictation::Dictation>) -> Result<Arc<Self>> {
        let path = leo_core::paths::config_dir()?.join("app-settings.json");
        let native = app.clone();
        let preferences = PreferenceStore::new(
            path.clone(),
            Arc::new(move |before, after| {
                if before.open_at_login != after.open_at_login {
                    if after.open_at_login {
                        native.autolaunch().enable()?;
                    } else {
                        native.autolaunch().disable()?;
                    }
                }
                if before.show_tray != after.show_tray {
                    if let Some(tray) = native.tray_by_id("leo") {
                        tray.set_visible(after.show_tray)?;
                    }
                }
                Ok(())
            }),
        );
        Ok(Arc::new(Self {
            app,
            preferences,
            path,
            dictation,
            updates: OnceLock::new(),
        }))
    }
    pub fn preferences(&self) -> Preferences {
        self.preferences.view()
    }
    pub fn apply_startup(&self) {
        let preferences = self.preferences();
        self.preferences.record(|| {
            if let Some(tray) = self.app.tray_by_id("leo") {
                tray.set_visible(preferences.show_tray)?;
            }
            Ok(())
        });
        if self.path.exists() && !cfg!(debug_assertions) {
            self.preferences.record(|| {
                if preferences.open_at_login {
                    self.app.autolaunch().enable()?;
                } else {
                    self.app.autolaunch().disable()?;
                }
                Ok(())
            });
        }
    }
    fn permission_view(&self) -> Value {
        json!({"microphone":crate::permissions::microphone(),"accessibility":crate::permissions::accessibility()})
    }
}
impl leo_web::app_settings::AppApi for AppSettings {
    fn view(&self) -> Result<Value> {
        let preferences = self.preferences();
        let mut preferences_view = serde_json::to_value(&preferences)?;
        preferences_view["writing_done"] = preferences.writing_done().into();
        let permissions = self.permission_view();
        let updates = self
            .updates
            .get()
            .map(|u| u.view())
            .unwrap_or(json!({"state":"idle"}));
        Ok(
            json!({"preferences":preferences_view,"problem":self.preferences.problem(),"permissions":permissions,"updates":updates,"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"unsigned":cfg!(target_os="macos") && option_env!("LEO_APP_SIGNED")!=Some("1"),"paths":{"notes":leo_core::paths::data_dir()?.join("notes"),"models":leo_services::config::choice::models_dir().join("speech")}}),
        )
    }
    fn change(&self, value: Value) -> Result<Value> {
        match value["action"].as_str() {
            Some("preference") => {
                let enabled = value["value"]
                    .as_bool()
                    .context("Choose On or Off for this setting.")?;
                let field = value["field"].as_str().context("Choose an app setting.")?;
                self.preferences
                    .change(PreferenceChange::Set(field.into(), enabled))?;
                if field == "automatic_updates" && enabled {
                    if let Some(updates) = self.updates.get() {
                        let _ = updates.change("check_updates");
                    }
                }
            }
            Some("finish_step") => self.preferences.change(PreferenceChange::Finish(
                value["step"].as_u64().context("Choose a setup step.")? as usize,
            ))?,
            Some("microphone") => crate::permissions::request_microphone(&self.app)?,
            Some("accessibility") => crate::permissions::request_accessibility(&self.app)?,
            Some("quit") => self.app.exit(0),
            Some("sign_in") => crate::sign_in::start(
                value["provider"]
                    .as_str()
                    .context("Choose Claude Code or Codex to sign in.")?,
            )?,
            Some(action @ ("check_updates" | "download_update" | "restart_update")) => {
                self.updates
                    .get()
                    .context("Updates are unavailable. Reopen Leo and try again.")?
                    .change(action)?;
            }
            _ => anyhow::bail!("Choose an action in Settings."),
        }
        self.view()
    }
    fn event_starting(&self, event: leo_web::app_settings::EventStarting) {
        crate::notify::starting(&self.app, event);
    }
    fn health(&self, notes: &Path) -> Result<Value> {
        use leo_web::dictation::DictationApi;
        let mut view = leo_host::health::scan(notes)?;
        let permissions = self.permission_view();
        let model = leo_services::speech::library::library()?.selected().is_ok();
        let dictation = self.dictation.view()?;
        let shortcut = dictation["shortcut"].clone();
        let mut checks = vec![
            json!({"name":"Microphone permission","state":if permissions["microphone"]=="allowed" {"ready"} else {"missing"},"fix":"Allow Leo to use the microphone in your system privacy settings.","action":"microphone"}),
            json!({"name":"Accessibility permission","state":if permissions["accessibility"]==true {"ready"} else {"missing"},"fix":"Allow Leo in System Settings → Privacy & Security → Accessibility to paste into other apps.","action":"accessibility"}),
            json!({"name":"Speech model","state":if model {"ready"} else {"missing"},"fix":"Download and use a model in Speech models.","section":"models"}),
            json!({"name":"Dictation shortcut","state":if shortcut["problem"].as_str().is_none_or(str::is_empty) {"ready"} else {"missing"},"fix":shortcut["problem"],"section":"shortcuts"}),
        ];
        if let Some(problem) = self.preferences.problem() {
            checks.push(json!({"name":"App preferences","state":"missing","fix":problem,"section":"general"}));
        }
        if crate::permissions::shortcut_clash(
            dictation["settings"]["shortcut"]
                .as_str()
                .unwrap_or_default(),
        ) {
            checks.push(json!({"name":"Shortcut clash","state":"warn","fix":if cfg!(target_os="macos") { "Alfred may also use Option+Space. Change its shortcut, or record another one in Dictation." } else { "Ctrl+Space may switch the current input method. Choose another shortcut in Dictation if it clashes." },"section":"dictation"}));
        }
        view["sections"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"title":"This app","checks":checks}));
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_preferences_keep_both_changes_and_roll_back_failed_native_effects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app-settings.json");
        let store = std::sync::Arc::new(PreferenceStore::new(
            path.clone(),
            std::sync::Arc::new(|_, _| Ok(())),
        ));
        let first = store.clone();
        let second = store.clone();
        let a = std::thread::spawn(move || {
            first
                .change(PreferenceChange::Set("automatic_updates".into(), false))
                .unwrap()
        });
        let b = std::thread::spawn(move || {
            second
                .change(PreferenceChange::Set("show_tray".into(), false))
                .unwrap()
        });
        a.join().unwrap();
        b.join().unwrap();
        let saved = Preferences::load(&path).unwrap();
        assert!(!saved.automatic_updates && !saved.show_tray);
        drop(store);
        let failed = PreferenceStore::new(
            path.clone(),
            std::sync::Arc::new(|_, _| anyhow::bail!("native setting failed")),
        );
        assert!(failed
            .change(PreferenceChange::Set("open_at_login".into(), false))
            .is_err());
        assert_eq!(Preferences::load(&path).unwrap(), saved);
        assert_eq!(failed.view(), saved);
    }
    #[test]
    fn a_failing_startup_effect_is_recorded_instead_of_returned() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreferenceStore::new(
            dir.path().join("app-settings.json"),
            std::sync::Arc::new(|_, _| Ok(())),
        );
        store.record(|| Ok(()));
        assert!(store.problem().is_none());
        store.record(|| anyhow::bail!("login item blocked"));
        assert!(store.problem().unwrap().contains("login item blocked"));
        store.record(|| anyhow::bail!("tray blocked"));
        assert!(store.problem().unwrap().contains("login item blocked"));
    }
    #[test]
    fn app_preferences_default_to_safe_updates_and_independent_onboarding_steps() {
        let mut preferences = Preferences::default();
        assert!(
            preferences.automatic_updates && preferences.install_on_quit && preferences.show_tray
        );
        preferences.finish_step(1).unwrap();
        assert_eq!(preferences.onboarding, [false, true, false]);
        preferences.finish_step(0).unwrap();
        preferences.finish_step(2).unwrap();
        assert_eq!(preferences.onboarding, [true, true, true]);
        assert!(!preferences.writing_done());
        preferences.finish_step(3).unwrap();
        assert_eq!(preferences.writing_done, Some(true));
        assert!(preferences.finish_step(4).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app-settings.json");
        preferences.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), preferences);
    }
    #[test]
    fn newer_files_load_and_writing_done_follows_old_setup() {
        let load = |json: &str| serde_json::from_str::<Preferences>(json).unwrap();
        assert!(load(r#"{"onboarding":[true,true,true]}"#).writing_done());
        assert!(!load(r#"{"onboarding":[true,true,false]}"#).writing_done());
        assert!(!load(r#"{"onboarding":[true,true,true],"writing_done":false}"#).writing_done());
        assert!(load(r#"{"writing_done":true}"#).writing_done());
        let newer = load(r#"{"automatic_updates":false,"from_a_newer_build":1}"#);
        assert!(!newer.automatic_updates);
    }
}
