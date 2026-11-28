use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::UNIX_EPOCH;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

#[cfg(target_os = "macos")]
const OFFER_FOR: Duration = Duration::from_secs(10 * 60);
const ASK_TRIES: u32 = 10;
const ASK_GAP: Duration = Duration::from_millis(500);

pub fn listen(app: &AppHandle) {
    if cfg!(target_os = "macos") {
        return;
    }
    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        let extra = |name: &str| {
            performed
                .notification()
                .and_then(|shown| shown.extra().get(name))
                .and_then(|value| value.as_str())
        };
        if performed.action_id() != "tap" {
            return;
        }
        if let Some(route) = extra("route") {
            open(&handle, "go", route);
        } else if let (Some(uid), Some(kind)) = (extra("uid"), extra("kind")) {
            open(&handle, "event", &format!("{kind}:{uid}"));
        }
    });
}

pub fn starting(app: &AppHandle, event: leo_web::app_settings::EventStarting) {
    let kind = if event.online { "call" } else { "microphone" };
    let early = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .is_ok_and(|now| (now.as_secs() as i64) < event.start.timestamp());
    let title = if event.lead > 0 && early {
        format!("{} starts in {} min", event.title, event.lead)
    } else {
        format!("{} is starting", event.title)
    };
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let page = format!("{kind}:{}", event.uid);
        let until =
            UNIX_EPOCH + Duration::from_secs(event.start.timestamp().max(0) as u64) + OFFER_FOR;
        std::thread::spawn(move || post(&app, title, BODY.into(), ("event", page), until));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(BODY)
        .extra("uid", &event.uid)
        .extra("kind", kind)
        .show();
}

pub fn remind(app: &AppHandle, reminder: leo_web::app_settings::Reminder) {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let until = std::time::SystemTime::now() + OFFER_FOR;
        std::thread::spawn(move || {
            post(
                &app,
                reminder.title,
                reminder.body,
                ("go", reminder.route),
                until,
            )
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app
        .notification()
        .builder()
        .title(reminder.title)
        .body(reminder.body)
        .extra("route", &reminder.route)
        .show();
}

fn open(app: &AppHandle, what: &str, which: &str) {
    static CLICKS: AtomicU64 = AtomicU64::new(0);
    crate::show(app);
    let click = CLICKS.fetch_add(1, Ordering::Relaxed);
    let app = app.clone();
    let what = serde_json::to_string(what).unwrap_or_default();
    let which = serde_json::to_string(which).unwrap_or_default();
    let script = format!(
        "(() => {{ const done = window.__leoAsked = window.__leoAsked || {{}}; \
         if (done[{click}] || !window.leoFromApp) return; done[{click}] = true; \
         window.leoFromApp({what}, {which}); }})()"
    );
    let _ = std::thread::Builder::new()
        .name("leo-event-page".into())
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

const BODY: &str = "Click to open Record, ready to record it.";

#[cfg(target_os = "macos")]
fn post(
    app: &AppHandle,
    title: String,
    body: String,
    click: (&str, String),
    until: std::time::SystemTime,
) {
    use mac_usernotifications::{block_on, blocking, Notification};
    static ASKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let allowed = {
        let _asking = ASKING.lock().unwrap_or_else(|e| e.into_inner());
        crate::permissions::notifications() != Some("denied")
            && matches!(blocking::request_auth(), Ok(true))
    };
    if !allowed {
        return;
    }
    let Ok(left) = until.duration_since(std::time::SystemTime::now()) else {
        return;
    };
    let sent = block_on(
        Notification::new()
            .title(title)
            .message(&body)
            .timeout(left)
            .send(),
    );
    if let Ok(shown) = sent {
        if block_on(shown.response()).is_ok_and(|response| response.is_default_action()) {
            open(app, click.0, &click.1);
        }
    }
}
