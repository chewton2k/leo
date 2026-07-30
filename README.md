# leo

> Notes for programmers — fast, local, plain-text, AI-powered.

`leo` is a lightweight note manager that lives entirely in your terminal. No Electron app, no subscription. Just run `leo` and start typing. With built-in AI features, leo can record lectures, transcribe speech into structured notes, answer inline questions, and sync your notes to GitHub.

## Install and Setup

```sh
git clone https://github.com/you/leo
cd leo
cargo install --path .
```

### Setup

`leo` works with no API keys at all if you run models locally:

```sh
brew install ollama whisper-cpp
ollama pull qwen3:8b
```

Otherwise, store a key in your OS keychain — not a plaintext file:

```sh
leo model login openrouter   # free models via openrouter/free
leo model login groq         # free Whisper transcription
leo model list               # check what's configured
```

`leo model login` reads the key with echo disabled, so it never appears on
screen or in your shell history, and offers to import an existing `.env` value
if it finds one. `leo model list` shows only the last four characters of any
key.

Tune providers and fallback order in `leo config edit`. Providers are tried in
order and unavailable ones (no key, no binary, closed port) are skipped
silently, so listing more providers than you have installed is fine.

API keys are only required for AI features (`listen`, `ask`). All other commands work without them.

The older `leo env` command still works and writes a plaintext `.env`; env vars
take precedence over the keychain, which is useful in CI.


### Troubleshooting Installation
If this doesn't work, you can run a check for 
```sh
~/.cargo/bin/leo 
```

If that works you just need to add it to your ~/.zshrc or ~/.bashrc through: 
```sh
export PATH="$HOME/.cargo/bin:$PATH"
```



## Installation after modifications

```sh
# Reinstall after changes
cargo install --path . --force

# Uninstall (won't delete your notes)
cargo uninstall leo
```

### Optional dependencies

| Tool | Required for | Install |
|------|-------------|---------|
| [SoX](https://sox.sourceforge.net/) | `listen` (audio recording) | `brew install sox` |
| [Pandoc](https://pandoc.org/) | `export` to docx, pdf, rtf, odt | `brew install pandoc` |
| [git](https://git-scm.com/) | `sync` (GitHub backup) | usually pre-installed |
| [Ollama](https://ollama.com/) | free local chat (no API key) | `brew install ollama` |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) | free local transcription | `brew install whisper-cpp` |

## Getting started

```sh
leo
```

This drops you into an interactive session:

```
  leo — notes for programmers
  0 notes · Type help to get started

leo> new Rust ownership notes
  (opens $EDITOR with a template — write your note, save, and quit)
  Created 3f2a1b4c

leo> list
    1  3f2a1b4c  2024-03-30 10:30  Rust ownership notes  [rust, learning]

leo> view 1
leo> edit 1
leo> quit
```

After `list` or `search`, notes are numbered — use `view 1`, `edit 2`, `delete 3` instead of typing IDs.

## Commands

### Notes

| Command | What it does | Shortcut |
|---------|-------------|----------|
| `new [title]` | Create a note (opens `$EDITOR`) | `n` |
| `list [#tag] [N]` | List notes, optionally filter by tag or limit count | `ls`, `l` |
| `view <note>` | View a note | `v` |
| `edit <note>` | Edit a note in `$EDITOR` | `e` |
| `delete <note>` | Delete a note | `rm`, `del`, `d` |
| `check <note> <N>` | Toggle checkbox N | `x` |
| `search <query>` | Search note titles | `find` |
| `search -f <query>` | Full-text search (titles + bodies) | |
| `tags` | Show all tags with counts | |

`<note>` can be a list number (`view 1`) or an ID prefix (`view 3f2a`).

### Directories

Organize notes into directories:

```
leo> mkdir cs130
leo> cd cs130
leo cs130> new Lecture 1
leo cs130> cd ..
leo> mv 1 cs130
```

| Command | What it does |
|---------|-------------|
| `mkdir <name>` | Create a directory |
| `cd <dir>` | Change directory (`..`, `/` supported) |
| `pwd` | Show current directory |
| `mv <note>... <dir>` | Move notes to a directory |
| `rmdir <name>` | Remove an empty directory |

### Creating notes

`new` opens your `$EDITOR` with a frontmatter template:

```markdown
---
title: My Note
tags: rust, learning
---
Write your note here. Full markdown supported.

- [ ] Checkboxes work
- [ ] Like this
```

Save and quit to create the note. Empty body cancels.

### Checklists

Notes support markdown checkboxes and bullets:

```
leo> view 1
  [1] ☐ Write tests
  [2] ☑ Fix login bug
  • Remember to deploy

leo> check 1 1
  ☑ Write tests
```

## AI features

### Reminders

```
leo> remind me to buy groceries
leo> hey leo remind me to call mom
```

Reminders are stored as checkboxes in a `#reminder` note. Toggle with `check`.

### Listen (speech-to-notes)

Record audio and get AI-structured notes:

```
leo> listen
  Recording... press Enter to stop
  Transcribing...
  Created "Intro to ML — Lecture 3" a1b2c3d4

leo> listen CS 101 Lecture       # custom title
leo> listen add 1                # append to existing note
```

**Requires:** SoX (`brew install sox`), plus at least one working provider in
each chain — either local (`ollama` + `whisper-cpp`) or a key for one cloud
provider (`leo model login openrouter`, `leo model login groq`).

### Inline AI prompts

Write `@leo` questions directly in a note and expand them with `ask`:

```markdown
## Rust ownership

@leo what is the difference between Box and Rc?
```

```
leo> ask 1
  Expanding 1 prompt...
  Updated "Rust ownership notes" 3f2a1b4c
```

The `@leo` line is replaced with the AI's answer inline. Works in both the REPL and as a CLI subcommand (`leo ask <id>`). Also triggers automatically when saving a note in `edit` if any `@leo` lines are present.

**Requires:** one working chat provider — a running `ollama`, or
`leo model login openrouter`.

### Export

```
leo> export 1 md
  Exported /Users/you/Desktop/My-Note.md
```

Formats: `txt`, `md`, `html`, `docx`, `pdf`, `rtf`, `odt` (last four need Pandoc).

### Model providers

Inspect and manage the AI providers behind `listen` and `ask`:

```sh
leo model list                 # both chains, models, and credential status
leo model test openrouter      # one minimal request to check it works
leo model login openrouter     # store a key in the OS keychain (echo disabled)
leo model logout openrouter    # remove it
leo config path                # where config.toml lives
leo config edit                # create/open config.toml in $EDITOR
```

`config.toml` holds the fallback chains and provider definitions:

```toml
[chat]
chain = ["ollama", "openrouter"]

[transcribe]
chain = ["whisper_cpp", "groq", "hf"]

[providers.ollama]
kind = "openai"                # any OpenAI-compatible endpoint
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
```

Keys never go in this file — they live in your OS keychain, or in env vars
(`OPENROUTER_API_KEY`, `GROQ_API_KEY`, `HF_API_KEY`), which take precedence.

### Serve

Access your notes from your phone or browser:

```
leo> serve
# or from the CLI:
leo serve --port 3131
```

Opens a web UI with a QR code for easy phone access on your local network.

## Sync (GitHub backup)

Back up and sync your notes via git. Notes are stored as plain `.md` files, so your repo is readable on GitHub as-is.

```
leo> sync init               # initialize a git repo in your notes directory
leo> sync connect <url>      # connect to a GitHub remote
leo> sync push               # push notes to GitHub
leo> sync pull               # pull notes from GitHub (reloads store)
leo> sync status             # show git status
```

Or as CLI subcommands:

```sh
leo sync init
leo sync connect https://github.com/you/leo-notes.git
leo sync push
leo sync pull
leo sync status
```

Notes are auto-committed on every save when a sync repo is initialized — no manual commits needed.

## Scripting

All commands work as CLI subcommands for scripts and one-liners:

```sh
leo new "Quick thought" --body "Remember to refactor auth" --tags todo
leo new "Meeting notes"          # opens $EDITOR when --body is omitted
leo list --tag todo
leo search "refactor" --full-text
leo delete 3f2a --force
leo remind "buy coffee"
leo listen --title "Meeting notes"
leo export 3f2a md
leo ask 3f2a
```

## Data storage

Notes are stored as individual `.md` files with YAML frontmatter:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/leo/` |
| Linux | `~/.local/share/leo/` |
| Windows | `%APPDATA%\leo\` |

Each note is a file like `<uuid>.md`. Directory structure is mirrored on disk. Existing `notes.json` data is automatically migrated on first run.

## License

MIT
