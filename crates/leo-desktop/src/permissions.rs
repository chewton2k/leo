#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
pub fn accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        let trusted = unsafe { AXIsProcessTrusted() };
        if trusted && !GRANT_RECORDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            record_grant();
        }
        trusted
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
#[cfg(target_os = "macos")]
static GRANT_RECORDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
fn grant_file() -> Option<std::path::PathBuf> {
    leo_core::paths::config_dir()
        .ok()
        .map(|dir| dir.join("accessibility-version"))
}
#[cfg(target_os = "macos")]
fn record_grant() {
    if let Some(file) = grant_file() {
        let _ = leo_core::paths::write_atomic(&file, env!("CARGO_PKG_VERSION"));
    }
}
#[cfg(any(target_os = "macos", test))]
fn stale_grant(recorded: Option<&str>, current: &str) -> bool {
    recorded.is_some_and(|version| version.trim() != current)
}
pub fn elevated_target() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return true;
        };
        let mut token = Default::default();
        let mut elevated = TOKEN_ELEVATION::default();
        let mut size = 0;
        let result = OpenProcessToken(process, TOKEN_QUERY, &mut token).and_then(|_| {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevated as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        });
        let _ = CloseHandle(token);
        let _ = CloseHandle(process);
        result.is_err() || elevated.TokenIsElevated != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn frontmost_app() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if app.processIdentifier() as u32 == std::process::id() {
            return None;
        }
        Some(app.localizedName()?.to_string()).filter(|name| !name.trim().is_empty())
    }
    #[cfg(windows)]
    unsafe {
        use windows::{
            core::PWSTR,
            Win32::{
                Foundation::CloseHandle,
                System::Threading::{
                    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION,
                },
                UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
            },
        };
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        let found = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        found.ok()?;
        let path = String::from_utf16_lossy(path.get(..size as usize)?);
        std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

pub fn focused_point() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::ffi::c_void;
        #[repr(C)]
        struct Point {
            x: f64,
            y: f64,
        }
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXUIElementCreateSystemWide() -> *const c_void;
            fn AXUIElementCopyAttributeValue(
                element: *const c_void,
                attribute: *const c_void,
                value: *mut *const c_void,
            ) -> i32;
            fn AXUIElementSetMessagingTimeout(element: *const c_void, seconds: f32) -> i32;
            fn AXValueGetValue(value: *const c_void, kind: i32, out: *mut c_void) -> bool;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFRelease(value: *const c_void);
        }
        struct Owned(*const c_void);
        impl Drop for Owned {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { CFRelease(self.0) };
                }
            }
        }
        fn attribute(element: *const c_void, name: &str) -> Option<Owned> {
            let name = objc2_foundation::NSString::from_str(name);
            let mut value = std::ptr::null();
            if unsafe {
                AXUIElementCopyAttributeValue(
                    element,
                    (&*name as *const objc2_foundation::NSString).cast(),
                    &mut value,
                )
            } != 0
                || value.is_null()
            {
                return None;
            }
            Some(Owned(value))
        }
        if !accessibility() {
            return None;
        }
        let system = Owned(AXUIElementCreateSystemWide());
        AXUIElementSetMessagingTimeout(system.0, 0.25);
        let app = attribute(system.0, "AXFocusedApplication")?;
        let window = attribute(app.0, "AXFocusedWindow")?;
        let position = attribute(window.0, "AXPosition")?;
        let size = attribute(window.0, "AXSize")?;
        let mut point = Point { x: 0.0, y: 0.0 };
        let mut extent = Point { x: 0.0, y: 0.0 };
        if !AXValueGetValue(position.0, 1, (&mut point as *mut Point).cast())
            || !AXValueGetValue(size.0, 2, (&mut extent as *mut Point).cast())
        {
            return None;
        }
        Some((point.x + extent.x / 2.0, point.y + extent.y / 2.0))
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::RECT,
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect},
        };
        let mut rect = RECT::default();
        GetWindowRect(GetForegroundWindow(), &mut rect).ok()?;
        Some((
            (rect.left as f64 + rect.right as f64) / 2.0,
            (rect.top as f64 + rect.bottom as f64) / 2.0,
        ))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

pub fn microphone() -> &'static str {
    if leo_services::session::mic::disabled() {
        return "unavailable";
    }
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let Some(media) = AVMediaTypeAudio else {
            return "unknown";
        };
        let status = AVCaptureDevice::authorizationStatusForMediaType(media);
        if status == AVAuthorizationStatus::Authorized {
            "allowed"
        } else if status == AVAuthorizationStatus::NotDetermined {
            "not_requested"
        } else {
            "denied"
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows::{
            Devices::Enumeration::{DeviceAccessInformation, DeviceAccessStatus, DeviceClass},
            Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
        };
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let status = DeviceAccessInformation::CreateFromDeviceClass(DeviceClass::AudioCapture)
            .and_then(|info| info.CurrentStatus());
        if initialized {
            CoUninitialize();
        }
        match status {
            Ok(DeviceAccessStatus::Allowed) => "allowed",
            Ok(DeviceAccessStatus::DeniedByUser | DeviceAccessStatus::DeniedBySystem) => "denied",
            _ => "unknown",
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        "unknown"
    }
}
#[cfg(any(target_os = "macos", test))]
const NOTIFICATION_FRESH: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(any(target_os = "macos", test))]
fn due(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|at| now.duration_since(at) >= NOTIFICATION_FRESH)
}
#[cfg(target_os = "macos")]
static LAST: std::sync::Mutex<Option<(std::time::Instant, Option<&'static str>)>> =
    std::sync::Mutex::new(None);
#[cfg(target_os = "macos")]
static PROBING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
fn probe() -> Option<&'static str> {
    use mac_usernotifications::AuthorizationStatus::*;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(mac_usernotifications::blocking::get_notification_settings());
    });
    let settings = rx
        .recv_timeout(std::time::Duration::from_millis(500))
        .ok()?
        .ok()?;
    Some(match settings.authorization_status {
        Authorized | Provisional | Ephemeral => "allowed",
        NotDetermined => "not_requested",
        _ => "denied",
    })
}
pub fn notifications() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        let answer = probe();
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), answer));
        answer
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
pub fn notifications_soon() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::Ordering::{AcqRel, Release};
        let last = *LAST.lock().unwrap_or_else(|e| e.into_inner());
        if due(last.map(|(at, _)| at), std::time::Instant::now()) && !PROBING.swap(true, AcqRel) {
            std::thread::spawn(|| {
                notifications();
                PROBING.store(false, Release);
            });
        }
        last.and_then(|(_, answer)| answer)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
pub fn forget_notifications() {
    #[cfg(target_os = "macos")]
    {
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
pub fn request_notifications(app: &tauri::AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if notifications() == Some("denied") {
            return open_system_settings(
                &format!(
                    "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={}",
                    app.config().identifier
                ),
                "System Settings could not be opened. Open System Settings → Notifications and turn on Leo.",
            );
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(mac_usernotifications::blocking::request_auth());
        });
        let _ = rx.recv_timeout(std::time::Duration::from_secs(30));
        notifications();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}
pub fn request_microphone(app: &tauri::AppHandle) -> anyhow::Result<()> {
    anyhow::ensure!(
        !leo_services::session::mic::disabled(),
        "Microphone capture is disabled in this session."
    );
    // macOS shows its dialog only once: after a denial a request does nothing,
    // so the privacy pane is the only place the user can turn Leo back on.
    #[cfg(target_os = "macos")]
    if microphone() == "denied" {
        return open_macos_privacy("Privacy_Microphone");
    }
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(|| unsafe {
        use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
        if let Some(media) = AVMediaTypeAudio {
            let callback = block2::RcBlock::new(|_: objc2::runtime::Bool| {});
            AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &callback);
        }
    })?;
    #[cfg(windows)]
    {
        let _ = app;
        open_privacy("ms-settings:privacy-microphone")?;
    }
    Ok(())
}
pub fn request_accessibility(app: &tauri::AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let recorded = grant_file().and_then(|file| std::fs::read_to_string(file).ok());
        if !accessibility() && stale_grant(recorded.as_deref(), env!("CARGO_PKG_VERSION")) {
            let _ = leo_core::children::quiet_command("/usr/bin/tccutil")
                .args(["reset", "Accessibility", app.config().identifier.as_str()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            record_grant();
        }
        tauri::async_runtime::block_on(
            tauri_plugin_macos_permissions::request_accessibility_permission(),
        );
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}
#[cfg(target_os = "macos")]
fn open_macos_privacy(pane: &str) -> anyhow::Result<()> {
    open_system_settings(
        &format!("x-apple.systempreferences:com.apple.preference.security?{pane}"),
        "System Settings could not be opened. Open System Settings → Privacy & Security → Microphone and turn on Leo.",
    )
}
#[cfg(target_os = "macos")]
fn open_system_settings(uri: &str, failed: &str) -> anyhow::Result<()> {
    let status = leo_core::children::quiet_command("open")
        .arg(uri)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    anyhow::ensure!(status.is_ok_and(|status| status.success()), "{failed}");
    Ok(())
}
#[cfg(windows)]
fn open_privacy(uri: &str) -> anyhow::Result<()> {
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let uri: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(uri.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(result.0 as usize>32,"Windows privacy settings could not be opened. Open Settings → Privacy & security → Microphone.");
    Ok(())
}

pub fn shortcut_clash(shortcut: &str) -> bool {
    let value = shortcut.to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        if !["option+space", "alt+space", "opt+space"].contains(&value.as_str()) {
            return false;
        }
        crate::helpers::capture(
            "pgrep",
            &[OsStr::new("-f"), OsStr::new("/Alfred[^/]*\\.app/")],
            std::time::Duration::from_secs(1),
        )
        .is_ok_and(|output| output.status.success())
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::GetKeyboardLayout,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        if !["ctrl+space", "control+space"].contains(&value.as_str()) {
            return false;
        }
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let language = (GetKeyboardLayout(thread).0 as usize) & 0x3ff;
        matches!(language, 4 | 17 | 18)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = value;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{due, stale_grant, NOTIFICATION_FRESH};
    use std::time::{Duration, Instant};

    #[test]
    fn a_recent_answer_is_reused_and_an_old_one_is_not() {
        let now = Instant::now();
        let ago = |secs: u64| now.checked_sub(Duration::from_secs(secs));
        assert!(due(None, now));
        assert!(!due(ago(9), now));
        assert!(due(ago(10), now));
    }

    #[test]
    fn a_minute_of_polling_starts_at_most_six_probes() {
        let start = Instant::now();
        let mut last = None;
        let mut probes = 0;
        for poll in 0..24 {
            let now = start + Duration::from_millis(2500 * poll);
            if due(last, now) {
                probes += 1;
                last = Some(now);
            }
        }
        assert_eq!(NOTIFICATION_FRESH, Duration::from_secs(10));
        assert!(probes <= 6, "{probes} probes in a minute");
    }

    #[test]
    fn reset_only_after_a_version_change_with_a_recorded_grant() {
        assert!(!stale_grant(None, "0.4.26"));
        assert!(!stale_grant(Some("0.4.26\n"), "0.4.26"));
        assert!(stale_grant(Some("0.4.25"), "0.4.26"));
    }
}
