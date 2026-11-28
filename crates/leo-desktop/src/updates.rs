use crate::app_settings::{AppSettings, UpdateApi};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};
const CHECK_GAP: Duration = Duration::from_secs(30 * 60);
const RETRY_GAP: Duration = Duration::from_secs(5 * 60);
const TICK: Duration = Duration::from_secs(60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MOST_UPDATE_BYTES: u64 = 1024 * 1024 * 1024;
const ENDPOINT: &str = "https://github.com/chewton2k/leo-cli/releases/latest/download/latest.json";
struct PublicDns;
impl reqwest_updater::dns::Resolve for PublicDns {
    fn resolve(&self, name: reqwest_updater::dns::Name) -> reqwest_updater::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            leo_services::web::public_dns(host)
                .await
                .map(|found| Box::new(found.into_iter()) as reqwest_updater::dns::Addrs)
                .map_err(|e| e.into())
        })
    }
}
enum Command {
    Check,
    Download,
    Install { lease: leo_core::activity::Lease },
    Stop,
}
struct Pending {
    update: Update,
    bytes: Option<Vec<u8>>,
}
pub struct Updates {
    app: AppHandle,
    preferences: Weak<AppSettings>,
    tx: mpsc::SyncSender<Command>,
    state: Mutex<Value>,
    ready: AtomicBool,
    operating: AtomicBool,
    installed: AtomicBool,
    quit_install: AtomicBool,
    leaving: AtomicBool,
    installed_lease: Mutex<Option<leo_core::activity::Lease>>,
}
impl Updates {
    pub fn start(app: AppHandle, preferences: &Arc<AppSettings>) -> Arc<Self> {
        let (tx, rx) = mpsc::sync_channel(4);
        let this = Arc::new(Self {
            app,
            preferences: Arc::downgrade(preferences),
            tx,
            state: Mutex::new(json!({"state":"idle","last_checked":null})),
            ready: AtomicBool::new(false),
            operating: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            quit_install: AtomicBool::new(false),
            leaving: AtomicBool::new(false),
            installed_lease: Mutex::new(None),
        });
        let worker = this.clone();
        std::thread::spawn(move || worker.run(rx));
        if !cfg!(debug_assertions)
            && std::env::var_os("LEO_NO_UPDATE_CHECK").is_none()
            && this.preferences().is_some_and(|p| p.automatic_updates)
        {
            let _ = this.change("check_updates");
        }
        this
    }
    fn preferences(&self) -> Option<crate::app_settings::Preferences> {
        self.preferences.upgrade().map(|p| p.preferences())
    }
    fn set(&self, key: &str, value: impl Into<Value>) {
        self.state.lock().unwrap_or_else(|e| e.into_inner())[key] = value.into();
    }
    fn failed(&self, message: &str) {
        self.set("error", message);
        self.set(
            "state",
            if self.ready.load(Ordering::Acquire) {
                "ready"
            } else {
                "error"
            },
        );
    }
    fn enqueue(&self, command: Command) -> Result<()> {
        ensure!(
            self.operating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "An update check or download is already running. Let it finish, then try again."
        );
        if self.tx.try_send(command).is_err() {
            self.operating.store(false, Ordering::Release);
            anyhow::bail!("Updates are busy. Try again in a moment.");
        }
        Ok(())
    }
    fn request_install(&self) -> Result<()> {
        ensure!(
            self.ready.load(Ordering::Acquire),
            "Download the update first, then restart Leo."
        );
        ensure!(
            self.operating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "An update check or download is already running. Let it finish, then try again."
        );
        let lease = match leo_core::activity::global().install() {
            Ok(lease) => lease,
            Err(error) => {
                self.operating.store(false, Ordering::Release);
                return Err(error);
            }
        };
        if self.tx.try_send(Command::Install { lease }).is_err() {
            self.operating.store(false, Ordering::Release);
            anyhow::bail!("Updates are busy. Try again in a moment.");
        }
        Ok(())
    }
    pub fn on_quit(&self) -> bool {
        if self.installed.load(Ordering::Acquire) {
            return false;
        }
        if hold_quit(false, leo_core::activity::global()) {
            return true;
        }
        if quit_install(
            self.preferences().is_some_and(|p| p.install_on_quit),
            self.ready.load(Ordering::Acquire),
            leo_core::activity::global().busy(),
        ) {
            self.quit_install.store(true, Ordering::Release);
            let started = self.request_install().is_ok();
            if !started {
                self.quit_install.store(false, Ordering::Release);
            }
            return started;
        }
        false
    }
    fn quit_install_failed(&self) {
        if self.quit_install.swap(false, Ordering::AcqRel) {
            self.ready.store(false, Ordering::Release);
            self.leaving.store(true, Ordering::Release);
            self.app.exit(0);
        }
    }
    #[cfg(target_os = "macos")]
    pub fn leaving(&self) -> bool {
        self.leaving.load(Ordering::Acquire)
    }
    pub fn stop(&self) {
        let _ = self.tx.try_send(Command::Stop);
    }
    fn run(self: Arc<Self>, rx: mpsc::Receiver<Command>) {
        let mut pending: Option<Pending> = None;
        let mut last: Option<(SystemTime, bool)> = None;
        loop {
            let command = match rx.recv_timeout(TICK) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if due(last, SystemTime::now())
                        && self.preferences().is_some_and(|p| p.automatic_updates)
                        && std::env::var_os("LEO_NO_UPDATE_CHECK").is_none()
                        && self
                            .operating
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        Command::Check
                    } else {
                        continue;
                    }
                }
            };
            self.set("error", Value::Null);
            match command {
                Command::Check => {
                    self.set("state", "checking");
                    let checked = tauri::async_runtime::block_on(self.check());
                    let now = SystemTime::now();
                    last = Some((now, checked.is_ok()));
                    self.set(
                        "last_checked",
                        now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                    );
                    match checked {
                        Ok(Some(update))=> {
                            let same=pending.as_ref().is_some_and(|p|p.update.version==update.version && p.update.signature==update.signature);
                            self.set("version",update.version.clone()); self.set("notes",update.body.as_deref().unwrap_or_default().chars().take(8192).collect::<String>());
                            if !same { pending=Some(Pending{update,bytes:None}); self.ready.store(false,Ordering::Release); }
                            if self.ready.load(Ordering::Acquire) { self.set("state","ready"); }
                            else if self.preferences().is_some_and(|p|p.install_on_quit) { if let Some(pending)=pending.as_mut() { self.download(pending); } }
                            else { self.set("state","available"); }
                        }
                        Ok(None)=> { pending=None; self.ready.store(false,Ordering::Release); self.set("state","current"); self.set("version",Value::Null); }
                        Err(_)=>self.failed("Leo could not check for updates. Check your connection and try Check now again."),
                    }
                }
                Command::Download => {
                    if let Some(pending) = pending.as_mut() {
                        self.download(pending);
                    } else {
                        self.failed("Check for updates first, then download the new version.");
                    }
                }
                Command::Install { lease } => {
                    if let Some(pending) = pending.as_ref().filter(|p| p.bytes.is_some()) {
                        self.set("state", "installing");
                        let result = leo_core::paths::config_dir().and_then(|config| {
                            apply(
                                lease,
                                &config,
                                || leo_services::speech::engine::engine().unload_worker(),
                                || {
                                    pending
                                        .update
                                        .install(pending.bytes.as_ref().unwrap())
                                        .map_err(anyhow::Error::from)
                                },
                            )
                        });
                        match result {
                            Ok(lease) => {
                                *self
                                    .installed_lease
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()) = Some(lease);
                                self.installed.store(true, Ordering::Release);
                                restart(&self.app);
                            }
                            Err(_) => {
                                self.quit_install_failed();
                                self.failed("The update could not be installed. Quit and reopen Leo, then try Restart to update again.");
                            }
                        }
                    } else {
                        drop(lease);
                        self.quit_install_failed();
                        self.failed("Check and download the update again before restarting.");
                    }
                }
                Command::Stop => return,
            }
            self.operating.store(false, Ordering::Release);
        }
    }
    async fn check(&self) -> Result<Option<Update>> {
        let key = include_str!("../updater-public-key.txt").trim();
        ensure!(!key.is_empty(), "Updates are not configured yet.");
        let app = self.app.clone();
        let updater = self
            .app
            .updater_builder()
            .pubkey(key)
            .endpoints(vec![ENDPOINT.parse()?])?
            .timeout(DOWNLOAD_TIMEOUT)
            .configure_client(|client| {
                client
                    .no_proxy()
                    .https_only(true)
                    .connect_timeout(Duration::from_secs(15))
                    .dns_resolver(Arc::new(PublicDns))
                    .redirect(reqwest_updater::redirect::Policy::custom(|attempt| {
                        if attempt.previous().len() < 5
                            && leo_services::web::public_https(attempt.url().as_str())
                        {
                            attempt.follow()
                        } else {
                            attempt.stop()
                        }
                    }))
            })
            .on_before_exit(move || crate::stop_engine(&app))
            .build()?;
        let update = tokio::time::timeout(CHECK_TIMEOUT, updater.check())
            .await
            .context("Update check timed out.")??;
        if let Some(update) = &update {
            ensure!(
                leo_services::web::public_https(update.download_url.as_str()),
                "The update download address is unsafe. Try again after the release is repaired."
            );
        }
        Ok(update)
    }
    fn download(&self, pending: &mut Pending) {
        self.set("state", "downloading");
        self.set("bytes", 0u64);
        self.set("total", Value::Null);
        let result = tauri::async_runtime::block_on(async {
            let exceeded = Arc::new(tokio::sync::Notify::new());
            let limit = exceeded.clone();
            let mut received = 0u64;
            let downloading = pending.update.download(
                |chunk, total| {
                    received = received.saturating_add(chunk as u64);
                    if received > MOST_UPDATE_BYTES || total.is_some_and(|n| n > MOST_UPDATE_BYTES)
                    {
                        limit.notify_one();
                    }
                    self.set("bytes", received);
                    self.set("total", json!(total));
                },
                || {},
            );
            tokio::select! {
                result=tokio::time::timeout(DOWNLOAD_TIMEOUT,downloading)=>Ok(result.context("Update download timed out.")??),
                _=exceeded.notified()=>Err(anyhow::anyhow!("The update download is too large.")),
            }
        });
        match result {
            Ok(bytes)=> { pending.bytes=Some(bytes); self.ready.store(true,Ordering::Release); self.set("state","ready"); }
            Err(_)=>self.failed("The update download could not be verified or completed. Check your connection and try Download again."),
        }
    }
}
impl UpdateApi for Updates {
    fn view(&self) -> Value {
        let mut view = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        view["busy"] = json!(leo_core::activity::global().busy());
        view
    }
    fn change(&self, action: &str) -> Result<()> {
        match action {
            "check_updates" => self.enqueue(Command::Check),
            "download_update" => self.enqueue(Command::Download),
            "restart_update" => self.request_install(),
            _ => anyhow::bail!("Choose Check now, Download or Restart to update."),
        }
    }
}
fn due(last: Option<(SystemTime, bool)>, now: SystemTime) -> bool {
    last.is_none_or(|(at, worked)| {
        now.duration_since(at).unwrap_or(Duration::MAX)
            >= if worked { CHECK_GAP } else { RETRY_GAP }
    })
}
fn hold_quit(installed: bool, gate: &leo_core::activity::Gate) -> bool {
    !installed && gate.installing()
}
fn quit_install(enabled: bool, ready: bool, busy: bool) -> bool {
    enabled && ready && !busy
}
fn apply(
    lease: leo_core::activity::Lease,
    config: &std::path::Path,
    stop: impl FnOnce() -> Result<()>,
    install: impl FnOnce() -> Result<()>,
) -> Result<leo_core::activity::Lease> {
    stop()?;
    crate::update_window::remember(config)?;
    if let Err(error) = install() {
        crate::update_window::take(config);
        return Err(error);
    }
    Ok(lease)
}

#[cfg(target_os = "macos")]
const REOPEN_SCRIPT: &str = r#"i=0; while kill -0 "$3" 2>/dev/null && [ "$i" -lt "$4" ]; do sleep 0.1; i=$((i+1)); done; "$1" "$2" || { sleep 1; "$1" "$2"; }"#;

#[cfg(target_os = "macos")]
const REOPEN_WAIT_TENTHS: u32 = 300;

#[cfg(target_os = "macos")]
fn bundle_of(binary: &std::path::Path) -> Option<std::path::PathBuf> {
    let macos = binary.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents")
        .then(|| bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
fn reopen_arguments(opener: &str, bundle: &std::path::Path, pid: u32, tenths: u32) -> Vec<String> {
    vec![
        "-c".into(),
        REOPEN_SCRIPT.into(),
        "sh".into(),
        opener.into(),
        bundle.display().to_string(),
        pid.to_string(),
        tenths.to_string(),
    ]
}

#[cfg(target_os = "macos")]
fn reopen_after_exit(bundle: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    leo_core::children::quiet_command("/bin/sh")
        .args(reopen_arguments(
            "/usr/bin/open",
            bundle,
            std::process::id(),
            REOPEN_WAIT_TENTHS,
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

fn restart(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(bundle) = std::env::current_exe().ok().and_then(|b| bundle_of(&b)) {
        if reopen_after_exit(&bundle).is_ok() {
            app.exit(tauri::RESTART_EXIT_CODE);
            return;
        }
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    #[test]
    fn checks_by_wall_clock_every_half_hour_and_retries_a_failure_after_five_minutes() {
        use std::time::{Duration, SystemTime};
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let after = |minutes: u64| at + Duration::from_secs(minutes * 60);
        assert!(super::due(None, at));
        assert!(!super::due(Some((at, true)), after(29)));
        assert!(super::due(Some((at, true)), after(30)));
        assert!(
            super::due(Some((at, true)), after(600)),
            "a long sleep counts"
        );
        assert!(!super::due(Some((at, false)), after(4)));
        assert!(super::due(Some((at, false)), after(5)));
        assert!(
            super::due(Some((at, true)), at - Duration::from_secs(60)),
            "a clock set back checks again"
        );
    }
    #[test]
    fn repeated_quit_waits_for_an_installation_but_recording_can_quit_normally() {
        let gate = leo_core::activity::Gate::default();
        let recording = gate.begin().unwrap();
        assert!(!super::hold_quit(false, &gate));
        drop(recording);
        let install = gate.install().unwrap();
        assert!(super::hold_quit(false, &gate));
        assert!(!super::hold_quit(true, &gate));
        drop(install);
        assert!(!super::hold_quit(false, &gate));
    }
    #[test]
    fn quit_installation_requires_ready_enabled_and_idle_in_every_order() {
        for enabled in [false, true] {
            for ready in [false, true] {
                for busy in [false, true] {
                    assert_eq!(
                        super::quit_install(enabled, ready, busy),
                        enabled && ready && !busy
                    );
                }
            }
        }
    }
    #[test]
    fn installation_stops_the_worker_first_and_excludes_new_capture() {
        let config = tempfile::tempdir().unwrap();
        let gate = leo_core::activity::Gate::default();
        let lease = gate.install().unwrap();
        let order = std::cell::RefCell::new(Vec::new());
        let completion = super::apply(
            lease,
            config.path(),
            || {
                assert!(gate.begin().is_err());
                order.borrow_mut().push("stop");
                Ok(())
            },
            || {
                assert!(gate.begin().is_err());
                order.borrow_mut().push("install");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), vec!["stop", "install"]);
        assert!(crate::update_window::take(config.path()));
        assert!(gate.begin().is_err());
        drop(completion);
        assert!(!gate.busy());
        let lease = gate.install().unwrap();
        assert!(super::apply(
            lease,
            config.path(),
            || anyhow::bail!("worker did not stop"),
            || panic!("must not install")
        )
        .is_err());
        assert!(!gate.busy());
        assert!(!crate::update_window::take(config.path()));
    }

    #[test]
    fn every_installation_marks_a_foreground_restart_and_failure_clears_it() {
        let config = tempfile::tempdir().unwrap();
        let gate = leo_core::activity::Gate::default();
        for fails in [false, true] {
            let result = super::apply(
                gate.install().unwrap(),
                config.path(),
                || Ok(()),
                || {
                    assert!(config.path().join("update-foreground").is_file());
                    if fails {
                        anyhow::bail!("installation failed");
                    }
                    Ok(())
                },
            );
            assert_eq!(result.is_err(), fails);
            assert_eq!(crate::update_window::take(config.path()), !fails);
            drop(result);
            assert!(!gate.busy());
        }
        let blocked = tempfile::NamedTempFile::new().unwrap();
        assert!(super::apply(
            gate.install().unwrap(),
            blocked.path(),
            || Ok(()),
            || panic!("must not install without remembering the foreground restart")
        )
        .is_err());
        assert!(!gate.busy());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn the_app_bundle_comes_from_the_running_binary() {
        let binary = std::path::Path::new("/Applications/Leo.app/Contents/MacOS/leo-app");
        assert_eq!(
            super::bundle_of(binary).unwrap(),
            std::path::Path::new("/Applications/Leo.app")
        );
        assert!(super::bundle_of(std::path::Path::new("/tmp/target/debug/leo-app")).is_none());
        assert!(super::bundle_of(std::path::Path::new("leo-app")).is_none());
    }

    #[cfg(target_os = "macos")]
    fn reopened_after(pid_lives: bool, tenths: u32) -> (std::time::Duration, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let opener = dir.path().join("open");
        let log = dir.path().join("opened");
        std::fs::write(
            &opener,
            format!("#!/bin/sh\necho \"$1\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut waited = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = waited.id();
        let started = std::time::Instant::now();
        let args = super::reopen_arguments(
            opener.to_str().unwrap(),
            std::path::Path::new("/Applications/Leo.app"),
            pid,
            tenths,
        );
        let mut script = std::process::Command::new("/bin/sh")
            .args(args)
            .spawn()
            .unwrap();
        if !pid_lives {
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(!log.exists(), "must wait for the old process");
            waited.kill().unwrap();
            waited.wait().unwrap();
        }
        script.wait().unwrap();
        let took = started.elapsed();
        waited.kill().ok();
        waited.wait().ok();
        (took, std::fs::read_to_string(log).unwrap())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_new_app_opens_once_the_old_one_has_exited() {
        let (took, opened) = reopened_after(false, 300);
        assert_eq!(opened, "/Applications/Leo.app\n");
        assert!(took >= std::time::Duration::from_millis(300));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_wait_is_bounded_when_the_old_process_never_exits() {
        let (took, opened) = reopened_after(true, 3);
        assert_eq!(opened, "/Applications/Leo.app\n");
        assert!(took < std::time::Duration::from_secs(5));
    }
}
