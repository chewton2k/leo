//! The full-screen shell.
//!
//! `App` owns the store and the selection state; every key press becomes an
//! [`Intent`] or a parsed [`Action`], and the handlers in [`crate::action`] do
//! the work. Rendering reads `App` and nothing else, so the panes stay
//! independently testable.

pub mod cmdline;
pub mod keys;
pub mod task;
pub mod view;

use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::{DefaultTerminal, Frame};

use crate::action::{
    self, Action, ConfirmedAction, Ctx, Effect, Kind, Line, ListenRequest, Outcome, Parsed,
    RealAi,
};
use crate::store::Store;
use cmdline::{CmdLine, CmdOutcome};
use task::{Job, TaskEvent};
use keys::{Intent, Pane};
use view::dirs::DirRow;
use view::notes::NoteRow;
use view::preview::Preview;

/// How long a status message stays before the status line goes quiet again.
const MESSAGE_TTL: Duration = Duration::from_secs(6);
/// Event-poll timeout. Short enough that a background task's progress appears
/// promptly, long enough not to spin the CPU.
const TICK: Duration = Duration::from_millis(120);

/// Which input surface is active.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    Command,
    Help,
    Confirm { prompt: String, on_yes: ConfirmedAction },
}

pub struct App {
    store: Store,
    current_dir: String,
    /// Note IDs in pane order; index+1 is the number the `:` line accepts.
    numbering: Vec<String>,
    note_sel: usize,
    dir_sel: usize,
    focus: Pane,
    mode: Mode,
    cmd: CmdLine,
    preview_scroll: u16,
    /// The last message and when it arrived, for the status line.
    message: Option<(Kind, String, Instant)>,
    /// Output shown in the preview instead of the selected note, keeping each
    /// line's styling. Cleared by Esc, by moving the selection, or by `clear`.
    pinned: Option<(String, Vec<Line>)>,
    /// The running recording, if any.
    recording: Option<Recording>,
    quit: bool,
}

/// A recording in progress and everything it has produced so far.
struct Recording {
    job: Job,
    /// What to do with the transcript when it finishes.
    req: ListenRequest,
    /// Status-line label from the worker.
    label: String,
    /// The condensed bullet stream, which is what the preview shows: a raw
    /// transcript is not readable while you are still listening.
    condensed: String,
    /// The raw rolling transcript, behind a toggle.
    raw: String,
    show_raw: bool,
}

impl App {
    pub fn new(store: Store) -> App {
        let current_dir = String::new();
        let numbering = action::numbering_for(&store, &current_dir);
        App {
            store,
            current_dir,
            numbering,
            note_sel: 0,
            dir_sel: 0,
            focus: Pane::Notes,
            mode: Mode::Normal,
            cmd: CmdLine::default(),
            preview_scroll: 0,
            message: None,
            pinned: None,
            recording: None,
            quit: false,
        }
    }

    // ── derived view data ───────────────────────────────────────────────────

    fn dir_rows(&self) -> Vec<DirRow> {
        view::dirs::rows(&self.current_dir, &self.store.subdirs(&self.current_dir))
    }

    fn note_rows(&self) -> Vec<NoteRow> {
        let notes: Vec<&crate::notes::Note> = self
            .numbering
            .iter()
            .filter_map(|id| self.store.find_note(id))
            .collect();
        view::notes::rows(&notes)
    }

    fn selected_id(&self) -> Option<&String> {
        self.numbering.get(self.note_sel)
    }

    /// The 1-based number of the selection, as a `:` line argument.
    fn selected_ref(&self) -> Option<String> {
        self.selected_id().map(|_| (self.note_sel + 1).to_string())
    }

    fn note_count(&self) -> usize {
        self.numbering.len()
    }

    fn say(&mut self, kind: Kind, text: impl Into<String>) {
        self.message = Some((kind, text.into(), Instant::now()));
    }

    /// Refresh the numbering after the store or directory changed, keeping the
    /// selection in range.
    fn resync(&mut self) {
        self.numbering = action::numbering_for(&self.store, &self.current_dir);
        if self.note_sel >= self.numbering.len() {
            self.note_sel = self.numbering.len().saturating_sub(1);
        }
        let dirs = self.dir_rows().len();
        if self.dir_sel >= dirs {
            self.dir_sel = dirs.saturating_sub(1);
        }
    }

    // ── input ───────────────────────────────────────────────────────────────

    fn on_key(&mut self, key: event::KeyEvent, terminal: &mut DefaultTerminal) -> Result<()> {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Confirm { prompt, on_yes } => {
                let yes = matches!(key.code, event::KeyCode::Char('y' | 'Y'));
                if yes {
                    let outcome = action::apply_confirmed(&mut self.store, &on_yes)?;
                    self.absorb(outcome, terminal)?;
                } else {
                    self.say(Kind::Dim, "Cancelled.");
                }
                // Mode was already reset to Normal by the take above.
                let _ = prompt;
                Ok(())
            }

            // Any key closes help, including `?` again — it is a toggle.
            Mode::Help => {
                self.mode = Mode::Normal;
                Ok(())
            }

            Mode::Command => {
                self.mode = Mode::Command;
                match self.cmd.key(key) {
                    CmdOutcome::Editing => Ok(()),
                    CmdOutcome::Cancel => {
                        self.mode = Mode::Normal;
                        Ok(())
                    }
                    CmdOutcome::Complete => {
                        // Completion arrives in a later stage; for now Tab is inert.
                        Ok(())
                    }
                    CmdOutcome::Submit(line) => {
                        self.mode = Mode::Normal;
                        self.run_line(&line, terminal)
                    }
                }
            }

            Mode::Normal => {
                self.mode = Mode::Normal;
                // While recording, a few keys mean something else: Enter and
                // Esc stop, `t` switches between the bullets and the raw text.
                if let Some(rec) = self.recording.as_mut() {
                    match key.code {
                        event::KeyCode::Enter | event::KeyCode::Esc => {
                            // Idempotent: pressing Enter again while the worker
                            // finishes must not look like a second command.
                            if !rec.job.stop_requested() {
                                rec.job.request_stop();
                                rec.label = "Finishing".to_string();
                                self.say(Kind::Dim, "Stopping...");
                            }
                            return Ok(());
                        }
                        event::KeyCode::Char('t') => {
                            rec.show_raw = !rec.show_raw;
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                let intent = keys::normal(key, self.focus);
                self.on_intent(intent, terminal)
            }
        }
    }

    fn on_intent(&mut self, intent: Intent, terminal: &mut DefaultTerminal) -> Result<()> {
        match intent {
            Intent::Nothing => Ok(()),
            Intent::Quit => {
                self.quit = true;
                Ok(())
            }

            Intent::Down | Intent::Up | Intent::First | Intent::Last => {
                self.move_selection(intent);
                Ok(())
            }

            Intent::FocusLeft => {
                self.focus = self.focus.left();
                Ok(())
            }
            Intent::FocusRight => {
                self.focus = self.focus.right();
                Ok(())
            }

            Intent::ScrollDown => {
                self.preview_scroll = self.preview_scroll.saturating_add(5);
                Ok(())
            }
            Intent::ScrollUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(5);
                Ok(())
            }

            Intent::Open => self.open(terminal),

            Intent::ToggleCheckbox => {
                let Some(note_ref) = self.selected_ref() else {
                    return Ok(());
                };
                // Toggle the first open box, which is what `x` means with no
                // number available from a single key press.
                let index = self
                    .selected_id()
                    .and_then(|id| self.store.find_note(id))
                    .and_then(|n| first_open_checkbox(&n.body))
                    .unwrap_or(1);
                self.run_action(Action::Check { note: note_ref, index }, terminal)
            }

            Intent::EditSelected => match self.selected_ref() {
                Some(note) => self.run_action(Action::Edit { note }, terminal),
                None => Ok(()),
            },

            Intent::DeleteSelected => match self.selected_ref() {
                Some(note) => self.run_action(Action::Delete { note }, terminal),
                None => Ok(()),
            },

            Intent::OpenCommand { seed } => {
                self.cmd.open(seed);
                self.mode = Mode::Command;
                Ok(())
            }

            // The finder arrives in a later stage.
            Intent::OpenFinder => {
                self.say(Kind::Dim, "Fuzzy find is not wired up yet — use : search");
                Ok(())
            }

            Intent::ToggleHelp => {
                self.mode = Mode::Help;
                Ok(())
            }

            Intent::Cancel => {
                self.pinned = None;
                self.mode = Mode::Normal;
                Ok(())
            }

            Intent::Reload => {
                self.store = Store::load_from(&self.store.notes_dir.clone())?;
                self.resync();
                self.say(Kind::Dim, "Reloaded.");
                Ok(())
            }
        }
    }

    fn move_selection(&mut self, intent: Intent) {
        match self.focus {
            Pane::Dirs => {
                let len = self.dir_rows().len();
                self.dir_sel = step(self.dir_sel, len, intent);
            }
            Pane::Notes => {
                let len = self.note_count();
                self.note_sel = step(self.note_sel, len, intent);
                // A new note means the old scroll position is meaningless.
                self.preview_scroll = 0;
                self.pinned = None;
            }
            Pane::Preview => match intent {
                Intent::Down => self.preview_scroll = self.preview_scroll.saturating_add(1),
                Intent::Up => self.preview_scroll = self.preview_scroll.saturating_sub(1),
                Intent::First => self.preview_scroll = 0,
                Intent::Last => self.preview_scroll = u16::MAX / 2,
                _ => {}
            },
        }
    }

    /// Enter: open the selected directory, or move focus onto the body.
    fn open(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        match self.focus {
            Pane::Dirs => {
                let rows = self.dir_rows();
                let Some(row) = rows.get(self.dir_sel) else {
                    return Ok(());
                };
                let path = row.target.clone();
                self.run_action(Action::Cd { path }, terminal)
            }
            Pane::Notes => {
                self.focus = Pane::Preview;
                Ok(())
            }
            Pane::Preview => Ok(()),
        }
    }

    // ── running actions ─────────────────────────────────────────────────────

    fn run_line(&mut self, line: &str, terminal: &mut DefaultTerminal) -> Result<()> {
        match action::parse(line) {
            Parsed::Empty => Ok(()),
            Parsed::Usage(usage) => {
                self.say(Kind::Warn, format!("Usage: {usage}"));
                Ok(())
            }
            Parsed::Unknown(verb) => {
                self.say(Kind::Bad, format!("Unknown command: {verb}"));
                Ok(())
            }
            Parsed::Action(action) => self.run_action(action, terminal),
        }
    }

    fn run_action(&mut self, action: Action, terminal: &mut DefaultTerminal) -> Result<()> {
        let outcome = match action::apply(
            action,
            &mut self.store,
            Ctx { current_dir: &self.current_dir, numbering: &self.numbering },
            &RealAi,
        ) {
            Ok(o) => o,
            // A handler failure is a status-line message, never a crash.
            Err(e) => {
                self.say(Kind::Bad, e.to_string());
                return Ok(());
            }
        };
        self.absorb(outcome, terminal)
    }

    /// Apply an outcome's state changes, show its lines, and perform its effect.
    fn absorb(&mut self, outcome: Outcome, terminal: &mut DefaultTerminal) -> Result<()> {
        if let Some(dir) = outcome.new_dir {
            self.current_dir = dir;
            self.note_sel = 0;
            self.dir_sel = 0;
            self.pinned = None;
        }

        match outcome.selection {
            Some(sel) => {
                self.numbering = sel;
                self.note_sel = 0;
            }
            None if outcome.dirty => self.resync(),
            None => {}
        }

        // Multi-line output goes to the preview; a single line is a status.
        let printable: Vec<&Line> =
            outcome.lines.iter().filter(|l| l.kind != Kind::Blank).collect();
        match printable.as_slice() {
            [] => {}
            [one] => self.say(one.kind, one.text.clone()),
            many => {
                let lines = many.iter().map(|l| (*l).clone()).collect();
                self.pinned = Some(("output".to_string(), lines));
                self.preview_scroll = 0;
            }
        }

        match outcome.effect {
            Effect::None => Ok(()),

            Effect::Quit => {
                self.quit = true;
                Ok(())
            }

            Effect::ShowNote { id } => {
                // Select it in the pane if it is visible, and focus the body.
                if let Some(pos) = self.numbering.iter().position(|n| n == &id) {
                    self.note_sel = pos;
                    self.pinned = None;
                } else if let Some(note) = self.store.find_note(&id) {
                    // Not in the current directory's listing, so show it
                    // directly rather than silently doing nothing.
                    let title = note.title.clone();
                    let lines = note.body.lines().map(Line::plain).collect();
                    self.pinned = Some((title, lines));
                }
                self.preview_scroll = 0;
                self.focus = Pane::Preview;
                Ok(())
            }

            Effect::ShowHelp => {
                self.mode = Mode::Help;
                Ok(())
            }

            // There is no scrollback to clear in a full-screen UI; drop any
            // pinned output instead, which is what the user means by it.
            Effect::ClearScreen => {
                self.pinned = None;
                self.message = None;
                Ok(())
            }

            Effect::Confirm { prompt, on_yes } => {
                self.mode = Mode::Confirm { prompt, on_yes };
                Ok(())
            }

            Effect::Edit(req) => self.suspend_with_store(terminal, |store| {
                crate::shell::run_editor(store, req, &RealAi)
            }),

            Effect::Listen(req) => {
                if self.recording.is_some() {
                    self.say(Kind::Warn, "Already recording — press Enter to stop.");
                    return Ok(());
                }
                self.recording = Some(Recording {
                    job: task::start_listen(req.screen),
                    req,
                    label: "Starting".to_string(),
                    condensed: String::new(),
                    raw: String::new(),
                    show_raw: false,
                });
                self.pinned = None;
                self.say(Kind::Dim, "Recording — Enter to stop, t toggles raw text.");
                Ok(())
            }

            Effect::Sync(a) => {
                let notes_dir = self.store.notes_dir.clone();
                let out = self.outside(terminal, || {
                    use crate::action::SyncAction;
                    match &a {
                        SyncAction::Init => crate::sync::init(&notes_dir),
                        SyncAction::Connect { url } => crate::sync::connect(&notes_dir, url),
                        SyncAction::Push => crate::sync::push(&notes_dir),
                        SyncAction::Pull => crate::sync::pull(&notes_dir),
                        SyncAction::Status => crate::sync::status(&notes_dir),
                    }
                })?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                } else {
                    // Pull rewrites files underneath us.
                    self.store = Store::load_from(&self.store.notes_dir.clone())?;
                    self.resync();
                    self.say(Kind::Good, "sync done.");
                }
                Ok(())
            }

            Effect::Model(a) => {
                let out = self.outside(terminal, || crate::run_model(a.clone()))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                Ok(())
            }

            Effect::Config(a) => {
                let out = self.outside(terminal, || crate::run_config(a.clone()))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                Ok(())
            }

            Effect::Env => {
                let out = self.outside(terminal, crate::open_env_file)?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                Ok(())
            }
        }
    }

    /// Leave the alternate screen, run `f` on the real terminal, then come
    /// back. Everything that writes to stdout or reads stdin — `$EDITOR`, git,
    /// the no-echo key prompt, the recorder — goes through here.
    fn outside<T>(&mut self, terminal: &mut DefaultTerminal, f: impl FnOnce() -> T) -> Result<T> {
        suspend(terminal)?;
        let result = f();
        resume(terminal)?;
        Ok(result)
    }

    /// Run a store-mutating job outside the TUI, then absorb its outcome.
    /// `self.store` is borrowed for the call, so this cannot go through
    /// [`Self::outside`]'s closure.
    fn suspend_with_store(
        &mut self,
        terminal: &mut DefaultTerminal,
        job: impl FnOnce(&mut Store) -> Result<Outcome>,
    ) -> Result<()> {
        suspend(terminal)?;
        let result = job(&mut self.store);
        resume(terminal)?;

        match result {
            Ok(outcome) => self.absorb(outcome, terminal),
            // A failed editor or recording is a status message, not a crash.
            Err(e) => {
                self.say(Kind::Bad, e.to_string());
                Ok(())
            }
        }
    }

    /// Absorb whatever the worker has sent since the last tick. Returns true
    /// when something changed and a redraw is warranted.
    fn pump_tasks(&mut self, terminal: &mut DefaultTerminal) -> Result<bool> {
        let Some(rec) = self.recording.as_mut() else {
            return Ok(false);
        };

        let events = rec.job.drain();
        if events.is_empty() && !rec.job.is_done() {
            return Ok(false);
        }

        let mut finished: Option<String> = None;
        let mut failure: Option<String> = None;
        let mut fallbacks: Vec<String> = Vec::new();

        for event in events {
            match event {
                TaskEvent::Started { label } | TaskEvent::Progress { label } => rec.label = label,
                TaskEvent::Transcript(text) => rec.raw = text,
                TaskEvent::LiveNote(text) => rec.condensed = text,
                TaskEvent::ProviderFallback { from, to } => {
                    fallbacks.push(format!("{from} unavailable, using {to}"))
                }
                TaskEvent::Finished { transcript } => finished = Some(transcript),
                TaskEvent::Failed(e) => failure = Some(e),
            }
        }

        for f in fallbacks {
            self.say(Kind::Warn, f);
        }

        if let Some(e) = failure {
            self.recording = None;
            self.say(Kind::Bad, e);
            return Ok(true);
        }

        if let Some(transcript) = finished {
            let rec = self.recording.take().expect("checked above");
            self.say(Kind::Dim, "Structuring notes...");
            // Draw once before blocking, so the user sees why the UI paused.
            terminal.draw(|frame| self.draw(frame))?;
            let outcome =
                action::apply_transcript(&mut self.store, &rec.req, &transcript, &RealAi);
            match outcome {
                Ok(outcome) => self.absorb(outcome, terminal)?,
                Err(e) => self.say(Kind::Bad, e.to_string()),
            }
            return Ok(true);
        }

        // The worker ended without a terminal event.
        if self.recording.as_ref().map(|r| r.job.is_done()).unwrap_or(false) {
            self.recording = None;
            self.say(Kind::Warn, "Recording ended unexpectedly.");
        }
        Ok(true)
    }

    // ── rendering ───────────────────────────────────────────────────────────

    fn draw(&self, frame: &mut Frame) {
        let f = view::layout(frame.area());

        let dir_rows = self.dir_rows();
        let note_rows = self.note_rows();

        view::dirs::render(frame, f.dirs, &dir_rows, self.dir_sel, self.focus == Pane::Dirs);
        view::notes::render(
            frame,
            f.notes,
            &note_rows,
            self.note_sel,
            self.focus == Pane::Notes,
        );

        let selected_note = self.selected_id().and_then(|id| self.store.find_note(id));
        let preview = match (&self.recording, &self.pinned, selected_note) {
            // A live recording owns the preview: that stream is the reason the
            // feature exists.
            (Some(rec), _, _) => {
                let (title, body) = if rec.show_raw {
                    ("live transcript (t for notes)", rec.raw.clone())
                } else if rec.condensed.is_empty() {
                    ("live notes (t for raw text)", "  listening...".to_string())
                } else {
                    ("live notes (t for raw text)", rec.condensed.clone())
                };
                Preview::Text { title: title.to_string(), body }
            }
            (None, Some((title, lines)), _) => Preview::Lines { title: title.clone(), lines },
            (None, None, Some(note)) => Preview::Note(note),
            (None, None, None) => Preview::Empty,
        };
        view::preview::render(
            frame,
            f.preview,
            &preview,
            self.preview_scroll,
            self.focus == Pane::Preview,
        );

        view::status::render_command(
            frame,
            f.command,
            self.mode == Mode::Command,
            self.cmd.text(),
            self.cmd.cursor(),
            None,
        );
        view::status::render_status(
            frame,
            f.status,
            &self.current_dir,
            self.live_message(),
            self.recording.as_ref().map(|r| r.label.as_str()),
        );

        match &self.mode {
            Mode::Help => view::help::render_help(frame, frame.area()),
            Mode::Confirm { prompt, .. } => {
                view::help::render_confirm(frame, frame.area(), prompt)
            }
            _ => {}
        }
    }

    /// The status message, if it has not aged out.
    fn live_message(&self) -> Option<(Kind, &str)> {
        self.message.as_ref().and_then(|(kind, text, at)| {
            (at.elapsed() < MESSAGE_TTL).then_some((*kind, text.as_str()))
        })
    }
}

/// Hand the terminal back to the shell: leave raw mode and the alternate
/// screen, but keep the same `Terminal` instance. Calling `ratatui::init()`
/// again instead would stack a second panic hook and build a second terminal
/// over the live one.
fn suspend(terminal: &mut DefaultTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Take the terminal back. The screen we left is gone, so blank both of
/// ratatui's buffers to force a full repaint on the next draw.
///
/// Deliberately not `Terminal::clear()`: that snapshots the cursor first, which
/// makes `CrosstermBackend` emit a Device Status Report (`ESC[6n`) and block
/// reading the terminal's reply. Anything that does not answer — a pty harness,
/// a dumb pipe, a terminal that swallowed the query while we were suspended —
/// hangs or errors the app out on resume. Resetting the buffers needs no
/// round trip.
fn resume(terminal: &mut DefaultTerminal) -> Result<()> {
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen, Clear(ClearType::All))?;
    // Two swaps reset both buffers, so the next diff has nothing to compare
    // against and repaints every cell.
    terminal.swap_buffers();
    terminal.swap_buffers();
    terminal.hide_cursor()?;
    Ok(())
}

/// Move a list selection, saturating at both ends rather than wrapping — a
/// wrap makes `j` on the last item feel like a jump.
fn step(current: usize, len: usize, intent: Intent) -> usize {
    if len == 0 {
        return 0;
    }
    match intent {
        Intent::Down => (current + 1).min(len - 1),
        Intent::Up => current.saturating_sub(1),
        Intent::First => 0,
        Intent::Last => len - 1,
        _ => current,
    }
}

/// The 1-based index of the first unchecked checkbox, counting every checkbox.
fn first_open_checkbox(body: &str) -> Option<usize> {
    let mut n = 0;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]") {
            n += 1;
            if t.starts_with("- [ ]") {
                return Some(n);
            }
        }
    }
    None
}

/// Run the TUI. `ratatui::init` installs a panic hook that restores the
/// terminal, so a panic cannot leave the user in raw mode.
pub fn run() -> Result<()> {
    let store = Store::load()?;
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, App::new(store));
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while !app.quit {
        terminal.draw(|frame| app.draw(frame))?;

        if !event::poll(TICK)? {
            // No input: give the worker a chance to report progress.
            app.pump_tasks(terminal)?;
            continue;
        }
        match event::read()? {
            // Only key *presses*: on Windows a release would double every key.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.on_key(key, terminal)?;
            }
            _ => {}
        }
        app.pump_tasks(terminal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_saturates_instead_of_wrapping() {
        assert_eq!(step(0, 3, Intent::Up), 0);
        assert_eq!(step(2, 3, Intent::Down), 2);
        assert_eq!(step(1, 3, Intent::Down), 2);
        assert_eq!(step(1, 3, Intent::Up), 0);
        assert_eq!(step(1, 3, Intent::First), 0);
        assert_eq!(step(0, 3, Intent::Last), 2);
    }

    #[test]
    fn stepping_an_empty_list_stays_at_zero() {
        for intent in [Intent::Down, Intent::Up, Intent::First, Intent::Last] {
            assert_eq!(step(0, 0, intent), 0);
        }
    }

    #[test]
    fn the_first_open_checkbox_is_found_by_overall_position() {
        // Checked boxes still count, because `check <note> <N>` numbers them all.
        assert_eq!(first_open_checkbox("- [x] done\n- [ ] next"), Some(2));
        assert_eq!(first_open_checkbox("- [ ] first"), Some(1));
        assert_eq!(first_open_checkbox("  - [ ] indented"), Some(1));
        assert_eq!(first_open_checkbox("- [x] all\n- [X] done"), None);
        assert_eq!(first_open_checkbox("no checkboxes"), None);
        assert_eq!(first_open_checkbox(""), None);
    }
}
