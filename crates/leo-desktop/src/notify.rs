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
        if let (Some(uid), Some(kind), "tap") = (extra("uid"), extra("kind"), performed.action_id())
        {
            open(&handle, &format!("{kind}:{uid}"));
        }
    });
}

pub fn starting(app: &AppHandle, event: leo_web::app_settings::EventStarting) {
    let kind = if event.online { "call" } else { "microphone" };
    let title = format!("{} is starting", event.title);
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let page = format!("{kind}:{}", event.uid);
        let until =
            UNIX_EPOCH + Duration::from_secs(event.start.timestamp().max(0) as u64) + OFFER_FOR;
        std::thread::spawn(move || post(&app, title, &page, until));
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

fn open(app: &AppHandle, event: &str) {
    static CLICKS: AtomicU64 = AtomicU64::new(0);
    crate::show(app);
    let click = CLICKS.fetch_add(1, Ordering::Relaxed);
    let app = app.clone();
    let event = serde_json::to_string(event).unwrap_or_default();
    let script = format!(
        "(() => {{ const done = window.__leoAsked = window.__leoAsked || {{}}; \
         if (done[{click}] || !window.leoFromApp) return; done[{click}] = true; \
         window.leoFromApp('event', {event}); }})()"
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
fn post(app: &AppHandle, title: String, page: &str, until: std::time::SystemTime) {
    use mac_usernotifications::{block_on, blocking, Notification};
    use std::sync::atomic::AtomicBool;
    static ASKING: AtomicBool = AtomicBool::new(false);
    if crate::permissions::notifications() == Some("denied") || ASKING.swap(true, Ordering::AcqRel)
    {
        return;
    }
    let allowed = matches!(blocking::request_auth(), Ok(true));
    ASKING.store(false, Ordering::Release);
    if !allowed {
        return;
    }
    let Ok(left) = until.duration_since(std::time::SystemTime::now()) else {
        return;
    };
    let sent = block_on(
        Notification::new()
            .title(title)
            .message(BODY)
            .timeout(left)
            .send(),
    );
    if let Ok(shown) = sent {
        if block_on(shown.response()).is_ok_and(|response| response.is_default_action()) {
            open(app, page);
        }
    }
}
