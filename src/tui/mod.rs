//! The full-screen shell.
//!
//! `App` owns the store and the selection state; every key press becomes an
//! [`Intent`] or a parsed [`Action`], and the handlers in [`crate::action`] do
//! the work. Rendering reads `App` and nothing else, so the panes stay
//! independently testable.

pub mod cmdline;
pub mod complete;
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
use ratatui::backend::Backend;
use ratatui::{Frame, Terminal};

use crate::action::{
    self, Action, ConfirmedAction, Ctx, Effect, Kind, Line, ListenRequest, Outcome, Parsed,
    RealAi,
};
use crate::store::Store;
use cmdline::{CmdLine, CmdOutcome};
use complete::{Completion, NoteChoice, Sources};
use task::{Job, TaskEvent};
use view::overlay::{Choice, Finder};
use keys::{Intent, Pane};
use view::dirs::DirRow;
use view::notes::NoteRow;
use view::preview::Preview;

/// The backend bound every terminal-taking method needs. `Backend` alone is not
/// enough: `?` on a draw has to convert the backend's error into `anyhow::Error`,
/// which requires it to be a `Send + Sync` std error. Both `CrosstermBackend`
/// and `TestBackend` satisfy this, so the App can be driven by either — which is
/// what makes the event handling testable without a real terminal.
trait TuiBackend: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> TuiBackend for B where B: Backend, B::Error: std::error::Error + Send + Sync + 'static {}

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
    Find,
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
    /// Tab-completion state, live only while cycling.
    completing: Option<Cycle>,
    finder: Option<Finder>,
    quit: bool,
}

/// Tab cycling: the candidates for one token and how far through them the user
/// has walked. Dropped as soon as the line changes any other way, so Tab never
/// replays a stale candidate list.
struct Cycle {
    completion: Completion,
    /// What the token was before the first Tab, so cycling back is possible.
    typed: String,
    index: usize,
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
            completing: None,
            finder: None,
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

    fn on_key<B: TuiBackend>(&mut self, key: event::KeyEvent, terminal: &mut Terminal<B>) -> Result<()> {
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

            Mode::Find => {
                self.mode = Mode::Find;
                self.on_find_key(key)
            }

            Mode::Command => {
                self.mode = Mode::Command;
                let outcome = self.cmd.key(key);
                // Any key other than Tab invalidates the candidate list.
                if outcome != CmdOutcome::Complete {
                    self.completing = None;
                }
                match outcome {
                    CmdOutcome::Editing => Ok(()),
                    CmdOutcome::Cancel => {
                        self.mode = Mode::Normal;
                        Ok(())
                    }
                    CmdOutcome::Complete => {
                        self.cycle_completion();
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

    fn on_intent<B: TuiBackend>(&mut self, intent: Intent, terminal: &mut Terminal<B>) -> Result<()> {
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

            Intent::OpenFinder => {
                self.finder = Some(Finder::open(self.all_note_choices()));
                self.mode = Mode::Find;
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
    fn open<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
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

    fn run_line<B: TuiBackend>(&mut self, line: &str, terminal: &mut Terminal<B>) -> Result<()> {
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

    fn run_action<B: TuiBackend>(&mut self, action: Action, terminal: &mut Terminal<B>) -> Result<()> {
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
    fn absorb<B: TuiBackend>(&mut self, outcome: Outcome, terminal: &mut Terminal<B>) -> Result<()> {
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
    fn outside<B: TuiBackend, T>(&mut self, terminal: &mut Terminal<B>, f: impl FnOnce() -> T) -> Result<T> {
        suspend(terminal)?;
        let result = f();
        resume(terminal)?;
        Ok(result)
    }

    /// Run a store-mutating job outside the TUI, then absorb its outcome.
    /// `self.store` is borrowed for the call, so this cannot go through
    /// [`Self::outside`]'s closure.
    fn suspend_with_store<B: TuiBackend>(
        &mut self,
        terminal: &mut Terminal<B>,
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

    // ── completion ──────────────────────────────────────────────────────────

    /// Candidate sources drawn from the store and config.
    fn sources(&self) -> Sources {
        Sources {
            dirs: self.store.subdirs(&self.current_dir),
            notes: self
                .numbering
                .iter()
                .enumerate()
                .filter_map(|(i, id)| {
                    self.store
                        .find_note(id)
                        .map(|n| NoteChoice { number: i + 1, title: n.title.clone() })
                })
                .collect(),
            tags: self.store.tags().into_iter().map(|(t, _)| t).collect(),
            providers: crate::config::Config::load()
                .providers
                .keys()
                .cloned()
                .collect(),
        }
    }

    /// Tab: complete the token, or step to the next candidate if already
    /// cycling. With one match this completes and stops; with several, each Tab
    /// advances and wraps around to what was typed.
    fn cycle_completion(&mut self) {
        if let Some(cycle) = self.completing.take() {
            let count = cycle.completion.matches.len();
            if count == 0 {
                return;
            }
            // One past the end restores the original text, so cycling is
            // never a trap.
            let next = (cycle.index + 1) % (count + 1);
            let (line, cursor) = if next == count {
                complete::apply(self.cmd.text(), &cycle.completion, &cycle.typed)
            } else {
                complete::apply(
                    self.cmd.text(),
                    &cycle.completion,
                    &cycle.completion.matches[next],
                )
            };
            self.cmd.set_with_cursor(&line, cursor);
            // The span to replace moved with the new text.
            let completion = Completion {
                start: cycle.completion.start,
                end: cursor,
                matches: cycle.completion.matches,
            };
            self.completing = Some(Cycle { completion, typed: cycle.typed, index: next });
            return;
        }

        let sources = self.sources();
        let completion = complete::complete(self.cmd.text(), self.cmd.cursor(), &sources);
        if completion.matches.is_empty() {
            return;
        }
        let typed: String = self
            .cmd
            .text()
            .chars()
            .skip(completion.start)
            .take(completion.end.saturating_sub(completion.start))
            .collect();

        let (line, cursor) = complete::apply(self.cmd.text(), &completion, &completion.matches[0]);
        self.cmd.set_with_cursor(&line, cursor);
        let completion = Completion { start: completion.start, end: cursor, matches: completion.matches };
        self.completing = Some(Cycle { completion, typed, index: 0 });
    }

    /// The ghost hint: what the top candidate would add, shown ahead of the
    /// cursor. Only computed while the `:` line is open and idle.
    fn ghost(&self) -> Option<String> {
        if self.mode != Mode::Command || self.completing.is_some() {
            return None;
        }
        let text = self.cmd.text();
        if text.is_empty() {
            return None;
        }
        let completion = complete::complete(text, self.cmd.cursor(), &self.sources());
        let typed: String = text
            .chars()
            .skip(completion.start)
            .take(completion.end.saturating_sub(completion.start))
            .collect();
        completion.ghost(&typed)
    }

    // ── finder ──────────────────────────────────────────────────────────────

    /// Every note in every directory, labelled with its directory so two notes
    /// sharing a title stay distinguishable.
    fn all_note_choices(&self) -> Vec<Choice> {
        self.store
            .list_notes(None, usize::MAX)
            .iter()
            .map(|n| Choice {
                id: n.id.clone(),
                label: if n.directory.is_empty() {
                    n.title.clone()
                } else {
                    format!("{}/{}", n.directory, n.title)
                },
            })
            .collect()
    }

    fn on_find_key(&mut self, key: event::KeyEvent) -> Result<()> {
        let Some(finder) = self.finder.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };

        match key.code {
            event::KeyCode::Esc => {
                self.finder = None;
                self.mode = Mode::Normal;
            }
            event::KeyCode::Enter => {
                let chosen = finder.selected().cloned();
                self.finder = None;
                self.mode = Mode::Normal;
                if let Some(choice) = chosen {
                    self.jump_to(&choice.id);
                }
            }
            event::KeyCode::Down => finder.down(),
            event::KeyCode::Up => finder.up(),
            event::KeyCode::Backspace => finder.backspace(),
            event::KeyCode::Char(c) if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                match c {
                    'n' => finder.down(),
                    'p' => finder.up(),
                    'c' => {
                        self.finder = None;
                        self.mode = Mode::Normal;
                    }
                    _ => {}
                }
            }
            event::KeyCode::Char(c) => finder.push(c),
            _ => {}
        }
        Ok(())
    }

    /// Select a note by id, following it into its directory when it is not in
    /// the current listing — otherwise Enter in the finder would appear to do
    /// nothing for a note stored elsewhere.
    fn jump_to(&mut self, id: &str) {
        let Some(dir) = self.store.find_note(id).map(|n| n.directory.clone()) else {
            return;
        };
        if dir != self.current_dir {
            self.current_dir = dir;
            self.dir_sel = 0;
            self.numbering = action::numbering_for(&self.store, &self.current_dir);
        }
        if let Some(pos) = self.numbering.iter().position(|n| n == id) {
            self.note_sel = pos;
        }
        self.pinned = None;
        self.preview_scroll = 0;
        self.focus = Pane::Notes;
    }

    /// Absorb whatever the worker has sent since the last tick. Returns true
    /// when something changed and a redraw is warranted.
    fn pump_tasks<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<bool> {
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

        let ghost = self.ghost();
        view::status::render_command(
            frame,
            f.command,
            self.mode == Mode::Command,
            self.cmd.text(),
            self.cmd.cursor(),
            ghost.as_deref(),
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
            Mode::Find => {
                if let Some(finder) = &self.finder {
                    view::overlay::render(frame, frame.area(), finder);
                }
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
fn suspend<B: TuiBackend>(terminal: &mut Terminal<B>) -> Result<()> {
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
fn resume<B: TuiBackend>(terminal: &mut Terminal<B>) -> Result<()> {
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

fn event_loop<B: TuiBackend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
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

    fn temp_app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_dir("cs130");
        store
            .create_note("Rust ownership", "- [ ] read\n- [x] done", vec!["rust".to_string()], "")
            .unwrap();
        store.create_note("Graph traversals", "- BFS", vec![], "").unwrap();
        store.create_note("Nested note", "body", vec![], "cs130").unwrap();
        store.save().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (App::new(store), dir)
    }

    /// Notes are sorted newest-first, so find one by title rather than index.
    fn select_titled(app: &mut App, title: &str) {
        let pos = app
            .numbering
            .iter()
            .position(|id| {
                app.store.find_note(id).map(|n| n.title == title).unwrap_or(false)
            })
            .expect("note is in the current listing");
        app.note_sel = pos;
    }

    #[test]
    fn tab_completes_a_verb_on_the_command_line() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");

        app.cycle_completion();
        assert_eq!(app.cmd.text(), "view");
        assert_eq!(app.cmd.cursor(), 4);
    }

    #[test]
    fn tab_completes_a_note_reference_to_its_number() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("view owner");

        // Notes list newest-first, so derive the expected number rather than
        // assuming creation order.
        let expected = app
            .numbering
            .iter()
            .position(|id| {
                app.store.find_note(id).map(|n| n.title == "Rust ownership").unwrap_or(false)
            })
            .map(|i| i + 1)
            .unwrap();

        app.cycle_completion();
        // Only the number is a valid argument; the title was just for matching.
        assert_eq!(app.cmd.text(), format!("view {expected}"));
    }

    #[test]
    fn tab_cycles_through_candidates_and_back_to_what_was_typed() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("s");

        app.cycle_completion();
        let first = app.cmd.text().to_string();
        app.cycle_completion();
        let second = app.cmd.text().to_string();
        assert_ne!(first, second, "a second Tab must advance");

        // Walking off the end restores the original text rather than trapping
        // the user in the candidate list.
        let mut guard = 0;
        while app.cmd.text() != "s" && guard < 50 {
            app.cycle_completion();
            guard += 1;
        }
        assert_eq!(app.cmd.text(), "s");
    }

    #[test]
    fn a_keystroke_after_tab_abandons_the_candidate_list() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");
        app.cycle_completion();
        assert!(app.completing.is_some());

        // Feeding any non-Tab key through the command-mode path clears the
        // cycle, so a later Tab re-derives candidates from the new text.
        let key = event::KeyEvent::new(event::KeyCode::Char('x'), event::KeyModifiers::NONE);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        app.mode = Mode::Command;
        app.on_key(key, &mut terminal).unwrap();
        assert!(app.completing.is_none());
        assert_eq!(app.cmd.text(), "viewx");
    }

    #[test]
    fn the_ghost_hint_shows_the_rest_of_the_top_match() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");
        assert_eq!(app.ghost().as_deref(), Some("w"));

        // Not shown once cycling has started: the line already holds the match.
        app.cycle_completion();
        assert_eq!(app.ghost(), None);
    }

    #[test]
    fn no_ghost_hint_outside_the_command_line() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Normal;
        app.cmd.open("vie");
        assert_eq!(app.ghost(), None);
    }

    #[test]
    fn the_frame_renders_the_completed_command_with_its_hint() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains(":view"), "ghost hint is not rendered:\n{out}");
    }

    #[test]
    fn the_finder_lists_notes_from_every_directory() {
        let (app, _d) = temp_app();
        let choices = app.all_note_choices();
        let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Rust ownership"), "{labels:?}");
        // A note outside the current directory is labelled with its path.
        assert!(labels.contains(&"cs130/Nested note"), "{labels:?}");
    }

    #[test]
    fn jumping_to_a_note_follows_it_into_its_directory() {
        let (mut app, _d) = temp_app();
        let nested = app
            .store
            .list_notes(None, 100)
            .iter()
            .find(|n| n.title == "Nested note")
            .map(|n| n.id.clone())
            .unwrap();

        assert_eq!(app.current_dir, "");
        app.jump_to(&nested);

        assert_eq!(app.current_dir, "cs130");
        assert_eq!(app.selected_id(), Some(&nested), "the note is selected");
        assert_eq!(app.focus, Pane::Notes);
    }

    #[test]
    fn jumping_to_a_note_in_the_current_directory_only_moves_the_selection() {
        let (mut app, _d) = temp_app();
        let id = app
            .store
            .list_notes(None, 100)
            .iter()
            .find(|n| n.title == "Rust ownership")
            .map(|n| n.id.clone())
            .unwrap();
        app.jump_to(&id);
        assert_eq!(app.current_dir, "");
        assert_eq!(app.selected_id(), Some(&id));
    }

    #[test]
    fn completion_sources_come_from_the_current_directory_and_store() {
        let (app, _d) = temp_app();
        let s = app.sources();
        assert!(s.dirs.contains(&"cs130".to_string()));
        assert!(s.tags.contains(&"rust".to_string()));
        // Only notes in the current listing are numbered.
        assert_eq!(s.notes.len(), 2);
        assert!(s.notes.iter().any(|n| n.title == "Rust ownership"));
    }

    #[test]
    fn x_toggles_the_first_open_checkbox_of_the_selected_note() {
        let (mut app, _d) = temp_app();
        select_titled(&mut app, "Rust ownership");
        let id = app.selected_id().cloned().unwrap();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        app.on_intent(Intent::ToggleCheckbox, &mut terminal).unwrap();

        assert!(
            app.store.find_note(&id).unwrap().body.contains("- [x] read"),
            "body: {}",
            app.store.find_note(&id).unwrap().body
        );
    }

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
