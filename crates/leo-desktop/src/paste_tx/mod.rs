#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub(crate) const QUIET_PERIOD: Duration = Duration::from_millis(200);

pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) const FAILED_INJECTION_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) const MAX_CLIPBOARD_BYTES: usize = 64 * 1024 * 1024;

pub(crate) const MOST_ENTRIES: usize = 256;

pub(crate) const TOO_LARGE: &str = "The clipboard is too large to preserve.";

pub(crate) const TOO_MANY: &str = "The clipboard cannot be preserved automatically.";

#[derive(Debug)]
pub(crate) struct TxState {
    pub published_at: Instant,
    pub injected_at: Option<Instant>,
    pub injection_failed: bool,
    pub receipts: Vec<Instant>,
    pub ownership_lost: bool,
    pub cancelled: bool,
    pub logged_receipt: bool,
}

impl TxState {
    pub fn new() -> Self {
        Self {
            published_at: Instant::now(),
            injected_at: None,
            injection_failed: false,
            receipts: Vec::new(),
            ownership_lost: false,
            cancelled: false,
            logged_receipt: false,
        }
    }

    pub fn record_receipt(&mut self, at: Instant) {
        if self.receipts.len() == 128 {
            self.receipts.remove(0);
        }
        self.receipts.push(at);
        if !self.logged_receipt {
            if let Some(injected) = self.injected_at {
                if at >= injected {
                    self.logged_receipt = true;
                    log::info!(
                        "[reliable-paste] clipboard read {}ms after chord",
                        at.duration_since(injected).as_millis()
                    );
                }
            }
        }
    }

    pub fn last_receipt_after_injection(&self) -> Option<Instant> {
        let injected = self.injected_at?;
        self.receipts.iter().copied().rev().find(|t| *t >= injected)
    }

    pub fn any_receipt_after_injection(&self) -> bool {
        self.last_receipt_after_injection().is_some()
    }
}

pub(crate) enum WaitDecision {
    KeepWaiting,
    Finish,
}

pub(crate) fn evaluate(state: &TxState, now: Instant) -> WaitDecision {
    if state.ownership_lost || state.cancelled {
        return WaitDecision::Finish;
    }
    if let Some(last) = state.last_receipt_after_injection() {
        if now.duration_since(last) >= QUIET_PERIOD {
            return WaitDecision::Finish;
        }
    }
    let deadline = if state.injection_failed {
        FAILED_INJECTION_TIMEOUT
    } else {
        RESTORE_TIMEOUT
    };
    if now.duration_since(state.published_at) >= deadline {
        return WaitDecision::Finish;
    }
    WaitDecision::KeepWaiting
}

const CHORD_HOLD_MS: u64 = 100;

pub(crate) type Read = Box<dyn Fn() -> bool + Send>;

pub(crate) fn keep_waiting(injected: bool, current: bool) -> bool {
    injected || current
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn restorable(types: &[&str], own: bool) -> bool {
    own || !types.iter().any(|kind| {
        *kind == "org.nspasteboard.ConcealedType" || *kind == "org.nspasteboard.TransientType"
    })
}

static OWN_COPY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MIN);

pub(crate) fn copied() {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    OWN_COPY.store(platform::count(), std::sync::atomic::Ordering::Release);
}

pub(crate) fn own_copy(count: i64) -> bool {
    same_copy(OWN_COPY.load(std::sync::atomic::Ordering::Acquire), count)
}

fn same_copy(recorded: i64, now: i64) -> bool {
    now != 0 && recorded == now
}

pub(crate) fn send_chord(
    enigo: &mut enigo::Enigo,
    state: &std::sync::Mutex<TxState>,
) -> Result<(), String> {
    sent(crate::input::send_paste_ctrl_v(enigo, CHORD_HOLD_MS), state)
}

fn sent(result: Result<(), String>, state: &std::sync::Mutex<TxState>) -> Result<(), String> {
    match result {
        Ok(()) => {
            log::info!("[reliable-paste] paste chord sent");
            Ok(())
        }
        Err(e) => {
            if let Ok(mut st) = state.lock() {
                st.injection_failed = true;
            }
            log::error!("[reliable-paste] failed to send paste chord: {e}");
            Err("The clipboard has your text, but the paste keys could not be sent.".into())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn try_reliable_paste(
    text: &str,
    app_handle: &tauri::AppHandle,
    enigo: &mut enigo::Enigo,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Read, String> {
    platform::run(text, app_handle, enigo, current)
}

pub(crate) fn flush(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    platform::flush_pending(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    #[cfg(target_os = "windows")]
    platform::flush_pending();
}

#[cfg(any(target_os = "macos", test))]
pub(crate) struct Snapshot(pub Vec<Vec<(String, Vec<u8>)>>);

#[cfg(test)]
mod tests {
    use super::*;

    fn state_after_publish(published_ago: Duration) -> TxState {
        let mut s = TxState::new();
        s.published_at = Instant::now() - published_ago;
        s
    }

    #[test]
    fn keeps_waiting_without_receipt_within_timeout() {
        let s = state_after_publish(Duration::from_millis(100));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_after_quiet_period_once_read() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(250));
        s.receipts.push(Instant::now() - QUIET_PERIOD);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn waits_through_quiet_period_after_recent_read() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        s.receipts.push(Instant::now() - Duration::from_millis(50));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn pre_injection_receipt_does_not_count() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.receipts.push(Instant::now() - Duration::from_millis(200));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        assert!(!s.any_receipt_after_injection());
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_on_timeout_without_receipt() {
        let s = state_after_publish(RESTORE_TIMEOUT);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn failed_injection_uses_short_timeout() {
        let mut s = state_after_publish(FAILED_INJECTION_TIMEOUT);
        s.injection_failed = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn only_a_wait_that_has_not_pasted_yet_is_cancelled() {
        assert!(keep_waiting(true, false));
        assert!(keep_waiting(true, true));
        assert!(keep_waiting(false, true));
        assert!(!keep_waiting(false, false));
    }

    #[test]
    fn concealed_or_transient_clipboards_are_not_restored_unless_leo_copied_them() {
        assert!(restorable(&["public.utf8-plain-text", "public.rtf"], false));
        assert!(restorable(&["org.nspasteboard.AutoGeneratedType"], false));
        let concealed = ["public.utf8-plain-text", "org.nspasteboard.ConcealedType"];
        assert!(!restorable(&concealed, false));
        assert!(restorable(&concealed, true));
        assert!(!restorable(&["org.nspasteboard.TransientType"], false));
        assert!(same_copy(7, 7));
        assert!(!same_copy(7, 8));
        assert!(!same_copy(0, 0), "a failed read is never Leo's copy");
        assert!(!same_copy(i64::MIN, 1));
    }

    #[test]
    fn a_failed_chord_is_an_error_that_keeps_the_transcript() {
        let state = std::sync::Mutex::new(TxState::new());
        assert!(sent(Ok(()), &state).is_ok());
        assert!(!state.lock().unwrap().injection_failed);
        let error = sent(Err("refused".into()), &state).unwrap_err();
        assert!(error.starts_with("The clipboard"));
        assert!(state.lock().unwrap().injection_failed);
    }

    #[test]
    fn ownership_loss_finishes_immediately() {
        let mut s = state_after_publish(Duration::from_millis(10));
        s.ownership_lost = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }
}

#[cfg(test)]
mod snapshot_tests {
    #[test]
    fn a_clipboard_snapshot_keeps_every_item_and_format() {
        let snapshot = super::Snapshot(vec![
            vec![
                ("text/plain".into(), b"hello".to_vec()),
                ("text/rtf".into(), b"{rtf}".to_vec()),
            ],
            vec![("public.file-url".into(), b"file:///tmp/a".to_vec())],
        ]);
        assert_eq!(snapshot.0.len(), 2);
        assert_eq!(snapshot.0[0].len(), 2);
    }
}
