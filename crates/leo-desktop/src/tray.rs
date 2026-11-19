//! The menu bar icon: quick actions (record, copy the last dictation, Settings,
//! updates) and what Leo is using right now (speech model, writing AI and its
//! plan limits, AI use since launch, version). The status rows are disabled
//! items whose text a background thread refreshes when the icon is clicked, so
//! opening the menu never waits on the disk or an agent.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::app_settings::UpdateApi;

/// How often the status rows are read again when nobody clicks the icon.
/// Codex's limits need an app-server round trip, so they are asked only after
/// a click, and then at most every five minutes (`usage::STALE_AFTER`).
const REFRESH: Duration = Duration::from_secs(5 * 60);

const OFFER_TITLE_CHARS: usize = 40;

/// A menu command is offered to the page this many times, this far apart.
const ASK_TRIES: u32 = 10;
const ASK_GAP: Duration = Duration::from_millis(500);

const OFFER_AT: usize = 4;
const OFFER_CHECK: Duration = Duration::from_secs(15);

struct TrayMenu(Menu<Wry>);

struct Status {
    speech: MenuItem<Wry>,
    writing: MenuItem<Wry>,
    limits: MenuItem<Wry>,
    used: MenuItem<Wry>,
    clicked: AtomicBool,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let info = |id: &str, text: &str| MenuItem::with_id(app, id, text, false, None::<&str>);
    let open = item("open", "Open Leo")?;
    let lecture = item("record-microphone", "Record a lecture")?;
    let call = item("record-call", "Record a call")?;
    let copy = item("copy-dictation", "Copy last dictation")?;
    let status = Arc::new(Status {
        speech: info("speech", "Speech: …")?,
        writing: info("writing", "Writing: …")?,
        limits: info("limits", "Plan use: unknown")?,
        used: info("used", "No AI requests since Leo started")?,
        clicked: AtomicBool::new(false),
    });
    let settings = item("settings", "Settings…")?;
    let updates = item("updates", "Check for updates…")?;
    let version = info("version", &format!("Leo {}", env!("CARGO_PKG_VERSION")))?;
    let quit = item("quit", "Quit Leo")?;
    let line = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &line()?,
            &lecture,
            &call,
            &copy,
            &line()?,
            &status.speech,
            &status.writing,
            &status.limits,
            &status.used,
            &line()?,
            &settings,
            &updates,
            &version,
            &line()?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("leo")
        .tooltip("Leo")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show(app),
            "record-microphone" => page(app, "record", "microphone"),
            "record-call" => page(app, "record", "call"),
            "copy-dictation" => copy_last_dictation(app),
            "settings" => page(app, "settings", "general"),
            "updates" => {
                if let Some(updates) = app.try_state::<Arc<crate::updates::Updates>>() {
                    // A check already running is fine: the page shows its result.
                    let _ = updates.change("check_updates");
                }
                page(app, "settings", "updates");
            }
            "quit" => app.exit(0),
            id => {
                if let Some(uid) = id.strip_prefix("event:") {
                    page(app, "event", uid);
                }
            }
        });
    app.manage(TrayMenu(menu.clone()));
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    let handle = app.clone();
    let rows = Arc::clone(&status);
    let refresher = std::thread::Builder::new()
        .name("leo-tray-status".into())
        .spawn(move || refresh(handle, rows))?
        .thread()
        .clone();
    let tray = tray.on_tray_icon_event(move |tray, event| {
        let down = matches!(
            event,
            tauri::tray::TrayIconEvent::Click {
                button_state: tauri::tray::MouseButtonState::Down,
                ..
            }
        );
        if down {
            status.clicked.store(true, Ordering::Release);
            refresher.unpark();
        }
        #[cfg(target_os = "macos")]
        if down {
            popup(tray, menu.clone());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = tray;
    });
    #[cfg(target_os = "macos")]
    let tray = tray.show_menu_on_left_click(false);
    let built = tray.build(app)?;
    #[cfg(target_os = "macos")]
    built.with_inner_tray_icon(|tray| tray.set_show_menu_on_right_click(false))?;
    #[cfg(not(target_os = "macos"))]
    let _ = built;
    Ok(())
}

#[cfg(target_os = "macos")]
const FLIP_GAP: Duration = Duration::from_secs(1);

#[cfg(target_os = "macos")]
fn needs_flip(active: bool, window_here: bool) -> bool {
    !active && !window_here
}

#[cfg(target_os = "macos")]
fn window_on_active_space(app: &AppHandle) -> bool {
    app.get_webview_window(crate::MAIN)
        .and_then(|window| window.ns_window().ok())
        .is_some_and(|raw| unsafe { &*raw.cast::<objc2_app_kit::NSWindow>() }.isOnActiveSpace())
}

#[cfg(target_os = "macos")]
fn popup<M: tauri::menu::ContextMenu + 'static>(tray: &tauri::tray::TrayIcon, menu: M) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy as Policy};
    static LAST_FLIP: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let app = tray.app_handle().clone();
    let _ = tray.with_inner_tray_icon(move |tray| {
        if let Some(status) = tray.ns_status_item() {
            let raw = menu.inner_context().ns_menu();
            if let Some(menu) = unsafe { raw.cast::<objc2_app_kit::NSMenu>().as_ref() } {
                let main = objc2::MainThreadMarker::new();
                let flip = main.filter(|main| {
                    needs_flip(
                        NSApplication::sharedApplication(*main).isActive(),
                        window_on_active_space(&app),
                    )
                });
                if flip.is_some() {
                    let mut last = LAST_FLIP.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(rest) = last.and_then(|at| FLIP_GAP.checked_sub(at.elapsed())) {
                        std::thread::sleep(rest);
                    }
                    *last = Some(std::time::Instant::now());
                }
                if let Some(main) = flip {
                    NSApplication::sharedApplication(main).setActivationPolicy(Policy::Accessory);
                }
                #[allow(deprecated)]
                status.popUpStatusItemMenu(menu);
                if let Some(main) = flip {
                    NSApplication::sharedApplication(main).setActivationPolicy(Policy::Regular);
                }
                if let Some(main) = main {
                    if let Some(button) = status.button(main) {
                        button.highlight(false);
                    }
                }
            }
        }
    });
}

/// Shows the window and asks the page to go somewhere (`leoFromApp` in
/// app/events.js). `which` is passed as a JSON string literal. The page may
/// still be loading (first launch, a reload), so the request is tried for a few
/// seconds and runs at most once (`__leoAsked` remembers each click's number).
pub(crate) fn page(app: &AppHandle, what: &'static str, which: &str) {
    static CLICKS: AtomicU64 = AtomicU64::new(0);
    if !crate::update_window::may_open(leo_core::activity::global()) {
        return;
    }
    crate::show(app);
    let click = CLICKS.fetch_add(1, Ordering::Relaxed);
    let app = app.clone();
    let which = serde_json::to_string(which).unwrap_or_default();
    let script = format!(
        "(() => {{ const done = window.__leoAsked = window.__leoAsked || {{}}; \
         if (done[{click}] || !window.leoFromApp) return; done[{click}] = true; \
         window.leoFromApp('{what}', {which}); }})()"
    );
    let _ = std::thread::Builder::new()
        .name("leo-tray-page".into())
        .spawn(move || {
            for _ in 0..ASK_TRIES {
                if !crate::update_window::may_open(leo_core::activity::global()) {
                    return;
                }
                if let Some(window) = app.get_webview_window(crate::MAIN) {
                    let _ = window.eval(&script);
                }
                std::thread::sleep(ASK_GAP);
            }
        });
}

pub(crate) fn offer(app: &AppHandle, which: String, title: String, until: SystemTime) {
    let Some(menu) = app.try_state::<TrayMenu>().map(|held| held.0.clone()) else {
        return;
    };
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("leo-tray-event".into())
        .spawn(move || {
            let Ok(item) = MenuItem::with_id(
                &app,
                format!("event:{which}"),
                offer_label(&title),
                true,
                None::<&str>,
            ) else {
                return;
            };
            let mut shown = false;
            loop {
                let ended = SystemTime::now() >= until;
                let wanted = !ended && !recording_live(&app);
                if wanted != shown {
                    let _ = if wanted {
                        menu.insert(&item, OFFER_AT)
                    } else {
                        menu.remove(&item)
                    };
                    shown = wanted;
                }
                if ended {
                    return;
                }
                std::thread::sleep(OFFER_CHECK);
            }
        });
}

fn offer_label(title: &str) -> String {
    let shown: String = if title.chars().count() > OFFER_TITLE_CHARS {
        title.chars().take(OFFER_TITLE_CHARS).chain(['…']).collect()
    } else {
        title.to_string()
    };
    format!("Get ready to record {}", shown.replace('&', "&&"))
}

fn recording_live(app: &AppHandle) -> bool {
    app.try_state::<crate::Engine>().is_some_and(|engine| {
        engine.0.lock().ok().is_some_and(|held| {
            held.as_ref()
                .is_some_and(|running| running.recording_live())
        })
    })
}

/// Reads the newest dictation off the main thread, so a busy database never
/// holds up the menu.
fn copy_last_dictation(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("leo-tray-copy".into())
        .spawn(move || {
            let last = leo_core::paths::data_dir()
                .ok()
                .and_then(|data| leo_web::dictation::newest(&data).ok())
                .flatten();
            if let Some(text) = last {
                let _ = app.clipboard().write_text(text);
            }
        });
}

fn ask_due(last: Option<&(String, Instant)>, provider: &str, now: Instant) -> bool {
    last.is_none_or(|(asked, at)| {
        asked != provider || now.duration_since(*at) >= leo_services::usage::STALE_AFTER
    })
}

fn refresh(app: AppHandle, status: Arc<Status>) {
    let mut asked: Option<(String, Instant)> = None;
    loop {
        let clicked = status.clicked.swap(false, Ordering::AcqRel);
        let speech = leo_services::speech::library::library()
            .ok()
            .and_then(|library| library.selected_name())
            .unwrap_or_else(|| "no model chosen".into());
        let _ = status.speech.set_text(format!("Speech: {speech}"));
        let config = leo_services::config::Config::load();
        let store = leo_services::config::secret::default_store();
        let provider = leo_services::config::choice::selection(
            &config,
            store.as_ref(),
            leo_services::config::edit::Task::Chat,
        )
        .map(|selection| selection.provider)
        .unwrap_or_default();
        let may_ask = clicked && ask_due(asked.as_ref(), &provider, Instant::now());
        if may_ask {
            asked = Some((provider, Instant::now()));
        }
        let writing = leo_services::web_settings::writing_status(may_ask);
        let ready = leo_services::health::writing(&config, store.as_ref(), None).state;
        let _ = status.writing.set_text(format!("Writing: {}", writing.0));
        let _ = status
            .limits
            .set_text(plan_use(ready, &writing.0, writing.1.as_deref()));
        let used = app.try_state::<crate::Engine>().and_then(|engine| {
            engine
                .0
                .lock()
                .ok()
                .and_then(|held| held.as_ref().map(|running| running.usage_line()))
        });
        if let Some(used) = used {
            let _ = status.used.set_text(used);
        }
        if !status.clicked.load(Ordering::Acquire) {
            std::thread::park_timeout(REFRESH);
        }
    }
}

fn plan_use(
    state: leo_services::health::WritingState,
    writing: &str,
    limits: Option<&str>,
) -> String {
    match limits {
        Some(limits) => format!("Plan use: {limits}"),
        None if state == leo_services::health::WritingState::NotChosen => {
            format!(
                "Plan use: {}no writing AI is chosen.",
                leo_services::ai::NO_WRITING_AI
            )
        }
        None if writing.starts_with("Codex") => {
            "Plan use: unavailable; open Codex to refresh its limits".into()
        }
        None if writing.starts_with("Claude Code") => {
            "Plan use: Claude Code reports limits with an answer".into()
        }
        None => "Plan use: this provider has not reported limits".into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_event_menu_label_escapes_ampersands_and_cuts_long_titles_by_characters() {
        use super::offer_label;
        assert_eq!(offer_label("Q&A"), "Get ready to record Q&&A");
        assert_eq!(
            offer_label("Design sync"),
            "Get ready to record Design sync"
        );
        let long = "é".repeat(41);
        assert_eq!(
            offer_label(&long),
            format!("Get ready to record {}…", "é".repeat(40))
        );
        assert_eq!(
            offer_label(&"é".repeat(40)),
            format!("Get ready to record {}", "é".repeat(40))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_menu_flips_the_app_only_when_it_is_away_from_this_desktop() {
        use super::needs_flip as flip;
        assert!(flip(false, false));
        assert!(!flip(true, false), "Leo in front never flips");
        assert!(!flip(false, true), "Leo beside you here never flips");
    }

    #[test]
    fn limits_explain_the_selected_provider_and_preserve_reported_use() {
        use leo_services::health::WritingState::{NotChosen, Ready};
        for (state, provider, expected) in [
            (NotChosen, "No writing AI chosen", "no writing AI is chosen"),
            (Ready, "Codex · model", "open Codex"),
            (Ready, "Claude Code · model", "Claude Code reports limits"),
            (
                Ready,
                "OpenAI · model",
                "this provider has not reported limits",
            ),
            (
                Ready,
                "Ollama · model",
                "this provider has not reported limits",
            ),
        ] {
            assert!(super::plan_use(state, provider, None).contains(expected));
            assert_eq!(
                super::plan_use(state, provider, Some("5h 20%")),
                "Plan use: 5h 20%"
            );
        }
    }

    #[test]
    fn a_codex_ask_that_found_nothing_is_not_repeated_within_the_stale_time() {
        use super::ask_due;
        use std::time::{Duration, Instant};
        let t = Instant::now();
        assert!(ask_due(None, "codex", t));
        let last = ("codex".to_string(), t);
        assert!(!ask_due(
            Some(&last),
            "codex",
            t + Duration::from_secs(4 * 60)
        ));
        assert!(ask_due(
            Some(&last),
            "codex",
            t + Duration::from_secs(5 * 60)
        ));
    }

    #[test]
    fn switching_writing_ai_asks_at_once() {
        use super::ask_due;
        use std::time::{Duration, Instant};
        let t = Instant::now();
        let last = ("codex".to_string(), t);
        assert!(ask_due(
            Some(&last),
            "claude_code",
            t + Duration::from_secs(60)
        ));
        let t1 = t + Duration::from_secs(60);
        let last = ("claude_code".to_string(), t1);
        assert!(ask_due(Some(&last), "codex", t1 + Duration::from_secs(60)));
    }

    #[test]
    fn ten_clicks_in_five_minutes_ask_once() {
        use super::ask_due;
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut asked: Option<(String, Instant)> = None;
        let mut asks = 0;
        for click in 0..10u64 {
            let now = start + Duration::from_secs(click * 20);
            if ask_due(asked.as_ref(), "codex", now) {
                asked = Some(("codex".into(), now));
                asks += 1;
            }
        }
        assert_eq!(asks, 1);
    }
}
