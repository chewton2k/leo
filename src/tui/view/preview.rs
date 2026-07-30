//! The preview pane: the selected note's body, or the transcript stream while
//! recording.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::line::border;
use crate::notes::Note;

/// What the pane is currently showing.
pub enum Preview<'a> {
    Empty,
    Note(&'a Note),
    /// Free text, used by the live transcription stream in a later stage.
    #[allow(dead_code)]
    Text { title: String, body: String },
    /// Handler output, keeping each line's `Kind` styling.
    Lines { title: String, lines: &'a [crate::action::Line] },
}

/// Style a body line by its markdown-ish shape, so checkboxes and headings are
/// scannable without a full markdown renderer.
fn styled(line: &str) -> TuiLine<'static> {
    let trimmed = line.trim_start();
    let style = if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
        Style::default().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
    } else if trimmed.starts_with("- [ ]") {
        Style::default()
    } else if trimmed.starts_with('#') {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    TuiLine::from(Span::styled(line.to_string(), style))
}

pub fn render(frame: &mut Frame, area: Rect, preview: &Preview<'_>, scroll: u16, focused: bool) {
    let (title, lines): (String, Vec<TuiLine>) = match preview {
        Preview::Empty => ("preview".to_string(), Vec::new()),
        Preview::Note(n) => (n.title.clone(), n.body.lines().map(styled).collect()),
        Preview::Text { title, body } => (title.clone(), body.lines().map(styled).collect()),
        Preview::Lines { title, lines } => (
            title.clone(),
            lines.iter().map(super::line::to_tui).collect(),
        ),
    };
    let line_count = lines.len();
    let scroll = clamp_scroll(scroll, line_count, area.height);

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border(focused))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

/// Clamp a scroll offset to something that still shows content.
pub fn clamp_scroll(scroll: u16, line_count: usize, viewport_height: u16) -> u16 {
    let visible = viewport_height.saturating_sub(2); // borders
    let max = (line_count as u16).saturating_sub(visible);
    scroll.min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_a_notes_title_and_body() {
        let note = Note::new("Graphs", "- BFS\n- DFS", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true))
            .unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Graphs"), "{out}");
        assert!(out.contains("BFS"), "{out}");
    }

    #[test]
    fn an_empty_preview_renders_the_placeholder_title() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Empty, 0, false))
            .unwrap();
        assert!(terminal.backend().to_string().contains("preview"));
    }

    #[test]
    fn scrolling_past_the_end_is_clamped_not_a_panic() {
        let note = Note::new("T", "line\n".repeat(3), vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 9999, true))
            .unwrap();

        assert_eq!(clamp_scroll(9999, 3, 6), 0, "3 lines fit in 4 rows");
        assert_eq!(clamp_scroll(9999, 100, 6), 96);
        assert_eq!(clamp_scroll(2, 100, 6), 2);
    }

    #[test]
    fn checked_boxes_are_struck_through_and_headings_bold() {
        assert!(styled("- [x] done").style_or_default().add_modifier.contains(Modifier::CROSSED_OUT));
        assert!(styled("## Heading").style_or_default().add_modifier.contains(Modifier::BOLD));
        assert!(!styled("- [ ] todo").style_or_default().add_modifier.contains(Modifier::CROSSED_OUT));
    }

    /// Helper: the style of a single-span line.
    trait StyleOf {
        fn style_or_default(&self) -> Style;
    }
    impl StyleOf for TuiLine<'_> {
        fn style_or_default(&self) -> Style {
            self.spans.first().map(|s| s.style).unwrap_or_default()
        }
    }
}
