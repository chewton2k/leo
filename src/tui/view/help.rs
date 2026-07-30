//! The help overlay and the confirmation prompt: two centered modals.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

/// Center a box of the given size inside `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

/// The keymap, plus the `:` verbs. Shown by `?`.
pub const KEYS: &[(&str, &str)] = &[
    ("j / k", "move down / up"),
    ("g / G", "first / last"),
    ("h / l", "switch pane"),
    ("Enter", "open directory, or focus the body"),
    ("x", "toggle the first open checkbox"),
    ("e", "edit in $EDITOR"),
    ("D", "delete (asks first)"),
    (":", "command line"),
    ("/", "search"),
    ("Tab", "complete on the : line"),
    ("Ctrl-P", "fuzzy find a note"),
    ("Ctrl-D / Ctrl-U", "scroll the preview"),
    ("Ctrl-R", "reload from disk"),
    ("?", "this help"),
    ("q", "quit"),
];

pub const VERBS_HELP: &str = "\
: new [title]        : list [#tag] [N]    : view <note>
: edit <note>        : delete <note>      : check <note> <N>
: search [-f] <q>    : tags               : remind <text>
: listen [title]     : listen add <note>  : ask <note>
: export <note> <fmt>: mkdir <name>       : cd <dir>
: mv <note>... <dir> : rmdir <name>       : pwd
: sync <init|connect|push|pull|status>
: model <list|test|login|logout> <provider>
: config <edit|path>";

pub fn render_help(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<TuiLine> = vec![TuiLine::from(Span::styled(
        "keys",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    for (key, what) in KEYS {
        lines.push(TuiLine::from(vec![
            Span::styled(format!("  {key:<16}"), Style::default().fg(Color::Cyan)),
            Span::raw(*what),
        ]));
    }
    lines.push(TuiLine::from(""));
    lines.push(TuiLine::from(Span::styled(
        "commands",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for line in VERBS_HELP.lines() {
        lines.push(TuiLine::from(Span::raw(format!("  {line}"))));
    }
    lines.push(TuiLine::from(""));
    lines.push(TuiLine::from(Span::styled(
        "  Esc or ? to close",
        Style::default().add_modifier(Modifier::DIM),
    )));

    let height = lines.len() as u16 + 2;
    let box_area = centered(area, 72, height);

    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title("help"),
            )
            .wrap(Wrap { trim: false }),
        box_area,
    );
}

/// A yes/no prompt. Destructive actions route through this rather than acting
/// on a single key press.
pub fn render_confirm(frame: &mut Frame, area: Rect, prompt: &str) {
    let box_area = centered(area, (prompt.len() as u16 + 14).min(area.width), 5);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(vec![
            TuiLine::from(""),
            TuiLine::from(Span::raw(format!("  {prompt}"))),
            TuiLine::from(Span::styled(
                "  y to confirm, anything else cancels",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title("confirm"),
        )
        .wrap(Wrap { trim: false }),
        box_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn centering_keeps_the_box_inside_the_area() {
        let area = Rect::new(0, 0, 80, 24);
        let c = centered(area, 40, 10);
        assert!(c.x + c.width <= 80);
        assert!(c.y + c.height <= 24);
        assert_eq!(c.width, 40);
        assert_eq!(c.height, 10);
    }

    /// A box larger than the terminal must shrink instead of overflowing.
    #[test]
    fn an_oversized_box_is_clamped_to_the_area() {
        let area = Rect::new(0, 0, 20, 6);
        let c = centered(area, 100, 40);
        assert_eq!(c.width, 20);
        assert_eq!(c.height, 6);
    }

    #[test]
    fn help_lists_the_documented_keys() {
        let mut t = Terminal::new(TestBackend::new(80, 40)).unwrap();
        t.draw(|f| render_help(f, f.area())).unwrap();
        let out = t.backend().to_string();
        for probe in ["switch pane", "fuzzy find", "command line", "quit"] {
            assert!(out.contains(probe), "help is missing {probe:?}:\n{out}");
        }
    }

    #[test]
    fn every_key_row_has_a_description() {
        for (key, what) in KEYS {
            assert!(!key.trim().is_empty());
            assert!(!what.trim().is_empty(), "{key} has no description");
        }
    }

    #[test]
    fn help_in_a_small_terminal_does_not_panic() {
        let mut t = Terminal::new(TestBackend::new(24, 6)).unwrap();
        t.draw(|f| render_help(f, f.area())).unwrap();
    }

    #[test]
    fn the_confirm_prompt_shows_the_question_and_the_keys() {
        let mut t = Terminal::new(TestBackend::new(60, 8)).unwrap();
        t.draw(|f| render_confirm(f, f.area(), "Delete Rust ownership?")).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("Delete Rust ownership?"), "{out}");
        assert!(out.contains("y to confirm"), "{out}");
    }
}
