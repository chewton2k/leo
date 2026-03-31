use std::io::{self, Write};

use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::store::Store;

pub fn run() -> Result<()> {
    let mut store = Store::load()?;
    let mut rl = DefaultEditor::new()?;
    let mut last_results: Vec<String> = Vec::new();

    let hist = history_path();
    let _ = rl.load_history(&hist);

    print_welcome(&store);

    // Pre-populate last_results so numeric IDs work immediately
    for note in store.list_notes(None, 20) {
        last_results.push(note.id.clone());
    }

    loop {
        let prompt = format!("{} ", "leo>".bold());
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(&line);

                let mut tokens = tokenize(&line);

                // Natural language prefix: "hey leo ..." or "leo ..."
                if tokens.len() >= 2
                    && tokens[0].eq_ignore_ascii_case("hey")
                    && tokens[1].eq_ignore_ascii_case("leo")
                {
                    tokens.drain(0..2);
                } else if tokens.len() >= 2
                    && tokens[0].eq_ignore_ascii_case("leo")
                {
                    tokens.drain(0..1);
                }

                if tokens.is_empty() {
                    continue;
                }

                let cmd = tokens[0].to_lowercase();
                let args = &tokens[1..];

                let result: Result<()> = match cmd.as_str() {
                    "new" | "n" => cmd_new(&mut store, args),
                    "list" | "ls" | "l" => {
                        cmd_list(&store, args, &mut last_results);
                        Ok(())
                    }
                    "view" | "v" => {
                        cmd_view(&store, args, &last_results);
                        Ok(())
                    }
                    "edit" | "e" => cmd_edit(&mut store, args, &last_results),
                    "delete" | "rm" | "del" | "d" => {
                        cmd_delete(&mut store, args, &last_results)
                    }
                    "check" | "uncheck" | "x" => cmd_check(&mut store, args, &last_results),
                    "search" | "find" => {
                        cmd_search(&store, args, &mut last_results);
                        Ok(())
                    }
                    "remind" | "rem" => cmd_remind(&mut store, args),
                    "listen" | "rec" => cmd_listen(&mut store, args),
                    "export" | "exp" => cmd_export(&store, args, &last_results),
                    "tags" => {
                        cmd_tags(&store);
                        Ok(())
                    }
                    "clear" => {
                        print!("\x1b[2J\x1b[H");
                        io::stdout().flush().ok();
                        Ok(())
                    }
                    "help" | "h" | "?" => {
                        print_help();
                        Ok(())
                    }
                    "quit" | "exit" | "q" => break,
                    _ => {
                        println!(
                            "  Unknown command: {}. Type {} for help.",
                            cmd.red(),
                            "help".bold()
                        );
                        Ok(())
                    }
                };

                if let Err(e) = result {
                    eprintln!("  {}: {e}", "error".red());
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

// ── ID resolution ───────────────────────────────────────────────────────────

/// Resolve user input to a note ID. Tries list number first, then ID prefix.
fn resolve_id(input: &str, store: &Store, last_results: &[String]) -> Option<String> {
    // Try as 1-based index into last list/search results
    if let Ok(n) = input.parse::<usize>() {
        if n >= 1 && n <= last_results.len() {
            let id = &last_results[n - 1];
            if store.find_note(id).is_some() {
                return Some(id.clone());
            }
        }
    }
    // Fall back to ID prefix
    if store.find_note(input).is_some() {
        Some(input.to_string())
    } else {
        None
    }
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
        "    {:<24} List recent notes",
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
    println!("  {}", "AI Features:".bold());
    println!(
        "    {:<24} Add a reminder",
        "remind <text>".cyan()
    );
    println!(
        "    {:<24} Record & transcribe notes",
        "listen [title]".cyan()
    );
    println!(
        "    {:<24} Export note to file",
        "export <note> <fmt>".cyan()
    );
    println!();
    println!("  {}", "Other:".bold());
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

// ── Commands ────────────────────────────────────────────────────────────────

fn cmd_new(store: &mut Store, args: &[String]) -> Result<()> {
    let title = if args.is_empty() {
        print!("  {}: ", "Title".bold());
        io::stdout().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        let t = buf.trim().to_string();
        if t.is_empty() {
            println!("  {}", "Cancelled.".dimmed());
            return Ok(());
        }
        t
    } else {
        args.join(" ")
    };

    print!("  {}: ", "Tags (comma-separated, enter to skip)".dimmed());
    io::stdout().flush()?;
    let mut tags_buf = String::new();
    io::stdin().read_line(&mut tags_buf)?;
    let tags: Vec<String> = tags_buf
        .trim()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    println!(
        "  {} {}",
        "Body".dimmed(),
        "(empty line to finish):".dimmed()
    );
    let mut body_lines = Vec::new();
    loop {
        print!("  {} ", "|".dimmed());
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        body_lines.push(line.trim_end().to_string());
    }
    let body = body_lines.join("\n");

    let note = store.create_note(title, body, tags)?;
    let short = note.id[..8].to_string();
    store.save()?;
    println!("  {} {}", "Created".green(), short.dimmed());
    Ok(())
}

fn cmd_list(store: &Store, args: &[String], last_results: &mut Vec<String>) {
    let mut tag: Option<String> = None;
    let mut limit: usize = 20;

    for arg in args {
        if let Some(t) = arg.strip_prefix('#') {
            tag = Some(t.to_string());
        } else if let Ok(n) = arg.parse::<usize>() {
            limit = n;
        }
    }

    let notes = store.list_notes(tag.as_deref(), limit);
    last_results.clear();

    if notes.is_empty() {
        if tag.is_some() {
            println!("  {}", "No notes with that tag.".dimmed());
        } else {
            println!("  No notes yet. Type {} to create one.", "new".bold());
        }
    } else {
        println!();
        for (i, note) in notes.iter().enumerate() {
            last_results.push(note.id.clone());
            println!(
                "  {} {}",
                format!("{:>3}", i + 1).yellow(),
                note.format_summary()
            );
        }
        println!();
    }
}

fn cmd_view(store: &Store, args: &[String], last_results: &[String]) {
    if args.is_empty() {
        println!("  Usage: view <note>");
        return;
    }
    match resolve_id(&args[0], store, last_results) {
        Some(id) => store.find_note(&id).unwrap().print_full(),
        None => println!("  {}", format!("No note found: {}", args[0]).red()),
    }
}

fn cmd_edit(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: edit <note>");
        return Ok(());
    }

    let id = match resolve_id(&args[0], store, last_results) {
        Some(id) => id,
        None => {
            println!("  {}", format!("No note found: {}", args[0]).red());
            return Ok(());
        }
    };

    let (title, current_body) = {
        let note = store.find_note(&id).unwrap();
        (note.title.clone(), note.body.clone())
    };

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vim".to_string());

    let tmp_path = std::env::temp_dir().join(format!(
        "leo-{}.md",
        &id[..std::cmp::min(8, id.len())]
    ));
    std::fs::write(&tmp_path, &current_body)?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()?;

    if !status.success() {
        println!("  {}", "Editor exited with an error.".red());
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }

    let new_body = std::fs::read_to_string(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);

    if new_body.trim() == current_body.trim() {
        println!("  {}", "No changes.".dimmed());
        return Ok(());
    }

    if store.update_body(&id, new_body) {
        store.save()?;
        println!("  {} {}", "Updated".green(), title);
    }
    Ok(())
}

fn cmd_delete(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: delete <note>");
        return Ok(());
    }

    let id = match resolve_id(&args[0], store, last_results) {
        Some(id) => id,
        None => {
            println!("  {}", format!("No note found: {}", args[0]).red());
            return Ok(());
        }
    };

    let title = store.find_note(&id).unwrap().title.clone();

    print!("  Delete {}? (y/n): ", title.bold());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if !input.trim().eq_ignore_ascii_case("y") {
        println!("  {}", "Cancelled.".dimmed());
        return Ok(());
    }

    if store.delete_note(&id) {
        store.save()?;
        println!("  {}", "Deleted.".green());
    }
    Ok(())
}

fn cmd_check(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.len() < 2 {
        println!("  Usage: check <note> <checkbox number>");
        return Ok(());
    }

    let id = match resolve_id(&args[0], store, last_results) {
        Some(id) => id,
        None => {
            println!("  {}", format!("No note found: {}", args[0]).red());
            return Ok(());
        }
    };

    let n: usize = match args[1].parse() {
        Ok(n) if n >= 1 => n,
        _ => {
            println!("  {}", "Checkbox number must be a positive integer.".red());
            return Ok(());
        }
    };

    match store.toggle_checkbox(&id, n) {
        Some(state) => {
            store.save()?;
            println!("  {state}");
        }
        None => {
            println!("  {}", format!("No checkbox #{n} in that note.").red());
        }
    }
    Ok(())
}

fn cmd_search(store: &Store, args: &[String], last_results: &mut Vec<String>) {
    if args.is_empty() {
        println!("  Usage: search <query>");
        return;
    }

    let full_text = args.first().map(|s| s == "-f").unwrap_or(false);
    let query_args = if full_text { &args[1..] } else { args };
    let query = query_args.join(" ");

    if query.is_empty() {
        println!("  Usage: search [-f] <query>");
        return;
    }

    let results = store.search(&query, full_text);
    last_results.clear();

    if results.is_empty() {
        println!("  {}", format!("No notes match '{query}'.").dimmed());
    } else {
        println!();
        for (i, note) in results.iter().enumerate() {
            last_results.push(note.id.clone());
            println!(
                "  {} {}",
                format!("{:>3}", i + 1).yellow(),
                note.format_summary()
            );
        }
        println!();
    }
}

fn cmd_remind(store: &mut Store, args: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: remind <what to remember>");
        println!("  Example: remind me to buy groceries");
        return Ok(());
    }

    // Normalize: strip "me to " or "me " prefix for natural language
    let text = args.join(" ");
    let text = text
        .strip_prefix("me to ")
        .or_else(|| text.strip_prefix("me "))
        .unwrap_or(&text)
        .trim();

    let item = format!("- [ ] {text}");

    // Look for existing reminder note, append or create
    if let Some(note) = store.find_by_tag_mut("reminder") {
        note.body.push('\n');
        note.body.push_str(&item);
        note.updated_at = chrono::Utc::now();
        store.save()?;
        println!("  {} {}", "Added".green(), text);
    } else {
        store.create_note("Reminders", &item, vec!["reminder".to_string()])?;
        store.save()?;
        println!(
            "  {} {}",
            "Created Reminders +".green(),
            text
        );
    }

    Ok(())
}

fn cmd_listen(store: &mut Store, args: &[String]) -> Result<()> {
    // Record audio
    let audio_path = crate::listen::record_audio()?;

    // Transcribe
    println!("  {}", "Transcribing...".cyan());
    let transcript = crate::ai::transcribe(&audio_path)?;
    let _ = std::fs::remove_file(&audio_path);

    if transcript.trim().is_empty() {
        println!("  {}", "No speech detected.".dimmed());
        return Ok(());
    }

    // Determine title and body
    let custom_title = if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    };

    println!("  {}", "Structuring notes...".cyan());
    let (ai_title, body) = crate::ai::structure_notes(&transcript)?;
    let title = custom_title.unwrap_or(ai_title);

    let note = store.create_note(&title, &body, vec!["listen".to_string()])?;
    let short = note.id[..8].to_string();
    store.save()?;

    println!(
        "  {} \"{}\" {}",
        "Created".green(),
        title.bold(),
        short.dimmed()
    );
    println!(
        "  {}",
        "Use 'export <note> <format>' to export this note.".dimmed()
    );

    Ok(())
}

fn cmd_export(store: &Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.len() < 2 {
        println!("  Usage: export <note> <format>");
        println!("  Formats: txt, md, html, docx, pdf, rtf, odt");
        return Ok(());
    }

    let id = match resolve_id(&args[0], store, last_results) {
        Some(id) => id,
        None => {
            println!("  {}", format!("No note found: {}", args[0]).red());
            return Ok(());
        }
    };

    let note = store.find_note(&id).unwrap();
    let format = args[1].to_lowercase();
    let format = format.trim_start_matches('.');

    // Export to Desktop > Home > current dir
    let output_dir = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let path = crate::export::export_note(note, format, &output_dir)?;
    println!("  {} {}", "Exported".green(), path.display());

    Ok(())
}

fn cmd_tags(store: &Store) {
    let tags = store.tags();
    if tags.is_empty() {
        println!("  {}", "No tags yet.".dimmed());
    } else {
        println!();
        for (tag, count) in &tags {
            println!(
                "  {} {}",
                format!("#{tag}").cyan(),
                format!("({count})").dimmed()
            );
        }
        println!();
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Split input on whitespace, keeping quoted strings together.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for ch in input.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '"' | '\'' => {
                    in_quotes = true;
                    quote_char = ch;
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                }
                _ => current.push(ch),
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn history_path() -> String {
    dirs::data_dir()
        .map(|d| d.join("leo").join("history.txt"))
        .unwrap_or_else(|| std::path::PathBuf::from(".leo_history"))
        .to_string_lossy()
        .to_string()
}
