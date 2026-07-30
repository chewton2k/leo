//! Terminal-bound work that [`crate::action`] handlers describe but cannot do.
//!
//! An [`Effect`] names something needing the user's terminal: spawning
//! `$EDITOR`, asking for confirmation, driving the microphone. The handlers
//! stay pure so they can be tested; these performers do the impure part. Both
//! the line shell and the CLI subcommands use them, so `leo edit 1` and
//! `leo> edit 1` cannot drift apart.

use anyhow::Result;
use colored::Colorize;

use crate::action::{
    self, Ai, ConfirmedAction, EditRequest, EditTarget, Kind, Line, ListenRequest, Outcome,
};
use crate::store::Store;

/// Style one output line. The only place [`Kind`] becomes color.
pub fn render(lines: &[Line]) {
    for line in lines {
        match line.kind {
            Kind::Blank => println!(),
            Kind::Plain => println!("  {}", line.text),
            Kind::Dim => println!("  {}", line.text.dimmed()),
            Kind::Good => println!("  {}", line.text.green()),
            Kind::Warn => println!("  {}", line.text.yellow()),
            Kind::Bad => println!("  {}", line.text.red()),
            Kind::Dir => println!("    {}", line.text.cyan().bold()),
        }
    }
}

fn editor_command() -> String {
    std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vim".to_string())
}

/// Spawn `$EDITOR` on the request's temp file, then feed the result back
/// through [`action::apply_edit`].
pub fn run_editor(store: &mut Store, req: EditRequest, ai: &dyn Ai) -> Result<Outcome> {
    std::fs::write(&req.path, &req.seed)?;

    let status = std::process::Command::new(editor_command())
        .arg(&req.path)
        .status()?;

    if !status.success() {
        let _ = std::fs::remove_file(&req.path);
        return Ok(Outcome::line(Line::bad("Editor exited with an error.")));
    }

    let raw = std::fs::read_to_string(&req.path)?;
    let _ = std::fs::remove_file(&req.path);

    // Expanding prompts blocks on the model, so say so first.
    if matches!(req.target, EditTarget::Existing { .. }) {
        let count = action::parse_frontmatter(&raw)
            .2
            .lines()
            .filter(|l| action::is_leo_prompt(l).is_some())
            .count();
        if count > 0 {
            println!(
                "  {}",
                format!("Expanding {count} prompt{}...", plural(count)).cyan()
            );
        }
    }

    action::apply_edit(store, &req.target, &raw, ai)
}

/// Ask before destroying anything. `assume_yes` is how `--force` skips it.
pub fn confirm(
    store: &mut Store,
    prompt: &str,
    on_yes: ConfirmedAction,
    assume_yes: bool,
) -> Result<Outcome> {
    if !assume_yes {
        use std::io::Write;
        print!("  {} (y/n): ", prompt.bold());
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(Outcome::line(Line::dim("Cancelled.")));
        }
    }
    action::apply_confirmed(store, &on_yes)
}

/// Record, transcribe, and structure into a note.
pub fn record_and_apply(store: &mut Store, req: ListenRequest, ai: &dyn Ai) -> Result<Outcome> {
    let audio_path = crate::listen::record_audio(req.screen)?;

    println!("  {}", "Transcribing...".cyan());
    let transcript = crate::ai::transcribe(&audio_path)?;
    let _ = std::fs::remove_file(&audio_path);

    if !transcript.trim().is_empty() {
        println!("  {}", "Structuring notes...".cyan());
    }
    action::apply_transcript(store, &req, &transcript, ai)
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
