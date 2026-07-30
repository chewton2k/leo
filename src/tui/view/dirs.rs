//! The directory pane.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::line::{border, selection};

/// One row of the pane. `..` and the current directory's children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRow {
    pub label: String,
    /// The path `cd` should receive when this row is opened.
    pub target: String,
}

/// Build the rows for a directory listing: an "up" entry when not at the root,
/// then each child.
pub fn rows(current_dir: &str, children: &[String]) -> Vec<DirRow> {
    let mut out = Vec::new();
    if !current_dir.is_empty() {
        out.push(DirRow { label: "..".to_string(), target: "..".to_string() });
    }
    for child in children {
        out.push(DirRow { label: format!("{child}/"), target: child.clone() });
    }
    out
}

pub fn render(frame: &mut Frame, area: Rect, rows: &[DirRow], selected: usize, focused: bool) {
    let items: Vec<ListItem> = rows.iter().map(|r| ListItem::new(r.label.clone())).collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border(focused))
                .title("dirs"),
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

    #[test]
    fn the_root_has_no_up_entry() {
        let r = rows("", &["cs130".to_string(), "cs162".to_string()]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].label, "cs130/");
        assert_eq!(r[0].target, "cs130");
    }

    #[test]
    fn a_subdirectory_gets_an_up_entry_first() {
        let r = rows("cs130", &["lec".to_string()]);
        assert_eq!(r[0].label, "..");
        assert_eq!(r[0].target, "..");
        assert_eq!(r[1].label, "lec/");
    }

    #[test]
    fn renders_the_directory_names() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let r = rows("cs130", &["lec".to_string()]);
        terminal
            .draw(|f| render(f, f.area(), &r, 0, true))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("dirs"), "{rendered}");
        assert!(rendered.contains(".."), "{rendered}");
        assert!(rendered.contains("lec/"), "{rendered}");
    }

    /// An out-of-range selection must clamp rather than panic.
    #[test]
    fn an_empty_or_overflowing_selection_does_not_panic() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal.draw(|f| render(f, f.area(), &[], 3, false)).unwrap();
        let r = rows("", &["a".to_string()]);
        terminal.draw(|f| render(f, f.area(), &r, 99, true)).unwrap();
    }
}
