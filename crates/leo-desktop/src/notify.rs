use std::time::{Duration, UNIX_EPOCH};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

const OFFER_FOR: Duration = Duration::from_secs(10 * 60);

pub fn listen(app: &AppHandle) {
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
    let builder = app
        .notification()
        .builder()
        .title(format!("{} is starting", event.title))
        .body("Click to open Record, ready to record it.")
        .extra("uid", &event.uid)
        .extra("kind", kind);
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        if let Ok(true) = mac_usernotifications::blocking::request_auth() {
            let _ = builder.show();
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = builder.show();
    let start = UNIX_EPOCH + Duration::from_secs(event.start.timestamp().max(0) as u64);
    crate::tray::offer(
        app,
        format!("{kind}:{}", event.uid),
        event.title,
        start + OFFER_FOR,
    );
}
