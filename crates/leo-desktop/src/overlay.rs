use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use leo_services::dictation::settings::{Popup, Settings};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};
#[cfg(target_os = "macos")]
tauri_panel! { panel!(DictationPanel { config: { can_become_key_window: false, is_floating_panel: true } }) }
#[cfg(target_os = "macos")]
static PLUGIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CREATING: Mutex<()> = Mutex::new(());
static FAILED: AtomicBool = AtomicBool::new(false);
const AFTER_PAGE_LOAD: &str = "after page load";
const ON_DEMAND: &str = "on demand";
const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 132.0;
#[cfg(target_os = "macos")]
pub fn plugin_registered(ok: bool) {
    PLUGIN.store(ok, std::sync::atomic::Ordering::Relaxed);
}
fn available() -> bool {
    #[cfg(target_os = "macos")]
    return PLUGIN.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(target_os = "macos"))]
    true
}
fn guarded<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("the popup could not be built".into()))
}
pub fn create_guarded(app: &AppHandle) -> Result<(), String> {
    if !available() {
        return Err("its plugin did not start".into());
    }
    let made = guarded(|| create(app).map_err(|e| e.to_string()));
    if made.is_err() {
        if let Some(window) = app.get_webview_window("dictation") {
            let _ = window.destroy();
        }
    }
    made
}
pub fn popup(app: &AppHandle, when: &str) -> Option<tauri::WebviewWindow> {
    if FAILED.load(Ordering::Acquire) {
        return None;
    }
    let _creating = CREATING.lock().unwrap_or_else(|e| e.into_inner());
    if FAILED.load(Ordering::Acquire) {
        return None;
    }
    if let Some(window) = app.get_webview_window("dictation") {
        return Some(window);
    }
    let began = std::time::Instant::now();
    match create_guarded(app) {
        Ok(()) => {
            eprintln!(
                "  dictation popup built in {} ms ({when})",
                began.elapsed().as_millis()
            );
            app.get_webview_window("dictation")
        }
        Err(why) => {
            FAILED.store(true, Ordering::Release);
            let sentence =
                crate::startup_problem(&[("dictation popup", Some(why))]).unwrap_or_default();
            eprintln!("  {sentence}");
            if let Some(settings) =
                app.try_state::<std::sync::Arc<crate::app_settings::AppSettings>>()
            {
                settings.note(sentence);
            }
            None
        }
    }
}
pub fn prepare(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("leo-popup".into())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    popup(&handle, AFTER_PAGE_LOAD);
                });
            }
            #[cfg(not(target_os = "macos"))]
            popup(&app, AFTER_PAGE_LOAD);
        });
}
fn create(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let panel = PanelBuilder::<_, DictationPanel>::new(app, "dictation")
            .url(tauri::WebviewUrl::App("dictation.html".into()))
            .title("Dictation")
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize::new(WIDTH, HEIGHT)))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| {
                w.decorations(false)
                    .transparent(true)
                    .focusable(false)
                    .visible(false)
            })
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()?;
        panel.hide();
    }
    #[cfg(not(target_os = "macos"))]
    tauri::WebviewWindowBuilder::new(
        app,
        "dictation",
        tauri::WebviewUrl::App("dictation.html".into()),
    )
    .title("Dictation")
    .inner_size(WIDTH, HEIGHT)
    .decorations(false)
    .transparent(true)
    .focused(false)
    .focusable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;
    Ok(())
}
pub fn show(
    app: &AppHandle,
    settings: &Settings,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) {
    if settings.popup == Popup::Off || !available() {
        return;
    }
    let app = app.clone();
    let top = settings.top;
    #[cfg(not(target_os = "macos"))]
    let Some(built) = popup(&app, ON_DEMAND) else {
        return;
    };
    let caller = app.clone();
    let _ = caller.run_on_main_thread(move || {
        if !current() {
            return;
        }
        #[cfg(target_os = "macos")]
        let Some(window) = popup(&app, ON_DEMAND) else {
            return;
        };
        #[cfg(not(target_os = "macos"))]
        let window = built;
        let point = crate::permissions::focused_point();
        let monitor = app
            .available_monitors()
            .ok()
            .and_then(|all| {
                all.into_iter().find(|m| {
                    let Some((x, y)) = point else {
                        return false;
                    };
                    let scale = if cfg!(target_os = "macos") {
                        m.scale_factor()
                    } else {
                        1.0
                    };
                    let p = m.position();
                    let s = m.size();
                    x >= p.x as f64 / scale
                        && y >= p.y as f64 / scale
                        && x < (p.x as f64 + s.width as f64) / scale
                        && y < (p.y as f64 + s.height as f64) / scale
                })
            })
            .or_else(|| {
                app.cursor_position()
                    .ok()
                    .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
            })
            .or_else(|| app.primary_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            #[cfg(windows)]
            place(&window, &monitor, top);
            #[cfg(not(windows))]
            {
                let scale = monitor.scale_factor();
                let p = monitor.position();
                let s = monitor.size();
                let x = p.x as f64 / scale + (s.width as f64 / scale - WIDTH) / 2.0;
                let y = p.y as f64 / scale
                    + if top {
                        46.0
                    } else {
                        s.height as f64 / scale - HEIGHT - 40.0
                    };
                let _ = window.set_position(tauri::LogicalPosition::new(x, y));
            }
        }
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(panel) = app.get_webview_panel("dictation") {
                panel.show();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window.show();
    });
}
#[cfg(any(windows, test))]
fn windows_bounds(
    origin: (i32, i32),
    size: (u32, u32),
    scale: f64,
    text_scale: f64,
    top: bool,
) -> (i32, i32, i32, i32) {
    let content = scale * text_scale;
    let width = (WIDTH * content).round().max(1.0) as i32;
    let height = (HEIGHT * content).round().max(1.0) as i32;
    let x = (origin.0 as f64 + (size.0 as f64 - width as f64) / 2.0).round() as i32;
    let y = if top {
        origin.1 as f64 + 46.0 * scale
    } else {
        origin.1 as f64 + size.1 as f64 - height as f64 - 40.0 * scale
    };
    (x, y.round() as i32, width, height)
}
#[cfg(windows)]
fn text_scale() -> f64 {
    use windows::{
        core::w,
        Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
        },
    };
    let mut percent = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Accessibility"),
            w!("TextScaleFactor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut percent as *mut u32).cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_SUCCESS {
        (percent as f64 / 100.0).clamp(1.0, 2.25)
    } else {
        1.0
    }
}
#[cfg(windows)]
fn place(window: &tauri::WebviewWindow, monitor: &tauri::Monitor, top: bool) {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER},
    };
    let p = monitor.position();
    let s = monitor.size();
    let (x, y, width, height) = windows_bounds(
        (p.x, p.y),
        (s.width, s.height),
        monitor.scale_factor(),
        text_scale(),
        top,
    );
    if let Ok(hwnd) = window.hwnd() {
        let _ = unsafe {
            SetWindowPos(
                HWND(hwnd.0),
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }
}
pub fn hide(app: &AppHandle) {
    if !available() {
        return;
    }
    let app = app.clone();
    let caller = app.clone();
    let _ = caller.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(panel) = app.get_webview_panel("dictation") {
                panel.hide();
            }
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(window) = app.get_webview_window("dictation") {
            let _ = window.hide();
        }
    });
}
pub fn tokens() -> &'static str {
    concat!(
        "\n",
        include_str!("../../leo-web/src/web/styles/01-tokens.css")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_while_building_the_popup_becomes_an_error() {
        let failed: Result<(), String> = guarded(|| panic!("no plugin"));
        assert_eq!(failed, Err("the popup could not be built".to_string()));
        assert_eq!(guarded(|| Ok(7)), Ok(7));
    }

    #[test]
    fn windows_bounds_use_the_destination_monitor_in_physical_pixels() {
        assert_eq!(
            windows_bounds((1920, 0), (3840, 2160), 1.5, 1.0, false),
            (3540, 1902, 600, 198)
        );
        assert_eq!(
            windows_bounds((-2560, -200), (2560, 1440), 1.25, 1.0, true),
            (-1530, -143, 500, 165)
        );
    }

    #[test]
    fn text_scale_grows_the_window_and_keeps_the_anchored_edge() {
        let (_, y, _, height) = windows_bounds((0, 0), (2560, 1440), 1.0, 1.0, false);
        let (x, scaled_y, width, scaled_height) =
            windows_bounds((0, 0), (2560, 1440), 1.0, 1.5, false);
        assert_eq!((width, scaled_height), (600, 198));
        assert_eq!(x, 980);
        assert_eq!(y + height, scaled_y + scaled_height);
        assert_eq!(windows_bounds((0, 0), (2560, 1440), 1.0, 1.5, true).1, 46);
    }
}
