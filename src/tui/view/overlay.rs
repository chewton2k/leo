//! The Ctrl-P fuzzy finder: a centered overlay that filters every note in
//! every directory as the user types.
//!
//! Independent of Tab completion — this one jumps to a note rather than editing
//! a command line — but it scores with the same matcher, so `c13gr` finds
//! `cs130/lec/graphs` in both.

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::Matcher;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::help::centered;

/// One searchable note. `label` is what is matched and shown: the title plus
/// its directory, so two notes with the same title are distinguishable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// The finder's state: the query, the ranked results, and the selection.
///
/// The full pool is kept so each keystroke can re-rank from scratch rather than
/// narrowing an already-filtered list, which would make backspace unable to
/// recover candidates it had dropped.
#[derive(Debug, Default)]
pub struct Finder {
    pool: Vec<Choice>,
    query: String,
    results: Vec<Choice>,
    selected: usize,
}

impl Finder {
    /// Open with every note ranked in natural order.
    pub fn open(pool: Vec<Choice>) -> Finder {
        let mut f = Finder { pool, query: String::new(), results: Vec::new(), selected: 0 };
        f.refilter();
        f
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[Choice] {
        &self.results
    }

    pub fn selected(&self) -> Option<&Choice> {
        self.results.get(self.selected)
    }

    pub fn push(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn down(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1).min(self.results.len() - 1);
        }
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn refilter(&mut self) {
        if self.query.is_empty() {
            self.results = self.pool.clone();
        } else {
            let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
            let pattern = Pattern::parse(&self.query, CaseMatching::Ignore, Normalization::Smart);
            let labels: Vec<String> = self.pool.iter().map(|c| c.label.clone()).collect();
            let ranked = pattern.match_list(labels, &mut matcher);
            self.results = ranked
                .into_iter()
                .filter_map(|(label, _)| self.pool.iter().find(|c| c.label == label).cloned())
                .collect();
        }
        // A shrinking result list must not leave the selection past the end.
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, finder: &Finder) {
    let height = (finder.results().len() as u16 + 4).min(area.height.saturating_sub(2)).max(5);
    let box_area = centered(area, (area.width * 3 / 4).max(30), height);

    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title("find note"),
        box_area,
    );

    let inner = Rect {
        x: box_area.x + 1,
        y: box_area.y + 1,
        width: box_area.width.saturating_sub(2),
        height: box_area.height.saturating_sub(2),
    };
    let [query_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);

    frame.render_widget(
        Paragraph::new(TuiLine::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan)),
            Span::raw(finder.query().to_string()),
        ])),
        query_area,
    );

    if finder.results().is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  no matches",
                Style::default().add_modifier(Modifier::DIM),
            )),
            list_area,
        );
        return;
    }

    let items: Vec<ListItem> = finder
        .results()
        .iter()
        .map(|c| ListItem::new(c.label.clone()))
        .collect();
    let mut state = ListState::default();
    state.select(Some(finder.selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        list_area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn pool() -> Vec<Choice> {
        vec![
            Choice { id: "a".to_string(), label: "cs130/lec/graphs".to_string() },
            Choice { id: "b".to_string(), label: "Rust ownership".to_string() },
            Choice { id: "c".to_string(), label: "cs162/midterm plan".to_string() },
        ]
    }

    #[test]
    fn opening_shows_every_note() {
        let f = Finder::open(pool());
        assert_eq!(f.results().len(), 3);
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("a"));
    }

    /// The spec's example: scattered characters across a path still match.
    #[test]
    fn scattered_characters_match_a_path() {
        let mut f = Finder::open(pool());
        for c in "c13gr".chars() {
            f.push(c);
        }
        assert_eq!(f.selected().map(|c| c.label.as_str()), Some("cs130/lec/graphs"));
    }

    #[test]
    fn typing_narrows_and_backspace_widens() {
        let mut f = Finder::open(pool());
        f.push('r');
        f.push('u');
        assert_eq!(f.results().len(), 1);
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("b"));

        f.backspace();
        f.backspace();
        assert_eq!(f.results().len(), 3, "an empty query restores the full list");
    }

    #[test]
    fn a_query_matching_nothing_leaves_no_selection() {
        let mut f = Finder::open(pool());
        for c in "zzzzz".chars() {
            f.push(c);
        }
        assert!(f.results().is_empty());
        assert!(f.selected().is_none());
    }

    #[test]
    fn the_selection_stays_in_range_as_results_shrink() {
        let mut f = Finder::open(pool());
        f.down();
        f.down();
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("c"));

        // Narrow to one result; the selection must follow.
        f.push('r');
        f.push('u');
        assert_eq!(f.results().len(), 1);
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("b"));
    }

    #[test]
    fn movement_saturates_at_both_ends() {
        let mut f = Finder::open(pool());
        f.up();
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("a"));
        for _ in 0..10 {
            f.down();
        }
        assert_eq!(f.selected().map(|c| c.id.as_str()), Some("c"));
    }

    #[test]
    fn an_empty_pool_is_navigable_without_panicking() {
        let mut f = Finder::open(Vec::new());
        f.down();
        f.up();
        f.push('x');
        f.backspace();
        assert!(f.selected().is_none());
    }

    #[test]
    fn renders_the_query_and_the_matches() {
        let mut f = Finder::open(pool());
        f.push('c');
        let mut t = Terminal::new(TestBackend::new(60, 12)).unwrap();
        t.draw(|frame| render(frame, frame.area(), &f)).unwrap();

        let out = t.backend().to_string();
        assert!(out.contains("find note"), "{out}");
        assert!(out.contains("cs130/lec/graphs"), "{out}");
    }

    #[test]
    fn renders_a_no_matches_notice() {
        let mut f = Finder::open(pool());
        for c in "zzz".chars() {
            f.push(c);
        }
        let mut t = Terminal::new(TestBackend::new(50, 10)).unwrap();
        t.draw(|frame| render(frame, frame.area(), &f)).unwrap();
        assert!(t.backend().to_string().contains("no matches"));
    }

    #[test]
    fn rendering_in_a_tiny_terminal_does_not_panic() {
        let f = Finder::open(pool());
        let mut t = Terminal::new(TestBackend::new(20, 5)).unwrap();
        t.draw(|frame| render(frame, frame.area(), &f)).unwrap();
    }
}
