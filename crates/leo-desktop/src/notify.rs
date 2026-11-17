use std::time::{Duration, UNIX_EPOCH};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

const OFFER_FOR: Duration = Duration::from_secs(10 * 60);

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
            crate::tray::page(&handle, "event", &format!("{kind}:{uid}"));
        }
    });
}

pub fn starting(app: &AppHandle, event: leo_web::app_settings::EventStarting) {
    let kind = if event.online { "call" } else { "microphone" };
    let title = format!("{} is starting", event.title);
    let page = format!("{kind}:{}", event.uid);
    let until = UNIX_EPOCH + Duration::from_secs(event.start.timestamp().max(0) as u64) + OFFER_FOR;
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let page = page.clone();
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
    crate::tray::offer(app, page, event.title, until);
}

const BODY: &str = "Click to open Record, ready to record it.";

#[cfg(target_os = "macos")]
fn post(app: &AppHandle, title: String, page: &str, until: std::time::SystemTime) {
    use mac_usernotifications::{block_on, blocking, Notification};
    use std::sync::atomic::{AtomicBool, Ordering};
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
            crate::tray::page(app, "event", page);
        }
    }
}
