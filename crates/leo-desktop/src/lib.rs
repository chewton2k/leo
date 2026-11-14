mod app_settings;
mod audio_feedback;
mod clipboard;
mod dictation;
#[cfg(target_os = "macos")]
mod helpers;
mod input;
mod links;
mod notify;
mod overlay;
mod paste_tx;
mod permissions;
mod port;
#[cfg(target_os = "macos")]
mod quit;
mod secure_input;
mod shell_path;
mod shortcut;
mod shortcut_events;
mod sign_in;
mod tray;
mod update_window;
mod updates;

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const MAIN: &str = "main";
const HIDDEN: &str = "--hidden";
const LAUNCHED: &str = "app-launched";

struct Engine(Mutex<Option<leo_web::Running>>);

pub fn run() {
    fix_path();
    let hidden = std::env::args().any(|a| a == HIDDEN);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN]),
        ))
        .invoke_handler(tauri::generate_handler![print_page, dictation_popup])
        .setup(move |app| {
            let config = leo_core::paths::config_dir().ok();
            let foreground = config.as_deref().is_some_and(update_window::take);
            let hidden = update_window::hidden(hidden, foreground);
            #[cfg(target_os = "macos")]
            app.handle().plugin(tauri_nspanel::init())?;
            overlay::create(app.handle())?;
            notify::listen(app.handle());
            let dictation = dictation::Dictation::new(app.handle().clone())?;
            let app_settings =
                app_settings::AppSettings::new(app.handle().clone(), dictation.clone())?;
            let mut powers = leo_host::powers();
            powers.dictation = Some(dictation.clone());
            powers.app_settings = Some(app_settings.clone());
            let updates = updates::Updates::start(app.handle().clone(), &app_settings);
            let _ = app_settings.updates.set(updates.clone());
            app.manage(updates);
            app.manage(app_settings.clone());
            app.manage(dictation);
            let wanted = config
                .as_deref()
                .map(port::remembered)
                .unwrap_or(port::DEFAULT_PORT);
            let started = tauri::async_runtime::block_on(leo_web::start(
                leo_web::StartOptions {
                    port: wanted,
                    token: leo_web::fresh_token(),
                },
                powers,
            ));
            let port = started.as_ref().map(|r| r.port()).unwrap_or(0);
            let address = match &started {
                Ok(running) => {
                    running.set_visible(!hidden);
                    if let Some(config) = &config {
                        let _ = port::remember(config, running.port());
                    }
                    WebviewUrl::External(
                        running
                            .sign_in_url()
                            .parse()
                            .expect("the sign-in address is a URL"),
                    )
                }
                Err(e) => WebviewUrl::App(
                    format!("index.html?problem={}", url_text(&format!("{e:#}"))).into(),
                ),
            };
            app.manage(Engine(Mutex::new(started.ok())));
            let builder = WebviewWindowBuilder::new(app, MAIN, address)
                .title("leo")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 480.0)
                .visible(!hidden)
                .focused(!hidden)
                .initialization_script(if hidden {
                    "window.__LEO_VISIBLE__ = false;"
                } else {
                    "window.__LEO_VISIBLE__ = true;"
                })
                .initialization_script(PRINT)
                .on_page_load(|window, payload| {
                    if payload.event() == tauri::webview::PageLoadEvent::Finished {
                        let visible = window.is_visible().unwrap_or(true)
                            && !window.is_minimized().unwrap_or(false);
                        sync_visible(&window, visible, true);
                        sync_fullscreen(&window, true);
                    }
                })
                // Tauri's own file-drop handler swallows the webview's drag events,
                // which broke every drag in the page (moving notes and folders,
                // combining notes). The page handles dropped files itself.
                .disable_drag_drop_handler()
                .on_navigation(move |url| {
                    if links::stays_inside(url, port) {
                        return true;
                    }
                    let _ = leo_core::open::link(url.as_str());
                    false
                })
                .on_new_window(move |url, _| {
                    if !links::stays_inside(&url, port) {
                        let _ = leo_core::open::link(url.as_str());
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .on_download(|_, event| {
                    match event {
                        tauri::webview::DownloadEvent::Requested { url, destination } => {
                            let suggested = destination
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .or_else(|| {
                                    url.path_segments()
                                        .and_then(|mut s| s.next_back().map(str::to_string))
                                })
                                .unwrap_or_default();
                            if let Some(folder) = downloads_folder() {
                                *destination = links::download_path(&folder, &suggested);
                                remember_download(destination.clone());
                            }
                        }
                        tauri::webview::DownloadEvent::Finished { success: true, .. } => {
                            reveal_last_download();
                        }
                        _ => {}
                    }
                    true
                });
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(18.0, 31.0))
                .initialization_script("window.__LEO_APP__ = 'mac';");
            let window = builder.build()?;
            let handle = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if cfg!(target_os = "macos") && handle.is_fullscreen().unwrap_or(false) {
                        leave_fullscreen_then_hide(&handle);
                    } else {
                        hide_window(&handle);
                    }
                }
                if let WindowEvent::Resized(_) = event {
                    let closing = CLOSING.load(std::sync::atomic::Ordering::Relaxed);
                    if closing != 0 && !handle.is_fullscreen().unwrap_or(false) {
                        finish_close_after(&handle, closing, Duration::from_millis(300));
                    }
                }
                if let WindowEvent::Focused(_) | WindowEvent::Resized(_) = event {
                    let visible = handle.is_visible().unwrap_or(true)
                        && !handle.is_minimized().unwrap_or(false);
                    set_visible(&handle, visible);
                    sync_fullscreen(&handle, false);
                }
            });
            #[cfg(target_os = "macos")]
            quit::menu(app.handle())?;
            tray::build(app.handle())?;
            open_at_login_once(app.handle(), config.as_deref());
            app_settings.apply_startup();
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("leo could not start");
    #[cfg(target_os = "macos")]
    let mut quitting = quit::Confirm::default();
    app.run(move |app, event| match event {
        RunEvent::Ready => {
            if app
                .get_webview_window(MAIN)
                .is_some_and(|w| w.is_visible().unwrap_or(false))
            {
                show(app);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show(app),
        RunEvent::ExitRequested { api, code, .. } => {
            #[cfg(target_os = "macos")]
            {
                let leaving = app
                    .try_state::<std::sync::Arc<updates::Updates>>()
                    .is_some_and(|updates| updates.leaving());
                if !quitting.allows(code, leaving, std::time::Instant::now()) {
                    api.prevent_exit();
                    show(app);
                    if let Some(window) = app.get_webview_window(MAIN) {
                        let _ = window.eval("window.leoFromApp?.('quit', '')");
                    }
                    return;
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = code;
            if app
                .try_state::<std::sync::Arc<updates::Updates>>()
                .is_some_and(|updates| updates.on_quit())
            {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => stop_engine(app),
        _ => {}
    });
}

const PRINT: &str = "(() => { const nativePrint = window.print.bind(window); window.print = () => window.__LEO_APP__ === 'mac' ? new Promise(done => setTimeout(done)).then(() => window.__TAURI_INTERNALS__.invoke('print_page')) : nativePrint(); })();";

#[tauri::command]
fn print_page(window: tauri::WebviewWindow) -> Result<(), String> {
    window
        .print()
        .map_err(|_| "Could not open the print dialog. Try exporting the note again.".into())
}

static LAST_DOWNLOAD: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

fn downloads_folder() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let folder = std::path::PathBuf::from(home).join("Downloads");
    folder.is_dir().then_some(folder)
}

fn remember_download(path: std::path::PathBuf) {
    if let Ok(mut last) = LAST_DOWNLOAD.lock() {
        *last = Some(path);
    }
}

fn reveal_last_download() {
    let path = LAST_DOWNLOAD.lock().ok().and_then(|mut last| last.take());
    let Some(path) = path else {
        return;
    };
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let _ = leo_core::children::quiet_command("open")
            .arg("-R")
            .arg(&path)
            .status();
    });
    #[cfg(windows)]
    let _ = leo_core::children::quiet_command("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

pub(crate) fn show(app: &AppHandle) {
    let app = app.clone();
    let caller = app.clone();
    let _ = caller.run_on_main_thread(move || {
        if !update_window::may_open(leo_core::activity::global()) {
            return;
        }
        if let Some(window) = app.get_webview_window(MAIN) {
            CLOSING.store(0, std::sync::atomic::Ordering::Relaxed);
            let _ = window.unminimize();
            let _ = window.show();
            set_visible(&window, true);
            #[cfg(target_os = "macos")]
            update_window::focus(&window);
            #[cfg(not(target_os = "macos"))]
            let _ = window.set_focus();
        }
    });
}

static CLOSING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLOSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn hide_window(window: &tauri::WebviewWindow) {
    let _ = window.hide();
    set_visible(window, false);
}

fn finish_close_after(window: &tauri::WebviewWindow, close: u64, wait: Duration) {
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        if CLOSING
            .compare_exchange(
                close,
                0,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            hide_window(&window);
        }
    });
}

fn leave_fullscreen_then_hide(window: &tauri::WebviewWindow) {
    if CLOSING.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        return;
    }
    let close = CLOSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    CLOSING.store(close, std::sync::atomic::Ordering::Relaxed);
    let _ = window.set_fullscreen(false);
    finish_close_after(window, close, Duration::from_millis(1200));
}

fn set_visible(window: &tauri::WebviewWindow, visible: bool) {
    sync_visible(window, visible, false);
}

fn sync_fullscreen(window: &tauri::WebviewWindow, force: bool) {
    static FULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let full = window.is_fullscreen().unwrap_or(false);
    if FULL.swap(full, std::sync::atomic::Ordering::Relaxed) != full || force {
        let _ = window.eval(if full {
            "document.documentElement.setAttribute('data-fullscreen', '')"
        } else {
            "document.documentElement.removeAttribute('data-fullscreen')"
        });
    }
}

fn sync_visible(window: &tauri::WebviewWindow, visible: bool, force: bool) {
    let changed = window
        .app_handle()
        .state::<Engine>()
        .0
        .lock()
        .ok()
        .and_then(|held| held.as_ref().map(|running| running.set_visible(visible)))
        .unwrap_or(false);
    if changed || force {
        let _ = window.eval(if visible {
            "window.__LEO_VISIBLE__ = true; window.dispatchEvent(new Event('leo-visibility'));"
        } else {
            "window.__LEO_VISIBLE__ = false; window.dispatchEvent(new Event('leo-visibility'));"
        });
    }
}

fn stop_engine(app: &AppHandle) {
    paste_tx::flush(app);
    if let Some(updates) = app.try_state::<std::sync::Arc<updates::Updates>>() {
        updates.stop();
    }
    if let Some(dictation) = app.try_state::<std::sync::Arc<dictation::Dictation>>() {
        dictation.shutdown();
    }
    sign_in::stop();
    let _ = leo_services::speech::engine::engine().stop();
    let running = app
        .state::<Engine>()
        .0
        .lock()
        .ok()
        .and_then(|mut held| held.take());
    leo_host::stop_writing();
    if let Some(running) = running {
        tauri::async_runtime::block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(6), running.stop()).await;
        });
    }
}

fn open_at_login_once(app: &AppHandle, config: Option<&std::path::Path>) {
    let Some(config) = config.filter(|_| !cfg!(debug_assertions)) else {
        return;
    };
    let marker = config.join(LAUNCHED);
    if marker.exists() {
        return;
    }
    if app.autolaunch().enable().is_ok() {
        let _ = std::fs::write(marker, "");
    }
}

fn fix_path() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let shell = std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/bin/zsh".into());
    let login = shell_path::login_path(&shell, Duration::from_secs(3));
    let current = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let path = shell_path::merged(
        &current,
        login.as_deref(),
        &shell_path::usual_folders(home.as_deref()),
    );
    std::env::set_var("PATH", path);
}

fn url_text(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[tauri::command]
fn dictation_popup(
    window: tauri::WebviewWindow,
    action: &str,
    state: tauri::State<'_, std::sync::Arc<dictation::Dictation>>,
) -> Result<serde_json::Value, String> {
    if window.label() != "dictation" {
        return Err("Open Dictation settings to control dictation.".into());
    }
    match action {
        "state" => {
            let mut view = state.popup();
            view["tokens"] = serde_json::json!(overlay::tokens());
            Ok(view)
        }
        "cancel" => {
            state.cancel();
            Ok(serde_json::json!({"ok":true}))
        }
        _ => Err("Choose an action on the dictation popup.".into()),
    }
}
