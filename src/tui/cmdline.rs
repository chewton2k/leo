//! The `:` command line.
//!
//! A single-line input with a cursor, kill/word motions, and history. This is
//! deliberately hand-rolled rather than pulled from `tui-textarea`: that crate
//! builds against ratatui 0.29 while the rest of this TUI is on 0.30, so its
//! widgets cannot render into our `Frame`, and pulling both would mean two
//! ratatui and two crossterm versions in one binary. A one-line input is also
//! all that is needed — an in-TUI text editor is explicitly out of scope, since
//! `e` hands editing to `$EDITOR`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What handling a key in the command line produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdOutcome {
    /// Still editing.
    Editing,
    /// The user pressed Enter; run this line.
    Submit(String),
    /// The user pressed Esc; close the line.
    Cancel,
    /// The user pressed Tab; the caller should offer completions.
    Complete,
}

#[derive(Debug, Default, Clone)]
pub struct CmdLine {
    text: String,
    /// Cursor position as a character index, not a byte index, so multi-byte
    /// input cannot split a character.
    cursor: usize,
    history: Vec<String>,
    /// How far back through history the user has walked, if at all.
    history_pos: Option<usize>,
}

impl CmdLine {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Byte offset of the cursor, for slicing.
    fn byte_at(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    /// Open the line, replacing its contents with `seed`.
    pub fn open(&mut self, seed: &str) {
        self.text = seed.to_string();
        self.cursor = self.len_chars();
        self.history_pos = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_pos = None;
    }

    /// Replace the whole line, putting the cursor at the end. Used when a
    /// completion is accepted.
    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.len_chars();
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
        // Editing a recalled line makes it the user's own draft again, so Down
        // must not snap it back to the next history entry.
        self.history_pos = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.len_chars() {
            return;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    /// Delete the word before the cursor, treating runs of spaces as part of
    /// the word so Ctrl-W after a trailing space still removes something.
    fn delete_word_back(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let start = self.byte_at(i);
        let end = self.byte_at(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor = i;
    }

    fn kill_to_start(&mut self) {
        let end = self.byte_at(self.cursor);
        self.text.replace_range(..end, "");
        self.cursor = 0;
    }

    fn kill_to_end(&mut self) {
        let start = self.byte_at(self.cursor);
        self.text.truncate(start);
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_pos {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.history_pos = Some(next);
        self.set(&self.history[next].clone());
    }

    fn history_next(&mut self) {
        match self.history_pos {
            None => {}
            Some(p) if p + 1 < self.history.len() => {
                self.history_pos = Some(p + 1);
                self.set(&self.history[p + 1].clone());
            }
            // Walking past the newest entry returns to an empty line.
            Some(_) => {
                self.history_pos = None;
                self.set("");
            }
        }
    }

    fn remember(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        // Don't stack duplicate consecutive entries.
        if self.history.last().map(|l| l == line).unwrap_or(false) {
            return;
        }
        self.history.push(line.to_string());
    }

    /// Feed one key press to the line.
    pub fn key(&mut self, key: KeyEvent) -> CmdOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if ctrl {
            match key.code {
                KeyCode::Char('a') => self.cursor = 0,
                KeyCode::Char('e') => self.cursor = self.len_chars(),
                KeyCode::Char('u') => self.kill_to_start(),
                KeyCode::Char('k') => self.kill_to_end(),
                KeyCode::Char('w') => self.delete_word_back(),
                KeyCode::Char('b') => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Char('f') => self.cursor = (self.cursor + 1).min(self.len_chars()),
                // Ctrl-C abandons the line, like a shell.
                KeyCode::Char('c') => return CmdOutcome::Cancel,
                _ => {}
            }
            return CmdOutcome::Editing;
        }

        match key.code {
            // Some terminals send LF rather than CR for Return; treat both as
            // submit so the line is never left with a stray newline in it.
            KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                let line = self.text.clone();
                self.remember(&line);
                self.clear();
                CmdOutcome::Submit(line)
            }
            KeyCode::Esc => {
                self.clear();
                CmdOutcome::Cancel
            }
            KeyCode::Tab => CmdOutcome::Complete,
            KeyCode::Char(c) => {
                self.insert(c);
                CmdOutcome::Editing
            }
            KeyCode::Backspace => {
                self.backspace();
                CmdOutcome::Editing
            }
            KeyCode::Delete => {
                self.delete_forward();
                CmdOutcome::Editing
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                CmdOutcome::Editing
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.len_chars());
                CmdOutcome::Editing
            }
            KeyCode::Home => {
                self.cursor = 0;
                CmdOutcome::Editing
            }
            KeyCode::End => {
                self.cursor = self.len_chars();
                CmdOutcome::Editing
            }
            KeyCode::Up => {
                self.history_prev();
                CmdOutcome::Editing
            }
            KeyCode::Down => {
                self.history_next();
                CmdOutcome::Editing
            }
            _ => CmdOutcome::Editing,
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

    fn typed(line: &str) -> CmdLine {
        let mut c = CmdLine::default();
        for ch in line.chars() {
            c.insert(ch);
        }
        c
    }

    #[test]
    fn typing_accumulates_text_and_moves_the_cursor() {
        let c = typed("list");
        assert_eq!(c.text(), "list");
        assert_eq!(c.cursor(), 4);
    }

    #[test]
    fn opening_with_a_seed_puts_the_cursor_after_it() {
        let mut c = CmdLine::default();
        c.open("search ");
        assert_eq!(c.text(), "search ");
        assert_eq!(c.cursor(), 7);
    }

    #[test]
    fn a_bare_line_feed_submits_like_enter() {
        let mut c = typed("tags");
        assert_eq!(c.key(key('\n')), CmdOutcome::Submit("tags".to_string()));
        assert_eq!(c.text(), "");
    }

    #[test]
    fn enter_submits_and_clears() {
        let mut c = typed("list");
        assert_eq!(c.key(code(KeyCode::Enter)), CmdOutcome::Submit("list".to_string()));
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn esc_cancels_and_clears() {
        let mut c = typed("half typed");
        assert_eq!(c.key(code(KeyCode::Esc)), CmdOutcome::Cancel);
        assert_eq!(c.text(), "");
    }

    #[test]
    fn tab_asks_for_completion_without_changing_the_line() {
        let mut c = typed("vie");
        assert_eq!(c.key(code(KeyCode::Tab)), CmdOutcome::Complete);
        assert_eq!(c.text(), "vie");
    }

    #[test]
    fn backspace_at_the_start_is_a_no_op() {
        let mut c = CmdLine::default();
        c.key(code(KeyCode::Backspace));
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn editing_happens_at_the_cursor_not_the_end() {
        let mut c = typed("lst");
        c.key(code(KeyCode::Left)); // between s and t
        c.key(code(KeyCode::Left)); // between l and s
        c.insert('i');
        assert_eq!(c.text(), "list");
        assert_eq!(c.cursor(), 2);

        c.key(code(KeyCode::Backspace));
        assert_eq!(c.text(), "lst");
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut c = typed("liist");
        c.key(code(KeyCode::Home));
        c.key(code(KeyCode::Delete));
        assert_eq!(c.text(), "iist");
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn cursor_motions_saturate_at_both_ends() {
        let mut c = typed("ab");
        for _ in 0..5 {
            c.key(code(KeyCode::Left));
        }
        assert_eq!(c.cursor(), 0);
        for _ in 0..5 {
            c.key(code(KeyCode::Right));
        }
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn ctrl_a_and_ctrl_e_jump_to_the_ends() {
        let mut c = typed("mv 1 cs130");
        c.key(ctrl('a'));
        assert_eq!(c.cursor(), 0);
        c.key(ctrl('e'));
        assert_eq!(c.cursor(), 10);
    }

    #[test]
    fn ctrl_u_and_ctrl_k_kill_to_the_ends() {
        let mut c = typed("mv 1 cs130");
        c.key(ctrl('a'));
        c.key(ctrl('f'));
        c.key(ctrl('f'));
        c.key(ctrl('f')); // after "mv "
        c.key(ctrl('u'));
        assert_eq!(c.text(), "1 cs130");

        let mut c = typed("mv 1 cs130");
        c.key(ctrl('a'));
        c.key(ctrl('k'));
        assert_eq!(c.text(), "");
    }

    #[test]
    fn ctrl_w_deletes_the_previous_word_including_trailing_space() {
        let mut c = typed("mv 1 cs130");
        c.key(ctrl('w'));
        assert_eq!(c.text(), "mv 1 ");

        let mut c = typed("mv 1 ");
        c.key(ctrl('w'));
        assert_eq!(c.text(), "mv ");
    }

    #[test]
    fn ctrl_c_abandons_the_line() {
        let mut c = typed("oops");
        assert_eq!(c.key(ctrl('c')), CmdOutcome::Cancel);
    }

    /// A cursor tracked in bytes would split these characters and panic.
    #[test]
    fn multibyte_input_is_edited_by_character() {
        let mut c = typed("héllo — ✓");
        assert_eq!(c.cursor(), 9);
        c.key(code(KeyCode::Backspace));
        assert_eq!(c.text(), "héllo — ");
        c.key(code(KeyCode::Home));
        c.key(code(KeyCode::Right));
        c.insert('X');
        assert_eq!(c.text(), "hXéllo — ");
    }

    #[test]
    fn history_walks_back_and_forward() {
        let mut c = CmdLine::default();
        for line in ["list", "tags"] {
            c.open(line);
            c.key(code(KeyCode::Enter));
        }

        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "tags");
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "list");
        // Walking past the oldest entry stays there.
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "list");

        c.key(code(KeyCode::Down));
        assert_eq!(c.text(), "tags");
        // Past the newest entry, back to an empty line.
        c.key(code(KeyCode::Down));
        assert_eq!(c.text(), "");
    }

    #[test]
    fn history_ignores_blank_and_repeated_entries() {
        let mut c = CmdLine::default();
        for line in ["list", "list", "   ", "tags"] {
            c.open(line);
            c.key(code(KeyCode::Enter));
        }
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "tags");
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "list");
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "list", "the duplicate was not stored twice");
    }

    #[test]
    fn typing_after_walking_history_leaves_history_mode() {
        let mut c = CmdLine::default();
        c.open("list");
        c.key(code(KeyCode::Enter));
        c.key(code(KeyCode::Up));
        assert_eq!(c.text(), "list");
        c.insert('x');
        c.key(code(KeyCode::Down));
        // Down no longer walks history, so the edited text survives.
        assert_eq!(c.text(), "listx");
    }
}
