# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture

`leo` is a desktop note app (Tauri, macOS and Windows) written in Rust, laid
out as a Cargo workspace. Each crate may depend only on the ones above it in this table, and
the compiler enforces it — do not add an upward dependency:

| Crate | Holds | Depends on |
|-------|-------|------------|
| `crates/leo-core` | `notes`, `store`, `filename`, `open` (opens a link with the system), `children` (stops a process tree, runs a bounded helper, lists the processes leo owns for Felix activity's Running now; `quiet_command` starts every helper program without a console window on Windows), `diag`, `keep` (retention choices), `paths` (and `own`, the names of every file leo writes in its folders), `recording`, `activity` | — |
| `crates/leo-services` | `ai/` (chains, prompts, live transcription), `config/` (providers, credentials), `health`, `providers`, `speech`, `dictation`, `session`, `update`, `usage`, `import` | core |
| `crates/leo-web` | leo's engine and pages: `lib.rs` (`start` → `Running`, router), `routes/` (handlers by area), `token.rs`, and the page in `src/web/` (`page-head.html` + `styles/*.css` + `page-tail.html` joined at compile time, `app/*.js` served as `/app.js`, `markdown.js`, …) | core |
| `crates/leo-host` | `powers()` (the AI, speech, settings, recording housekeeping and import powers handed to the engine) and `leo-test-server` (Playwright's engine) | core, services, web |
| `crates/leo-desktop` | The Tauri app `leo-app`: window, menu bar icon, single instance, open at login, PATH from the login shell, links/downloads/print | core, services, host, web |
| root `leo` | `main.rs` and `cli/`: `leo` opens the app, `leo uninstall`, `--version` | core, services |

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
and weekly/manual runs test everything; failed and cancelled pushes never
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
`leo` (opens the app), `leo uninstall` and `--version`; `leo update` was removed
on 2026-11-17 (the user's call: "get rid of leo update terminal commands only
keep leo uninstall", and "yes keep leo and leo --version"), so updates come only
from the app's own signed updater. Plans and
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
`body` (Markdown), `directory` (empty string = root), `created_at`, and
`updated_at`; `extra` keeps the front matter leo does not own, `tags` included.

`config.toml` lives in `<config_dir>/leo/`, separately from notes. Machine-local
settings and credentials are excluded from note exports.

### Recent additions worth knowing

- `leo_services::doctor::scan` (sections leo/notes/AI/recording; local checks
  only, no network, microphone or AI probes, and the session's recent store
  warnings (`diag::recent`)) is shown in Settings → Health.
- Recording loop: `leo_services::session::recorder` (`record`, `resume`,
  `finish`, `write_up`, `Event`); `Capture` takes native microphone/screen audio.
- `Store::unreadable`: files that failed to parse; `save` never deletes them.
  Leo removes its own temporary files older than an hour: in the notes folders at
  load, in every other folder it writes at launch (`paths::sweep_stale_temps`).
- Trash: `save` moves any note file that vanished (and was not just moved:
  its id is not live) to `<notes>/.trash/<same relative path>`, mtime = when
  deleted. Load and save drop an identical trash copy of a live note. The idle
  Keeping pass removes readable notes past the Keeping choice
  (`keep::DEFAULT_TRASH_DAYS`, 30) after a complete scan. Divergent copies stay.
  Trash records digests at entry and checks them before unlinking. A trashed copy of a
  live note is hidden, and its trash clock starts only when it can first be shown. Empty trash
  and Delete for good skip a file they cannot verify and report it; leo keeps an index of the trash (device-local `trash.json` under `<data>/.leo-keeping/`), checked against each file's size and times; the list opens only new or changed files, and every deletion checks the trash first; trashed files leo cannot read are listed by file name with Delete for good only. The Trash page restores and empties.
- Dependents (2026-11-17, the owner's call): what belongs to a note (pictures by
  link, original files, recording sources, task details, margin rows, reading place,
  read copies, meaning vectors and summaries) stays while it is in the trash, comes
  back with it, and goes when it is deleted for good. Nothing leo removes on its own
  goes in one step (2026-11-18): a picture nothing names (no note, trashed note, card,
  board, task, chat, recording or other text file, however the name is written) is
  moved to `<notes>/.trash/attachments/` after the trash period and deleted a trash
  period later, and a link to it brings it back; a note that expires from the trash or
  has been gone outside leo for seven days (only a note leo saw leave, while its folder
  still holds its other notes on the same disk; a renamed headerless file is the same
  note) has its files moved to `<data>/held/` and its rows marked, deleted a trash
  period later, and given back if the note returns. Periods count only time leo saw
  pass after entry (one monotonic clock). An unreadable or unknown Keeping choice
  stops expiry. Outside removals use the volume seen while the note was present,
  and unknown volumes never qualify. Delete for good and Empty trash delete at
  once. A picture is leo's while it matches the
  SHA-256 digest (with size and time) recorded when leo saved it (`<notes>/.leo/pictures-made`); older ones
  are listed on the Trash page as "Pictures no note uses", deleted only when asked.
  Picture uploads check the attachment folder again before publishing, and record
  metadata from the published file handle. The ledger stays under the resolved
  notes folder and opens without following a link. When leo is not sure, it keeps
  the file. Counts and clock live beside the local trash index. A hidden copy goes only while the verified live file still has the same stamp.
- Updates: the app's signed updater is the only update path
  (`crates/leo-desktop/src/updates.rs`; `leo update` removed 2026-11-17).
  `LEO_NO_UPDATE_CHECK=1` disables its automatic check (e2e and the browser
  tests set it). The install scripts take `LEO_INSTALL_DIR`, recorded in private
  atomic `app-cli-dir` outside the app so `leo uninstall` finds a custom shim
  directory, and `LEO_INSTALL_SKIP_PATH`; an older copy's `leo update` still
  runs the latest release's install script. `scripts/install-app.ps1` keeps its
  `LEO_UPDATE_PARENT_PID` wait (2026-11-17, the orchestrator's call under the
  owner's principles): Windows builds from before that date still set it from
  their own `leo update`, and it can go once no supported build sets it.
- Note files are `<title>.md` (`filename::file_name`: FS-illegal chars to
  `-`, ≤100 chars and ≤200 bytes; same title in a folder → `Title (2).md`,
  oldest first, compared case-insensitively; `Store::assign_paths`). Windows
  device names get a dash (`CON-.md`, `filename::reserved_on_windows`) on every
  platform, since notes sync to Windows; new folders must pass
  `filename::portable_folder`. Old
  `<id>.md` files are renamed on the next save. A file name that is neither an
  id nor its title's name becomes the title on load (Obsidian rename).
  When two files share an id, the one still named after its header title
  keeps it; the other gets a new id, written on the next save.
- Headerless `.md` files load as notes: title = stem, id = `stable_id(path)`
  (FNV-1a 128 → UUID v8, golden-tested: never change it), dates = mtime. `Note.extra` keeps unknown front matter, `tags` included,
  as values in file order (a note leo edits is written back in leo's layout).
  Note constructors take title, body and folder. Old browser requests may still
  send tags, which the engine ignores and leaves as frontmatter.
- `Store.seen: id → Seen { path, disk, model }`: save writes only notes whose
  rendered hash changed; if the file on disk no longer matches `disk`
  (edited/renamed/deleted elsewhere) and leo changed the note too, it writes
  `<title> (conflict from leo)` with a new id (one copy per note, rewritten
  by later saves until the store reloads; the outside edit is then loaded)
  (`CONFLICT_SUFFIX`, listed by doctor) instead of overwriting; deleted notes are trashed only if their file
  is still what leo saw. Writes are temp-file + rename.
- `Store::changed_on_disk`/`refresh` (fingerprint of paths, sizes, mtimes);
  refresh re-reads only files whose size, modified or change time changed, or
  whose note differs in memory; the result equals a fresh load.
  `Store::changes_since(seen)` hands readers the ids changed since their last
  look, or `Everything`.
  A save never absorbs an unread outside edit (its fingerprint moves only by the
  files it wrote); `store_now` reconciles first, other mutations when the watcher
  reported a change, the 60 s check is due, there is no watcher, or on Windows
  (2026-11-17, the owner's call: the most efficient and scalable method); hot read-only
  paths use the existing filesystem watcher with a 60 s reconciliation fallback.
  Returning to the native window forces a check before serving cached reads.
- Never slice strings by byte offsets computed from lengths: use char indices
  or `str::get`.
- Tests must never open the microphone: doctor's recording section only reads the
  device name (`health::microphone_device`), and tests set `LEO_NO_MICROPHONE`.
- Install: `install.sh` downloads the latest GitHub release. The install scripts refuse a running leo before downloading, publish the command and record before swapping the app, and put back the app, command and record if any step fails before the install is complete; a failed PATH line is a warning. Releases are
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
  results are twenty local rows in `leo.db`, kept whole, any length (2026-11-17,
  principle 1; the list shows each row's first 2,000 characters and opens the whole
  text on demand), never notes or sync data; an edited
  row keeps its `original` text (Show original). Custom words fix spelling in
  `dictation/text.rs` and, where the speech model supports it, are passed to it
  as vocabulary (a recited-list echo is stripped); the AI check refuses changed
  words, numbers and added symbols; filler rules follow a `whatlang` guess of the
  text. The echo filter ports OpenWhispr's `dictionaryEchoFilter.js` and the
  Recent-dictations everyday-word list its `correctionLearner.js` (MIT, credited
  in NOTICE); the self-correction rules are leo's own. Robustness details:
  leo-desktop, leo-services and leo-web CLAUDE.md "Dictation".
- Recording is captured in Rust, including two native tracks for calls; hiding
  the window cannot stop it. Recordings can label who spoke (Record page
  switch, per recording; on a call only the Others track, as Others 1, 2…;
  a call app's name wins; 2026-11-22, the owner's call; leo-services CLAUDE.md
  "Speaker labels"). A call's You lines that are the other side's echo are
  marked `echo` (leo-services CLAUDE.md "Call echo"). App Nap is held while recording. Missing models
  leave audio pending and recoverable; the app can resume native sessions.
  Screenshots are pasted into the Record page with their timestamp.
- Call speaker names (2026-11-22): while a Call records, the Others track reads
  the call app's window every 400 ms (`leo_core::call_names`: Zoom, Teams, Meet
  in a browser; macOS Accessibility, Windows UI Automation, in-process readers in
  `leo-desktop/src/call_names.rs` handed down through `leo_host::listener`),
  bound to the one call it finds first. A line of the other side is named only
  when one person other than the user was marked talking for more than half of
  its spoken span, or the call has exactly one other person by the count the app
  shows; anything else stays "Others". Never guessed, never the user's own name (as the call app marks it, or the computer account's full name), nothing leaves the
  computer (the account's full name only vetoes; the one-other-person rule needs
  the call app's own marker of the user); tests use fixtures and fake readers. The name lands in `speaker`.
  Credited in NOTICE (its name appears only there). Meet, measured on a live call of
  one on 2026-11-24 (`fixtures/meet-live-*.json`, names invented, the second person
  and the talking class added by hand): the user's tile carries their Meet name, not
  "You", and is told by its own controls ("Backgrounds and effects", English) or the
  People panel's "(You)" row; "Contributors N" is the count. Whether the talking class
  still marks the speaker is not yet seen on a call of two. FaceTime is not read.
  Chromium freezes the tree of a hidden window, mark and all, so a hidden window (or
  a locked screen) is not read and a Meet or Teams mark that stays on through 3 s
  without sound (`echo::sound`, the track's own levels) names only the line it came
  with (`doubt_marks_through_silence`). A Meet line is named only once the user's tile
  is known, a mark on a tile whose name cannot be read names nobody, and a line under
  1.2 s is named only well inside one person's marks.
- Windows child programs stay quiet. Model and toolchain caches stay under the
  existing models directory. Platform installers and release bundles are
  described by their scripts and workflow, and uninstall retains the existing
  data-removal controls.
- Note connections (cache code still says `graph`): see crates/leo-web/CLAUDE.md.
- Tasks (`<notes>/.leo/tasks/`), boards and cards (`.leo/boards/`, `.leo/cards/`)
  are one-JSON-line-per-save files (newest line wins, tombstone deletes); boards
  are not calendar tasks. Calendar, boards and their Felix tools: see
  crates/leo-web/CLAUDE.md.
- Tags (2026-11-17, the owner's call, replacing the 2026-10-08 line): leo does
  not read, filter or search tags. A `tags:` entry in a file is kept like every
  front-matter property leo does not own; `#word` searches the word.
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
  stored cloud speech key retirement, the old Google calendar sign-in clean-up).

Recording sources live in `leo-core::recording` (`Archive`, and `Profile {context,
wants}`: calendar-event details and what the user wants from the notes), shared by
the app, kept per recording as a small header and one-per-line passage and point
files. Combining notes keeps the full source label on copied recordings
(`recording::copy_to`). Recordings are listed by header and read a page at a
time, with no limit on their number or size (an older single-file recording is read as it is and rewritten in the new
form, keeping it as `<id>.original`, the first time it is fixed or added to). Saved recording session IDs make new notes and appends idempotent;
retain sources before saving the note, then commit the session after the note is
durable. Transcript reads leave the files alone. A revision writer publishes
line files without clobbering and only removes superseded lines whose bytes
match the digests recorded when leo wrote them. Default AI gap-filling and
merging of typed points always stay, whatever the user asks for. Note workflows
(folder defaults, note formats, vocabulary, saved
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
discards unfinished recordings and expires audio kept from saved sessions; a kept
session names its note (`Manifest.note`), so its audio can go with that note.
A recovery job carries its session id, so Delete and
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
active apps, removes owned login entries and links, runs every step, removes the
app last and only when every other step worked, and lists what it could not do;
it keeps notes, their original
files and recording transcripts, and unrelated files; in every folder, `leo` ones
included, it removes only the names in `leo_core::paths::own` (a `leo` folder goes
whole only when it holds nothing else): what today's code writes,
plus a legacy list of what earlier versions wrote, which only ever shrinks
(2026-11-17: what a kept note depends on is kept, and a service removes exactly
what leo manages, including what it managed before; `tests/owned_files.rs` fails
when code writes a bare name not listed there). Recordings that belong to kept or
trashed notes, saved audio awaiting recovery and unfinished recordings all stay
in `recordings/`. Held originals and
transcripts stay in both default and custom data folders. Uninstall defers locked
Windows removal, writing its output to `leo-uninstall.log` in the temp folder. Shell-profile edits are atomic and
retain permissions. Tests use scratch bundles and ad-hoc signatures, never the
real signing keychain. Four persistent signing secrets gate release before bump;
private values are generated outside the repository and added by the user.

Windows speech and Rust binaries share the dynamic CRT; desktop release bundles
include the required Visual C++ runtime beside the executables.
