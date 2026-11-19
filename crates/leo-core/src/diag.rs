//! Where background diagnostics go.
//!
//! Anything below the UI layer — a keychain that will not
//! answer, a malformed config, chunk progress during a long transcription —
//! reports through here instead of writing to stderr itself.

use std::collections::VecDeque;
use std::sync::Mutex;

const MOST_KEPT: usize = 20;
const MOST_CHARS: usize = 300;

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn remember(list: &mut VecDeque<String>, message: &str) {
    let message: String = message.chars().take(MOST_CHARS).collect();
    if list.contains(&message) {
        return;
    }
    list.push_back(message);
    while list.len() > MOST_KEPT {
        list.pop_front();
    }
}

/// Report something the user may want to know but that is not a failure of the
/// thing they asked for.
pub fn warn(message: impl Into<String>) {
    let message = message.into();
    eprintln!("  {message}");
    remember(
        &mut RECENT.lock().unwrap_or_else(|e| e.into_inner()),
        &message,
    );
}

/// The last different warnings of this session, oldest first.
pub fn recent() -> Vec<String> {
    RECENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_five_different_warnings_keep_the_last_twenty() {
        let mut list = VecDeque::new();
        for i in 0..25 {
            remember(&mut list, &format!("warning {i}"));
        }
        assert_eq!(list.len(), 20);
        assert_eq!(list.front().unwrap(), "warning 5");
        assert_eq!(list.back().unwrap(), "warning 24");
    }

    #[test]
    fn one_warning_sent_thirty_times_appears_once() {
        let mut list = VecDeque::new();
        for _ in 0..30 {
            remember(&mut list, "same");
        }
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn a_long_warning_is_kept_as_three_hundred_characters() {
        let mut list = VecDeque::new();
        remember(&mut list, &"x".repeat(1000));
        assert_eq!(list[0].chars().count(), 300);
    }

    #[test]
    fn a_multibyte_warning_is_cut_on_a_character_boundary() {
        let mut list = VecDeque::new();
        remember(&mut list, &"é".repeat(400));
        assert_eq!(list[0], "é".repeat(300));
    }

    #[test]
    fn warn_reaches_recent() {
        warn("diag test marker reaches recent");
        assert!(recent().contains(&"diag test marker reaches recent".to_string()));
    }
}
