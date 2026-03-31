# leo

> Notes for programmers — fast, local, plain-text, AI-powered.

`leo` is a lightweight note manager that lives entirely in your terminal. No Electron app, no cloud sync, no subscription. Just run `leo` and start typing. With built-in AI features, leo can record lectures, transcribe speech into structured notes, manage reminders, and export to any format.

## Install

```sh
git clone https://github.com/you/leo
cd leo
cargo install --path .
```

## Reinstall & uninstall

```sh
# Reinstall after making changes (overwrites the existing binary)
cargo install --path . --force

# Uninstall
cargo uninstall leo
```

The binary lives at `~/.cargo/bin/leo` — there's only ever one copy. Uninstalling won't delete your notes.

## Setup

Copy the example env file and add your [OpenRouter](https://openrouter.ai/) API key:

```sh
cp .env.example .env
# Then edit .env with your key
```

```
OPENROUTER_API_KEY=sk-or-your-key-here
```

The API key is required for the `listen` command (audio transcription + note structuring). All other commands work without it.

### Optional dependencies

| Tool | Required for | Install |
|------|-------------|---------|
| [SoX](https://sox.sourceforge.net/) | `listen` (audio recording) | `brew install sox` |
| [Pandoc](https://pandoc.org/) | `export` to docx, pdf, rtf, odt | `brew install pandoc` |

## Getting started

```sh
leo
```

That drops you into an interactive session:

```
  leo — notes for programmers
  0 notes · Type help to get started

leo> new
  Title: Rust ownership notes
  Tags (comma-separated, enter to skip): rust, learning
  Body (empty line to finish):
  | Each value has exactly one owner.
  | When the owner goes out of scope, the value is dropped.
  |
  Created 3f2a1b4c

leo> list
    1  3f2a1b4c  2024-03-30 10:30  Rust ownership notes  [rust, learning]

leo> view 1
────────────────────────────────────────────────────────────
Title: Rust ownership notes
ID:    3f2a1b4c-...
Tags:  rust, learning
...

leo> quit
Goodbye!
```

After `list` or `search`, notes are numbered — use `view 1`, `edit 2`, `delete 3` instead of typing IDs.

## Commands

| Command | What it does | Shortcut |
|---------|-------------|----------|
| `new [title]` | Create a new note | `n` |
| `list [#tag] [N]` | List recent notes, filter by tag, limit count | `ls`, `l` |
| `view <note>` | View a note's full content | `v` |
| `edit <note>` | Edit note body in `$EDITOR` | `e` |
| `delete <note>` | Delete a note (with confirmation) | `rm`, `del`, `d` |
| `check <note> <N>` | Toggle checkbox N in a note | `x` |
| `search <query>` | Search note titles | `find` |
| `search -f <query>` | Full-text search (titles + bodies) | |
| `tags` | Show all tags and their counts | |
| `clear` | Clear the screen | |
| `help` | Show all commands | `h`, `?` |
| `exit` | Exit | `q`, `quit` |

`<note>` can be a list number (`view 1`) or an ID prefix (`view 3f2a`).

## AI Features

### Reminders

Add reminders naturally — leo keeps them all in a single "Reminders" note with checkboxes you can toggle:

```
leo> remind me to buy groceries
  Added buy groceries

leo> hey leo remind me to call mom
  Added call mom

leo> remind submit homework by friday
  Added submit homework by friday
```

All reminders are stored as checkboxes (`- [ ] ...`) in a note tagged `#reminder`. Use `check` to mark them done.

You can also say `hey leo remind me to ...` for a more natural feel — the `hey leo` prefix is recognized automatically.

### Listen (AI-powered class/meeting notes)

Record audio from your microphone and let AI transcribe and structure it into organized notes:

```
leo> listen
  Recording... press Enter to stop

  Recording stopped. (~45s, 1.4MB)
  Transcribing...
  Structuring notes...
  Created "Intro to Machine Learning — Lecture 3" a1b2c3d4
  Use 'export <note> <format>' to export this note.
```

Provide a custom title if you want:

```
leo> listen CS 101 Lecture
```

Notes created by `listen` are tagged `#listen` and contain AI-structured content with headings, bullet points, and action items extracted from the transcript.

**Requirements:** `OPENROUTER_API_KEY` in `.env` + SoX installed (`brew install sox`).

### Export

Export any note to a file on your Desktop:

```
leo> export 1 md
  Exported /Users/you/Desktop/My-Note.md

leo> export 1 txt
  Exported /Users/you/Desktop/My-Note.txt

leo> export 1 html
  Exported /Users/you/Desktop/My-Note.html

leo> export 1 docx
  Exported /Users/you/Desktop/My-Note.docx
```

| Format | How |
|--------|-----|
| `txt` | Plain text with metadata |
| `md` | Markdown (native) |
| `html` | Standalone HTML with styling |
| `docx`, `pdf`, `rtf`, `odt` | Via Pandoc (`brew install pandoc`) |

## Checklists

Notes support markdown-style checkboxes and bullet lists:

```
leo> new Todo
  Tags: work
  Body (empty line to finish):
  | - [ ] Write tests
  | - [ ] Update docs
  | - [x] Fix login bug
  | - Remember to deploy
  |
  Created a1b2c3d4

leo> view 1
  ...
  [1] ☐ Write tests
  [2] ☐ Update docs
  [3] ☑ Fix login bug
  • Remember to deploy

leo> check 1 1
  ☑ Write tests
```

Checkboxes are numbered in `view` so you can toggle them with `check <note> <N>`.

## Scripting

For scripts and one-liners, subcommands still work:

```sh
leo new "Quick thought" --body "Remember to refactor auth" --tags todo
leo list --tag todo
leo search "refactor" --full-text
leo delete 3f2a --force
leo remind "buy coffee"
leo listen --title "Meeting notes"
leo export 3f2a md
```

## Data storage

Notes are stored as a single JSON file:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/leo/notes.json` |
| Linux | `~/.local/share/leo/notes.json` |
| Windows | `%APPDATA%\leo\notes.json` |

Command history is saved alongside at `history.txt`.

## License

MIT
