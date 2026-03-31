mod ai;
mod export;
mod listen;
mod notes;
mod repl;
mod store;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// leo — notes for programmers.
/// Run with no arguments to enter the interactive terminal.
#[derive(Parser)]
#[command(name = "leo", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new note
    New {
        /// Title of the note
        title: String,

        /// Body text
        #[arg(short, long, allow_hyphen_values = true)]
        body: Option<String>,

        /// Tags, comma-separated (e.g. rust,cli)
        #[arg(short, long, value_delimiter = ',')]
        tags: Vec<String>,
    },

    /// List all notes (newest first)
    List {
        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Maximum number of notes to show
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// View the full content of a note
    View {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Edit an existing note in $EDITOR
    Edit {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Delete a note
    Delete {
        /// Note ID (or unique prefix)
        id: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Search notes by title or body content
    Search {
        /// Search query
        query: String,

        /// Also search inside note bodies
        #[arg(short, long)]
        full_text: bool,
    },

    /// Add a reminder (creates or appends to a Reminders note)
    Remind {
        /// What to remember
        text: Vec<String>,
    },

    /// Record audio and create structured notes from speech
    Listen {
        /// Optional title (AI generates one if omitted)
        #[arg(short, long)]
        title: Option<String>,
    },

    /// Export a note to a file (txt, md, html, docx, pdf, rtf, odt)
    Export {
        /// Note ID (or unique prefix)
        id: String,

        /// Output format
        format: String,
    },
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        None => {
            if std::io::stdin().is_terminal() {
                repl::run()
            } else {
                eprintln!("leo: interactive mode requires a terminal. Use subcommands for scripting.");
                std::process::exit(1);
            }
        }
        Some(cmd) => run_command(cmd),
    }
}

fn run_command(cmd: Commands) -> Result<()> {
    let mut store = store::Store::load()?;

    match cmd {
        Commands::New { title, body, tags } => {
            let body = body.unwrap_or_default();
            let note = store.create_note(title, body, tags)?;
            println!("Created note {}", &note.id[..8]);
        }

        Commands::List { tag, limit } => {
            let notes = store.list_notes(tag.as_deref(), limit);
            if notes.is_empty() {
                println!("No notes yet. Run `leo` to get started.");
            } else {
                for note in notes {
                    note.print_summary();
                }
            }
        }

        Commands::View { id } => match store.find_by_index_or_prefix(&id) {
            Some(note) => note.print_full(),
            None => eprintln!("No note found: {id}"),
        },

        Commands::Edit { id } => {
            let (current_body, title, resolved_id) = match store.find_by_index_or_prefix(&id) {
                Some(n) => (n.body.clone(), n.title.clone(), n.id.clone()),
                None => {
                    eprintln!("No note found: {id}");
                    return Ok(());
                }
            };
            let id = resolved_id;

            let editor = std::env::var("EDITOR")
                .or_else(|_| std::env::var("VISUAL"))
                .unwrap_or_else(|_| "vim".to_string());

            let tmp = std::env::temp_dir().join(format!("leo-{}.md", &id));
            std::fs::write(&tmp, &current_body)?;

            let status = std::process::Command::new(&editor).arg(&tmp).status()?;

            if status.success() {
                let new_body = std::fs::read_to_string(&tmp)?;
                let _ = std::fs::remove_file(&tmp);
                if new_body.trim() != current_body.trim() && store.update_body(&id, new_body) {
                    println!("Updated {title}.");
                } else {
                    println!("No changes.");
                }
            } else {
                let _ = std::fs::remove_file(&tmp);
                eprintln!("Editor exited with error.");
            }
        }

        Commands::Delete { id, force } => {
            let resolved_id = match store.find_by_index_or_prefix(&id) {
                Some(n) => n.id.clone(),
                None => {
                    eprintln!("No note found: {id}");
                    return Ok(());
                }
            };
            let id = resolved_id;
            if !force {
                if let Some(note) = store.find_note(&id) {
                    print!("Delete \"{}\"? [y/N] ", note.title);
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            if store.delete_note(&id) {
                println!("Deleted.");
            } else {
                eprintln!("No note found: {id}");
            }
        }

        Commands::Search { query, full_text } => {
            let results = store.search(&query, full_text);
            if results.is_empty() {
                println!("No notes match '{query}'.");
            } else {
                for note in results {
                    note.print_summary();
                }
            }
        }

        Commands::Remind { text } => {
            let text = text.join(" ");
            let text = text
                .strip_prefix("me to ")
                .or_else(|| text.strip_prefix("me "))
                .unwrap_or(&text)
                .trim();
            let item = format!("- [ ] {text}");

            if let Some(note) = store.find_by_tag_mut("reminder") {
                note.body.push('\n');
                note.body.push_str(&item);
                note.updated_at = chrono::Utc::now();
                println!("Added: {text}");
            } else {
                store.create_note("Reminders", &item, vec!["reminder".to_string()])?;
                println!("Created Reminders + {text}");
            }
        }

        Commands::Listen { title } => {
            let audio_path = listen::record_audio()?;

            println!("Transcribing...");
            let transcript = ai::transcribe(&audio_path)?;
            let _ = std::fs::remove_file(&audio_path);

            if transcript.trim().is_empty() {
                println!("No speech detected.");
                return Ok(());
            }

            println!("Structuring notes...");
            let (ai_title, body) = ai::structure_notes(&transcript)?;
            let title = title.unwrap_or(ai_title);

            let note = store.create_note(&title, &body, vec!["listen".to_string()])?;
            println!("Created \"{}\" {}", title, &note.id[..8]);
        }

        Commands::Export { id, format } => {
            let note = match store.find_by_index_or_prefix(&id) {
                Some(n) => n,
                None => {
                    eprintln!("No note found: {id}");
                    return Ok(());
                }
            };
            let format = format.trim_start_matches('.');
            let output_dir = dirs::desktop_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let path = export::export_note(note, format, &output_dir)?;
            println!("Exported to {}", path.display());
        }
    }

    store.save()?;
    Ok(())
}
