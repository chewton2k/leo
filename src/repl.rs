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
    let mut current_dir = String::new(); // "" = root

    let hist = history_path();
    let _ = rl.load_history(&hist);

    print_welcome(&store);

    // Pre-populate last_results so numeric IDs work immediately
    for note in store.list_notes_in_dir("", None, 20) {
        last_results.push(note.id.clone());
    }

    loop {
        let prompt = if current_dir.is_empty() {
            format!("{} ", "leo>".bold())
        } else {
            format!("{} ", format!("leo {}>", current_dir).bold())
        };
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

                let mut refresh = false;
                let result: Result<()> = match cmd.as_str() {
                    "new" | "n" => {
                        let r = cmd_new(&mut store, args, &current_dir);
                        refresh = true;
                        r
                    }
                    "list" | "ls" | "l" => {
                        cmd_list(&store, args, &mut last_results, &current_dir);
                        Ok(())
                    }
                    "view" | "v" => {
                        cmd_view(&store, args, &last_results);
                        Ok(())
                    }
                    "edit" | "e" => cmd_edit(&mut store, args, &last_results),
                    "delete" | "rm" | "del" | "d" => {
                        let r = cmd_delete(&mut store, args, &last_results);
                        refresh = true;
                        r
                    }
                    "check" | "uncheck" | "x" => cmd_check(&mut store, args, &last_results),
                    "search" | "find" => {
                        cmd_search(&store, args, &mut last_results);
                        Ok(())
                    }
                    "remind" | "rem" => {
                        let r = cmd_remind(&mut store, args);
                        refresh = true;
                        r
                    }
                    "listen" | "rec" => {
                        let r = cmd_listen(&mut store, args, &current_dir);
                        refresh = true;
                        r
                    }
                    "export" | "exp" => cmd_export(&store, args, &last_results),
                    "ask" | "expand" => {
                        let r = cmd_ask(&mut store, args, &last_results);
                        refresh = true;
                        r
                    }
                    "tags" => {
                        cmd_tags(&store);
                        Ok(())
                    }
                    "mkdir" => cmd_mkdir(&mut store, args, &current_dir),
                    "cd" => {
                        match resolve_cd(args, &store, &current_dir) {
                            Ok(new_dir) => {
                                current_dir = new_dir;
                                refresh = true;
                            }
                            Err(msg) => println!("  {}", msg.red()),
                        }
                        Ok(())
                    }
                    "pwd" => {
                        if current_dir.is_empty() {
                            println!("  /");
                        } else {
                            println!("  /{}", current_dir);
                        }
                        Ok(())
                    }
                    "mv" | "move" => {
                        let r = cmd_mv(&mut store, args, &last_results);
                        refresh = true;
                        r
                    }
                    "rmdir" => cmd_rmdir(&mut store, args, &current_dir),
                    "env" => {
                        if let Err(e) = crate::open_env_file() {
                            eprintln!("  {}: {e}", "error".red());
                        }
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
                    "sync" => {
                        let notes_dir = store.notes_dir.clone();
                        let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
                        match subcmd {
                            "init" => crate::sync::init(&notes_dir)?,
                            "connect" => match args.get(1) {
                                Some(url) => crate::sync::connect(&notes_dir, url)?,
                                None => println!("  Usage: sync connect <url>"),
                            },
                            "push" => crate::sync::push(&notes_dir)?,
                            "pull" => {
                                crate::sync::pull(&notes_dir)?;
                                store = Store::load_from(&notes_dir)?;
                                refresh = true;
                            }
                            "status" => crate::sync::status(&notes_dir)?,
                            _ => println!(
                                "  Usage: sync <init | connect <url> | push | pull | status>"
                            ),
                        }
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

                if refresh {
                    last_results.clear();
                    for note in store.list_notes_in_dir(&current_dir, None, 20) {
                        last_results.push(note.id.clone());
                    }
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

/// Resolve user input to a note ID.
/// Tries: list number → ID prefix → title match.
/// Prints its own error/disambiguation messages and returns None on failure.
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
    // Try ID prefix
    if store.find_note(input).is_some() {
        return Some(input.to_string());
    }
    // Try title match (case-insensitive substring)
    let matches = store.find_by_title(input);
    if matches.len() == 1 {
        return Some(matches[0].id.clone());
    }
    if matches.len() > 1 {
        println!(
            "  {}",
            format!("Multiple notes match \"{input}\":").yellow()
        );
        for note in &matches {
            let pos = last_results.iter().position(|id| id == &note.id);
            let pos_str = match pos {
                Some(p) => format!("{:>3}", p + 1),
                None => "   ".to_string(),
            };
            println!(
                "  {} {} {}",
                pos_str.yellow(),
                note.id[..8].dimmed(),
                note.title.bold(),
            );
        }
        println!(
            "  {}",
            "Use a number or ID prefix to pick one.".dimmed()
        );
        return None;
    }
    // Not found
    println!("  {}", format!("No note found: {input}").red());
    None
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

// ── Commands ────────────────────────────────────────────────────────────────

fn cmd_new(store: &mut Store, args: &[String], current_dir: &str) -> Result<()> {
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

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vim".to_string());

    let tmp_path = std::env::temp_dir().join(format!("leo-new-{}.md", uuid::Uuid::new_v4()));
    let file_content = format!(
        "---\ntitle: {}\ntags: \n---\n",
        title,
    );
    std::fs::write(&tmp_path, &file_content)?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()?;

    if !status.success() {
        println!("  {}", "Editor exited with an error.".red());
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }

    let raw = std::fs::read_to_string(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);

    let (new_title, tags, body) = parse_frontmatter(&raw);
    let title = if new_title.is_empty() { title } else { new_title };

    if body.trim().is_empty() {
        println!("  {}", "Empty note, cancelled.".dimmed());
        return Ok(());
    }

    let note = store.create_note(title, body, tags, current_dir)?;
    let short = note.id[..8].to_string();
    store.save()?;
    println!("  {} {}", "Created".green(), short.dimmed());
    Ok(())
}

fn cmd_list(store: &Store, args: &[String], last_results: &mut Vec<String>, current_dir: &str) {
    let mut tag: Option<String> = None;
    let mut limit: usize = 20;

    for arg in args {
        if let Some(t) = arg.strip_prefix('#') {
            tag = Some(t.to_string());
        } else if let Ok(n) = arg.parse::<usize>() {
            limit = n;
        }
    }

    // Show subdirectories first
    let subdirs = store.subdirs(current_dir);
    let notes = store.list_notes_in_dir(current_dir, tag.as_deref(), limit);
    last_results.clear();

    if subdirs.is_empty() && notes.is_empty() {
        if tag.is_some() {
            println!("  {}", "No notes with that tag.".dimmed());
        } else {
            println!("  No notes yet. Type {} to create one.", "new".bold());
        }
    } else {
        println!();
        for name in &subdirs {
            println!("    {}", format!("{name}/").cyan().bold());
        }
        if !subdirs.is_empty() && !notes.is_empty() {
            println!();
        }
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
    let input = args.join(" ");
    if let Some(id) = resolve_id(&input, store, last_results) {
        store.find_note(&id).unwrap().print_full();
    }
}

fn cmd_edit(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: edit <note>");
        return Ok(());
    }

    let input = args.join(" ");
    let id = match resolve_id(&input, store, last_results) {
        Some(id) => id,
        None => return Ok(()),
    };

    let (old_title, old_tags, old_body) = {
        let note = store.find_note(&id).unwrap();
        (note.title.clone(), note.tags.clone(), note.body.clone())
    };

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vim".to_string());

    let tmp_path = std::env::temp_dir().join(format!(
        "leo-{}.md",
        &id[..std::cmp::min(8, id.len())]
    ));

    let file_content = format!(
        "---\ntitle: {}\ntags: {}\n---\n{}",
        old_title,
        old_tags.join(", "),
        old_body
    );
    std::fs::write(&tmp_path, &file_content)?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()?;

    if !status.success() {
        println!("  {}", "Editor exited with an error.".red());
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }

    let raw = std::fs::read_to_string(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);
    let (parsed_title, tags, mut body) = parse_frontmatter(&raw);
    let title = if parsed_title.is_empty() { old_title.clone() } else { parsed_title };

    // Expand any @leo prompts in one pass, then save
    let leo_count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
    if leo_count > 0 {
        eprintln!(
            "  {}",
            format!(
                "Expanding {} prompt{}...",
                leo_count,
                if leo_count == 1 { "" } else { "s" }
            )
            .cyan()
        );
        let (expanded, _) = expand_leo_prompts(&body, &title)?;
        body = expanded;
    }

    if title == old_title && tags == old_tags && body.trim() == old_body.trim() {
        println!("  {}", "No changes.".dimmed());
        return Ok(());
    }

    let note = store.find_note_mut(&id).unwrap();
    note.title = title.clone();
    note.tags = tags;
    note.body = body;
    note.updated_at = chrono::Utc::now();
    store.save()?;
    println!("  {} {}", "Updated".green(), title);
    Ok(())
}

/// Parse a frontmatter block from editor content.
/// Expected format:
///   ---
///   title: My Title
///   tags: tag1, tag2
///   ---
///   Body content...
pub fn parse_frontmatter(raw: &str) -> (String, Vec<String>, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        // No frontmatter — treat entire content as body, no title/tag changes
        return (String::new(), Vec::new(), raw.to_string());
    }

    // Find the closing ---
    let after_open = &trimmed[3..].trim_start_matches(|c: char| c == '-');
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    if let Some(close_pos) = after_open.find("\n---") {
        let front = &after_open[..close_pos];
        let body_start = close_pos + 4; // skip "\n---"
        let body = after_open[body_start..]
            .strip_prefix('\n')
            .unwrap_or(&after_open[body_start..]);

        let mut title = String::new();
        let mut tags = Vec::new();

        for line in front.lines() {
            if let Some(val) = line.strip_prefix("title:") {
                title = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("tags:") {
                tags = val
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        (title, tags, body.to_string())
    } else {
        // Malformed frontmatter — return raw as body
        (String::new(), Vec::new(), raw.to_string())
    }
}

fn cmd_delete(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: delete <note>");
        return Ok(());
    }

    let input = args.join(" ");
    let id = match resolve_id(&input, store, last_results) {
        Some(id) => id,
        None => return Ok(()),
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

    // Last arg is the checkbox number, everything before is the note reference
    let note_input = args[..args.len() - 1].join(" ");
    let id = match resolve_id(&note_input, store, last_results) {
        Some(id) => id,
        None => return Ok(()),
    };

    let n: usize = match args.last().unwrap().parse() {
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
            let dir_info = if note.directory.is_empty() {
                String::new()
            } else {
                format!("  {}", format!("{}/", note.directory).dimmed())
            };
            println!(
                "  {} {}{}",
                format!("{:>3}", i + 1).yellow(),
                note.format_summary(),
                dir_info
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
    // Reminders always live at root
    if let Some(note) = store.find_by_tag_mut("reminder") {
        note.body.push('\n');
        note.body.push_str(&item);
        note.updated_at = chrono::Utc::now();
        store.save()?;
        println!("  {} {}", "Added".green(), text);
    } else {
        store.create_note("Reminders", &item, vec!["reminder".to_string()], "")?;
        store.save()?;
        println!(
            "  {} {}",
            "Created Reminders +".green(),
            text
        );
    }

    Ok(())
}

/// Strip `--screen` from raw REPL args. Returns `(screen_mode, remaining_args)`.
fn parse_screen_flag(args: &[String]) -> (bool, Vec<String>) {
    let screen = args.iter().any(|a| a == "--screen");
    let remaining = args
        .iter()
        .filter(|a| a.as_str() != "--screen")
        .cloned()
        .collect();
    (screen, remaining)
}

fn cmd_listen(store: &mut Store, args: &[String], current_dir: &str) -> Result<()> {
    // Parse --screen flag; strip it so the rest of the logic sees clean args
    let (screen, remaining) = parse_screen_flag(args);
    let args = remaining.as_slice();

    // Check for "add <note>" subcommand
    let append_to = if !args.is_empty() && args[0].eq_ignore_ascii_case("add") {
        if args.len() < 2 {
            println!("  Usage: listen add <note>");
            return Ok(());
        }
        let target = args[1..].join(" ");
        // Validate the note exists before recording
        if store.find_by_index_or_prefix(&target).is_none() {
            println!("  {} No note found: {}", "Error:".red(), target);
            return Ok(());
        }
        Some(target)
    } else {
        None
    };

    // Record audio
    let audio_path = crate::listen::record_audio(screen)?;

    // Transcribe
    println!("  {}", "Transcribing...".cyan());
    let transcript = crate::ai::transcribe(&audio_path)?;
    let _ = std::fs::remove_file(&audio_path);

    if transcript.trim().is_empty() {
        println!("  {}", "No speech detected.".dimmed());
        return Ok(());
    }

    println!("  {}", "Structuring notes...".cyan());

    if let Some(target) = append_to {
        // Append to existing note
        let existing_body = store.find_by_index_or_prefix(&target).unwrap().body.clone();
        let new_content = crate::ai::structure_notes_append(&transcript, &existing_body)?;

        let note = store.find_by_index_or_prefix_mut(&target).unwrap();
        note.body = format!("{}\n\n{}", note.body, new_content);
        note.updated_at = chrono::Utc::now();
        let title = note.title.clone();
        let short = note.id[..8].to_string();
        store.save()?;

        println!(
            "  {} \"{}\" {}",
            "Updated".green(),
            title.bold(),
            short.dimmed()
        );
    } else {
        // Create new note in current directory
        let custom_title = if args.is_empty() {
            None
        } else {
            Some(args.join(" "))
        };

        let (ai_title, body) = crate::ai::structure_notes(&transcript)?;
        let title = custom_title.unwrap_or(ai_title);

        let note = store.create_note(&title, &body, vec!["listen".to_string()], current_dir)?;
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
    }

    Ok(())
}

fn cmd_ask(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: ask <note>");
        return Ok(());
    }

    let input = args.join(" ");
    let id = match resolve_id(&input, store, last_results) {
        Some(id) => id,
        None => return Ok(()),
    };

    let (title, body) = {
        let note = store.find_note(&id).unwrap();
        (note.title.clone(), note.body.clone())
    };

    let leo_count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
    if leo_count == 0 {
        println!("  {}", "No @leo prompts found in this note.".dimmed());
        return Ok(());
    }

    eprintln!(
        "  {}",
        format!(
            "Expanding {} prompt{}...",
            leo_count,
            if leo_count == 1 { "" } else { "s" }
        )
        .cyan()
    );

    let (expanded_body, _) = expand_leo_prompts(&body, &title)?;

    let note = store.find_note_mut(&id).unwrap();
    note.body = expanded_body;
    note.updated_at = chrono::Utc::now();
    let short = note.id[..8].to_string();
    let title = note.title.clone();
    store.save()?;

    println!(
        "  {} \"{}\" {}",
        "Updated".green(),
        title.bold(),
        short.dimmed()
    );
    Ok(())
}

fn cmd_export(store: &Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.len() < 2 {
        println!("  Usage: export <note> <format>");
        println!("  Formats: txt, md, html, docx, pdf, rtf, odt");
        return Ok(());
    }

    // Last arg is the format, everything before is the note reference
    let format = args.last().unwrap().to_lowercase();
    let note_input = args[..args.len() - 1].join(" ");
    let id = match resolve_id(&note_input, store, last_results) {
        Some(id) => id,
        None => return Ok(()),
    };

    let note = store.find_note(&id).unwrap();
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

// ── Directory commands ─────────────────────────────────────────────────────

fn cmd_mkdir(store: &mut Store, args: &[String], current_dir: &str) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: mkdir <name>");
        return Ok(());
    }

    let name = args.join(" ").trim().to_string();
    if name.is_empty() {
        println!("  Usage: mkdir <name>");
        return Ok(());
    }

    // Resolve relative to current directory
    let full_path = if current_dir.is_empty() {
        name.trim_matches('/').to_string()
    } else {
        format!("{}/{}", current_dir, name.trim_matches('/'))
    };

    if store.dir_exists(&full_path) {
        println!("  {}", format!("Directory already exists: {full_path}/").dimmed());
        return Ok(());
    }

    store.create_dir(&full_path);
    store.save()?;
    println!("  {} {}", "Created".green(), format!("{full_path}/").cyan());
    Ok(())
}

fn resolve_cd(args: &[String], store: &Store, current_dir: &str) -> std::result::Result<String, String> {
    if args.is_empty() {
        // cd with no args goes to root
        return Ok(String::new());
    }

    let target = args.join(" ");
    let target = target.trim();

    if target == "/" || target == "~" {
        return Ok(String::new());
    }

    if target == ".." {
        // Go up one level
        return if current_dir.is_empty() {
            Ok(String::new()) // already at root
        } else if let Some(pos) = current_dir.rfind('/') {
            Ok(current_dir[..pos].to_string())
        } else {
            Ok(String::new()) // one level deep, go to root
        };
    }

    // Support "../sibling" style paths
    let mut base = current_dir.to_string();
    let mut remaining = target;

    while let Some(rest) = remaining.strip_prefix("../") {
        base = if let Some(pos) = base.rfind('/') {
            base[..pos].to_string()
        } else {
            String::new()
        };
        remaining = rest;
    }
    if remaining == ".." {
        base = if let Some(pos) = base.rfind('/') {
            base[..pos].to_string()
        } else {
            String::new()
        };
        remaining = "";
    }

    let full_path = if remaining.is_empty() {
        base
    } else if remaining.starts_with('/') {
        // Absolute path
        remaining.trim_matches('/').to_string()
    } else if base.is_empty() {
        remaining.trim_matches('/').to_string()
    } else {
        format!("{}/{}", base, remaining.trim_matches('/'))
    };

    if full_path.is_empty() || store.dir_exists(&full_path) {
        Ok(full_path)
    } else {
        Err(format!("No such directory: {full_path}/"))
    }
}

fn cmd_mv(store: &mut Store, args: &[String], last_results: &[String]) -> Result<()> {
    if args.len() < 2 {
        println!("  Usage: mv <note>... <directory>");
        println!("  Examples: mv 1 cs130    mv 1 2 3 cs130    mv 1 /");
        return Ok(());
    }

    // Last arg is the target directory, everything before is notes
    let target_dir = args.last().unwrap().trim_matches('/');
    if !target_dir.is_empty() && !store.dir_exists(target_dir) {
        println!("  {}", format!("No such directory: {target_dir}/").red());
        return Ok(());
    }

    let note_args = &args[..args.len() - 1];
    let mut moved = 0;

    for arg in note_args {
        let id = match resolve_id(arg, store, last_results) {
            Some(id) => id,
            None => continue,
        };

        match store.move_note(&id, target_dir) {
            Some(title) => {
                let dest = if target_dir.is_empty() { "/" } else { target_dir };
                println!("  {} \"{}\" to {}", "Moved".green(), title, dest.cyan());
                moved += 1;
            }
            None => println!("  {}", format!("Failed to move note: {arg}").red()),
        }
    }

    if moved > 0 {
        store.save()?;
    }
    Ok(())
}

fn cmd_rmdir(store: &mut Store, args: &[String], current_dir: &str) -> Result<()> {
    if args.is_empty() {
        println!("  Usage: rmdir <name>");
        return Ok(());
    }

    let name = args.join(" ").trim().to_string();
    let full_path = if current_dir.is_empty() {
        name.trim_matches('/').to_string()
    } else {
        format!("{}/{}", current_dir, name.trim_matches('/'))
    };

    if !store.dir_exists(&full_path) {
        println!("  {}", format!("No such directory: {full_path}/").red());
        return Ok(());
    }

    if store.delete_dir(&full_path) {
        store.save()?;
        println!("  {} {}", "Removed".green(), format!("{full_path}/").dimmed());
    } else {
        println!("  {}", "Directory is not empty.".red());
    }
    Ok(())
}

// ── Inline AI prompt expansion ───────────────────────────────────────────────

/// If `line` starts with `@leo <question>` (case-insensitive, leading-whitespace-tolerant),
/// returns the trimmed question text. Returns `None` for bare `@leo` or unrelated lines.
pub fn is_leo_prompt(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("@leo ") {
        let q = trimmed[5..].trim();
        if q.is_empty() { None } else { Some(q) }
    } else {
        None
    }
}

/// Process all `@leo` lines in `body`. For each one:
///   - extracts up to 5 lines of context before and after it
///   - calls `ai::expand_prompt`
///   - replaces the line with the AI expansion
///   - on AI failure, leaves the line untouched and prints a warning
///
/// Returns `(updated_body, count_of_prompts_processed)`.
pub fn expand_leo_prompts(body: &str, title: &str) -> anyhow::Result<(String, usize)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut count = 0;

    for (i, &line) in lines.iter().enumerate() {
        if let Some(question) = is_leo_prompt(line) {
            let before_start = i.saturating_sub(5);
            let before = lines[before_start..i].join("\n");
            let after_end = (i + 6).min(lines.len());
            let after = lines[(i + 1)..after_end].join("\n");
            let local_context = format!("{before}\n{after}");

            eprintln!("    {}", format!("→ {question}").dimmed());

            match crate::ai::expand_prompt(question, &local_context, title, body) {
                Ok(expansion) if !expansion.is_empty() => {
                    result.push(expansion);
                    count += 1;
                }
                Ok(_) => {
                    eprintln!("  {}", "Warning: empty AI response, keeping prompt.".yellow());
                    result.push(line.to_string());
                }
                Err(e) => {
                    eprintln!("  {}: {e}", "Warning".yellow());
                    result.push(line.to_string());
                }
            }
        } else {
            result.push(line.to_string());
        }
    }

    Ok((result.join("\n"), count))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_screen_flag_present() {
        let args = vec![
            "--screen".to_string(),
            "add".to_string(),
            "mynote".to_string(),
        ];
        let (screen, remaining) = parse_screen_flag(&args);
        assert!(screen);
        assert_eq!(remaining, vec!["add".to_string(), "mynote".to_string()]);
    }

    #[test]
    fn parse_screen_flag_absent() {
        let args = vec!["add".to_string(), "mynote".to_string()];
        let (screen, remaining) = parse_screen_flag(&args);
        assert!(!screen);
        assert_eq!(remaining, vec!["add".to_string(), "mynote".to_string()]);
    }

    #[test]
    fn parse_screen_flag_only() {
        let args = vec!["--screen".to_string()];
        let (screen, remaining) = parse_screen_flag(&args);
        assert!(screen);
        assert!(remaining.is_empty());
    }

    #[test]
    fn parse_screen_flag_mid_position() {
        let args = vec![
            "add".to_string(),
            "--screen".to_string(),
            "mynote".to_string(),
        ];
        let (screen, remaining) = parse_screen_flag(&args);
        assert!(screen);
        assert_eq!(remaining, vec!["add".to_string(), "mynote".to_string()]);
    }

    #[test]
    fn test_is_leo_prompt_basic() {
        assert_eq!(is_leo_prompt("@leo how does TLB work?"), Some("how does TLB work?"));
    }

    #[test]
    fn test_is_leo_prompt_case_insensitive() {
        assert_eq!(is_leo_prompt("@Leo expand on this"), Some("expand on this"));
        assert_eq!(is_leo_prompt("@LEO what is X?"), Some("what is X?"));
    }

    #[test]
    fn test_is_leo_prompt_bare_returns_none() {
        assert!(is_leo_prompt("@leo").is_none());
        assert!(is_leo_prompt("@leo ").is_none());
    }

    #[test]
    fn test_is_leo_prompt_non_prompt_returns_none() {
        assert!(is_leo_prompt("## Heading").is_none());
        assert!(is_leo_prompt("- bullet point").is_none());
        assert!(is_leo_prompt("").is_none());
    }

    #[test]
    fn test_is_leo_prompt_leading_whitespace() {
        assert_eq!(is_leo_prompt("  @leo what is a semaphore?"), Some("what is a semaphore?"));
    }

    #[test]
    fn test_expand_leo_prompts_no_prompts_unchanged() {
        let body = "## Virtual Memory\n\nPage tables map virtual to physical.";
        let (result, count) = expand_leo_prompts(body, "OS Notes").unwrap();
        assert_eq!(result, body);
        assert_eq!(count, 0);
    }
}
