use std::path::Path;

const FOREGROUND: &str = leo_core::paths::own::UPDATE_FOREGROUND;

#[cfg(target_os = "macos")]
pub fn focus(window: &tauri::WebviewWindow) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindow};

    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let Ok(raw) = window.ns_window() else { return };
    let window = unsafe { &*raw.cast::<NSWindow>() };
    window.setCollectionBehavior(on_space(window.collectionBehavior()));
    window.deminiaturize(None);
    #[allow(deprecated)]
    NSApplication::sharedApplication(main).activateIgnoringOtherApps(true);
    window.makeMainWindow();
    window.makeKeyAndOrderFront(None);
}

#[cfg(target_os = "macos")]
fn on_space(
    behavior: objc2_app_kit::NSWindowCollectionBehavior,
) -> objc2_app_kit::NSWindowCollectionBehavior {
    use objc2_app_kit::NSWindowCollectionBehavior as B;
    (behavior & !B::CanJoinAllSpaces) | B::MoveToActiveSpace
}

pub fn may_open(gate: &leo_core::activity::Gate) -> bool {
    !gate.installing()
}

pub fn remember(config: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(config)?;
    let fresh = tempfile::NamedTempFile::new_in(config)?;
    fresh.persist(config.join(FOREGROUND))?;
    Ok(())
}

pub fn take(config: &Path) -> bool {
    std::fs::remove_file(config.join(FOREGROUND)).is_ok()
}

pub fn hidden(at_login: bool, foreground: bool) -> bool {
    at_login && !foreground
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn opening_moves_to_one_space_and_preserves_other_window_behaviors() {
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        for behavior in [
            B::empty(),
            B::CanJoinAllSpaces,
            B::FullScreenPrimary,
            B::Managed | B::CanJoinAllSpaces,
        ] {
            let next = super::on_space(behavior);
            assert!(next.contains(B::MoveToActiveSpace));
            assert!(!next.contains(B::CanJoinAllSpaces));
            assert_eq!(
                next & !B::MoveToActiveSpace,
                behavior & !B::CanJoinAllSpaces
            );
        }
    }

    #[test]
    fn opening_waits_for_installation_and_recovers_on_failure() {
        let gate = leo_core::activity::Gate::default();
        assert!(super::may_open(&gate));
        let recording = gate.begin().unwrap();
        assert!(super::may_open(&gate));
        drop(recording);
        let installation = gate.install().unwrap();
        assert!(!super::may_open(&gate));
        drop(installation);
        assert!(super::may_open(&gate));
    }

    #[test]
    fn an_update_restart_is_foreground_once_even_after_starting_at_login() {
        let dir = tempfile::tempdir().unwrap();
        for at_login in [false, true] {
            assert_eq!(super::hidden(at_login, super::take(dir.path())), at_login);
            super::remember(dir.path()).unwrap();
            assert!(!super::hidden(at_login, super::take(dir.path())));
            assert_eq!(super::hidden(at_login, super::take(dir.path())), at_login);
        }
    }
}
