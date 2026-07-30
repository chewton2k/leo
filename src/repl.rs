//! The line-oriented shell.
//!
//! This is a thin driver: it reads a line, parses it into an [`Action`], hands
//! it to the handlers in [`crate::action`], and renders the [`Outcome`]. All
//! command logic lives in `action.rs`, so the TUI can apply the same Actions
//! without duplicating any of it. What remains here is genuinely
//! terminal-bound: readline, `$EDITOR`, the confirmation prompt, recording, and
//! turning [`Kind`] into `colored` styles.

use std::io::{self, Write};

use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::action::{self, Action, Ctx, Effect, Outcome, Parsed, RealAi};
use crate::shell;
use crate::store::Store;

pub fn run() -> Result<()> {
    let mut store = Store::load()?;
    let mut rl = DefaultEditor::new()?;
    let mut current_dir = String::new(); // "" = root
    let ai = RealAi;

    let hist = history_path();
    let _ = rl.load_history(&hist);

    print_welcome(&store);

    // Pre-populate the numbering so `view 1` works before the first `list`.
    let mut numbering = action::numbering_for(&store, &current_dir);

    loop {
        let prompt = if current_dir.is_empty() {
            format!("{} ", "leo>".bold())
        } else {
            format!("{} ", format!("leo {current_dir}>").bold())
        };

        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(&line);

                let action = match action::parse(&line) {
                    Parsed::Action(a) => a,
                    Parsed::Empty => continue,
                    Parsed::Usage(usage) => {
                        println!("  Usage: {usage}");
                        continue;
                    }
                    Parsed::Unknown(verb) => {
                        println!(
                            "  Unknown command: {}. Type {} for help.",
                            verb.red(),
                            "help".bold()
                        );
                        continue;
                    }
                };

                let quit = match dispatch(action, &mut store, &mut current_dir, &mut numbering, &ai)
                {
                    Ok(quit) => quit,
                    Err(e) => {
                        eprintln!("  {}: {e}", "error".red());
                        false
                    }
                };
                if quit {
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Error: {e}");
                break;
            }
        }
    }

    if let Some(parent) = std::path::Path::new(&hist).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = rl.save_history(&hist);
    println!("{}", "Goodbye!".dimmed());
    Ok(())
}

/// Apply one action and render it. Returns true when the shell should exit.
fn dispatch(
    action: Action,
    store: &mut Store,
    current_dir: &mut String,
    numbering: &mut Vec<String>,
    ai: &RealAi,
) -> Result<bool> {
    let outcome = action::apply(
        action,
        store,
        Ctx { current_dir, numbering: numbering.as_slice() },
        ai,
    )?;
    absorb(outcome, store, current_dir, numbering, ai)
}

/// Render an outcome, apply its state changes, and perform its effect —
/// which may itself produce another outcome to absorb.
fn absorb(
    outcome: Outcome,
    store: &mut Store,
    current_dir: &mut String,
    numbering: &mut Vec<String>,
    ai: &RealAi,
) -> Result<bool> {
    shell::render(&outcome.lines);

    if let Some(dir) = outcome.new_dir {
        *current_dir = dir;
    }
    match outcome.selection {
        // A handler that produced its own numbering wins.
        Some(sel) => *numbering = sel,
        // Otherwise a mutation may have invalidated the old numbering.
        None if outcome.dirty => *numbering = action::numbering_for(store, current_dir),
        None => {}
    }

    match outcome.effect {
        Effect::None => Ok(false),
        Effect::Quit => Ok(true),

        Effect::ShowNote { id } => {
            if let Some(note) = store.find_note(&id) {
                note.print_full();
            }
            Ok(false)
        }

        Effect::ShowHelp => {
            print_help();
            Ok(false)
        }

        Effect::ClearScreen => {
            print!("\x1b[2J\x1b[H");
            io::stdout().flush().ok();
            Ok(false)
        }

        Effect::Edit(req) => {
            let next = shell::run_editor(store, req, ai)?;
            absorb(next, store, current_dir, numbering, ai)
        }

        Effect::Confirm { prompt, on_yes } => {
            let next = shell::confirm(store, &prompt, on_yes, false)?;
            absorb(next, store, current_dir, numbering, ai)
        }

        Effect::Listen(req) => {
            let next = shell::record_and_apply(store, req, ai)?;
            absorb(next, store, current_dir, numbering, ai)
        }

        Effect::Sync(a) => {
            let notes_dir = store.notes_dir.clone();
            use crate::action::SyncAction;
            match a {
                SyncAction::Init => crate::sync::init(&notes_dir)?,
                SyncAction::Connect { url } => crate::sync::connect(&notes_dir, &url)?,
                SyncAction::Push => crate::sync::push(&notes_dir)?,
                SyncAction::Status => crate::sync::status(&notes_dir)?,
                SyncAction::Pull => {
                    crate::sync::pull(&notes_dir)?;
                    // Pull rewrites files underneath us, so reload.
                    *store = Store::load_from(&notes_dir)?;
                    *numbering = action::numbering_for(store, current_dir);
                }
            }
            Ok(false)
        }

        Effect::Model(a) => {
            crate::run_model(a)?;
            Ok(false)
        }

        Effect::Config(a) => {
            crate::run_config(a)?;
            Ok(false)
        }

        Effect::Env => {
            crate::open_env_file()?;
            Ok(false)
        }
    }
}

fn history_path() -> String {
    dirs::data_dir()
        .map(|d| d.join("leo").join("history.txt"))
        .unwrap_or_else(|| std::path::PathBuf::from(".leo_history"))
        .to_string_lossy()
        .to_string()
}
// ── Welcome & help ──────────────────────────────────────────────────────────

fn print_welcome(store: &Store) {
    let count = store.notes.len();
    let word = if count == 1 { "note" } else { "notes" };
    println!();
    println!(
        "  {} {}",
        "leo".bold(),
        "— notes for programmers".dimmed()
    );
    println!(
        "  {} {} · Type {} to get started",
        count,
        word,
        "help".bold()
    );
    println!();
}

fn print_help() {
    println!();
    println!("  {}", "Commands:".bold());
    println!(
        "    {:<24} Create a new note",
        "new [title]".cyan()
    );
    println!(
        "    {:<24} List notes in current dir",
        "list [#tag] [N]".cyan()
    );
    println!(
        "    {:<24} View a note",
        "view <note>".cyan()
    );
    println!(
        "    {:<24} Edit note body in $EDITOR",
        "edit <note>".cyan()
    );
    println!(
        "    {:<24} Delete a note",
        "delete <note>".cyan()
    );
    println!(
        "    {:<24} Check/uncheck a checkbox",
        "check <note> <N>".cyan()
    );
    println!(
        "    {:<24} Search note titles",
        "search <query>".cyan()
    );
    println!(
        "    {:<24} Full-text search",
        "search -f <query>".cyan()
    );
    println!(
        "    {:<24} Show all tags",
        "tags".cyan()
    );
    println!();
    println!("  {}", "Directories:".bold());
    println!(
        "    {:<24} Create a directory",
        "mkdir <name>".cyan()
    );
    println!(
        "    {:<24} Change directory",
        "cd <dir>".cyan()
    );
    println!(
        "    {:<24} Show current directory",
        "pwd".cyan()
    );
    println!(
        "    {:<24} Move notes to a directory",
        "mv <note>... <dir>".cyan()
    );
    println!(
        "    {:<24} Remove empty directory",
        "rmdir <name>".cyan()
    );
    println!();
    println!("  {}", "AI Features:".bold());
    println!(
        "    {:<24} Add a reminder",
        "remind <text>".cyan()
    );
    println!(
        "    {:<32} Record & transcribe notes",
        "listen [--screen] [title]".cyan()
    );
    println!(
        "    {:<32} Record & add to existing note",
        "listen [--screen] add <note>".cyan()
    );
    println!(
        "    {:<24} Expand @leo prompts in a note",
        "ask <note>".cyan()
    );
    println!(
        "    {:<24} Export note to file",
        "export <note> <fmt>".cyan()
    );
    println!();
    println!("  {}", "Sync:".bold());
    println!(
        "    {:<24} Initialize git repo for notes",
        "sync init".cyan()
    );
    println!(
        "    {:<24} Connect to a GitHub remote",
        "sync connect <url>".cyan()
    );
    println!(
        "    {:<24} Push notes to remote",
        "sync push".cyan()
    );
    println!(
        "    {:<24} Pull notes from remote",
        "sync pull".cyan()
    );
    println!(
        "    {:<24} Show git status",
        "sync status".cyan()
    );
    println!();
    println!("  {}", "Other:".bold());
    println!(
        "    {:<24} Edit API keys",
        "env".cyan()
    );
    println!(
        "    {:<24} Clear the screen",
        "clear".cyan()
    );
    println!(
        "    {:<24} Show this help",
        "help".cyan()
    );
    println!(
        "    {:<24} Exit leo",
        "exit".cyan()
    );
    println!();
    println!(
        "  {} use list numbers ({}) or ID prefixes",
        "Notes:".bold(),
        "view 1".cyan()
    );
    println!(
        "  {} n, ls, v, e, rm, x, find, rem, rec, exp, h, q",
        "Shortcuts:".bold()
    );
    println!(
        "  {} \"hey leo remind me to ...\" works too!",
        "Tip:".bold()
    );
    println!(
        "  {} txt, md, html, docx, pdf, rtf, odt",
        "Export formats:".bold()
    );
    println!();
}

