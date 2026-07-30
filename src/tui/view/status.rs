//! The `:` line and the status line beneath it.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::action::Kind;

use super::line::style_for;

/// Draw the command line. When `active`, the terminal cursor is placed in it so
/// the user sees a real caret rather than a drawn one.
///
/// `ghost` is the completion hint shown ahead of the cursor; it is not part of
/// the text and is never submitted.
pub fn render_command(
    frame: &mut Frame,
    area: Rect,
    active: bool,
    text: &str,
    cursor: usize,
    ghost: Option<&str>,
) {
    if !active {
        let hint = Span::styled(
            "  :  command    ?  help    Ctrl-P  find    q  quit",
            Style::default().add_modifier(Modifier::DIM),
        );
        frame.render_widget(Paragraph::new(TuiLine::from(hint)), area);
        return;
    }

    let mut spans = vec![
        Span::styled(":", Style::default().fg(Color::Cyan)),
        Span::raw(text.to_string()),
    ];
    if let Some(ghost) = ghost {
        if !ghost.is_empty() {
            spans.push(Span::styled(
                ghost.to_string(),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
    }
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);

    // +1 for the leading ":".
    let x = area.x + 1 + cursor as u16;
    frame.set_cursor_position(Position::new(x.min(area.x + area.width.saturating_sub(1)), area.y));
}

/// One line of transient state: where we are, and what just happened.
pub fn render_status(
    frame: &mut Frame,
    area: Rect,
    dir: &str,
    message: Option<(Kind, &str)>,
    busy: Option<&str>,
) {
    let where_ = if dir.is_empty() { "/".to_string() } else { format!("/{dir}") };
    let mut spans = vec![Span::styled(
        format!(" {where_} "),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];

    if let Some(label) = busy {
        spans.push(Span::styled(
            format!("• {label} "),
            Style::default().fg(Color::Yellow),
        ));
    }

    if let Some((kind, text)) = message {
        spans.push(Span::styled(text.to_string(), style_for(kind)));
    }

    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn an_inactive_command_line_shows_the_key_hints() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), false, "", 0, None)).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("command"), "{out}");
        assert!(out.contains("quit"), "{out}");
    }

    #[test]
    fn an_active_command_line_shows_a_colon_and_the_text() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, "list", 4, None)).unwrap();
        assert!(t.backend().to_string().contains(":list"));
    }

    #[test]
    fn the_ghost_hint_follows_the_typed_text() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, "vie", 3, Some("w"))).unwrap();
        assert!(t.backend().to_string().contains(":view"), "{}", t.backend().to_string());
    }

    /// A cursor beyond the pane must be clamped, not passed through.
    #[test]
    fn a_cursor_past_the_edge_is_clamped() {
        let mut t = Terminal::new(TestBackend::new(10, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, &"x".repeat(50), 50, None))
            .unwrap();
    }

    #[test]
    fn the_status_line_shows_the_directory_and_the_last_message() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| {
            render_status(f, f.area(), "cs130/lec", Some((Kind::Good, "Created abc")), None)
        })
        .unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("/cs130/lec"), "{out}");
        assert!(out.contains("Created abc"), "{out}");
    }

    #[test]
    fn the_root_directory_shows_as_a_slash() {
        let mut t = Terminal::new(TestBackend::new(20, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "", None, None)).unwrap();
        assert!(t.backend().to_string().contains(" / "), "{}", t.backend().to_string());
    }

    #[test]
    fn a_busy_label_is_visible_alongside_the_message() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "", None, Some("Recording 00:12"))).unwrap();
        assert!(t.backend().to_string().contains("Recording 00:12"));
    }
}
