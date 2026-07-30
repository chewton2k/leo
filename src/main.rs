mod action;
mod ai;
mod config;
mod export;
mod listen;
mod notes;
mod repl;
mod store;
mod sync;
mod web;

use std::io::IsTerminal;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;

use config::secret::{redact, resolve, KeyringStore, SecretStore};
use config::Config;

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

        /// Append to an existing note instead of creating a new one
        #[arg(short, long)]
        add: Option<String>,

        /// Capture system audio instead of microphone (requires BlackHole: brew install blackhole-2ch)
        #[arg(long)]
        screen: bool,
    },

    /// Export a note to a file (txt, md, html, docx, pdf, rtf, odt)
    Export {
        /// Note ID (or unique prefix)
        id: String,

        /// Output format
        format: String,
    },

    /// Expand all @leo prompts in a note using AI
    Ask {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Start a web server to view/edit notes from your phone
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3131)]
        port: u16,
    },

    /// Open the .env config file to set API keys
    Env,

    /// Sync notes via git / GitHub
    Sync {
        #[command(subcommand)]
        command: SyncCommands,
    },

    /// Inspect, test, and authenticate AI model providers
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// Open or show the leo model config file
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// Show configured chains, reachability, and credential status
    List,
    /// Send one minimal request to a provider to check it works
    Test {
        /// Provider name from your config
        name: String,
    },
    /// Store a provider's API key in your OS keychain
    Login {
        /// Provider name from your config
        name: String,
    },
    /// Remove a provider's API key from your OS keychain
    Logout {
        /// Provider name from your config
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Open config.toml in $EDITOR, creating it if absent
    Edit,
    /// Print the path to config.toml
    Path,
}

/// The clap surface and the TUI's `:` line both funnel into one vocabulary, so
/// `leo model login x` and `:model login x` cannot drift apart.
impl From<ModelCommands> for action::ModelAction {
    fn from(c: ModelCommands) -> Self {
        match c {
            ModelCommands::List => action::ModelAction::List,
            ModelCommands::Test { name } => action::ModelAction::Test { name },
            ModelCommands::Login { name } => action::ModelAction::Login { name },
            ModelCommands::Logout { name } => action::ModelAction::Logout { name },
        }
    }
}

impl From<ConfigCommands> for action::ConfigAction {
    fn from(c: ConfigCommands) -> Self {
        match c {
            ConfigCommands::Edit => action::ConfigAction::Edit,
            ConfigCommands::Path => action::ConfigAction::Path,
        }
    }
}

#[derive(Subcommand)]
enum SyncCommands {
    /// Initialize a git repo for your notes (run this first)
    Init,
    /// Connect the notes repo to a GitHub remote
    Connect {
        /// Remote URL, e.g. https://github.com/user/leo-notes.git
        /// or git@github.com:user/leo-notes.git (SSH)
        url: String,
    },
    /// Push notes to the remote
    Push,
    /// Pull notes from the remote
    Pull,
    /// Show git status of the notes repo
    Status,
}

fn main() -> Result<()> {
    // Load .env from the leo data directory so the installed binary finds it
    // regardless of working directory. Also try current directory for development.
    if let Some(data_dir) = dirs::data_dir() {
        dotenvy::from_path(data_dir.join("leo").join(".env")).ok();
    }
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Serve { port }) => {
            tokio::runtime::Runtime::new()?.block_on(web::serve(port))
        }
        Some(Commands::Env) => open_env_file(),
        Some(Commands::Sync { command }) => run_sync(command),
        Some(Commands::Model { command }) => run_model(command.into()),
        Some(Commands::Config { command }) => run_config(command.into()),
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
            if let Some(body) = body {
                let note = store.create_note(title, body, tags, "")?;
                println!("Created note {}", &note.id[..8]);
            } else {
                // Open $EDITOR for the body
                let editor = std::env::var("EDITOR")
                    .or_else(|_| std::env::var("VISUAL"))
                    .unwrap_or_else(|_| "vim".to_string());

                let tmp = std::env::temp_dir().join(format!("leo-new-{}.md", uuid::Uuid::new_v4()));
                let file_content = format!(
                    "---\ntitle: {}\ntags: {}\n---\n",
                    title,
                    tags.join(", "),
                );
                std::fs::write(&tmp, &file_content)?;

                let status = std::process::Command::new(&editor).arg(&tmp).status()?;

                if status.success() {
                    let raw = std::fs::read_to_string(&tmp)?;
                    let _ = std::fs::remove_file(&tmp);
                    let (new_title, new_tags, body) = repl::parse_frontmatter(&raw);
                    let title = if new_title.is_empty() { title } else { new_title };
                    let tags = if new_tags.is_empty() { tags } else { new_tags };

                    if body.trim().is_empty() {
                        println!("Empty note, cancelled.");
                    } else {
                        let note = store.create_note(title, body, tags, "")?;
                        println!("Created note {}", &note.id[..8]);
                    }
                } else {
                    let _ = std::fs::remove_file(&tmp);
                    eprintln!("Editor exited with error.");
                }
            }
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
            let (old_title, old_tags, old_body, resolved_id) =
                match store.find_by_index_or_prefix(&id) {
                    Some(n) => (
                        n.title.clone(),
                        n.tags.clone(),
                        n.body.clone(),
                        n.id.clone(),
                    ),
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
            let file_content = format!(
                "---\ntitle: {}\ntags: {}\n---\n{}",
                old_title,
                old_tags.join(", "),
                old_body
            );
            std::fs::write(&tmp, &file_content)?;

            let status = std::process::Command::new(&editor).arg(&tmp).status()?;
            if !status.success() {
                let _ = std::fs::remove_file(&tmp);
                eprintln!("Editor exited with error.");
                return Ok(());
            }

            let raw = std::fs::read_to_string(&tmp)?;
            let _ = std::fs::remove_file(&tmp);
            let (parsed_title, tags, mut body) = repl::parse_frontmatter(&raw);
            let title = if parsed_title.is_empty() { old_title.clone() } else { parsed_title };

            // Expand any @leo prompts in one pass, then save
            let leo_count = body.lines().filter(|l| repl::is_leo_prompt(l).is_some()).count();
            if leo_count > 0 {
                println!(
                    "Expanding {} prompt{}...",
                    leo_count,
                    if leo_count == 1 { "" } else { "s" }
                );
                let (expanded, _) = repl::expand_leo_prompts(&body, &title)?;
                body = expanded;
            }

            if title != old_title || tags != old_tags || body.trim() != old_body.trim() {
                let note = store.find_note_mut(&id).unwrap();
                note.title = title.clone();
                note.tags = tags;
                note.body = body;
                note.updated_at = chrono::Utc::now();
                println!("Updated \"{}\".", title);
            } else {
                println!("No changes.");
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
                store.create_note("Reminders", &item, vec!["reminder".to_string()], "")?;
                println!("Created Reminders + {text}");
            }
        }

        Commands::Listen { title, add, screen } => {
            let audio_path = listen::record_audio(screen)?;

            println!("Transcribing...");
            let transcript = ai::transcribe(&audio_path)?;
            let _ = std::fs::remove_file(&audio_path);

            if transcript.trim().is_empty() {
                println!("No speech detected.");
                return Ok(());
            }

            println!("Structuring notes...");

            if let Some(target) = add {
                let existing_body = match store.find_by_index_or_prefix(&target) {
                    Some(n) => n.body.clone(),
                    None => {
                        eprintln!("No note found: {target}");
                        return Ok(());
                    }
                };
                let new_content = ai::structure_notes_append(&transcript, &existing_body)?;
                let note = store.find_by_index_or_prefix_mut(&target).unwrap();
                note.body = format!("{}\n\n{}", note.body, new_content);
                note.updated_at = chrono::Utc::now();
                println!("Updated \"{}\" {}", note.title, &note.id[..8]);
            } else {
                let (ai_title, body) = ai::structure_notes(&transcript)?;
                let title = title.unwrap_or(ai_title);
                let note = store.create_note(&title, &body, vec!["listen".to_string()], "")?;
                println!("Created \"{}\" {}", title, &note.id[..8]);
            }
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

        Commands::Ask { id } => {
            let (note_id, title, body) = match store.find_by_index_or_prefix(&id) {
                Some(n) => (n.id.clone(), n.title.clone(), n.body.clone()),
                None => {
                    eprintln!("No note found: {id}");
                    return Ok(());
                }
            };

            let leo_count = body.lines().filter(|l| repl::is_leo_prompt(l).is_some()).count();
            if leo_count == 0 {
                println!("No @leo prompts found in this note.");
                return Ok(());
            }

            println!(
                "Expanding {} prompt{}...",
                leo_count,
                if leo_count == 1 { "" } else { "s" }
            );
            let (expanded_body, _) = repl::expand_leo_prompts(&body, &title)?;

            let note = store.find_note_mut(&note_id).unwrap();
            note.body = expanded_body;
            note.updated_at = chrono::Utc::now();
            println!("Updated \"{}\" {}", note.title, &note.id[..8]);
        }

        Commands::Serve { .. }
        | Commands::Env
        | Commands::Sync { .. }
        | Commands::Model { .. }
        | Commands::Config { .. } => {
            unreachable!("handled in main()")
        }
    }

    store.save()?;
    Ok(())
}

fn run_sync(command: SyncCommands) -> Result<()> {
    let store = store::Store::load()?;
    match command {
        SyncCommands::Init => sync::init(&store.notes_dir),
        SyncCommands::Connect { url } => sync::connect(&store.notes_dir, &url),
        SyncCommands::Push => sync::push(&store.notes_dir),
        SyncCommands::Pull => sync::pull(&store.notes_dir),
        SyncCommands::Status => sync::status(&store.notes_dir),
    }
}

/// Describe where a provider's credential comes from — never what it is.
fn describe_credential(provider: &str, key_env: Option<&str>, store: &dyn SecretStore) -> String {
    let Some(var) = key_env else {
        return "no key needed".to_string();
    };
    if let Ok(v) = std::env::var(var) {
        if !v.trim().is_empty() {
            return format!("key from env {var} ({})", redact(&v));
        }
    }
    // `None` for key_env here on purpose: the env var was just checked above,
    // so this call is only asking the keychain.
    match resolve(provider, None, store) {
        Some(secret) => format!("key in keychain ({})", redact(secret.as_str())),
        None => format!("no key (run `leo model login {provider}`)"),
    }
}

/// Build the single transcription provider named by `name`, regardless of
/// whether it appears in the `[transcribe]` chain — `leo model test <name>`
/// must work for a provider a user is still setting up.
fn build_one_transcriber(
    name: &str,
    pc: &config::provider::ProviderConfig,
    store: &dyn SecretStore,
) -> Option<Box<dyn ai::provider::TranscribeProvider>> {
    use config::provider::ProviderKind;
    match pc.kind {
        Some(ProviderKind::Hf) => Some(Box::new(ai::provider::hf::HfTranscribe::new(
            name.to_string(),
            pc,
            resolve(name, pc.key_env.as_deref(), store),
        ))),
        Some(ProviderKind::Groq) => Some(Box::new(ai::provider::groq::GroqTranscribe::new(
            name.to_string(),
            pc,
            resolve(name, pc.key_env.as_deref(), store),
        ))),
        Some(ProviderKind::WhisperCpp) => Some(Box::new(
            ai::provider::whisper_cpp::WhisperCppTranscribe::new(name.to_string(), pc),
        )),
        _ => None,
    }
}

pub fn run_model(command: action::ModelAction) -> Result<()> {
    let cfg = Config::load();
    let store = KeyringStore;

    match command {
        action::ModelAction::List => {
            if !store.available() {
                println!(
                    "  {}",
                    "keychain unavailable on this system — keys must come from env vars".yellow()
                );
            }

            for (task, chain) in [
                ("chat", &cfg.chat.chain),
                ("transcribe", &cfg.transcribe.chain),
            ] {
                println!("\n  {} chain:", task.bold());
                if chain.is_empty() {
                    println!("    {}", "(empty)".dimmed());
                }
                for (i, name) in chain.iter().enumerate() {
                    let Some(pc) = cfg.provider(name) else {
                        println!(
                            "    {}. {} {}",
                            i + 1,
                            name.red(),
                            "— no [providers] block with this name".dimmed()
                        );
                        continue;
                    };
                    let cred = describe_credential(name, pc.key_env.as_deref(), &store);
                    let model = pc.model.as_deref().unwrap_or("(default)");
                    println!("    {}. {} — {} — {}", i + 1, name.bold(), model, cred);
                }
            }

            let chat = ai::provider::build_chat_chain(&cfg, &store);
            let trans = ai::provider::build_transcribe_chain(&cfg, &store);
            println!(
                "\n  ready now: chat {} / transcribe {}",
                chat.iter().filter(|p| p.available()).count(),
                trans.iter().filter(|p| p.available()).count()
            );
            println!();
            Ok(())
        }

        action::ModelAction::Test { name } => {
            let Some(pc) = cfg.provider(&name) else {
                anyhow::bail!("no provider named '{name}' in your config");
            };

            let started = std::time::Instant::now();
            let result = match pc.kind {
                Some(config::provider::ProviderKind::Openai) => {
                    let key = resolve(&name, pc.key_env.as_deref(), &store);
                    let p = ai::provider::openai::OpenAiChat::new(name.clone(), pc, key);
                    if !p.available() {
                        anyhow::bail!("{}", p.unavailable_reason());
                    }
                    use ai::provider::ChatProvider;
                    p.complete(&ai::provider::ChatRequest {
                        prompt: "Reply with the single word: ok".to_string(),
                        temperature: 0.0,
                        max_tokens: 16,
                    })
                    .map(|s| s.trim().to_string())
                    .map_err(|e| anyhow::anyhow!("{e}"))
                }
                Some(_) => {
                    // Transcription providers need audio; check reachability
                    // only, so testing one costs nothing and needs no mic.
                    match build_one_transcriber(&name, pc, &store) {
                        Some(p) if p.available() => Ok("available".to_string()),
                        Some(p) => anyhow::bail!("{}", p.unavailable_reason()),
                        None => anyhow::bail!("provider '{name}' could not be built"),
                    }
                }
                None => anyhow::bail!("provider '{name}' has no `kind`"),
            };

            match result {
                Ok(reply) => {
                    println!(
                        "  {} {name} responded in {:?}: {reply}",
                        "ok".green(),
                        started.elapsed()
                    );
                    Ok(())
                }
                Err(e) => {
                    println!("  {} {name}: {e}", "failed".red());
                    Ok(())
                }
            }
        }

        action::ModelAction::Login { name } => {
            if cfg.provider(&name).is_none() {
                println!(
                    "  {} no [providers.{name}] block in your config — storing the key anyway.",
                    "note".yellow()
                );
                println!("  Run `leo config edit` to add it, or check `leo model list`.");
            }
            let key_env = cfg.provider(&name).and_then(|p| p.key_env.clone());

            // Offer to import an existing .env value rather than making the
            // user paste it again.
            if let Some(var) = key_env.as_deref() {
                if let Ok(existing) = std::env::var(var) {
                    if !existing.trim().is_empty() {
                        println!("  Found {var} in your environment ({}).", redact(&existing));
                        print!("  Import it into the keychain? [Y/n] ");
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer)?;
                        if !answer.trim().eq_ignore_ascii_case("n") {
                            store.set(&name, existing.trim())?;
                            println!("  {} stored for {name}.", "ok".green());
                            println!(
                                "  You can now remove this line from your .env:\n    {var}=..."
                            );
                            return Ok(());
                        }
                    }
                }
            }

            // Read with echo disabled: never on screen, never in shell history,
            // never an argv value visible to other processes.
            let secret = rpassword::prompt_password(format!("  API key for {name}: "))?;
            if secret.trim().is_empty() {
                anyhow::bail!("no key entered");
            }
            store.set(&name, secret.trim())?;
            println!("  {} stored for {name}.", "ok".green());
            Ok(())
        }

        action::ModelAction::Logout { name } => {
            store.delete(&name)?;
            println!("  {} removed key for {name}.", "ok".green());
            Ok(())
        }
    }
}

pub fn run_config(command: action::ConfigAction) -> Result<()> {
    match command {
        action::ConfigAction::Path => {
            println!("{}", Config::config_path()?.display());
            Ok(())
        }
        action::ConfigAction::Edit => {
            let (path, created) = Config::ensure_exists()?;
            if created {
                println!("  Created {}", path.display());
            }
            let editor = std::env::var("EDITOR")
                .or_else(|_| std::env::var("VISUAL"))
                .unwrap_or_else(|_| "vim".to_string());
            std::process::Command::new(editor).arg(&path).status()?;
            Ok(())
        }
    }
}

pub fn open_env_file() -> Result<()> {
    let env_path = dirs::data_dir()
        .context("Could not determine user data directory")?
        .join("leo")
        .join(".env");

    if let Some(parent) = env_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !env_path.exists() {
        std::fs::write(
            &env_path,
            "# leo API keys\n\
             # Get your OpenRouter key at https://openrouter.ai/keys\n\
             OPENROUTER_API_KEY=\n\
             \n\
             # Get your Hugging Face key at https://huggingface.co/settings/tokens\n\
             HF_API_KEY=\n\
             \n\
             # Get your Groq key at https://console.groq.com/keys (fallback transcription)\n\
             GROQ_API_KEY=\n",
        )?;
    }

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vim".to_string());

    std::process::Command::new(&editor)
        .arg(&env_path)
        .status()?;

    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use config::secret::MemoryStore;
    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn describes_a_missing_credential() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_DESC_A");
        let store = MemoryStore::default();
        let s = describe_credential("openrouter", Some("LEO_TEST_DESC_A"), &store);
        assert!(s.contains("no key"), "got: {s}");
    }

    #[test]
    fn describes_a_stored_credential_without_revealing_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_DESC_B");
        let store = MemoryStore::default();
        store.set("openrouter", "sk-or-v1-supersecret9999").unwrap();

        let s = describe_credential("openrouter", Some("LEO_TEST_DESC_B"), &store);
        assert!(s.contains("keychain"), "got: {s}");
        assert!(s.contains("9999"), "should show last four: {s}");
        assert!(!s.contains("supersecret"), "LEAKED THE KEY: {s}");
    }

    #[test]
    fn describes_an_env_credential_as_coming_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_DESC_C", "env-key-8888");
        let store = MemoryStore::default();
        let s = describe_credential("openrouter", Some("LEO_TEST_DESC_C"), &store);
        std::env::remove_var("LEO_TEST_DESC_C");

        assert!(s.contains("env"), "got: {s}");
        assert!(!s.contains("env-key-8888"), "LEAKED THE KEY: {s}");
    }

    /// An env var set for one provider must not be reported as the credential
    /// of a provider that names a different (unset) var.
    #[test]
    fn env_of_another_provider_is_not_reported() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_DESC_OTHER", "not-mine-7777");
        std::env::remove_var("LEO_TEST_DESC_MINE");
        let store = MemoryStore::default();
        let s = describe_credential("groq", Some("LEO_TEST_DESC_MINE"), &store);
        std::env::remove_var("LEO_TEST_DESC_OTHER");

        assert!(s.contains("no key"), "got: {s}");
        assert!(!s.contains("7777"), "reported another provider's key: {s}");
    }

    #[test]
    fn a_keyless_provider_is_described_as_needing_none() {
        let store = MemoryStore::default();
        let s = describe_credential("ollama", None, &store);
        assert!(s.contains("no key needed"), "got: {s}");
    }
}
