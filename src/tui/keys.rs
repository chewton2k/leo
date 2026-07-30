//! Keymap: a key press plus the current focus becomes one [`Intent`].
//!
//! Kept as a pure function so the whole keymap is table-testable without a
//! terminal. Anything that needs arguments goes through the `:` line instead of
//! growing a chord here.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Which pane has focus. Movement keys mean different things in each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Dirs,
    Notes,
    Preview,
}

/// What a key press means. Most map to a UI movement; `Command`-family
/// intents open input surfaces, and the rest become [`crate::action::Action`]s
/// once the App knows what is selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Nothing,
    Quit,
    /// Move the focused pane's selection.
    Down,
    Up,
    /// Jump to the first or last item of the focused pane.
    First,
    Last,
    /// Move focus left or right.
    FocusLeft,
    FocusRight,
    /// Scroll the preview without moving the note selection.
    ScrollDown,
    ScrollUp,
    /// Act on the current selection: open a directory, or focus a note's body.
    Open,
    /// Toggle the first unchecked checkbox of the selected note.
    ToggleCheckbox,
    /// Edit the selected note in `$EDITOR`.
    EditSelected,
    /// Delete the selected note, with confirmation.
    DeleteSelected,
    /// Open the `:` line, optionally pre-filled (`/` seeds `search `).
    OpenCommand { seed: &'static str },
    /// Open the fuzzy finder overlay.
    OpenFinder,
    ToggleHelp,
    /// Leave whatever overlay or mode is active.
    Cancel,
    /// Re-read the store from disk.
    Reload,
}

/// Map a key press in Normal mode to an intent.
pub fn normal(key: KeyEvent, focus: Pane) -> Intent {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Control chords first: they must not be shadowed by the plain letters.
    if ctrl {
        return match key.code {
            KeyCode::Char('p') => Intent::OpenFinder,
            KeyCode::Char('c') => Intent::Quit,
            KeyCode::Char('d') => Intent::ScrollDown,
            KeyCode::Char('u') => Intent::ScrollUp,
            KeyCode::Char('r') => Intent::Reload,
            _ => Intent::Nothing,
        };
    }

    match key.code {
        KeyCode::Char('q') => Intent::Quit,
        KeyCode::Char('j') | KeyCode::Down => Intent::Down,
        KeyCode::Char('k') | KeyCode::Up => Intent::Up,
        KeyCode::Char('g') | KeyCode::Home => Intent::First,
        KeyCode::Char('G') | KeyCode::End => Intent::Last,
        KeyCode::Char('h') | KeyCode::Left => Intent::FocusLeft,
        KeyCode::Char('l') | KeyCode::Right => Intent::FocusRight,
        KeyCode::Enter => Intent::Open,
        KeyCode::Char('x') => Intent::ToggleCheckbox,
        KeyCode::Char('e') => Intent::EditSelected,
        KeyCode::Char('D') => Intent::DeleteSelected,
        KeyCode::Char(':') => Intent::OpenCommand { seed: "" },
        // `/` is a shorthand for the search verb, so one keymap entry covers it.
        KeyCode::Char('/') => Intent::OpenCommand { seed: "search " },
        KeyCode::Char('?') => Intent::ToggleHelp,
        KeyCode::Esc => Intent::Cancel,
        // In the preview pane, space and the page keys scroll.
        KeyCode::Char(' ') | KeyCode::PageDown if focus == Pane::Preview => Intent::ScrollDown,
        KeyCode::PageUp if focus == Pane::Preview => Intent::ScrollUp,
        KeyCode::PageDown => Intent::ScrollDown,
        KeyCode::PageUp => Intent::ScrollUp,
        _ => Intent::Nothing,
    }
}

impl Pane {
    pub fn left(self) -> Pane {
        match self {
            Pane::Dirs => Pane::Dirs,
            Pane::Notes => Pane::Dirs,
            Pane::Preview => Pane::Notes,
        }
    }

    pub fn right(self) -> Pane {
        match self {
            Pane::Dirs => Pane::Notes,
            Pane::Notes => Pane::Preview,
            Pane::Preview => Pane::Preview,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// The keymap documented in the spec, one row per binding.
    #[test]
    fn documented_bindings_map_as_specified() {
        let cases: &[(KeyEvent, Intent)] = &[
            (key('j'), Intent::Down),
            (key('k'), Intent::Up),
            (key('h'), Intent::FocusLeft),
            (key('l'), Intent::FocusRight),
            (key('x'), Intent::ToggleCheckbox),
            (key(':'), Intent::OpenCommand { seed: "" }),
            (key('/'), Intent::OpenCommand { seed: "search " }),
            (key('q'), Intent::Quit),
            (key('e'), Intent::EditSelected),
            (key('?'), Intent::ToggleHelp),
            (code(KeyCode::Enter), Intent::Open),
            (ctrl('p'), Intent::OpenFinder),
        ];
        for (k, expected) in cases {
            assert_eq!(normal(*k, Pane::Notes), *expected, "for {k:?}");
        }
    }

    #[test]
    fn arrow_keys_mirror_the_vi_movement_keys() {
        assert_eq!(normal(code(KeyCode::Down), Pane::Notes), Intent::Down);
        assert_eq!(normal(code(KeyCode::Up), Pane::Notes), Intent::Up);
        assert_eq!(normal(code(KeyCode::Left), Pane::Notes), Intent::FocusLeft);
        assert_eq!(normal(code(KeyCode::Right), Pane::Notes), Intent::FocusRight);
    }

    /// Ctrl chords must not be shadowed by the plain letter binding.
    #[test]
    fn ctrl_chords_are_not_confused_with_plain_letters() {
        assert_eq!(normal(ctrl('d'), Pane::Notes), Intent::ScrollDown);
        assert_eq!(normal(key('D'), Pane::Notes), Intent::DeleteSelected);
        assert_eq!(normal(ctrl('u'), Pane::Notes), Intent::ScrollUp);
        assert_eq!(normal(ctrl('c'), Pane::Notes), Intent::Quit);
        assert_eq!(normal(ctrl('r'), Pane::Notes), Intent::Reload);
        // An unbound chord does nothing rather than falling through to a letter.
        assert_eq!(normal(ctrl('z'), Pane::Notes), Intent::Nothing);
    }

    /// Delete is capital-D: a single lowercase key must never destroy a note.
    #[test]
    fn no_lowercase_key_deletes_anything() {
        for c in 'a'..='z' {
            assert_ne!(
                normal(key(c), Pane::Notes),
                Intent::DeleteSelected,
                "lowercase {c} must not delete"
            );
        }
    }

    #[test]
    fn space_scrolls_only_in_the_preview_pane() {
        assert_eq!(normal(key(' '), Pane::Preview), Intent::ScrollDown);
        assert_eq!(normal(key(' '), Pane::Notes), Intent::Nothing);
    }

    #[test]
    fn focus_moves_and_saturates_at_the_edges() {
        assert_eq!(Pane::Dirs.left(), Pane::Dirs);
        assert_eq!(Pane::Dirs.right(), Pane::Notes);
        assert_eq!(Pane::Notes.left(), Pane::Dirs);
        assert_eq!(Pane::Notes.right(), Pane::Preview);
        assert_eq!(Pane::Preview.right(), Pane::Preview);
        assert_eq!(Pane::Preview.left(), Pane::Notes);
    }

    #[test]
    fn unbound_keys_do_nothing() {
        assert_eq!(normal(key('§'), Pane::Notes), Intent::Nothing);
        assert_eq!(normal(code(KeyCode::F(5)), Pane::Notes), Intent::Nothing);
    }
}
