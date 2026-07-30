//! Pane rendering.
//!
//! Every view takes plain data rather than the `App`, so each can be rendered
//! into a `TestBackend` and asserted on without a store, a terminal, or any
//! network access.

pub mod dirs;
pub mod help;
pub mod line;
pub mod notes;
pub mod preview;
pub mod status;

use ratatui::layout::{Constraint, Layout, Rect};

/// Where each piece of the screen goes.
pub struct Frames {
    pub dirs: Rect,
    pub notes: Rect,
    pub preview: Rect,
    /// The `:` line.
    pub command: Rect,
    /// One line of transient state below the command line.
    pub status: Rect,
}

/// Split the terminal into three panes over a command line and a status line.
///
/// The dirs pane gets a fixed narrow column and the preview a proportional
/// share, so the notes list — the pane the user drives most — keeps the rest.
pub fn layout(area: Rect) -> Frames {
    let [body, command, status] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let [dirs, notes, preview] = Layout::horizontal([
        Constraint::Length(18),
        Constraint::Min(24),
        Constraint::Percentage(40),
    ])
    .areas(body);

    Frames { dirs, notes, preview, command, status }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_reserves_one_line_each_for_the_command_and_status_lines() {
        let f = layout(Rect::new(0, 0, 100, 30));
        assert_eq!(f.command.height, 1);
        assert_eq!(f.status.height, 1);
        // The status line is the last row.
        assert_eq!(f.status.y, 29);
        assert_eq!(f.command.y, 28);
    }

    #[test]
    fn the_three_panes_tile_the_body_without_gaps() {
        let f = layout(Rect::new(0, 0, 100, 30));
        assert_eq!(f.dirs.x, 0);
        assert_eq!(f.notes.x, f.dirs.x + f.dirs.width);
        assert_eq!(f.preview.x, f.notes.x + f.notes.width);
        assert_eq!(f.preview.x + f.preview.width, 100);
        for pane in [f.dirs, f.notes, f.preview] {
            assert_eq!(pane.height, 28);
        }
    }

    /// A short or narrow terminal must still produce a valid layout rather
        /// than panicking on a negative remainder.
    #[test]
    fn a_tiny_terminal_still_lays_out() {
        for (w, h) in [(20, 5), (40, 3), (10, 4), (200, 60)] {
            let f = layout(Rect::new(0, 0, w, h));
            assert!(f.command.height <= 1);
            assert!(f.dirs.width <= w);
            assert!(f.preview.x + f.preview.width <= w);
        }
    }
}
