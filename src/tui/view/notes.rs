//! The notes pane: the numbered list the user drives most.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::line::{border, selection};
use crate::notes::Note;

/// One row: the same 1-based number the `:` line accepts, plus a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRow {
    pub number: usize,
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
}

pub fn rows(notes: &[&Note]) -> Vec<NoteRow> {
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| NoteRow {
            number: i + 1,
            id: n.id.clone(),
            title: n.title.clone(),
            tags: n.tags.clone(),
        })
        .collect()
}

fn item(row: &NoteRow) -> ListItem<'static> {
    let mut spans = vec![
        Span::styled(
            format!("{:>3} ", row.number),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(row.title.clone()),
    ];
    if !row.tags.is_empty() {
        spans.push(Span::styled(
            format!("  [{}]", row.tags.join(", ")),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    ListItem::new(TuiLine::from(spans))
}

pub fn render(frame: &mut Frame, area: Rect, rows: &[NoteRow], selected: usize, focused: bool) {
    let title = if rows.is_empty() {
        "notes (empty)".to_string()
    } else {
        format!("notes ({})", rows.len())
    };

    let list = List::new(rows.iter().map(item).collect::<Vec<_>>())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border(focused))
                .title(title),
        )
        .highlight_style(selection(focused));

    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(selected.min(rows.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn note(title: &str, tags: &[&str]) -> Note {
        Note::new(
            title,
            "body",
            tags.iter().map(|t| t.to_string()).collect(),
            "",
        )
    }

    #[test]
    fn rows_are_numbered_from_one() {
        let a = note("First", &[]);
        let b = note("Second", &["rust"]);
        let r = rows(&[&a, &b]);
        assert_eq!(r[0].number, 1);
        assert_eq!(r[1].number, 2);
        assert_eq!(r[1].tags, vec!["rust"]);
    }

    #[test]
    fn renders_numbers_titles_and_tags() {
        let a = note("Rust ownership", &["rust", "learning"]);
        let r = rows(&[&a]);
        let mut terminal = Terminal::new(TestBackend::new(50, 5)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true)).unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Rust ownership"), "{out}");
        assert!(out.contains("rust, learning"), "{out}");
        assert!(out.contains("1"), "{out}");
        assert!(out.contains("notes (1)"), "{out}");
    }

    #[test]
    fn an_empty_list_says_so_and_does_not_panic() {
        let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
        terminal.draw(|f| render(f, f.area(), &[], 0, false)).unwrap();
        assert!(terminal.backend().to_string().contains("empty"));
    }

    #[test]
    fn a_title_longer_than_the_pane_does_not_panic() {
        let a = note(&"x".repeat(500), &[]);
        let r = rows(&[&a]);
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true)).unwrap();
    }
}
