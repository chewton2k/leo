use chrono::{DateTime, Utc};
use colored::Colorize;
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

    /// Free-form tags for organisation
    pub tags: Vec<String>,
}

impl Note {
    /// Create a new note with generated id and current timestamps.
    pub fn new(title: impl Into<String>, body: impl Into<String>, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            body: body.into(),
            created_at: now,
            updated_at: now,
            tags,
        }
    }

    /// One-line summary as a formatted String.
    pub fn format_summary(&self) -> String {
        let date = self.updated_at.format("%Y-%m-%d %H:%M");
        let id_short = &self.id[..8];
        let tags = if self.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", self.tags.join(", ").dimmed())
        };
        format!(
            "{} {} {}{}",
            id_short.dimmed(),
            date.to_string().cyan(),
            self.title.bold(),
            tags,
        )
    }

    /// Print compact one-line summary (used by CLI subcommands).
    pub fn print_summary(&self) {
        println!("{}", self.format_summary());
    }

    /// Full view including body with rendered checkboxes and lists.
    pub fn print_full(&self) {
        let created = self.created_at.format("%Y-%m-%d %H:%M UTC");
        let updated = self.updated_at.format("%Y-%m-%d %H:%M UTC");

        println!("{}", "─".repeat(60).dimmed());
        println!("{} {}", "Title:".bold(), self.title);
        println!("{} {}", "ID:   ".bold(), self.id.dimmed());
        println!("{} {}", "Tags: ".bold(), self.tags.join(", "));
        println!("{} {}", "Created:".bold(), created.to_string().dimmed());
        println!("{} {}", "Updated:".bold(), updated.to_string().dimmed());
        println!("{}", "─".repeat(60).dimmed());
        println!();
        println!("{}", self.render_body());
        println!();
    }

    /// Render the body with pretty checkboxes and bullet lists.
    /// Checkboxes are numbered so they can be toggled with `check`.
    pub fn render_body(&self) -> String {
        let mut checkbox_num = 0u32;
        self.body
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if let Some(rest) = trimmed
                    .strip_prefix("- [x] ")
                    .or_else(|| trimmed.strip_prefix("- [X] "))
                {
                    checkbox_num += 1;
                    format!(
                        "  {} {} {}",
                        format!("[{checkbox_num}]").dimmed(),
                        "☑".green(),
                        rest.dimmed(),
                    )
                } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                    checkbox_num += 1;
                    format!(
                        "  {} {} {}",
                        format!("[{checkbox_num}]").dimmed(),
                        "☐".white(),
                        rest,
                    )
                } else if let Some(rest) = trimmed.strip_prefix("- ") {
                    format!("  • {rest}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Toggle the Nth checkbox (1-based). Returns the new state text, or None
    /// if no such checkbox exists.
    pub fn toggle_checkbox(&mut self, n: usize) -> Option<String> {
        let mut checkbox_num = 0usize;
        let lines: Vec<String> = self.body.lines().map(|l| l.to_string()).collect();
        let mut new_lines = lines.clone();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let is_checked =
                trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ");
            let is_unchecked = trimmed.starts_with("- [ ] ");

            if is_checked || is_unchecked {
                checkbox_num += 1;
                if checkbox_num == n {
                    let indent = &line[..line.len() - trimmed.len()];
                    let rest = &trimmed[6..];
                    if is_unchecked {
                        new_lines[i] = format!("{indent}- [x] {rest}");
                        self.body = new_lines.join("\n");
                        self.updated_at = Utc::now();
                        return Some(format!("{} {rest}", "☑".green()));
                    } else {
                        new_lines[i] = format!("{indent}- [ ] {rest}");
                        self.body = new_lines.join("\n");
                        self.updated_at = Utc::now();
                        return Some(format!("{} {rest}", "☐".white()));
                    }
                }
            }
        }
        None
    }

    /// Returns true if the query matches the title (case-insensitive).
    pub fn matches_title(&self, query: &str) -> bool {
        self.title.to_lowercase().contains(&query.to_lowercase())
    }

    /// Returns true if the query matches the title or body (case-insensitive).
    pub fn matches_full_text(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.title.to_lowercase().contains(&q) || self.body.to_lowercase().contains(&q)
    }
}
