# leo-web: leo's engine and the page

leo-web depends only on leo-core. Everything that needs services (AI, speech,
settings, import, recording housekeeping) arrives as a `Powers` field built by
`leo_host::powers()` (writer, quick, toolless, chat, light_chat, lighter,
toolless_chat, settings, speech, ollama, dictation, app_settings, importer,
listener, reader, seer, meaning, room, …; see `Powers` in `lib.rs`).

This file holds the rules. How each screen behaves, area by area, is in
`docs/behaviour.md` (not auto-loaded): read the section for the area you change.

## Layout

- `lib.rs`: `Powers`, `AppState` (holds `powers: Powers` and derefs to it; plus
  `with_store`, `read_store`, `store_now`, `fresh`), `start` → `Running`,
  `start_in`. `router()` is in `routes/mod.rs`; handlers live in
  `routes/<area>.rs`; items are `pub(crate)` only when another file uses them.
  Handler tests are in `src/tests/<area>.rs` (helpers `state_with`, `run`,
  `wait_for`, `json_of` in `src/tests/mod.rs`).
- The page is assembled at compile time in `routes/assets.rs` (`concat!`):
  `web/page-head.html`, then `web/styles/NN-*.css` in number order, then
  `web/page-tail.html`. There is no index.html. Node tests read the same page
  through `tests/page-source.js` (`pageHtml`, `pageCss`, every `styles/*.css`
  sorted), so a new stylesheet must also be added to the `HTML` list in assets.rs.
- `/app.js` is `web/app/*.js` joined in one closure (`APP_JS`). The parts share
  scope on purpose (pages reassign `state`). Add a new part to the list:
  `every_part_of_the_page_script_is_served` fails otherwise and
  `the_page_script_parses` runs `node --check` on the whole.
- Pure helpers are separate files served on their own and node-tested in
  `tests/*.test.js`: `markdown.js`, `editing.js`, `schedule.js`, `saving.js`,
  `recording.js`, `margin.js`. `chat.js`, `doc.js`, `pane-resize.js`, `prefs.js`
  and `theme.js` are separate scripts too.
- Everything leo-web computes or keeps besides notes and uploads lives in one
  SQLite file, `<data>/leo.db` (`db.rs`; rusqlite bundled, WAL, 0600, never
  synced): chats (+ FTS5), chat documents, review progress, picture captions,
  meaning vectors, note connections, calendar feeds, note-task metadata,
  reading positions, margin threads, dictations, usage days, recordings whose
  notes are not written yet. Old caption, graph and meaning JSON files are
  brought in on first open and renamed `*.before-database` (browser tests seed
  note connections this way). Writes touch only what changed.

## Serving and security

- `start(StartOptions { port, token }, powers) -> Running` binds `127.0.0.1`
  only (the requested port, else the next free of 19), spawns the background
  tasks (graph, idle, Keeping expiry, calendar) and the server;
  `Running::{port, sign_in_url, stop}`. `stop` stops the background tasks first,
  drains requests and streams for at most `shutdown::GRACE` (5 s), then aborts
  the server; warm-session cleanup has `shutdown::CLEANUP` (1 s). Tests use
  `start_in(Places { store, sessions }, …)`. The app passes `fresh_token()`;
  `leo-test-server` uses `saved_token()`, the `<config>/serve-token` file the
  specs read.
- Only the app's window talks to the engine: `same_host` refuses any Host but
  `127.0.0.1:<port>`/`localhost:<port>` (421); `?token=` on GET `/` → 303 + a
  `leo_session` cookie (`SameSite=Strict`, 30 days, `serve-sessions.json`). No
  tunnel, Wi-Fi bind, QR, signed-in browsers list or link rotation.
- Page CSP: no inline JS (`script-src 'self'`), `data:` images, `frame-src
  'self'` (original viewer), `font-src 'self'` (KaTeX and the bundled
  fonts). Mermaid 11.4.1, KaTeX 0.16.11, CodeMirror parsers and the latin
  variable fonts Instrument Sans (note titles and text, `--writing-font`, roman and italic) and
  Newsreader (Felix's margin voice, `--voice`, roman and italic; SIL OFL,
  @fontsource-variable 5.3.0) are vendored under `web/vendor/` (served
  `immutable`; the fonts' npm tarball sha512 and per-file sha256 are recorded in
  NOTICE; the fonts at `/vendor/fonts-5.3.0/`, their
  `@font-face` in `page-head.html`). `margin-notes.js` redraws the margin once
  both families have loaded (Instrument Sans roman and italic, Newsreader roman and italic), since the swap moves measured text.
- Keys and calendar links are write-only: accepted from the window, never
  returned to the page or put in exports. A stored key shows as a
  `secret::masked` placeholder; nothing changes until Save.
- Every success is JSON or a bodiless 204 (`api()` treats an empty body as
  `null`; a browser test sweeps the API). Session-gated JSON inputs are bounded.

Decided: one app, window only (2026-10-31).

## Security and cleanup boundaries

- Sign-in files use unique synced 0600 temporary files and atomic replacement;
  loading rejects symbolic/hard links, nonregular files and oversized data and
  repairs permissions. A session persistence failure refuses authentication.
- Original attachments and the `jsonline.rs` files (tasks, boards, cards) check
  every path component inside their data root (`contained_path`), read with
  nofollow regular-file opens, write atomically and refuse symbolic links.
- Database handles are cached for up to 32 paths; filesystem checks and
  open/close happen outside the database cache mutex.
- Blocking work (native settings, CLI refreshes, model work, filesystem walks,
  Keeping expiry) runs off async request workers and outside the store lock.
- Point and screenshot workers keep their save guard through I/O and the
  in-memory acknowledgement even if the HTTP client leaves.
- Recording warnings keep at most 40 entries of 2,000 Unicode characters.
- Keeping's expiry refuses linked recording and originals roots; every pass is
  bounded, never follows symlinks, stays in its folder and skips what it cannot
  delete.
- Credentials never reach the page or note exports. Writing Settings offers
  Ollama, Codex and Claude Code only; removed AGY/Copilot choices and sign-in
  commands are unavailable, legacy saved profiles stay on disk untouched.
  Speech is local only; there are no speech key controls.
- The test-only `LEO_TEST_FAKE_AI` is read by `leo-test-server` only, never the app.

## Page architecture

- Hash router (`#/`, `#/f/dir`, `#/n/id`, `#/new/dir`, `#/search/q`, `#/trash`,
  `#/calendar`, `#/boards[/id]`, `#/record[/dir]`, `#/settings[/<section>]`),
  `data-action` delegation (`actions.js`). Every page bumps `seq`; a slow load
  whose `seq` changed draws nothing. Router completion and errors check the
  render generation and current hash, so late failures never redirect, toast or
  overwrite a newer page (also on a return to the same route).
- Arriving on a page closes sheets left from before but keeps one opened on the
  new address (`scrim.openedOn`).
- Narrow windows (600–999 px, 2026-11-14, the user's call): the icon rail stays;
  the sidebar button opens the explorer as a drawer over the page (`side-drawer`;
  Esc, a click outside or opening a note closes it; focus goes to Filter and back).
  The More sheet (`#menu`) is for phones only (under 600 px).
- Layout variables on `body`: `--side`, `--right`, `--mid`. Fixed things
  (toolbar, select bar, toasts, FABs, `#dock`) use them so nothing covers
  anything. Page toolbars share one 760 px maximum width.
- UI preferences survive a port change (2026-11-14): localStorage is per origin and
  the engine port drifts when the saved one is busy, so every `leo-*` key (not
  `leo-draft*`, `leo-chat-v1` or `leo-search`: content, not settings) is mirrored in `<data>/ui-prefs.json` (`prefs.rs`: keys
  `^leo-[a-z0-9-]{1,48}$`, 16 KB per value, 128 KB total, temp file + rename,
  merged per key under a lock). `/prefs.js` (behind the session gate, `no-store`,
  loads before `/theme.js`) seeds localStorage from the file (the file's keys win; keys it lacks are
  left alone; an empty file makes the page upload its keys once; an oversize
  value removes the stored copy; a failed PUT is queued again), patches `Storage` `setItem`
  and `removeItem` to send a debounced `PUT /api/ui-prefs` and flushes on
  `pagehide` and hidden. `LEO_NO_UI_PREFS=1` turns it off (the browser tests'
  shared server sets it so one test's choices never reach the next;
  `prefs.spec.js` starts its own engine without it).
- Theme: `/theme.js` loads in `<head>` (no inline script under the CSP) and sets
  `html[data-theme]` and the other per-browser `data-*` before paint. Anything
  drawn in JavaScript asks `leoTheme.dark()` and redraws on `leoTheme.onChange`,
  never the media query directly. Print overrides with `:root:root`, so a PDF is
  always light; the outline never prints.
- Selection: `body` is `user-select: none`; the allow list in
  `web/styles/02-base.css` turns it back on for inputs, `.doc`, `.prose`, chat
  and margin text, errors, transcripts, descriptions and code. New text people
  would copy needs a class on that list; hints and card descriptions are chrome.
  Native drag starts only from explicit note/folder sources. Selected text is a
  warm ink wash (`::selection`, `--text` at 22%; leo sets no other selection
  colour). Ink details (2026-11-15): a Markdown `---` (`.prose hr`, also the
  doc's rule block) is a drawn pencil line (an SVG mask, plain 1 px `--line` in
  print and forced colours); scrollbars are thin and warm (`* { scrollbar-width:
  thin }`, `:root` `scrollbar-color`), and `html.page-bar-on` still hides the
  page's own bar for `.page-bar`. Focus rings are unchanged.
- Escape is always `preventDefault`ed in a capture listener, so it closes sheets
  and never leaves macOS full screen. Passive reveals never open a sheet.
- Shortcut labels use `ON_MAC` (Ctrl on Windows); monospace falls back to Consolas.
- Native quit and lifecycle events only flush drafts; they keep the page.

## Shared components (new UI must use these)

- A few options: `segmented()` (settings-navigation.js; `field` mode fires
  `change` on a hidden input, `action` mode makes `data-action` buttons with
  `aria-pressed`; a 4th choice item is the tooltip; `syncSegmented(box, from)`
  after a redraw). Picture choices: `choiceCards()` (same two modes; `columns`).
- On/off: `settingsSwitch()`; checkboxes only for task completion and picking
  several items (Trash). Task checks (`.prose .task input`, `.sched-check`,
  `.cal-check`) are real checkboxes drawn as a pencil box and tick (masks on
  `::before`/`::after`, four variants picked by the last digit of `data-box` or
  `data-id`; the tick takes `--tone`); dark mode breaks the tick with the static
  `--chalk-break` mask; forced colours get the native box.
- Dropdowns: a `<select>` inside `.select-control`; select-menu.js turns every
  one into the app's menu (not under forced colours), keeping id, value and
  `change`. Do not hand-build list menus.
- Text fields: `.field` (icon-led, sheets) or `.set-input` (Settings rows).
  Folder/Title/What do you want: `noteFields()` (uploads.js), shared by the
  upload sheet and Record page.
- Confirmations: `confirmSheet({ title, text, yes, action, data, danger, cancel })`
  for permanent losses only. Moving to the trash asks nothing and offers Undo for
  10 s. Removing a key or speech model is a two-row `popupMenu`.
- Menus: `popupMenu(owner, { label, rows, point, find })` is the one dropdown
  (a row's `next` is a second step). `sheet()` focuses the first control and
  traps Tab. Rename and New folder use `inlineEdit` over the tree row.
- Empty pages `empty(icon, title, text)`; an empty state with nothing to do yet
  (Trash, Boards, calendar Tasks, Chats, Search no-results, home) takes one
  single-stroke line drawing from `DRAWN` (app/core.js; `fill="none"`,
  `currentColor`, shown in `--muted`, no colour) in place of the icon; chat.js
  gets them as `drawings`. Home's arrow points at the explorer and shows only
  while it is open (≥1000 px, not `side-mini`). Titles stay in the app's sans
  (2026-11-15, the user's call); counts `.badge`; small faint
  sentence-case labels share one rule (`.side-group`, `.source-label`, `.quick-due-head`,
  `.chat-history-group`, `.pick-head`).
- Kept on purpose: the model picker, the margin-mode menu and chat.js's attach
  and access menus (they move through `leoMotion`).
- The leo mark (`#leo-mark`) is the one working mark (2026-11-15): it breathes
  (`margin-breathe`) only on `.margin-drop` while Felix reads or thinks, and sits
  still in the Reads along chip, at the start of the calendar now-line (in place of
  its dot) and in `#save-state` (full ink while editing or saving, drying once to .45
  when Saved; the words stay as hidden text, "Not saved" stays visible). No other
  spinner or second breathing mark.
- Colour: ink for actions and selection, `--focus` for focus cues, tone only as
  dots and ticks (calendar events keep their colour fills). One exception,
  Felix's own blue (`--felix`: light #3f6d97, dark #b4cfe7, from his sprite;
  2026-11-15, the user's call), only while he works: the reading blot
  (`.margin-waiting` or the gutter `.margin-mark.reading`) and pencil
  (`path.pencil`), the caret after a
  streaming reply (`.msg.pending`) and the dot on his button
  (`#chat-toggle.working`, synced with the chat body's `aria-busy`). Never links,
  buttons, selection, tags or text.
- Surfaces (2026-11-14, the user's call: modern, no outlines): separate by tone
  (`--ground` < `--page` < `--raised`) and warm two-layer shadows (`--shadow` at
  rest, `--shadow-pop` for menus, toasts and sheets, `--shadow-lift` while
  dragging); dark mode lifts with lighter surfaces and a faint top highlight.
  Inputs, tables, code and focus rings keep a clear edge; so do board lanes, one
  step off the board's tone with a 1px inset edge (they vanish otherwise).

Decided: reuse components throughout (2026-11-06).

## Motion rules (the single place for these)

Quick and caused: nothing moves without a reason, nothing bounces on everyday UI
(Felix the mascot keeps his springs; his cameos sit on the chat composer and
Quick Leo only, 50 px, whole pixels, never on home), nothing tweens while held. Every scripted
animation is skipped under `stillMotion()`; CSS motion sits under
`prefers-reduced-motion: no-preference` or the global reduce rule.

- Tokens (`web/styles/01-tokens.css`) and script twins (`app/core.js`):
  `--ease-arrive`/`EASE_OUT` entrances, `--ease-move`/`EASE_MOVE` things that
  stay and change position, `--ease-exit`/`EASE_EXIT` exits, `--ease-drawer` the
  chat panel. Durations `--t-press` 100, `--t-follow`/`MOTION_MS` 180,
  `--t-open`/`OPEN_MS` 200, `--t-drawer`/`DRAWER_MS` 240. An exit is its entrance
  × 0.8 (`--t-*-out`, `EXIT_SHARE`, `LEAVE_MS`). Drop-downs run 150–250 ms. A
  symmetric position change is a move, not an exit.
- chat.js, doc.js, recording.js and pane-resize.js reach the helpers through
  `window.leoMotion` (absent in some fixtures: every call has a plain fallback);
  doc.js reads the curve from the CSS token.
- Press: pressables scale to .97 with the individual `scale` property (one
  grouped rule at the end of the styles), never draggable rows, `.actions`
  tooltips or disabled controls.
- Knobs `knobStretch(el, pseudo)` (12% along the travel, on `scale`, ends at 1).
  Popovers `unfold(menu, from)` / `fold(menu, then)` (`popEdge` origin, start at
  .95, clip reveal; reversing mid-run starts from the live clip, opacity and
  transform, never a snap; a closing context menu takes `.popup-menu-gone` and
  `aria-hidden`). Stagger `cascade(list)` 30 ms (cap 150) on first arrival only,
  never a re-render. Moves `flipList`, `pillGlide`, `settle` (arc; lifts onto a
  fixed carrier when clipped), `flyInto`. Pulse `pulse(el, { lift, ring })`, one
  320 ms, no repeats. Overlap `overlapOut(el)` (the clone keeps its classes:
  tests scope lookups to `#app`). Colour arrives with the thumb or as a wipe,
  never on hover; `iconSwap`. Rubber band `rubber`, `pastEdge`, no overshoot.
- Board lanes and cards and the Chats list only fade (transforms break
  `kanbanFit` and a scrolled list's `scrollTop`).
- Two signatures (2026-11-15): write-on, the task tick wipes in over 160 ms
  (`--ease-pencil`, a transition, so a redraw never replays it; a box redrawn
  inside the 160 ms carries `data-writing` and `--tick-at` from `tickWrite` and
  resumes with `tick-write`) and today's ring
  draws once over 300 ms with `pencil-draw` when the Calendar opens
  (`cal.ringAt`; redraws inside the 300 ms resume it with a negative delay,
  later ones draw the end frame); ink settles, `inkLand(id)` when leo creates a
  card, task or note and `inkSettle(find)` after each redraw: the row starts in
  `--text` with a 12% ink wash and fades to its own colours over 600 ms
  (`INK_MS`, colour only, resumed by elapsed time). Explorer notes count as new
  when a refresh brings an id created in the last minute. The explorer fold is the one
  width animation (`--side` as an `@property`).
- Cancelling a menu animation clears closing state synchronously and detaches its
  old cancellation callback before a new animation owns that state.

Decided: the owner's motion list (2026-11-06); only quick functional motion.

## Notes, editing and browsing

- No editor view: `editing.js` splits the body into blocks, `doc.js` renders
  each with `markdown.js` and swaps a tapped block for a textarea of its raw
  lines. The file stays plain Markdown: never emoji (icons are `:name:` codes,
  `NOTE_ICONS`), underline only as the exact `<u>…</u>` pair (every other tag
  stays text), picture widths Obsidian-style (`![alt|400](path)`,
  `![[file|400]]`, 16–4000 px). Nesting and list styles change only the page,
  never the file.
- `markdown.js` escapes first and numbers checkboxes exactly like
  `Note::toggle_checkbox`; `MARKER` is the one list-marker definition and
  `listLevels` the one forward pass; leo-core's `Note::checkbox_line` stays
  context-free and counts the same lines (a box always makes an item).
  `interactive: false` makes boxes read-only. ```svg is a sandboxed
  `<img src="data:…">`; outside images stay links.
- Caret/selection conversion uses UTF-16 offsets throughout. A box toggles only
  from a click within `DRAG_CLICK_DISTANCE`. A Tab/Shift+Tab that changes
  nothing writes nothing. macOS `--` replacement corrupts text, so `look` turns
  spellcheck off for tables, code, math and lines starting with `-`, `|` or `:`.
- Saving: autosave PATCHes 700 ms after the last change with `base` = the loaded
  `version` (FNV-1a); 409 → "<title> (conflict from leo)". `saving.js` keeps
  edits that did not reach leo and retries them on the next load; saving admits
  bounded Unicode JSON bodies. Up to
  `MOST_UNDO` page undo snapshots per note. Navigation serialises the departing
  save; a failed save keeps the editor and address; only a draft persisted in
  this browser may offer Leave, draft kept (rechecked at activation).
- Pictures: `<notes>/attachments/` (`leo_core::attachments`, sniffed
  PNG/JPEG/GIF/WebP only, ≤20 MB). Upload names (`safe_file_name`) drop
  trailing dots/spaces and dash Windows device names (`CON-.pdf`); older stored
  names still open (`stored_file_name`).
- Browsing never calls an AI. The explorer is the only folder listing: `#/` is
  an empty workspace, legacy `#/f/<path>` selects the folder then replaces the
  hash with `#/`. Explorer actions capture their target before opening a menu
  or sheet; moving or trashing the open note's folder flushes its draft first.
  Never two rows highlighted. Saved open folders (`leo-side-folders`) hold only
  folders the user opened; reveals (`side.revealed`) are never saved.
  Beside Filter: a sort menu (Name, Last edited, Created; only open tasks) and a
  flip button that reverses it (its icon shows the order: a down arrow beside
  A–Z/Z–A letters or wide/narrow bars, 2026-11-12), both driving `/api/notes` `sort`/`show` for every
  folder's notes (folders stay by name), saved as `leo-side-order` (2026-11-11).
- Quick Leo opens with Cmd/Ctrl+K only (Cmd/Ctrl+F removed 2026-11-09); Cmd/Ctrl+N
  makes a note in the current folder, never inside a sheet, an inline edit, while
  composing or when locked. These two and Ask Felix (Cmd/Ctrl+Shift+E) are the
  defaults of three customizable page shortcuts (Settings → Shortcuts): `SHORTCUTS`
  in `core.js`, saved per window as `leo-shortcuts` in localStorage (like the
  appearance keys), read with `shortcutKeys`/`pressed`, and shown through
  `shortcutText`. Every label is built at draw time (the New note button titles and
  the selection toolbar's Ask Felix title carry theirs). While one is being recorded a
  window capture-phase keydown listener swallows every key; reserved keys
  (Cmd/Ctrl + q w r c v x z y a s b i u `,` and Shift+Z), another page shortcut and
  the dictation shortcut are refused with the reason.
  `leo-last-place` restores only on a new window session.
- Explorer reads: 100 per page, four concurrent reads with ten-second aborts, 64
  cached folders, 32 automatic visible reads; late reads check generation and
  entry identity.
- Folders: create, rename, move, trash with Undo (POST `/api/trash/move`,
  `/api/trash/restore {ids, dirs}`, `/api/dirs/rename`); `Store::move_dir`
  refuses itself, its own subfolder or a taken name. New folders must pass
  `filename::portable_folder`; slashes are valid in titles, never folder names.
- Combine (`combine.rs`): POST `combine` starts a background job
  (`AppState.combines`, at most 20, kept 6 h, metered `combine:<id>`); review
  saves through version-checked POST `combined` or DELETE `/api/combines/{job}`;
  Undo is POST `uncombine`. The AI writes one note, leo puts back anything it
  dropped. Source parts are sized by the provider's graph budget (≤20,000
  characters per source, 40,000 per request, ≤480,000 input, 128 requests);
  fenced code and display maths stay whole; a closing fence needs the opener's
  character and at least its length with no trailing text. Two combines at once,
  15-minute between-request deadline; any failed part leaves both notes
  unchanged. Saving copies the other note's transcripts (`recording::copy_to`).
  In the explorer a note dropped on a note moves it unless held on that row for
  `SPRING_MS` (arms Combine: `.combine-wait`, then `.combine-armed`; the folder
  is not lit); the note menu's Combine into… (`explorerCombineMenu`) lists at
  most 1000 notes (2026-11-09).
- Search (`search.rs`): titles and acronyms, text, graph concepts and summaries,
  then meaning (`vectors.rs`, `CLOSE_ENOUGH`); hits carry `why`. Reading
  position `/api/notes/{id}/reading`; related notes `/api/notes/{id}/related`.
- Nothing reflows the writing column: the outline gutter and the margin are
  reserved before the first heading or comment exists.
- Outline: at most 300 headings (excluding code, maths, quotes, tables); preview
  ≤24 lines / 800 UTF-16 units, escaped; preview updates never redraw the editor.

Decided: Obsidian-style live preview, nothing jumps when a line opens
(2026-11-02); `/` quick menu writes plain Markdown (2026-11-09); tags hidden on
the web (root CLAUDE.md).

## Felix (`chat.rs`, `routes/felix.rs`, `tools.rs`, `routing.rs`, `web/chat.js`)

- POST `/api/chat` streams NDJSON (`answer`, `sources`, `t`, `step`,
  `proposal`, `ask`, `quiz`, `plan`, `spent`, `reset`, `done`/`error`). User
  prompt order: `<selected_passage>`, `<documents>`, notes, the chat, then
  `<today>` (`chat::today_line`). Notes and documents sit in tags that end in a
  random per-run suffix (`chat::fence`), so their text cannot forge a closing
  tag, and tool results cannot close their own `<tool_result>` tag either.
- One Felix, no modes: `HELPING` makes him decide per message whether to answer,
  teach or test; an open note alone never starts a session. `LEARNING` is not in
  the system prompt: Felix calls `study_rules`, and practice, marking and a
  running session send `<study_rules>` (`Reads.study_rules` sends it once). The
  `chats.mode` column stays for old rows; request and saved `mode` are ignored.
- LearnHarness is AGPL-3.0 and leo is MIT: borrow its method only in our own
  words, never paste its prompts or code.
- Warm sessions (`warm.rs`): up to three, sharing a 480,000-character ceiling,
  ten idle minutes, Settings changes clear all; eviction and process cleanup run
  outside the slot mutex and off async workers. Only unchanged history plus the
  last reply/new message can continue. Automatic note retrieval is separate from
  warm capacity: reducing excerpts must not evict a long conversation early.
- Idle compaction: after `COMPACT_IDLE_MS` (10 min) the page POSTs
  `/api/chat/compact`; with at least `COMPACT_LEAST_CHARS` (3,000) before the
  last `chat::COMPACT_KEEP` (4) turns the server drops the warm session and
  summarises in the background (`kept_from`, hash-checked). The chat on screen
  is unchanged.
- Long chats: `turns_for(room)`; the message being answered and the newest long
  earlier message keep up to `MESSAGE_CHARS` (32,000), Felix's newest reply
  `LAST_REPLY_CHARS` (12,000), full at `FULL_ROOM` (32,000), never below
  `TURN_CHARS` (4,000). Warm follow-ups and steering (`steer::MOST_CHARS`) use the
  same cap. `clip_turn` closes a fence it cut and says what was left out.
- Tools: a text protocol (`<tool>{…}</tool>`) for every provider plus native
  sessions (Claude Code MCP bridge, isolation-capable Codex, compatible Ollama),
  all generated from `tools::SPECS` and `check`ed. At most `MOST_STEPS` (12)
  steps and `MOST_PROPOSALS` (8) changes (every change tool counts). The text
  manual stays under 14,500 bytes; manuals keep every parameter/return contract,
  and all permitted tools stay discoverable (no keyword filtering). Broken JSON
  is repaired when it can be (`tools::repaired`) before refusal, then
  `Call::normalized` repairs what keeps its meaning (tool-name case, `options`
  joined with ` | `, quiz kind synonyms, a multiple-choice answer matched to
  its option, fewer than 2 options as free response, a fill-in blanked on a
  whole-word match); it never adds an option, a no-match answer stays refused,
  `quiz_from` stays strict (missing explain, give-away longest option), a
  second question in one reply or after a finished session gets an empty step
  that is never sent, and a covered idea stays refused. On the page a failed
  step is hidden once the same tool (quiz and ask_user count as one) later
  succeeds (`fixedSteps`). While Felix works the reply shows only his newest
  step, in one `.msg-status` row at the top of the reply. A broken agent
  session sends `{"reset"}` and is redone through the text protocol
  (`FALLBACK_TRIES`). Felix has no web tool of leo's own.
- Panel layout: docked beside the note (1200 px and up) the note's page ends at
  the panel with an 8 px `--ground` strip between them. Attached notes, passages
  and files stay above the composer (`#chat-refs`; files are 52 px tiles, notes
  `noteCard`s); the note finder (`#chat-pick`) is a popover at the paperclip and
  never pushes the layout (2026-11-09).
- Change tools become `Proposal` cards, validate against the store, run through
  POST `/api/felix/act` (reusing normal routes) and return the inverse as `undo`.
  edit_note/create_note Apply POSTs `/api/notes/{id}/suggestion` (409 unless
  `find` is there exactly once). delete_folder and delete_board are always a
  card, even in Auto. add_margin_note's quote must be in the note and it never
  changes the note. `open_note` reads bounded `part`s that preserve every
  Unicode character.
- Successful replies may contain only proposal, ask or quiz cards. Stopped means
  cancelled; saved replies keep elapsed ms and a cancellation flag.
- Quizzes: marking never waits for a Felix reply. Multiple choice (needs
  `explain`) and matching fill-ins are marked by the page; the rest POST
  `/api/quiz/mark` (quick model, `MARK`; `confidence` validated, `thinking`
  bounded to `MOST_THINKING`). How sure is a 0–100% bar (`q.surePct`; `sureOf`
  maps ≥75 sure, ≥40 think, else guess for `SURE`); thinking is sent only while
  its switch is on (2026-11-09). Hidden grading accepts only the exact verdict
  words; a hidden check is authoritative for the card; an invalid check leaves
  the answer ungraded and releases the warm session. Predictions (`predict:
  yes`) are never marked, scored or counted. `quiz_from` shuffles options and
  refuses `gives_itself_away`. Sessions stop at `MOST_SESSION_QUESTIONS`; review
  (`review.rs`) brings misses back after 20 h. The `aside` answer line must stay
  one line.
- Routing (`routing::light`): only obvious cases (short lookups and greetings,
  no attachments, files, selection, study or Auto access, ≤6 turns) go to the
  quick tier, other languages stay on the chosen model; a chat moves up once when needed, never down while warm
  (`Slot::light`); a failing light writer gives way on the last try, so `Auto`
  names who really answered. `Prefs.auto_model` turns it off.
- Copy/Retry/Edit drop the last pair with POST `/api/chats/{id}/drop`
  `{count, total}` (409 unless the saved chat has `total` messages).
- Files for Felix (`chat_files`): ≤10 per chat, text only, deleted with the
  chat. Add to note (`routes/insert.rs`) is version-checked and request ids
  dedupe. Chat titles are local; history shows titles only.
- Felix draws only when asked to see, draw or visualize something (2026-11-14,
  replacing the 2026-11-09 threshold): one stepped ```svg (viewBox 320 wide,
  `var(--text|muted|accent|accent-soft)`, top-level `<g data-step data-caption>`),
  never Mermaid or text characters; margin reads never draw; `drawing::drawn`
  runs only when `drawing::asks_to_see` the user's message. `diagrams.js`
  `stepDrawings` parses it with `DOMParser` only to build step variants and fill
  in theme colours, and shows them as `<img>` data URLs, never in the page.
  User-written ```mermaid still renders. `BASE` stays under 4,500 bytes.
- `REPLY_STYLE` is not changed by prompt-size work. Escape belongs to the owner
  inside the page first; an open sheet always wins.

Decided: no modes, the agent decides (2026-11-05); marking never waits for
Felix (2026-10-19).

## Margin (`margin.rs`, `routes/margin.rs`, `web/margin.js`, `app/margin-notes.js`)

- Storage: `margin_threads`, `margin_modes`, `margin_state`, `margin_personas`,
  `margin_instructions` in leo.db, never note files or git. At most `MOST_OPEN`
  (24) open threads per note (held inside the INSERT and on reopening; a read-along saves up to 48) and
  `MOST_CLOSED` (48). Replies only while open, never two at once for a thread
  (`MOST_REPLY` 4,000, `MOST_MESSAGES`, the last 8 sent). `/reply` takes `request` (a uuid per
  typed message, kept on retry) and checks `insert::remembered` before the replying guard,
  so a resent reply gets the first answer; a resend while it runs is 409 `busy`.
- A thread whose passage is gone is not shown (2026-11-09; there is no lost-place
  box).
- A passage found only by its context (`marginKit.find` answers `how: 'context'`) keeps
  its note, with one muted line, "The passage changed since Felix wrote this."
  (`.margin-changed`, set by `marginFill`); an exact match shows nothing.
- An open thread shows its last 6 messages (`SHOWN_MESSAGES`); older ones sit behind a
  "Show N earlier" button (`.margin-earlier`) that shows them all for that note.
- Notes the page cannot place do not fill the limit: `marginDraw` collects their ids
  (`marginPage.gone`, by `marginKit.find`) and asks, Read through and reopening send
  them as `gone`; the open limit is then `min(MOST_OPEN + n, 2 × MOST_OPEN)`
  (`margin::room`, `n` counts only this note's open ids), passed to `put` and
  `set_status` as `limit`.
- Resolved passages stay findable (2026-11-14, the user's call: no Undo toast): GET
  `.../margin` adds `resolved: [{id, quote, prefix, suffix}]` (`margin::resolved_on`: status
  resolved, newest first, no messages, never dismissed). The page keeps them in
  `marginPage.resolved` (never in `threads`) and `marginRevisits` draws a dotted underline
  (`.margin-revisit`, text at 20%) for each it can place: a button on the first line (title
  "Reopen Felix's note", focusable) and aria-hidden spans on the others, a 5 px strip below
  the text line so the text stays editable. Click or Enter/Space runs `marginReopen`: POST status `open` with
  `gone`, then the returned thread opens. Resolving, and a read's tidied ids, join the list.
- Whole notes (2026-11-15, the user's call, replacing the first-line fold): a
  closed note shows its whole text and dims to .7 until lit; its text as a whole
  stops at 12 lines, the last shown block cut with a fade (`.margin-cut`) and
  the blocks after it hidden (`.margin-over`), both set by `marginCuts` (one
  read pass, then the writes) only when the text overflows; figures always show whole. Pins: `margin_threads.pinned`
  (guarded `ALTER`, db `VERSION` 4, in `margin.json`), PATCH
  `.../threads/{id}` `{pinned}`; a pinned or newest read-along note
  (`marginPage.newest`, `data-newest`) stays undimmed. An open thread's
  messages sit right of a 1 px `--line` rule; its one action row is Resolve,
  Dismiss, Add to note, expand, then pin and chat in `.margin-tools` at the
  row's end. No ×: Esc (anywhere, a sheet or menu first) or a click outside closes, a typed reply staying put; `.margin-toggle` is the
  closed opener and focus target and takes no clicks while open.
- Width: `marginLayout` sets `--margin-w` each pass from `marginKit.arrangement`
  and `marginWidth` (room beside the note's `max-width`, 240–360 px; Both
  becomes Right when it would leave notes under 300 px and one side gives 300).
- Deletes: a trashed note keeps its threads; deleting from the trash removes them
  (`forget_notes`). `sweep_store` waits `SWEEP_AFTER_DAYS` (7) and never runs
  while no notes loaded or a file is unreadable. Export adds `leo/margin.json`
  (personas and instructions are not exported).
- Modes `along` (default for notes leo made after `Prefs.since`; a v4 id, never a
  headerless file) or `ask`. A note's own row wins; turning the default off pins
  notes made while it was on (`margin::pin_modes`, before prefs are written). A
  fresh install stores `since = now` on its first `prefs()` read.
- Read-along POST `/api/notes/{id}/margin/read` `{version, blocks}`: the server
  refuses a stale version (409), a note not `along` or not edited in this engine
  run (403, `Reads::edited` set only by PATCH, so browsing never reads), a
  streaming reply, the gap, overlapping reads and the daily cap (429 with `why`).
  The cap is checked before the model is asked and spent in one transaction only after it answered (silence counts, a failed read does not; the note is saved even if another read used the last one).
  A read-along never refuses for a full margin (2026-11-14, the user's call): it saves
  with `limit` 2 × `MOST_OPEN` (the storage guard, 48), and at 48 open it first
  resolves the oldest open note that is not pinned, not `keep` (the thread open on the
  page, sent as `keep`) and not being replied to (`margin::free_oldest`), listing it
  in `resolved`; with none free it answers `{failed: true}` with `margin::FULL` (no Settings problem), before the model is asked or, if the last free note was taken meanwhile, after it (its blocks are forgotten, so nothing is spent and the text is read again). The page never drops the open thread or one with typed text from `resolved`: it POSTs status `open` for it.
  Only Read through still answers 409 `full`.
  Quiet tidy (2026-11-14, the user's call): after a read-along note is saved, while more
  than `TIDY_TO` (5) notes are open, the least recently updated ones are resolved
  (`margin::resolve_untouched`) if they have `source` `read` (not asks, chat-made or
  Read through notes, which are saved as `through`), no message from you, are not pinned,
  `keep`, being replied to or the note just saved; their ids join `resolved` and the page
  removes them with no toast. A note reopened from its underline has a fresh
  `updated_at`, so it is the last to go.
  Silence (`{}`/empty) is success; malformed JSON or prose is a failure
  (`{failed: true}`). ≤8,000 characters in, `READ_TOKENS` (220) out, bounded
  when parsed (CLIs ignore `max_tokens`). Gaps `READ_GAPS` (default 90 s);
  caps `DAILY_CAPS` 20/40/80/`NO_CAP` (default 40).
- Read through `.../margin/analyze` `{version, blocks, most}`: free (never checks
  or spends the daily cap); keeps the version, streaming, overlap and `MOST_OPEN`
  guards. Parts ≤6 blocks / 8,000 characters, at most 8 parts; ≤`MOST_PER_PASS`
  (3) threads per part, each reply capped at `THROUGH_TOKENS` (700); a run ends
  at the page's `THROUGH_MOST` (12) notes. Undo DELETE
  `.../threads/{id}` is 409 `replying`/`replied`.
- Add to note weaves: POST `.../threads/{id}/weave` rewrites the quote's
  blank-line block with `toolless_chat` (`WEAVE_SYSTEM`, `margin::woven` refuses a
  rewrite under half or over 3× + 400 of the block or with a new link),
  version-checked before and after the AI; only a lost, code or long passage falls
  back to `/insert`.
- A read-along also sends up to 1,500 characters of the note before the first changed block (`margin::earlier_text`: its last occurrence, cut at a block boundary, fenced as `earlier_text`) as context only; the model's raw quote must lie inside a changed block, else the read is silent. Read through sends none.
- The model's quote must be in the note now; reads grow it to the whole
  terminated sentence(s) it touches (`margin::whole_sentences`, else unchanged),
  then `margin::crowded` allows one open note per sentence. The server anchors on prefix + quote + suffix exactly and
  never falls back to another occurrence. Asks: 3 to `MOST_ASKED` (1,000)
  characters, else the chat; read-along and `add_margin_note` quotes
  `MOST_QUOTE` (600). The selection toolbar (`passages.js`) has Felix this
  (fixed ⌘↵/Ctrl+Enter), Ask Felix and Visualize; the two quick ones are asks
  with a preset question (`ASK_PRESETS`).
- Reads use `Powers.toolless`, never `quick` (`no_tools`: `--tools ""`,
  `web_search="disabled"`, chain cut to its first provider, so a read fails
  closed). Replies and asks use `Powers.toolless_chat`. Every read is a fresh
  process, no warm session, one retry on a stall.
- Prompt safety: note text sits between tags with a random suffix and is never
  instructions; thread messages are one JSON record per line inside them.
  Plain messages use `textContent`; diagrams use the escaped renderer, strict
  Mermaid and inert SVG. Personas and note instructions go after the prompt's
  first paragraph (`margin::voiced`, `margin::instructed`), so the safety and
  format rules come last. Personas: built-ins plus ≤`MOST_PERSONAS` (8) (name
  ≤32, instructions ≤500, engine-made ids); never for Felix chat. Note
  instructions ≤`MOST_INSTRUCTIONS` (500), control characters refused.
- Settings says text stays here only for a loopback address and a model without
  `cloud` in its name (`ai::reads_stay_here`). Failure text is the writer's own
  sentence (`leo_core::writing_said`), `no_writer()` or `margin::PROBLEM`.
- Chat context: `margin::context` adds open threads (≤3,000 characters and an
  eighth of the note room) with a fence stable while content is (prefix caches).
- Load margin prefs and reserve their space before positioning search matches or
  restoring reading; check the page sequence after loading. Read completion
  checks session and request identity. While a read runs, the `#leo-mark`
  symbol (`.margin-waiting`, breathing by opacity) and a pencil under the
  changed block (`path.pencil` per line over max(650 ms, width ÷ 1.6 px per ms), 900 ms for the side line; one side line for
  4+ lines) are never a `.margin-note` and never in `threads`; the read's end,
  an edit and leaving clear them (`marginReadingDone`, `marginPencilGone`). A lit
  note's tether (`path.tether`, 2026-11-15) is a tie (`.tie`) and, for a passage over more than one line, a
  spine (`.spine`): a vertical line 8 px outside the column on the note's side (right) or 3 px left of
  the list markers (left; `marginMarker` measures the bullet, number or checkbox with a hidden probe item, per layout pass, over every block the passage touches, else the block's
  text start −6). The tie runs from the spine's middle (one line: 8 px past its end, or that left edge)
  to the note's first line; a multi-line note aligns its first line with the passage's middle. It shows
  while the note is hovered, focused or open, and always (fainter) for a pinned note. A newly shown
  tether draws passage to note with `pencil-draw` (`pathLength` 1: tie 450 ms, then spine 350 ms),
  once per light-up; repaints keep the path, so they never restart it; none under `stillMotion()`.
  The solid underline (one per text line) and the spine are drawn with `marginWobble` (the pencil's jitter), phased by
  `marginSeed(thread id)` plus the line index, so a repaint, scroll or relayout never reshapes them; their
  endpoints sit exactly where a straight line's would, and the tie stays a plain cubic.
  `doc.spanRects` measures text nodes only, one rect per visual line (never a whole `li` box).
- Tests: `src/tests/margin.rs` drives the real engine over TCP with fake powers.

Decided: margin threads never in files (2026-11-04); read-along default for new
notes (2026-11-08); one note per sentence (2026-11-09); no lost-place box
(2026-11-09).

## Uploads, originals, recording

- POST `/api/import` runs the `Importer` on a thread; "What do you want" goes in
  `<what_the_user_wants>`. Several files make a note each (`run_each`); a failed
  file is listed in `error` and the rest are made. `into` (a note id, 404 if
  gone, never with `each`) never touches the note: the job returns `section`
  and the page appends it through version-checked `/insert`, so typing during an
  upload never makes a conflict copy.
- Originals live in `<data>/attachments/<note-id>/` (never synced), named by
  `free_original_name`; figures go to the AI via `leo_services::figures`.
- Recording (`record.rs`, `recording.js`): capture runs in Rust through the
  listener; the page never opens a browser microphone or screen picker and the
  server takes no audio from the page. Native jobs have no page heartbeat.
  `capture_activity` is reserved before a job is created; its lease blocks
  updater installation through the durable save. Recovery lists only what the
  `Housekeeper` lists (`pending_recordings`); a recovery job carries its session id.
- Pre-Start checks (`Housekeeper::checks`: `speech`, `writing`, `headset`) are
  cheap and never open the microphone.
- When no writing AI wrote the notes (`writer_failed`), the transcript is the
  note and leo.db `writeup_pending` marks it until PATCH `written: true`.
- Screenshots: pasted PNG/JPEG/GIF/WebP, MIME from bytes; late responses cannot
  attach to another recording.
- Share and PDF: Save .md is a Blob download (no route); print CSS hides every
  fixed overlay with fixed `@page` margins; a completed export cannot rename a
  newer page.

Decided: one note per uploaded file (2026-10-20).

## Note connections (`graph.rs`; "map" in storage)

Embeddings, summaries, concepts and note links are shared by search, related
notes and Felix, and refresh when idle. The graph page, canvas renderer and
`/api/graph*` routes are removed; connection caches stay. Graph, vector and
caption maintenance starts after its first interval and defers while the window
is hidden or the capture activity gate is busy.

## Calendar and tasks (`calendar.rs`, `ics.rs`, `tasks.rs`, `routes/tasks.rs`, `app/calendar.js`, `schedule.js`)

- Calendars: up to five secret iCal links through `CalendarAccess`
  (`secret::default_store`), never sent to the page; leo-host fetches them
  (`fetch_public`, capped at `CALENDAR_BYTES`). Feeds cached in leo.db with ETag /
  Last-Modified. GET `/api/schedule?from&to&live` (≤43 days).
- Offer to record: `calendar::offer_to_record`, a 60 s task (only with
  `Powers.app_settings`) over cached expansions after `catch_up` (same `Busy`
  guard): a timed event once when start−2 min ≤ now < start+10 min and nothing
  is recording (`record::live`); it never presses Start. leo.db `calendar_prompted(uid, start)` dedupes
  (pruned after 2 days); `offer` in calendars.json (absent = on).
- Standalone tasks: `<notes>/.leo/tasks/<id>.json` (`tasks::tasks_dir`), one
  JSON line per save through `jsonline.rs` (one generic reader/writer for any
  `Line`: `DIR`, `id`, `stamp`; shared with boards and cards), written atomically; the newest `updated_at`
  wins; deletes write a tombstone (`deleted`). Rows in the old leo.db `tasks`
  table are not read. Descriptions are optional plain text ≤4,000 characters.
  One mutex (`tasks::writing`) serialises task writes; task and board file work
  runs on blocking workers and never holds the notes store lock.
- Note tasks: checkboxes under Action items (or To do, Next steps, Deadlines) plus
  promoted ones (`note_tasks`, located by text and neighbours, never the wrong
  box). A date change is written into the checkbox (409 with a preview when the
  old date is gone); checkbox descriptions never rewrite note content.
- Repeating: `repeat` weekdays 0 (Mon)…6, `done_on` ≤ `MOST_DONE_DAYS`, never
  overdue. `moved` maps an occurrence's original date to `{date, time, end}`
  (≤ `MOST_MOVED`, absent when empty so old files are unchanged); `on` stays the
  original date. `tasks::occurs_on` and `schedule.occurrences` apply it; .ics
  adds EXDATE + RECURRENCE-ID; Google gets the weekly rule only.
- Felix's task tools preserve missing descriptions, an explicit empty update
  clears one; Delete Undo restores the original id with a fresh timestamp so
  the restore wins over its tombstone.
- Event descriptions (`schedule.richText`) keep only line breaks, bold, italics,
  lists, paragraphs and https/mailto links (`noopener`).
- Calendar page lays out in normal flow beside the rail and explorer, never
  `position: fixed` over `--side`. Saved task times never change for layout;
  a zero-height resize never writes an end. Polls wait during drags (`cal.stale`).
- Task blocks are as tall as their time (min 15 min, 18 px; `calLeast`): 2 to 3 PM
  ends at the 3 PM line and a resize shrinks the block live. Only a task with no end
  keeps a default span (`CAL_DUE_MIN`, 78 min). Under an hour a block takes `inline`:
  title, time on one line. Today is a pencil ring (`calRing`, inline SVG, one of
  `CAL_RINGS` by day of month, inside the old 26/24 px circle) in the mini
  calendar, week header and month.
- The Tasks card in `#dock` is folded to a badge at every start and opens only
  while hovered or holding keyboard focus (CSS `:hover`/`:focus-within`, the card is
  focusable); nothing is saved and a click never pins it (mouse presses on it do
  not take focus). Home draws today's and overdue
  tasks inline under the quick bar (`#home-today`, the same rows) and hides the card.
- POST `/api/tasks` takes `request` (uuid, kept across a retry of the same quick-add): a
  repeated one is answered with the first task (`insert::remembered_creation`, scope `task`, a queue of its own beside the margin's `insert::remembered`).
- Export adds `tasks.json`; POST `/api/tasks/import` (ids dedupe; a newer local
  copy is kept).

## Boards (`boards.rs`, `routes/boards.rs`, `board_tools.rs`, `app/boards.js`)

- Files `<notes>/.leo/boards/<b…>.json` (name, columns `{id, name}`, 1–12) and
  `<notes>/.leo/cards/<c…>.json` (board, column, `rank`, title, subheading,
  description ≤8,000, priority `""|low|medium|high`), read and written through
  `jsonline.rs` like tasks: one JSON line per save, atomic, nofollow reads,
  newest `updated_at` wins, tombstone deletes; a file counts only when its name
  is its id. Limits: 100 boards, 5,000 cards. Not calendar tasks.
- Order is `rank` (f64): a move writes only the moved card (`boards::between`)
  unless the gap is gone, then `settle` renumbers that column. A card whose
  column vanished shows in the first column and is rewritten only when it moves.
  One mutex (`boards::writing`) serialises writes.
- API: GET/POST `/api/boards`, GET/PATCH/DELETE `/api/boards/{id}` (removed
  columns' cards go to `move_to` or the first column; DELETE returns
  `{board, cards}`), POST `/api/boards/restore`, POST `/api/boards/{id}/cards`,
  PATCH/DELETE `/api/cards/{id}`, POST `/api/cards/{id}/move`
  `{board?, column?, after?}` (missing = end, null = top). Export adds
  `leo/boards.json`. Card create takes `request` (a uuid per user action,
  kept when the same card is submitted again after a failure): a repeated one is
  answered with the first result (`insert::remembered_creation`, scope
  `card:<board>`) and applies nothing twice. Moves take none (nothing retries them).
- `description` stays plain Markdown. Leaving the card panel settles every field;
  a failed save or over-limit description (`KANBAN_MOST_DESCRIPTION`) keeps the
  panel, address and text, so a 400 never costs text; Discard is the only way
  out that drops it. A board fetched after a change is dropped if a write began
  meanwhile.
  A card move (keyboard, status menu) and ticking a task done apply and redraw
  first, then save; a failed save puts the card or tick back and shows the error toast.
  Each move carries a per-card number: only the latest move's answer or rollback is applied, and the board is fetched once when the last pending move settles.
- Dragging is pointer events (5 px, or a 300 ms touch hold), capture on `#app`.

Decided: Jira-style boards on their own page (2026-11-02); the card panel is not
a modal.

## Settings, Keeping, export

- Settings go through `SettingsApi` (`leo_host::WebSettings`); `Powers.speech`
  (`SpeechApi`, `/api/speech`), `Powers.ollama` (`OllamaApi`,
  `/api/writing/ollama`), `Powers.dictation` (`DictationApi`, `/api/dictation`,
  200 KB input) and `Powers.app_settings` (`AppApi`, `/api/app`, `/api/health`)
  are session-gated with bounded JSON; work runs off async tasks and outside
  store locks; every success is JSON, errors actionable. Progress and polling
  update only their own card: fields, drafts, focus and caret survive
  (`app/refresh.js` reconciles by stable keys). Setup's three completion flags
  stay compatible.
- Ollama pull: refuses models not in the catalog, TooBig ones and a second pull
  (409); cancel is immediate and a stale pull thread cannot write. Detection never
  changes the chosen provider; `GET /api/writing?probe=signin` is the only place
  setup runs the sign-in check.
- Dictation history is the leo.db `dictations` table (insert and twenty-row
  eviction in one transaction). Copy resolves a stored ID and discards
  page-supplied text; edit refuses an unknown id. Nothing becomes a note or sync
  file; leo never watches other apps. No Save button: changes save at once,
  in order (`saveDictation`). The first edit keeps the `original` text (a
  nullable column added on open); the row shows Edited and a Show original
  toggle, and word offers skip short (< 3 letters) and everyday words. The
  Shortcut row is a keycap field and one Change button: while recording it shows
  the held keys live, the page saves the captured chord when the native recorder
  reports it, Esc (or a click elsewhere) cancels, and a note says Saved or why
  not. Detect language is hidden for a model that cannot detect language, and
  setup picks the system language, else `en`, for such a model.
- Settings look (2026-11-14): a choice of two or three that reads better as a
  picture is `choiceCards` (theme, Felix, width, text size, lettering, margin
  layout, new-note mode, Reading pace); long and numeric lists stay `segmented`.
  A switch row is a bordered box with a one-line `.set-desc`. The Dictation
  shortcut lives only in Settings → Shortcuts (`dictationShortcutRow`, its Change
  button runs the native `record_shortcut` flow; `dictationSection` keeps
  `pollDictation` running on both pages) and Health's shortcut check points there.
 (`{section, label, keywords, id}`)
  and the row's `data-find` attribute are added together (the Shortcuts section
  passes its ids as literals to `shortcutRow`, so the test finds them);
  `settings-navigation.test.js` fails on an unknown section, a duplicate id or
  an id no source draws. Routes stay `#/settings/<section>`; `#/trash` still
  works and a bare `#/settings` never reopens Trash.
- Usage: ordinary Settings reads never refresh coding-plan limits; POST
  `/api/settings/usage` (`{}` Codex only, `{ "claude": true }` also Claude)
  behind one refresh slot; a failed refresh keeps saved meters. `usage::Usage`
  keeps content-free counts, `usage_days`, `usage_calls` and `usage_tools` (90 days), never prompt or reply
  text; `/api/processes` never sends arguments to the page.
- Keeping (`keeping.rs`, `app/keeping.js`, `/api/keep`): one list (7/30/90 days,
  1 year, Forever) for Trash, Chats, Uploaded files, Recording transcripts,
  Unfinished recording audio. Shortening asks first with a count from POST
  `/api/keep/preview` (`keeping::would_delete`, deletes nothing); one save at a
  time. Forever deletes nothing. Expiry runs at `start_in`, daily and after POST
  `/api/keep` (`keeping::expire_now`): uploads by file mtime (the folder and note
  never touched), transcripts in `recording-sources/`, unfinished audio through
  `Housekeeper::pending_recordings` + `discard_recording` and
  `expire_kept_audio`, skipping the session in use (`RecordJob::session_id`) and
  locked sessions. DELETE `/api/record/{id}` also deletes a waiting recording
  without a job. The Storage page and `/api/storage` are gone; files only it
  managed stay untouched.
- Export: zip of notes, optional uploads, chats, trash; never config or keys.
  Restore tasks refuses a zip or file without task ids.
- Appearance keys are per browser (`leo-theme`, `leo-note-width`,
  `leo-text-size`, `leo-felix-letters`) and set before paint by `/theme.js`;
  only reading text multiplies `--read`.

Decided: Keeping replaces Storage (2026-11-05); Quit leo is not in Settings; Writing
AI uses a pen icon, never a sparkle.

## Idle work and freshness

- `read_store` takes an immutable Store callback and coalesces filesystem scans
  with core's Watch: it checks first, on relevant/rescan notifications, every
  60 s and after native foreground return; a missing watcher checks every access.
  `with_store` and `store_now` always reconcile before mutations: a save must
  never absorb an unread external edit into its fingerprint.
- App-status polling stops while the document or window is hidden and drops late
  responses on visibility changes; return refreshes once. Automatic status GETs
  do not reset user-idle time. Dictation, model-download and recording display
  polls pause while hidden.

## Retired interview practice

Removed 2026-11-05. Legacy `interviews`, `interview_keys` and
`interview_problem_undo` tables and rows stay in leo.db, unused, never purged or
migrated. Old interview chats open normally (cards show a short retirement
message); old routes fall back to the workspace; `leo-interview-*` localStorage
keys and downloaded runtimes stay untouched.

## Testing

- Never run full suites locally (no workspace `cargo test`, no full Playwright
  run). For website work no local Cargo builds: quick JS checks, one node test
  file, one `-g` test; CI does the rest.
- Node tests run only in CI's node job (`node --test crates/leo-web/tests/*.test.js`),
  not through cargo.
- Browser tests (`tests/browser/`) serve `leo-test-server`
  (`target/debug/leo-test-server`, or `LEO_TEST_BIN`) with a fresh disposable
  home, `LEO_NO_UPDATE_CHECK`, `LEO_NO_MICROPHONE` and the coding providers
  pointed at missing fixture binaries, so detection never inspects the
  developer's accounts. The `phone` project runs only
  the "on a phone" test in flows.spec.js. There is no visual suite.
- Fixture specs (boards.spec.js, calendar.spec.js) run the real CSS and page code
  over an in-memory API; flows.spec.js joins the page to the real routes.
