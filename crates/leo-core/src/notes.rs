use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier (UUID v4)
    pub id: String,

    /// Short title shown in list view
    pub title: String,

    /// Full Markdown body
    pub body: String,

    /// Creation timestamp (UTC)
    pub created_at: DateTime<Utc>,

    /// Last-modified timestamp (UTC)
    pub updated_at: DateTime<Utc>,

    /// Directory path (empty string = root)
    #[serde(default)]
    pub directory: String,

    #[serde(default)]
    pub pinned: bool,

    #[serde(skip)]
    pub extra: serde_yaml::Mapping,
}

impl Note {
    /// Create a new note with generated id and current timestamps.
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        directory: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            body: body.into(),
            created_at: now,
            updated_at: now,
            directory: directory.into(),
            pinned: false,
            extra: serde_yaml::Mapping::new(),
        }
    }

    /// Each checkbox in the body, in order: true when ticked. The same lines
    /// [`Note::toggle_checkbox`] counts, so an index here is an index there.
    pub fn checkboxes(&self) -> Vec<bool> {
        self.body
            .lines()
            .filter_map(|l| checkbox_line(l).map(|b| b.ticked))
            .collect()
    }

    pub fn toggle_checkbox(&mut self, n: usize) -> Option<bool> {
        let mut seen = 0usize;
        let mut out = String::with_capacity(self.body.len());
        let mut result = None;
        for piece in self.body.split_inclusive('\n') {
            let line = piece.trim_end_matches(['\n', '\r']);
            let found = result.is_none();
            let hit = checkbox_line(line).filter(|_| {
                seen += 1;
                found && seen == n
            });
            match hit {
                Some(b) => {
                    let flipped = if b.ticked { ' ' } else { 'x' };
                    out.push_str(&line[..b.state]);
                    out.push(flipped);
                    out.push_str(&piece[b.state + 1..]);
                    result = Some(!b.ticked);
                }
                None => out.push_str(piece),
            }
        }
        if result.is_some() {
            self.body = out;
            self.updated_at = Utc::now();
        }
        result
    }
}

pub struct Checkbox<'a> {
    pub ticked: bool,
    pub state: usize,
    pub text: &'a str,
}

pub fn checkbox_line(line: &str) -> Option<Checkbox<'_>> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let marker = match trimmed.chars().next()? {
        '-' | '*' | '+' => 1,
        c if c.is_ascii_digit() => {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            match trimmed[digits..].chars().next() {
                Some('.' | ')') => digits + 1,
                _ => return None,
            }
        }
        c if c.is_ascii_alphabetic() => {
            let run = trimmed.chars().take_while(|x| *x == c).count();
            let rest = &trimmed[run..];
            match rest.chars().next() {
                _ if run > 2 => return None,
                Some(')') => run + 1,
                Some('.') if c.is_ascii_lowercase() || rest[1..].starts_with("  ") => run + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    let after_marker = &trimmed[marker..];
    let gap = after_marker.len() - after_marker.trim_start_matches(' ').len();
    if !(1..=4).contains(&gap) {
        return None;
    }
    let rest = after_marker[gap..].strip_prefix('[')?;
    let mut chars = rest.chars();
    let ticked = match chars.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    let after = chars.as_str().strip_prefix(']')?;
    let text = if after.is_empty() {
        ""
    } else {
        after.strip_prefix(' ')?
    };
    Some(Checkbox {
        ticked,
        state: indent + marker + gap + 1,
        text,
    })
}

pub fn question_words(question: &str) -> Vec<String> {
    const FILLER: &[&str] = &[
        "the", "and", "for", "are", "was", "were", "what", "which", "who", "whom", "when", "where",
        "why", "how", "did", "does", "do", "about", "with", "that", "this", "these", "those",
        "from", "into", "have", "has", "had", "you", "your", "our", "can", "could", "would",
        "should", "tell", "explain", "cover", "covered", "say", "said", "there", "their", "they",
        "them", "then", "than", "been", "being", "any", "all", "some", "more", "most", "also",
        "just", "not", "but", "out", "use", "used", "using", "give", "show",
    ];
    question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !FILLER.contains(w))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_list_style_of_checkbox_counts_and_toggles() {
        let mut note = Note::new(
            "T",
            "- [ ] dash\n* [ ] star\n+ [x] plus\n1. [ ] one\n2) [X] two\n  - [ ]\n-   [ ] wide\n*  [x] wide\n-[ ] not\n-     [ ] code\n- [y] not\ncode [ ] not\n",
            "",
        );
        assert_eq!(
            note.checkboxes(),
            vec![false, false, true, false, true, false, false, true]
        );
        for n in 1..=8 {
            assert!(note.toggle_checkbox(n).is_some(), "box {n}");
        }
        assert_eq!(
            note.body,
            "- [x] dash\n* [x] star\n+ [ ] plus\n1. [x] one\n2) [ ] two\n  - [x]\n-   [x] wide\n*  [ ] wide\n-[ ] not\n-     [ ] code\n- [y] not\ncode [ ] not\n"
        );
        assert!(note.toggle_checkbox(9).is_none());
    }

    #[test]
    fn lettered_checkboxes_count_like_the_page_counts_them() {
        let note = Note::new(
            "T",
            "a) [ ] one\nbb. [x] two\nA) [ ] three\nB.  [x] four\nC. [ ] name\nabc) [ ] no\nab) [ ] no\naaa. [ ] no\n",
            "",
        );
        assert_eq!(note.checkboxes(), vec![false, true, false, true]);
    }

    #[test]
    fn checkboxes_lists_each_box_in_order() {
        let note = Note::new("T", "- [ ] a\ntext\n  - [x] b\n- [X] c\n- [ ]\n", "");
        assert_eq!(note.checkboxes(), vec![false, true, true, false]);
    }
}
