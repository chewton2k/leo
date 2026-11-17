use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

pub enum Pasted {
    Done,
    Manual(String),
    Unsure(String),
}

fn paste_error(e: &str) -> String {
    log::warn!("[reliable-paste] {e}");
    if e.to_lowercase().contains("failed") {
        "Automatic paste could not preserve your clipboard. Copy your text from Recent dictations."
            .into()
    } else if e.contains("Recent dictations") {
        e.into()
    } else {
        format!("{e} Copy your text from Recent dictations.")
    }
}

pub fn copy(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|_| "The clipboard could not be opened. Try Copy again.".into())
}
pub(crate) fn copy_private(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_secret(text)
        .map(|()| crate::paste_tx::copied())
        .map_err(|_| "The clipboard could not be opened. Try Copy again.".into())
}
pub fn paste(
    app: &AppHandle,
    text: &str,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Pasted, String> {
    if !current() {
        return Ok(Pasted::Done);
    }
    if !crate::permissions::accessibility() || crate::permissions::elevated_target() {
        copy_private(app, text)?;
        return Ok(Pasted::Manual(if cfg!(target_os="macos") { "Press ⌘V. Allow Leo in System Settings → Privacy & Security → Accessibility for automatic paste." } else { "Press Ctrl+V. Windows cannot paste into this elevated window automatically." }.into()));
    }
    crate::input::wait_for_modifiers_up();
    if !current() {
        return Ok(Pasted::Done);
    }
    let app = app.clone();
    let caller = app.clone();
    let text = text.to_owned();
    let (done, finished) = std::sync::mpsc::sync_channel(1);
    let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let abandoned = timed_out.clone();
    caller
        .run_on_main_thread(move || {
            if abandoned.load(std::sync::atomic::Ordering::Acquire) || !current() {
                let _ = done.send(Ok(None));
                return;
            }
            let result = (|| {
                if app.try_state::<crate::input::EnigoState>().is_none() {
                    app.manage(crate::input::EnigoState::new().map_err(|e| {
                        log::warn!("[reliable-paste] {e}");
                        "Keyboard input could not start. Try again."
                    })?);
                }
                let input = app.state::<crate::input::EnigoState>();
                input
                    .0
                    .with(|enigo| {
                        crate::paste_tx::try_reliable_paste(&text, &app, enigo, current.clone())
                    })
                    .ok_or("Keyboard input is busy. Try again.")?
            })();
            let _ = done.send(result.map(Some).map_err(|e| paste_error(&e)));
        })
        .map_err(|e| e.to_string())?;
    let Some(read) = finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| {
            timed_out.store(true, std::sync::atomic::Ordering::Release);
            "Paste is taking too long. Copy the text from Recent dictations.".to_owned()
        })??
    else {
        return Ok(Pasted::Done);
    };
    let started = std::time::Instant::now();
    while !read() {
        if started.elapsed() >= READ_WAIT {
            return Ok(Pasted::Unsure(
                "The paste may not have landed. Copy it from Recent dictations.".into(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    Ok(Pasted::Done)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_clipboard_limits_have_one_wording() {
        assert_eq!(
            super::paste_error(crate::paste_tx::TOO_LARGE),
            "The clipboard is too large to preserve. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error(crate::paste_tx::TOO_MANY),
            "The clipboard cannot be preserved automatically. Copy your text from Recent dictations."
        );
    }

    #[test]
    fn clipboard_problems_keep_their_words_and_others_read_the_same() {
        assert_eq!(
            super::paste_error("The clipboard is too large to preserve."),
            "The clipboard is too large to preserve. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error("A clipboard format could not be preserved."),
            "A clipboard format could not be preserved. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error("Keyboard input is busy. Try again."),
            "Keyboard input is busy. Try again. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error("Keyboard input could not start. Try again."),
            "Keyboard input could not start. Try again. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error(
                "Automatic paste took too long. Copy your text from Recent dictations."
            ),
            "Automatic paste took too long. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error("OpenClipboard failed: Access is denied."),
            "Automatic paste could not preserve your clipboard. Copy your text from Recent dictations."
        );
        assert_eq!(
            super::paste_error("declareTypes:owner: failed"),
            "Automatic paste could not preserve your clipboard. Copy your text from Recent dictations."
        );
    }
}
