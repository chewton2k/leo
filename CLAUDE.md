# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

`leo` is a desktop note app (Tauri, macOS and Windows) written in Rust, laid
out as a Cargo workspace. Each crate may depend only on the ones above it in this table, and
the compiler enforces it — do not add an upward dependency:

| Crate | Holds | Depends on |
|-------|-------|------------|
| `crates/leo-core` | `notes`, `store`, `filename`, `open` (opens a link with the system), `children` (stops a process tree, lists the processes leo owns for Felix activity's Running now; `quiet_command` starts every helper program without a console window on Windows), `diag`, `keep` (retention choices), `paths`, `recording`, `activity` | — |
| `crates/leo-services` | `ai/` (chains, prompts, live transcription), `config/` (providers, credentials), `health`, `providers`, `speech`, `dictation`, `session`, `update`, `usage`, `import` | core |
| `crates/leo-web` | leo's engine and pages: `lib.rs` (`start` → `Running`, router), `routes/` (handlers by area), `token.rs`, and the page in `src/web/` (`page-head.html` + `styles/*.css` + `page-tail.html` joined at compile time, `app/*.js` served as `/app.js`, `markdown.js`, …) | core |
| `crates/leo-host` | `powers()` (the AI, speech, settings, recording housekeeping and import powers handed to the engine) and `leo-test-server` (Playwright's engine) | core, services, web |
| `crates/leo-desktop` | The Tauri app `leo-app`: window, menu bar icon, single instance, open at login, PATH from the login shell, links/downloads/print | core, services, host, web |
| root `leo` | `main.rs` and `cli/`: `leo` opens the app, `leo update`, `leo uninstall`, `--version` | core, services |

Verify with `cargo test` at the root (runs every crate but `leo-desktop`, which
is outside `default-members`) and `cargo clippy --workspace --all-targets`. Do not
run `cargo build`, `cargo install` or `cargo tauri build` unless asked; the one local
build allowed is `pnpm --dir crates/leo-desktop tauri dev` to try the real app by
hand (2026-10-31, the user's call). CI tests and lints `leo-desktop` on macOS and
Windows only (the app needs Rust 1.90; services/core retain their 1.88 check).

Never run the full test suites on the developer's machine (no
workspace-wide `cargo test`, no full Playwright run; the user's call,
2026-10-10). Build out every feature asked for using only quick targeted
checks (`cargo check`, `cargo fmt`, `cargo clippy`, one crate's or one
test's run, a single Playwright `-g` test, one node test file), then commit,
push, and let CI run the full suite. Check CI once; fix a red run with a
follow-up push. CI compares main with the newest verified ancestor (a successful
CI push or a newer release), and pull requests with their base. Unknown inputs
and nightly/manual runs test everything; failed and cancelled pushes never
advance the baseline. `scripts/ci-scope.sh` selects independent Rust, browser,
Node checks; CONTRIBUTING.md describes the selection.

For website changes, do not build leo or run local Cargo checks/tests; rely on
CI for compiled and browser validation (the user's call, 2026-10-11). Lightweight
JavaScript checks and manual checks with fakes are allowed. Reuse the website's
existing components, controls, spacing and themes; new features should look like
the rest of leo, including folder controls and note browsing.

Leo is one app (2026-10-31, the user's call: "a normal app users click"). The
window shows the website, served by leo-web's engine started inside the app on
127.0.0.1 (`leo_web::start`, a remembered port, a per-launch token the window
signs in with). Nothing else can reach it: a Host check refuses any other host
(421) and the cookie is `SameSite=Strict`. There is no `leo serve`, no tunnel,
no phone or other-browser access, and no terminal interface; the terminal keeps
`leo` (opens the app), `leo update`, `leo uninstall` and `--version`. Plans and
the design live in `docs/superpowers/` (gitignored, local).

Subsystem notes load only when working in that crate:
`crates/leo-services/CLAUDE.md`, `crates/leo-web/CLAUDE.md`,
`crates/leo-desktop/CLAUDE.md`.

### Notes stay local

Leo no longer sets up or runs git/GitHub backup. Saves, idle work, visibility
changes and quit never commit or push. Existing repositories/remotes and legacy
`[sync]` settings are left untouched; the zip export in Settings → Keeping remains available.

### Capability checks

`health.rs` answers "can leo do X here?" in one place, so the doctor scan and
the pre-flight before recording agree, and every gap arrives with what fixes it
rather than as a failure discovered mid-use.
`health::writing` is the one writing-AI answer (Ready, NotInstalled,
NotRunning, NoModel, NotChosen, with its fix): `ai::writing_ready` and the
tray use its cheap path (no HTTP, never NoModel); the doctor's chat check
(`health::chain_check`), the recording pre-flight (leo-host's
`Housekeeping::checks`) and Settings (`web_settings::describe`'s `writing`) pass
the listed local models (`choice::local_models`: one loopback Ollama `/api/tags`
call with its short timeout), so an Ollama with no models is NoModel there.

Two rules there, both learned from tests that hung:

* **Never reach the keychain implicitly.** Every function takes
  `store: &dyn SecretStore`. An unsigned test binary reading the real keychain
  blocks on a permission dialog nobody can click — the suite took 1542 seconds
  before this was a parameter.
* **Never resolve DNS.** `to_socket_addrs` has no timeout, so `port_open` parses
  literal addresses and localhost only. A bogus hostname cost 900 seconds.

Checks must stay cheap enough to run on startup: a key presence check, a PATH
scan, a loopback connect with a 300 ms timeout, or that one loopback model
list. Never a request that leaves the machine.

### Data model

Notes are stored as one Markdown file per note, with YAML frontmatter, under
the platform data directory (`<data_dir>/leo/notes/`). `store.rs` derives a
note's `directory` from its path on disk, and `directories.json` tracks empty
directories. Each `Note` has `id` (UUID v4), `title`,
`body` (Markdown), `tags`, `directory` (empty string = root), `created_at`, and
`updated_at`.

`config.toml` lives in `<config_dir>/leo/`, separately from notes. Machine-local
settings and credentials are excluded from note exports.

### Recent additions worth knowing

- `leo_services::doctor::scan` (sections leo/notes/AI/recording; local checks
  only, no network, microphone or AI probes) is shown in Settings → Health.
- Recording loop: `leo_services::session::recorder` (`record`, `resume`,
  `finish`, `write_up`, `Event`); `Capture` takes native microphone/screen audio.
- `Store::unreadable`: files that failed to parse; `save` never deletes them.
- Trash: `save` moves any note file that vanished (and was not just moved:
  its id is not live) to `<notes>/.trash/<same relative path>`, mtime = when
  deleted. `tidy_trash` (load, a save that writes a new id, and hourly on save) drops live ids and ones past
  the Keeping choice (`keep::DEFAULT_TRASH_DAYS`, 30). The Trash page restores and empties.
- Updates: `LEO_NO_UPDATE_CHECK=1` disables the app's automatic update check
  (`crates/leo-desktop/src/updates.rs`; e2e and the browser tests set it).
  `leo update` reads the latest tag with `update::latest_release()` from the
  `releases/latest` redirect (not the API: rate limits; `LEO_LATEST_RELEASE`
  pins it), then runs the release's app installer with a bounded checked download; script-download
  failures cannot become successful empty shell pipelines. `LEO_INSTALL_DIR`
  comes from private atomic `app-cli-dir` outside the app, preserving a custom
  shim directory across in-app updates. `LEO_UPDATE_SCRIPT` injects a local
  installer in tests. Windows defers installation until the CLI exits.
- Note files are `<title>.md` (`filename::file_name`: FS-illegal chars to
  `-`, ≤100 chars and ≤200 bytes; same title in a folder → `Title (2).md`,
  oldest first, compared case-insensitively; `Store::assign_paths`). Windows
  device names get a dash (`CON-.md`, `filename::reserved_on_windows`) on every
  platform, since notes sync to Windows; new folders must pass
  `filename::portable_folder`. Old
  `<id>.md` files are renamed on the next save. A file name that is neither an
  id nor its title's name becomes the title on load (Obsidian rename).
- Headerless `.md` files load as notes: title = stem, id = `stable_id(path)`
  (FNV-1a 128 → UUID v8, golden-tested: never change it), dates = mtime, tags
  from list or string. `Note.extra` keeps unknown front matter.
- `Store.seen: id → Seen { path, disk, model }`: save writes only notes whose
  rendered hash changed; if the file on disk no longer matches `disk`
  (edited/renamed/deleted elsewhere) and leo changed the note too, it writes
  `<title> (conflict from leo)` with a new id (`CONFLICT_SUFFIX`, listed by
  doctor) instead of overwriting; deleted notes are trashed only if their file
  is still what leo saw. Writes are temp-file + rename.
- `Store::changed_on_disk`/`refresh` (fingerprint of paths, sizes, mtimes).
  Engine mutations reconcile before writing; hot read-only
  paths use the existing filesystem watcher with a 60 s reconciliation fallback.
  Returning to the native window forces a check before serving cached reads.
- Never slice strings by byte offsets computed from lengths: use char indices
  or `str::get`.
- Tests must never open the microphone: doctor's recording section only reads the
  device name (`health::microphone_device`), and tests set `LEO_NO_MICROPHONE`.
- Install: `install.sh` downloads the latest GitHub release. Releases are
  automatic: `ci.yml` calls `release.yml` after every job passes on a push to
  main; `scripts/next-version.sh` picks the version (patch + 1, or Cargo.toml's
  if newer, nothing if src/crates/Cargo.* are unchanged since the last tag,
  version-only lines ignored); the `bump` job runs `scripts/set-version.sh`,
  commits `release: vX.Y.Z` to main as github-actions[bot], and the build and
  tag use that commit. So after any push that releases, `git pull` before the
  next push. HTTPS via rustls.
- Writing choices: Ollama, Codex and Claude Code. Their credentials stay with
  their own programs. Speech is local only: a shared catalog model and language
  under `models_dir()/speech`, selected in `<config>/speech-model.json`.
  `leo-services::speech` verifies downloads, isolates native transcribe-cpp
  0.4.2 in a supervised child, prioritizes dictation and pins the model during
  recording. Cloud speech choices fall back to local speech; any keys
  already stored stay in leo's credentials file.
- Dictation preferences/text rules live in `leo-services::dictation`; desktop
  owns global shortcuts, permissions, in-memory capture, transactional paste
  and a bundled nonactivating popup. Writing AI runs on dictations only when
the user turns on the AI writing switch (2026-11-02, the user's call; off by default; on keeps a stored `selected`, else `quick`). Recent
  results are twenty local rows in `leo.db`, never notes or sync data; an edited
  row keeps its `original` text (Show original). Custom words fix spelling in
  `dictation/text.rs` and, where the speech model supports it, are passed to it
  as vocabulary (a recited-list echo is stripped); the AI check refuses changed
  words, numbers and added symbols; filler rules follow a `whatlang` guess of the
  text. The echo filter ports OpenWhispr's `dictionaryEchoFilter.js` and the
  Recent-dictations everyday-word list its `correctionLearner.js` (MIT, credited
  in NOTICE); the self-correction rules are leo's own. Robustness details:
  leo-desktop, leo-services and leo-web CLAUDE.md "Dictation".
- Recording is captured in Rust, including two native tracks for calls; hiding
  the window cannot stop it. App Nap is held while recording. Missing models
  leave audio pending and recoverable; the app can resume native sessions.
  Screenshots are pasted into the Record page with their timestamp.
- Windows child programs stay quiet. Model and toolchain caches stay under the
  existing models directory. Platform installers and release bundles are
  described by their scripts and workflow; updates check the selected catalog
  speech model, and uninstall retains the existing data-removal controls.
- Note connections (cache code still says `graph`): see crates/leo-web/CLAUDE.md.
- Tasks (`<notes>/.leo/tasks/`), boards and cards (`.leo/boards/`, `.leo/cards/`)
  are one-JSON-line-per-save files (newest line wins, tombstone deletes); boards
  are not calendar tasks. Calendar, boards and their Felix tools: see
  crates/leo-web/CLAUDE.md.
- Tags are hidden everywhere a person looks (decided 2026-10-08: folders,
  search and note connections cover it): no tag column, completion, Tags page or
  chips on the web. Tags already in files are kept and still searchable with `#word`
  (Obsidian).
- Felix in the margin: threads in `leo.db`, never note files or git; reads use
  `Powers.toolless` (no tools at all). See crates/leo-web/CLAUDE.md.
- Menu bar, study order, idle compaction and in-app drag and drop
  (2026-11-02, the user's notes): see crates/leo-desktop/CLAUDE.md (tray,
  `disable_drag_drop_handler`) and crates/leo-web/CLAUDE.md (Felix study order,
  idle compaction).
- Keeping: `leo_core::keep` (`<data>/keep.json`, beside notes, not synced):
  `trash_days` (default 30) drives `tidy_trash`; `chat_days`, `upload_days`,
  `transcript_days` and `audio_days` (default forever) drive leo-web's
  `keeping.rs` (see crates/leo-web/CLAUDE.md). `keep::describe` words a choice;
  `keep::load` reads each field on its own, so one bad value never resets the others.
- Cut-down (2026-11-05, the user's call): the Storage page is replaced by
  Keeping; Felix has no web tool of leo's own (Claude Code and Codex keep
  theirs); the first-run manual note is gone; the legacy fallbacks and
  one-time migrations are gone (`notes.json`, old tasks and chats, the direct
  Anthropic wire, `codex exec`, the sign-in device list, AGY/Copilot cleanup,
  stored cloud speech key retirement).

Recording sources live in `leo-core::recording` (`Archive`, and `Profile {context,
wants}`: calendar-event details and what the user wants from the notes), shared by
the app. Saved recording session IDs make new notes and appends idempotent;
retain sources before saving the note, then commit the session after the note is
durable. Default AI gap-filling and merging of typed points always stay, whatever
the user asks for. Note workflows (folder defaults, note formats, vocabulary, saved
actions, chat scope) were removed on 2026-10-10, the user's call: the "What do you
want from the notes?" box replaces formats.

Interview coding practice was removed by the user's decision on 2026-11-05.
Felix keeps quizzes, study sessions and review. Legacy interview database tables
and rows stay unused; old interview-style chats open normally and retired cards
show a short removal message. Downloaded runtimes stay
on disk, untouched.

Felix has no modes (2026-11-05, the user's call): one prompt, and he works out
from each message whether to answer, teach or quiz. Lessons follow the method of
LearnHarness (measure, plan, teach step by step, practise), written in leo's own
words: LearnHarness is AGPL-3.0 and leo is MIT, so its code and prompt text must
never be copied in. See crates/leo-web/CLAUDE.md.

Keeping expiry runs on blocking workers so filesystem walks do not hold the
notes lock. The `Housekeeper` in `crates/leo-web/src/record.rs` lists and
discards unfinished recordings and expires audio kept from saved sessions and
old browser capture. A recovery job carries its session id, so Delete and
expiry act on the recording being recovered, never on the job's own id.

Security cleanup: note roots refuse symlinks, folder scans have depth bounds, and displayed short IDs use
Unicode characters. Credential updates use a bounded writer plus a cross-process private lock;
use `secret::default_store()` (private file by default, optional keychain), never a
backend directly. `.superpowers/` stays local
and must never be staged or committed.

Settings uses grouped `#/settings/<section>` routes in a sidebar with icons (a rounded panel set apart from the explorer; panel and section are centered together), Trash lives there too (Data), and on/off settings are switches, short choices sliding segmented controls.
`leo_web::app_settings::AppApi` bridges native preferences, permission requests,
Health and signed updates; web depends only on core. Core's activity gate holds
recording, dictation and clipboard restoration leases and excludes installation.
Health uses local doctor checks without microphone or AI requests. First-launch
setup groups microphone and macOS Accessibility on one page with live checkmarks,
then offers a speech model (Download and use finishes the step), then a writing AI
from what is on this computer (`GET /api/writing?probe=signin`, the only place
setup runs the sign-in check; Use saves the provider as Settings does); existing
completion flags remain compatible.
Desktop navigation uses an icon rail beside a folder-and-note explorer. Folder
labels expand the tree; individual note rows open their notes.

Desktop releases contain macOS arm64/x64 DMGs and signed app archives, Windows
x64 NSIS and a signed latest.json. `scripts/desktop_release.py` validates asset
sets and builds the updater manifest. `scripts/activate_app_installers.py` refuses
to replace main installers until a complete published app release exists; the
publish job then commits that cutover. Prepared installers live in
`scripts/install-app.{sh,ps1}`. Linux exits before writes. No installer downloads
models: first-launch setup owns that choice.

The CLI is bundled with the app; macOS links to it, Windows uses a UTF-8 cmd shim
that preserves its caller's code page. Uninstall validates bundle identity, refuses
active apps, removes owned login entries and links, keeps notes and unrelated
files, and defers locked Windows removal. Shell-profile edits are atomic and
retain permissions. Tests use scratch bundles and ad-hoc signatures, never the
real signing keychain. Four persistent signing secrets gate release before bump;
private values are generated outside the repository and added by the user.

Windows speech and Rust binaries share the dynamic CRT; desktop release bundles
include the required Visual C++ runtime beside the executables.
