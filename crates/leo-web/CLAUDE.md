# leo-web: leo's engine and the page

leo-web depends only on leo-core. Everything that needs services (AI, speech,
settings, import, recording housekeeping) arrives as a `Powers` field built by
`leo_host::powers()` (writer, quick, toolless, chat, light_chat, lighter,
toolless_chat, settings, speech, ollama, dictation, app_settings, importer,
listener, reader, seer, meaning, room, …; see `Powers` in `lib.rs`).

This file holds the rules. How each screen behaves, area by area, is in
`docs/behaviour.md` (not auto-loaded): read the section for the area you change.

## Layout

Test fixtures retain their `TempDir` guard until stores and database connections
drop, so notes and `leo.db` are removed automatically; never persist a temporary
directory with `keep()` just to pass its path to a fixture.

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
  synced): chats (+ FTS5; chat messages are rows, `chat_messages` with
  `chat_words` for search, db `VERSION` 7: a save adds only new rows, a chat
  keeps every message and is read 200 at a time, a chat saved before is moved to
  rows on its next save; 2026-11-17, the owner's principle 1: the 400-message cut
  and its page slice are gone), chat documents, review progress, picture captions,
  meaning vectors, note connections, calendar feeds, note-task metadata,
  reading positions, margin threads, dictations, usage days, recordings whose
  notes are not written yet, a search index of note text (`notes_text`, trigram,
  no copy of the text; no database version: it is checked against the notes by
  hash at every start). Old caption, graph and meaning JSON files are
  brought in on first open and renamed `*.before-database` (browser tests seed
  note connections this way), and deleted 30 days later by the daily Keeping pass
  once their table in leo.db holds rows and no `leo.db.damaged-*` sits beside it. Writes touch only what changed. New databases
  give deleted space back in small steps when leo is idle and not recording
  (`Db::give_back`, hourly, 256 pages a step, from 1,024 free pages). A database
  made before 2026-11 is rewritten once while idle into a new file and swapped in
  (`Db::rewrite`, just before `give_back`); the swap is the only step that holds the
  connection, and it is abandoned if any other connection to the file is open.
  The cache-capacity rewrite fixture runs in its own process because the connection
  cache is process-wide. A damaged leo.db
  is set aside as `leo.db.damaged-<stamp>` (Health says where); one from a
  newer leo is refused before it is opened.

## Serving and security

- `start(StartOptions { port, token }, powers) -> Running` binds `127.0.0.1`
  only (the requested port, else the next free of 19), spawns the background
  tasks (graph, idle, Keeping expiry, calendar) and the server;
  `Running::{port, sign_in_url, stop}`. `stop` stops the background tasks first,
  drains requests and streams for at most `shutdown::GRACE` (5 s), then aborts
  the server; it first signals a native recording to stop, refuses new
  recordings, and waits for the worker within the same grace; warm-session cleanup has `shutdown::CLEANUP` (1 s). Tests use
  `start_in(Places { store, sessions }, …)`. The app passes `fresh_token()`;
  `leo-test-server` uses `saved_token()`, the `<config>/serve-token` file the
  specs read.
- Only the app's window talks to the engine: `same_host` refuses any Host but
  `127.0.0.1:<port>`/`localhost:<port>` (421); `?token=` on GET `/` → 303 + a
  `leo_session` cookie (`SameSite=Strict`, 30 days, `app-sessions.json`; a
  damaged sessions file loads as none). No
  tunnel, Wi-Fi bind, QR, signed-in browsers list or link rotation.
- A route that carries the person's own text takes a body of any honest size (2026-11-17, the user's call: no request-size limit):
  create, save, `suggestion`, `insert`, `felix/act`, `tasks/import`, a recording's pasted `snapshot` (a screenshot of any size) and `/api/chat` carry `routes::text_limit()`
  (`TEXT_BYTES`, 512 MiB, the one memory bound: a body is read whole and parsed whole, so one request
  costs about three times its size). Hostile input is stopped before the body is read, not by its size: the
  engine listens on 127.0.0.1 only, refuses any Host but its own (421) and any request without the window's
  per-launch session (401). Routes that never carry a note's text keep axum's 2 MB default; the combine, margin,
  chat-save, import, picture, preferences and dictation routes keep their own limits.
- Page CSP: no inline JS (`script-src 'self'`), `data:` images, `frame-src
  'self'` (original viewer), `font-src 'self'` (KaTeX and the bundled
  fonts). Mermaid 11.17.2, KaTeX 0.16.11, CodeMirror parsers and the latin
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
  `null`; a browser test sweeps the API); every `/api/` refusal is JSON `{error}`
  (`reply::worded` words a bare one by its status; the folder routes say the folder
  sentence). Session-gated JSON inputs are bounded.

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
- Keeping's expiry refuses linked recording and originals roots; every pass walks its folder entry by entry in bounded memory (2026-11-17, the owner's principle 1: no entry count; uploads are deleted as they are met and the preview gives way every 1,000 entries when it is abandoned) (a recordings pass deletes in batches of 256 under the recordings lock until done), never follows symlinks, stays in its folder and skips what it cannot
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
- Cheap toggles change the screen first and put it back if the save fails
  (2026-11-14): task done and type, card moves, priority and delete, margin pin,
  resolve, dismiss and mode, calendar drags. Card moves, margin pins, task types
  and card priorities roll back only for the newest request. Toasts with Undo and
  note reloads wait for the answer. Version-checked writes, file operations
  (trash, move, rename, folders), writes whose answer the page needs (new ids)
  and AI calls wait on purpose.
- Narrow windows (600–999 px, 2026-11-14, the user's call): the icon rail stays;
  the sidebar button opens the explorer as a drawer over the page (`side-drawer`;
  Esc, a click outside or opening a note closes it; focus goes to Filter and back).
  The More sheet (`#menu`) is for phones only (under 600 px).
- Layout variables on `body`: `--side`, `--right`, `--mid`. Fixed things
  (toolbar, select bar, toasts, FABs, `#dock`) use them so nothing covers
  anything. Page toolbars share one geometry: 760 px at most below 1000 px; from 1000 px
  the bar spans the page less `min(8%, 128px)` each side, so Felix's button sits in the same
  spot on every page (2026-11-16, replacing the bar that followed each page's content).
- UI preferences survive a port change (2026-11-14): localStorage is per origin and
  the engine port drifts when the saved one is busy, so every `leo-*` key (not
  `leo-draft*` or `leo-chat-v1`: content, not settings) is mirrored in `<data>/ui-prefs.json` (`prefs.rs`: keys
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
  colour). Note text (`.doc`) paints that same wash through `::highlight(leo-pick)`
  (doc.js, one page-wide `selectionchange` listener, priority under search; `pick-paint`
  on the doc after its first paint makes its native `::selection` transparent), so only
  the selected text is tinted, never the gaps between blocks (2026-11-15, the user's call);
  line editors, images and forced colours keep the native selection. Ink details (2026-11-15): a Markdown `---` (`.prose hr`, also the
  doc's rule block) is a drawn pencil line (an SVG mask, plain 1 px `--line` in
  print and forced colours); an open folder's guide (`.side-kids::before`) is a
  pencil stroke with a hook at the bottom (two SVG mask layers, the line stretched,
  the hook a fixed 10 px); a note ends with a 40 px pencil stroke
  (`#doc > .doc-tail::before`, hidden in print and while the note is empty,
  `doc-empty` set by doc.js on every draw and change);
  scrollbars are thin and warm (`* { scrollbar-width:
  thin }`, `:root` `scrollbar-color`), and `html.page-bar-on` still hides the
  page's own bar for `.page-bar`. Focus rings are unchanged. Repeated style rules are written once (2026-11-18): one comma-list rule per focus-ring offset (a second one only where a rule between its members sets an outline for the same element), one rule for the chip on-state, one for the cut-off-with-dots trio (`overflow: hidden; text-overflow: ellipsis; white-space: nowrap`) per run of rules no other rule can interrupt, and the tokens `--ring-soft`, `--t-fade` and `--r-panel`. A new rule that repeats one of these joins the shared rule; `node tests/style-merge.check.js <base ref>` (Playwright library only, no server) compares the computed style of every changed selector against a base commit in light, dark, narrow and forced colours and must report 0 differences.
- Copying a selection that holds a picture (note reading view and Felix's answers, 2026-11-18) also puts
  `text/html` on the clipboard: `leoMarkdown.selectionCopy` draws each loaded picture to a canvas and
  `copyHtml` rebuilds the selection from an allow list (no script, handlers, styles or leo addresses; a
  picture over 16 megapixels or 20 MB, or past 60 MB in all, stays its alt text). A selection without a picture is untouched.
  From a note (2026-11-23), `text/plain` leaves the pictures out (a typed caption stays,
  `leoEditing.withoutPictures`) and the html's outer `div` carries `data-leo="the markdown"`. That `div` (Felix's answers too)
  is `color:inherit`, because WebKit otherwise writes the page's own text colour, white in dark mode, and black is unreadable
  on a dark page. A picture alone (clicked, or the only thing selected) is copied with `navigator.clipboard.write` as
  `image/png` and that html (a JPEG there when the PNG passes 15 MB; halved in size until the PNG is under 60 MB, since
  WebKit refuses over about 100 MB), called inside the copy event and never with `clipboardData.setData`, which WebKit
  refuses together. A picture not loaded yet is loaded first the same way, alone or with text. Cut always uses the plain path.
  A selection that names an outside picture (`https:`, `//`) keeps its markdown as the text and gets no wrapper.
  `onLeoPaste` (doc.js, capture) puts the markdown back, with no upload, only when `leoEditing.ownCopy` says
  it is leo's: its text without pictures equals `text/plain` (pictures only when there is none) and no picture is
  outside. It goes to the caret, the selection or under the clicked picture; pictures alone with none of these go
  to the end. A picture alone whose path is relative to another folder (`data-leo-from`) is pasted as a new picture.
  Any other paste is handled as before.
- Escape is always `preventDefault`ed in a capture listener, so it closes sheets
  and never leaves macOS full screen. Passive reveals never open a sheet.
- Shortcut labels use `ON_MAC` (Ctrl on Windows); monospace falls back to Consolas.
- Native quit and lifecycle events only flush drafts; they keep the page.

## Shared components (new UI must use these)

- A few options: `segmented()` (settings-navigation.js; `field` mode fires
  `change` on a hidden input, `action` mode makes `data-action` buttons with
  `aria-pressed`; a 4th choice item is the tooltip; `syncSegmented(box, from)`
  after a redraw). Picture choices: `choiceCards()` (same two modes).
- On/off: `settingsSwitch()`; checkboxes only for task completion and picking
  several items (Trash). Task checks (`.prose .task input`, `.sched-check`,
  `.cal-check`) are real checkboxes drawn as a pencil box and tick (masks on
  `::before`/`::after`, four variants picked by the last digit of `data-box` or
  `data-id`; the tick takes `--tone`); dark mode breaks the tick with the static
  `--chalk-break` mask; forced colours get the native box. The same tick stands
  alone as `.pencil-tick` (ink, `currentColor`) for Health's passed checks,
  first-run setup's Allowed, the start of Felix's finished step summary (`.msg-trace`)
  and a code block's Copy once it copied; `.writing` writes it on with `tick-write`.
- Dropdowns: a `<select>` inside `.select-control`; select-menu.js turns every
  one into the app's menu (not under forced colours), keeping id, value and
  `change`. Do not hand-build list menus.
- Text fields: `.field` (icon-led, sheets) or `.set-input` (Settings rows).
  Folder/Title/What do you want: `noteFields()` (uploads.js) in the upload
  sheet; the Record page draws the folder list (`folderOptions`) in a chip.
- Confirmations: `confirmSheet({ title, text, yes, action, data, danger, cancel })`
  for permanent losses only. Moving to the trash asks nothing and offers Undo for
  10 s. The time left is a pencil line under the toast (`.toast-time`, the
  Markdown rule's stroke) rubbed out from the right by a `clip-path` wipe, none under
  Reduced Motion; a toast's action (Undo) is a small plain button on a 10% ink tint, and an error toast keeps
  the toast surface with a red alert mark in front, never a red fill (2026-11-21, the user's call). Removing a key or speech model is a two-row `popupMenu`.
- Menus: `popupMenu(owner, { label, rows, point, find })` is the one dropdown
  (a row's `next` is a second step). Clicking its owner again closes it, as does a click
  outside (2026-11-16); a context menu (`point`) reopens at the new point instead, and the click
  WebKit sends after a Control-click never reaches what the menu belongs to. Side
  panels opened from a card or row (the card panel, the calendar peek) close on a click outside
  through `closeOnOutsidePress(inside, leave)` in `popup-menu.js` (2026-11-21, the user's call):
  it acts on the click after a primary press outside the panel, its opener, menus, sheets and toasts,
  so drags and presses begun inside never close, and `leave` is the panel's own Escape path. `sheet()` focuses the first control and
  traps Tab. Rename uses `inlineEdit` over the tree row; New folder opens a real
  row in place (`sideMakingOpen`: `side.making` draws a keyed `.side-making` row at the
  top of its parent, opening over `MOTION_MS` and folding back on Esc, none under
  `stillMotion()`), its name typed on a `--focus` underline (`.inline-edit.making`).
  Rows hover with `--menu-hover` (`--tint` light, `--wash` dark, where `--tint`
  would dip below `--raised`); separators inset 12 px; a faint keycap
  (`.popup-rows kbd`, `aria-hidden`) only where a real shortcut acts: New note
  when its folder is ⌘N's folder and no sheet is open (2026-11-15).
- Empty pages `empty(icon, title, text)` (title at 500, text held to 36 characters a line,
  2026-11-21, the user's call); an empty state with nothing to do yet
  (Trash, Boards, calendar Tasks, Chats, Search no-results, home) takes one
  single-stroke line drawing from `DRAWN` (app/core.js; `fill="none"`,
  `currentColor`, shown in `--muted`, no colour) in place of the icon; chat.js
  gets them as `drawings`. Three error states take one too (2026-11-15): Can't
  reach leo (`DRAWN.offline`), the chat's no-writing-AI reply (`drawings.noink`,
  72×55) and Keeping could not be opened (`DRAWN.broken`); the missing-note toast
  and Felix's read-failure status stay text. Home's arrow points at the explorer and shows only
  on the first day (no notes, no tasks) while it is open (≥1000 px, not `side-mini`). Titles stay in the app's sans
  (2026-11-15, the user's call); counts `.badge`; small faint
  sentence-case labels share one rule (`.source-label`, `.quick-due-head`,
  `.chat-history-group`, `.pick-head`, `.settings-group-title`; Settings joined 2026-11-21, the user's call; its stroke is hidden when the nav becomes pills), ending in the note rule's pencil stroke
  (`::after`, same mask, a plain 1 px line in print and forced colours). The current board tab (`.kanban-tab.on::after`) borrows the same stroke.
- Kept on purpose: the model picker, the margin-mode menu and chat.js's attach
  and access menus (they move through `leoMotion`).
- The leo mark (`#leo-mark`) is the one working mark (2026-11-15): it breathes
  (`margin-breathe`) only on `.margin-drop` while Felix reads or thinks and on
  `.msg-step-busy` (chat.js `BLOT`, in `--felix`) in place of the live step's icon
  and before a quiz's "Checking your answer…"/"Felix is reading your answer…"
  (that card shows no typing dots), and sits
  still in the Reads along chip, at the start of the calendar now-line (in place of
  its dot) and in `#save-state` (full ink while editing or saving, drying once to .45
  when Saved; the words stay as hidden text, "Not saved" stays visible). It also
  shows leo's own background work in ink (the dock's head, the calendar's "Updating…",
  Plan limits' Refresh, Health's "Checking…"; 2026-11-21, the user's call: the ring spinner
  is gone). No other spinner or second breathing mark.
- Colour: ink for actions and selection, `--focus` for focus cues (Felix blue since
  2026-11-16, the user's call: every former purple, `--focus` and the indigo tone
  `--hue-indigo` used by calendar assignments, callouts and note cards, is #3f6d97 /
  #b4cfe7; Felix's pixel pen keeps its colours), tone only as
  dots and ticks (calendar events keep their colour fills; board priority strokes are tone too). One exception,
  Felix's own blue (`--felix`: light #3f6d97, dark #b4cfe7, from his sprite;
  2026-11-15, the user's call), only while he works: the reading blot
  (`.margin-waiting` or the gutter `.margin-mark.reading`) and pencil
  (`path.pencil`), the caret after a
  streaming reply (`.msg.pending`) and the dot on his button
  (`#chat-toggle.working`, synced with the chat body's `aria-busy`), the chat's
  working blot (`.msg-step-busy`), the head on his replies (`.msg-who`: the leo
  mark and "Felix" in his voice, not on a reply that follows another of his) and
  the ring of the idea being asked in the study strip (`.study-idea.now
  .study-ring`, not under forced colours) (2026-11-21, the user's call; the quiz
  card's heading `.quiz-kind` takes his voice, not his blue), the first 600 ms of what he just made
  (`inkLand(id, true)`), and while he
  waits on you: "Waiting for you" with its dot (`.proposal-wait`) on the latest
  reply's proposals until Apply or Dismiss (no coloured edge), the Ask Felix box in the
  margin (the draft's reply border and its passage's underline, wash and tether, 2026-11-15),
  and the still 6 px
  dot after the open note's explorer row while he reads along (`.side-reading`,
  from `marginPage.reading`; margin-notes.js redraws the explorer when a read
  starts and ends; 2026-11-15), and the ink blot on a note leo wrote that has not been
  opened (`.side-unopened`; 2026-11-24, the user's call). Never links,
  buttons, selection, tags or text (his name in the head excepted).
- Surfaces (2026-11-14, the user's call: modern, no outlines): separate by tone
  (`--ground` < `--page` < `--raised`) and warm two-layer shadows (`--shadow` at
  rest, `--shadow-pop` for menus, toasts, sheets and the open note in Marks, with no
  hairline ring since 2026-11-16, `--shadow-lift` while
  dragging); dark mode lifts with lighter surfaces and a faint top highlight.
  Inputs, tables, code and focus rings keep a clear edge; so do board lanes, one
  step off the board's tone with a 1px inset edge (they vanish otherwise).
  Neighbouring Settings switch rows share one `--raised` panel with `--shadow`,
  split by a `--line` hairline inset 14 px; a hint or another kind of row starts
  a new panel; forced colours restore the row border (2026-11-15). One switch size everywhere, 36×22 with an 18 px knob
  (2026-11-21, the user's call); the quiz toggle no longer overrides it.

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
  grouped rule at the end of the styles), never draggable rows or disabled controls.
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
  `kanbanFit` and a scrolled list's `scrollTop`); the one exception (2026-11-15,
  the user's call) is a dragged card's ghost, which tilts 1.5° (`--tilt`, a
  registered angle written into the ghost's transform after its position, never
  the `rotate` property, which pivots about the page origin) arriving with `lift`
  (its keyframe at offset 0, so it starts flat) and returning to 0 in `settle` (`liftedRect` carries `tilt`); none under Reduced
  Motion, none for lane or calendar ghosts.
- Opening a note fades only `#doc` from .6 to 1 over 140 ms (`arriveOnPage`,
  `EASE_OUT`, only when `swap()` says it opened, none under `stillMotion()`); the
  title stays still.
- Two signatures (2026-11-15): write-on, the task tick wipes in over 160 ms
  (`--ease-pencil`, a transition, so a redraw never replays it; a box redrawn
  inside the 160 ms carries `data-writing` and `--tick-at` from `tickWrite` and
  resumes with `tick-write`) and today's ring
  draws once over 300 ms with `pencil-draw` when the Calendar opens
  (`cal.ringAt`; redraws inside the 300 ms resume it with a negative delay,
  later ones draw the end frame), and a marked quiz's check or × writes on over
  160 ms (`QUIZ_MARK_MS`, `pencil-draw`, resumed by elapsed time); ink settles, `inkLand(id)` when leo creates a
  card, task or note and `inkSettle(find)` after each redraw: the row starts in
  `--text` with a 12% ink wash and fades to its own colours over 600 ms
  (`INK_MS`, colour only, resumed by elapsed time). `inkLand(id, true)` is
  Felix's variant: core.js `onOrganized` lands an applied add_card, add_task or
  add_margin_note (the id read from its `undo`, `FELIX_MAKES`) starting in
  `--felix` with no wash; the margin settles `.margin-felix` after each
  `marginDraw`; home's newest tally stroke writes on over 160 ms with
  `pencil-draw`, resumed by elapsed time from `tickedAt`. Explorer notes count as new
  when a refresh brings an id created in the last minute. The explorer fold is the one
  width animation (`--side` as an `@property`).
- Loaders (2026-11-15): `skeleton(n, surface)` (search, board, settings,
  keeping, trash, health, plan) holds faint pencil lines that redraw on a calm loop
  (`sk-draw`: a 400 ms wipe in `--ease-pencil`, 30 ms stagger, then a rest to 1.6 s, repeating
  while the skeleton is shown and gone with it; only under `prefers-reduced-motion:
  no-preference`, so none under `stillMotion()`). Each skeleton starts from the shared clock
  (`--sk-at`, a negative delay from `performance.now() % 1600`), so one drawn again continues
  the same phase instead of restarting. No shimmer; the loop is for skeletons only
  (2026-11-21, the user's call, reversing "no loop"; 12 px between cards, 12 and 16 px between lines).
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
  never the file. A click or a double-click on a picture frames it and never opens its
  Markdown (2026-11-21, the user's call; Shift-click or a double-click beside it does). A
  picture is one caret position (`ed.rawOffset`, `shownBefore` in doc.js), and so is a formula, drawn or not (`markdown.mathSpans`, `.math`); in a list or
  checkbox item a click on the picture frames it like any other, and a click beside it, left
  or right of its box on its row, puts the caret before or after it (2026-11-21, the user's
  call). Backspace or Delete on a framed picture removes it and leaves the caret in its place
  (`ed.removePicture`); a Backspace outside any text field is `preventDefault`ed in a window
  listener (events.js) so WebKit never goes back a page (2026-11-22, the user's call).
- Note page only (2026-11-15; `leoDoc.mount(..., { noteTools: true })` in showNote,
  passed to `md.render` as `noteTools`; board cards, chat, margin and previews render
  without it, byte-identical): a column whose body cells are all numbers (%, a
  currency sign, a sign and thousands commas allowed) takes `align-right` unless the
  Markdown aligned it; a fenced block becomes `<div class="code" data-lang>` around
  the `<pre>` with an icon-only `.code-copy` button (no text nodes; the label is
  `::before { content: attr(data-lang) }` in the block's top-right corner, in the pre's own top padding, so the code starts at the top with no band for it, 2026-11-21, the user's call). Copy shows on
  hover and `:focus-visible`, takes its own keys, copies `pre code` text only, never
  opens the block, says the result in the hidden `#code-said` status region and shows
  `.pencil-tick.writing` with aria-label "Copied" for 1.2 s. `#doc` styles: links take
  the search match's three pencil strokes as a `box-decoration-break: clone`
  background (colours baked into the data URIs: a grey equal to ink 45% dark / 55%
  light at rest, full ink on hover, so a wrapped link gets a stroke per line); quotes
  are in ink and Newsreader (`--felix-letters`, 1.06 em) with no fill, beside a pencil bracket (the folder-guide
  stroke with 10 px hooks, ink 38%), also while open (2026-11-21, the user's call); tables lose grid lines and the
  header fill (everywhere `.prose th` is a 0.92 em medium-weight label in ink with no fill, 2026-11-21, the user's call), every other body row sits on `--wash`, and each `th::after` draws the
  double pencil rule across its cell, edge to edge. Print and forced colours keep
  the old look: a plain underline, the 3 px quote bar, row rules. The title row's chips
  (`.margin-mode`) sit 16 px apart with 14 px icons; the two Felix chips (not the ⋯) are plain
  text, no ring, fill or side padding, and only take the text colour on hover; the ⋯ keeps its
  4/8 padding and hover fill (2026-11-21, the user's call: "just keep text"). A picture whose alt text says something (not empty, not a file name, not "Pasted picture") sits in a
  `.note-fig` span whose `::after` shows it as a 13 px caption on the note page (generated
  content, so no text node reaches the caret or margin maths; the Markdown is unchanged;
  2026-11-21, the user's call). A heading block (not the first)
  has 16 px of padding above its heading, open or not, so it leads the text below (2026-11-21, the
  user's call). The line under
  the title (`#note-facts`) holds the save state, `#save-state` (the drying mark, its
  words hidden, or a visible problem such as "Not saved"), then `#note-edited` ("Edited …",
  "Edited just now" after a save, hidden while a problem shows), then the read time (`#note-read`, " · N min read" from
  `md.readTime`, 238 words a minute, none for an empty note, refreshed 300 ms after an
  edit) and, on a note made from a recording, " · Transcript" (a quiet underlined
  `.facts-link`, 2026-11-22, the owner's call); the line has no word count (2026-11-16, the user's call), but the ⋯ menu ends
  with one quiet footer row, the word count alone (`md.wordCount`, no read time or last
  edit; 2026-11-21, the user's call: "maybe we just need the word count and that's it"). It is skipped by search highlights and hidden in print. The note's
  tools (transcript, Tasks, Add picture, Move to folder, Read through and Share as second
  steps, Move to trash) are one ⋯ menu (`#note-more`, `noteMenu` in sheets.js); Read through
  (2026-11-21, the user's call: no button in the note head) lists the readers, the note's
  own checked; picking one runs one pass in that reader's voice (`persona` in the analyze body,
  used only by Read through; 400 `persona` "That reader is gone." when unknown) and leaves the
  note's own reader alone; while a read runs the row
  is "Stop reading through", and it is off for an unsaved note;
  there is no floating note toolbar. `#dock` (the Tasks card and the background-activity
  panel, so a finished combine's Review stays reachable) shows on note pages again
  (2026-11-21, the user's call); the page reserves bottom room for it (`--dock-room`,
  set by `fitTasksWindow` from the dock's height, read by `body.on-note main`). A live
  recording off the Record page is the first row of that panel (`pill()` in
  recording.js fires `leo-recording`; there is no floating pill). The
  top bar shows no breadcrumbs on any page (`.crumbs` is kept invisible as the bar's
  spacer). On note pages at 1000 px and up the bar takes no room (2026-11-16): it is a
  zero-height sticky overlay holding only Felix's button, at the same spot as on every other
  page, and the chip row starts one bar's height below it (`--bar-height` + 8 px), an empty
  band above the note. There is no Back button anywhere (2026-11-16, the user's call); Escape
  goes back. Felix's chat starters are Teach me this note step by step, Quiz me on this
  note and Make me a 3-day review plan; on a note made from a recording (`recorded`, passed
  to `setContext`) also What was decided?, What are the open questions? and Action items as
  tasks (2026-11-21, the user's call). A selection over `MOST_ASKED` shows "Select
  less to ask Felix" in the Felix bar.
- `markdown.js` escapes first and numbers checkboxes exactly like
  `notes::checkbox_line`; `MARKER` is the one list-marker definition and
  `listLevels` the one forward pass; leo-core's `notes::checkbox_line` stays
  context-free and counts the same lines (a box always makes an item).
  `interactive: false` makes boxes read-only. ```svg is a sandboxed
  `<img src="data:…">`; outside images stay links.
- Caret/selection conversion uses UTF-16 offsets throughout. A box toggles only
  from a click within `DRAG_CLICK_DISTANCE`. A Tab/Shift+Tab that changes
  nothing writes nothing. macOS `--` replacement corrupts text, so `look` turns
  spellcheck off for tables, code, math and lines starting with `-`, `|` or `:`.
- Saving: autosave PATCHes 700 ms after the last change with `base` = the loaded
  `version` (FNV-1a); 409 → "<title> (conflict from leo)". `saving.js` keeps
  edits that did not reach leo and retries them on the next load; a note of
  any size saves (see Serving and security). Up to
  `MOST_UNDO` page undo steps per note, each only its changed span. A keystroke
  hands the changed lines along and joins nothing (`doc.js` `changed(edit)`,
  `source()` joins once per change); the draft is written at most four times a
  second, then before a save, on hide and at quit; drafts over 256 KB
  (`SMALL_DRAFT`) are one IndexedDB record (`leo-drafts`), and `saving.settle()`
  makes every edit safe or gives up after 5 s (2026-11-17, the user's call: no
  typed text is lost). Navigation serialises the departing
  save; a failed save keeps the editor and address; only a draft persisted in
  this browser may offer Leave, draft kept (rechecked at activation).
- Pictures: `<notes>/attachments/` (`leo_core::attachments`, sniffed
  PNG/JPEG/GIF/WebP only), of any size, streamed to disk after their first bytes are checked
  (`PUT /api/pictures/{name}`, body limit off, 60 s without a byte is a refusal; 2026-11-18, the owner's principle 1); `/api/image` streams a picture back in 64 KB pieces. Upload names (`safe_file_name`) drop
  trailing dots/spaces and dash Windows device names (`CON-.pdf`); older stored
  names still open (`stored_file_name`). Counted per linking note; removed by the
  rule in root CLAUDE.md, Dependents (`dependents::pictures_asked`: every text file in
  the notes folder, live and held recordings, task descriptions and chat text can
  name pictures. Saved chat rows carry quiz and study content. Typed recording
  points live in session or transcript files. The pass scans outside the notes
  lock and checks counts, revisions and the saved file digest before deletion).
- Browsing never calls an AI. The explorer is the only folder listing: `#/` is
  an empty workspace, legacy `#/f/<path>` selects the folder then replaces the
  hash with `#/`. Explorer actions capture their target before opening a menu
  or sheet; moving or trashing the open note's folder flushes its draft first.
  Never two rows highlighted. Saved open folders (`leo-side-folders`) hold only
  folders the user opened; reveals (`side.revealed`) are never saved.
  Note rows edited less than 30 minutes ago carry a 4 px ink dot after the title
  (`.side-fresh`, `SIDE_FRESH_MS`; not when sorted by Last edited, which shows the
  time); one timer set to the earliest expiry (plus `SIDE_FRESH_GROUP`, 1 s, so
  notes saved together clear in one redraw) redraws the explorer, re-armed if it
  fires early, checked again on focus and visibilitychange (2026-11-15). Unprompted
  explorer redraws (expiry, Felix's read starting and ending, queued off the read)
  go through `sideRedrawSoon`, which waits while any menu is open.
  A note leo writes in the background (a recording, an uploaded file) carries a still blue
  ink blot after its title (`.side-unopened`, replacing the dot) until it is opened, also
  after a restart; a closed folder holding one carries it instead (2026-11-24, the user's
  call). leo.db `unopened_notes` is written under the notes lock before the note is saved,
  so the stamp change that shows the note shows its mark; it is a dependent row of the note.
  `GET /api/notes/unopened` answers id to folder, read with the folder list; a note's GET
  says `unopened` and `showNote` then sends DELETE `/api/notes/{id}/unopened`.
  Beside Filter: one sort button whose menu holds Sort by (Name, Last edited, Created),
  Order (the sort's two directions, e.g. A to Z / Z to A) and only open tasks
  (2026-11-16, replacing the flip button), driving `/api/notes` `sort`/`show` for every
  folder's notes (folders stay by name), saved as `leo-side-order` (2026-11-11).
  The explorer is kept simple (2026-11-16, the user's call): New note (with its
  shortcut) and New folder share one row (`.side-group`, still the top-level drop
  target and root context menu, their icons on one level), there is no Folders label,
  each folder row ends with its quiet note count (`.side-count`, 2026-11-21, the
  user's call) and note rows carry no icon, their labels keeping the folders' column. Rows
  are 32 px with an 8 px icon gap and a 16 px indent per level, and the current row keeps
  its weight: the pill alone marks it.
- Cmd/Ctrl+F on a note page opens find in note (2026-11-21, the user's call; nothing
  new on other pages): `noteFind` in `core.js` puts a `.note-find` bar in the note and
  searches `#doc` (Enter and Shift+Enter step, Esc closes and gives focus back, reopening
  the line editor it closed); `highlightSearch(root, query, at)` matches the phrase with
  no cap or timer and paints the current match as `leo-found`; F is in `SHORTCUT_RESERVED`.
- Quick Leo opens with Cmd/Ctrl+K only; Cmd/Ctrl+N
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
- Explorer reads (`/api/notes?rows=true`: no note text, hash or summary): 100 per page, four concurrent reads with ten-second aborts, 64
  cached folders, 32 automatic visible reads; late reads check generation and
  entry identity. An unchanged `/api/rev` stamp keeps a cached folder only if its last
  successful read happened under that stamp (`entry.rev`).
- Folders: create, rename, move, trash with Undo (POST `/api/trash/move`,
  `/api/trash/restore {ids, dirs}`, `/api/dirs/rename`); `Store::move_dir`
  refuses itself, its own subfolder or a taken name. New folders must pass
  `filename::portable_folder`; slashes are valid in titles, never folder names.
- Combine (`combine.rs`): POST `combine` starts a background job
  (`AppState.combines`, at most 20, kept 6 h, metered `combine:<id>`); review
  saves through version-checked POST `combined` or DELETE `/api/combines/{job}`;
  Undo is POST `uncombine`. The AI writes one note, leo puts back anything it
  dropped. Source parts are sized by the provider's graph budget (≤20,000
  characters per source, 40,000 per request; no total: 2026-11-17, the owner's principle 1; the parts are
  read one at a time, the start answer carries `parts`, the job view `done`, and DELETE on a working job stops it);
  fenced code and display maths stay whole; a closing fence needs the opener's
  character and at least its length with no trailing text. Two combines at once,
  15-minute limit on each answer (the clock restarts after every answer, and a wait on the writer is cut off at it); any failed part leaves both notes
  unchanged. Saving copies the other note's transcripts (`recording::copy_to`)
  and gives it the other note's original files (`share_originals`) before the
  notes lock is taken; both versions are checked again after the copy, and a
  change in between removes the copies and answers 409.
  In the explorer a note dropped on a note moves it unless held on that row for
  `SPRING_MS` (arms Combine: `.combine-wait`, then `.combine-armed`; the folder
  is not lit); the note menu's Combine into… (`explorerCombineMenu`) lists the
  first 50 notes by title and finds any note by typing, through the page's search, with
  Show more for further pages (2026-11-17, the user's call: no fixed limit on data;
  overturns 2026-11-09).
- Search (`search.rs`): titles and acronyms, text, graph concepts and summaries,
  then meaning (`vectors.rs`, `CLOSE_ENOUGH`); hits carry `why`. Text matches for
  words of three or more characters come from the index (`search_index.rs`) when it is in
  step with the store, else from the notes themselves. Every word
  counts, a repeated one looked up once; a query of more than `WORDS_UNDER_LOCK`
  (16) different words is matched on a copy outside the notes lock. Results are
  read a page at a time (`offset`, `limit`, `x-total`; a given `limit` is never
  cut, none means 300); exact ties go by note id, and later pages come from the
  ranking kept for the newest search (`AppState.searched`, dropped when the
  store, the vectors or the graph cache change). Result cards
  mark matches with `<mark>` in ink and a pencil underline (`.card mark::after`,
  three masks by `nth-of-type`, a plain underline in forced colours); the in-note
  `::highlight(leo-search)` keeps its strong fill. Reading
  position `/api/notes/{id}/reading`; related notes `/api/notes/{id}/related`.
- Nothing reflows the writing column: the outline gutter and the margin are
  reserved before the first heading or comment exists.
- Resizing stays cheap on long notes (2026-11-21, measured in WebKit on a 200 KB note):
  `.note-layout` is not a size container (a container query re-resolved every note element's
  style on each width change); it keeps the same containment through `contain`, and `leoDoc.mount`
  sets `data-wide` on its `.note-layout` (`OUTLINE_WIDE`, 900 px of room) before the note is drawn, on each window
  `resize` and a frame after each ResizeObserver report (toggling inside the callback resized `body` under the page bar's
  observer, a ResizeObserver loop error in WebKit), read through
  `:where(...)` so specificity is unchanged. `.note-layout` pins the page-layout variables
  (`--side`, `--right`, `--mid`, `--explorer-w`, `--chat-w`, `--dock-room`) so a
  pane drag or the Tasks card restyles only the shell, never the note: nothing inside a note may
  read them. Note prose has no `text-wrap: pretty` (WebKit re-balances every paragraph on each
  relayout). The page-width drag writes the article's own `max-width` and commits
  `--note-width` once on release. The explorer's three parts (`.side > *`) pin the same
  variables and `#doc` pins `--margin-w`, so folding the explorer or opening Felix restyles
  neither the explorer's rows nor the note (2026-11-22, measured in WebKit): nothing inside
  them may read those variables. Opening Felix only refits the explorer's width
  (`sideResize.fit()`), never redraws it, and `refreshPanel` leaves a subtree equal to the new
  one alone (`isEqualNode`), except around `LIVE_STATE` controls, whose values the markup
  cannot show.
- Outline (2026-11-17, the user's call: no fixed limit on data): every heading
  (excluding code, maths, quotes, tables), on the rail 24 ticks at a time
  (`OUTLINE_PAGE`, only that page's ticks are drawn) with Earlier/Later arrows;
  each heading row names its `block`, and the current heading is found by binary
  search over those blocks' positions, touching only the previous and new
  button and tick. The closed heading list uses `display: none` so WebKit skips
  layout work for its rows until the disclosure opens. The preview is
  ≤24 lines / 800 UTF-16 units, escaped, always beside the rail; preview updates
  never redraw the editor.

Decided: Obsidian-style live preview, nothing jumps when a line opens
(2026-11-02); `/` quick menu writes plain Markdown (2026-11-09); tags are not
read (root CLAUDE.md, 2026-11-17).

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
  words, never paste its prompts or code. Jane Street's socratic-tutor (MIT) lent
  the rule that an idea is shown only by two right answers (2026-11-16, the user's
  call): `scoreSession(session, quiz, correct)` counts a right answer that is not a
  guess, not after hints, not a step with `more` and not a retest of the idea whose
  last answer was wrong (the page sets `more` on that one, whether it comes in the
  marking reply or after Next question), and closes the idea at two; sessions stop
  at 30 questions. Each study turn ends with `Session::state` (`<study_session>`:
  every idea's standing and what to ask next, after the conversation so the cached
  prefix stays), so Felix picks open ideas without refused calls, and `ENDED` and
  `SESSION_DONE` carry `Session::unshown`. Refusals say to correct the call without
  mentioning it. Felix keeps no running score.
- Warm sessions (`warm.rs`): up to three, sharing a 480,000-character ceiling,
  idle for the provider's prompt-cache lifetime (`warm::idle_for`, by `Spent.kind`: Codex 30
  minutes, OpenAI's documented minimum for GPT-5.6 and later; others ten,
  2026-11-21, the user's call), Settings changes clear all; eviction and process cleanup run
  outside the slot mutex and off async workers. Only unchanged history plus the
  last reply/new message can continue. Automatic note retrieval is separate from
  warm capacity: reducing excerpts must not evict a long conversation early.
- Idle compaction: 30 s before that lifetime ends (`warm::compact_after`, sent
  as `compact_after` on each answer's `done` line; ten minutes, `COMPACT_IDLE_MS`,
  for providers without a known cache, 2026-11-21, the user's call) the page POSTs
  `/api/chat/compact`; with at least `COMPACT_LEAST_CHARS` (3,000) before the
  last `chat::COMPACT_KEEP` (4) turns the server drops the warm session and
  summarises in the background (`kept_from`, hash-checked). The chat on screen
  is unchanged.
- Long chats: `turns_for(room)`; the message being answered and the newest long
  earlier message keep up to `MESSAGE_CHARS` (32,000), Felix's newest reply
  `LAST_REPLY_CHARS` (12,000), full at `FULL_ROOM` (32,000), never below
  `TURN_CHARS` (4,000). Warm follow-ups and steering (`steer::MOST_CHARS`) use the
  same cap. `clip_turn` closes a fence it cut and says what was left out.
- Documents: `read_document` takes several documents (comma-separated) and `count`
  parts in one call, at most `READ_PARTS` (3) parts' worth, naming what it left; `HELPING`
  tells Felix to read every part a study request covers first (2026-11-18).
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
  step, in one `.msg-status` row at the top of the reply, worded in the present
  tense (`doing`, keyed by the step's verb; unknown or failed steps keep their
  words) in `--felix-letters` italic `--faint`, the Felix serif in chat chrome (also his name on replies),
  because it is Felix speaking; the finished summary adds " · N s" only for
  replies timed on this page (`timed`) (2026-11-15). A broken agent
  session sends `{"reset"}` and is redone through the text protocol
  (`FALLBACK_TRIES`). Felix has no web tool of leo's own.
- Panel layout: docked beside the note (1200 px and up) the note's page ends at
  the panel with an 8 px `--ground` strip between them. Attached notes, passages
  and files stay above the composer (`#chat-refs`; files are 52 px tiles, notes
  `noteCard`s); the note finder (`#chat-pick`) is a popover at the paperclip and
  never pushes the layout (2026-11-09).
- Sources under an answer (2026-11-23, the user's call): one source stays the
  plain line (`div.msg-from` with its "Sources" label); two or more fold into
  `details.msg-from` ("N sources" and the step chevron), one row a line when open,
  its open state kept per message (`openSources`) like the finished-steps summary.
- Change tools become `Proposal` cards, validate against the store, run through
  POST `/api/felix/act` (reusing normal routes) and return the inverse as `undo`.
  edit_note/create_note Apply POSTs `/api/notes/{id}/suggestion` (409 unless
  `find` is there exactly once). delete_folder and delete_board are always a
  card, even in Auto. add_margin_note's quote must be in the note and it never
  changes the note. `open_note` reads bounded `part`s that preserve every
  Unicode character.
- Successful replies may contain only proposal, ask or quiz cards. Stopped means
  cancelled; saved replies keep elapsed ms and a cancellation flag.
- Quizzes: marking never waits for a Felix reply; two marks at a time; an
  abandoned mark, rewrite, Settings test or document read stops its AI program
  (`stop_when_dropped`). Multiple choice (needs
  `explain`) and matching fill-ins are marked by the page; the rest POST
  `/api/quiz/mark` (quick model, `MARK`, with `confidence` validated). The card
  keeps all thinking. To mark or continue, Felix reads its first 2,000 characters
  (`THINKING_READ_CHARS`, `MOST_THINKING` in `aside`). The box shows the read
  budget through `data-reads`. How sure is a 0–100% bar (`q.surePct`, `sureOf`
  maps ≥75 sure, ≥40 think, else guess for `SURE`); thinking is sent only while
  its switch is on (2026-11-09); the switch is toggled in place and its box slides open and shut with
  `leoMotion.slide` (2026-11-18). Hidden grading accepts only the exact verdict
  words; a hidden check is authoritative for the card; an invalid check leaves
  the answer ungraded and releases the warm session. Predictions (`predict:
  yes`) are never marked, scored or counted. `quiz_from` shuffles options and
  refuses `gives_itself_away`. Sessions stop at `MOST_SESSION_QUESTIONS`; review
  (`review.rs`) brings misses back after 20 h; a mark lasts as long as its chat. The `aside` answer line must stay
  one line.
- Marked quizzes read like a teacher's marks (2026-11-15, the user's call:
  green and red kept, in muted chalk tones): `--chalk-good`/`--chalk-bad` (one
  pair per theme, ≥ 4.5:1 on every chat surface, `tokens.test.js`) outline the
  right and wrong options and the card, colour the verdict words and `.verdict`
  badges, and stroke the `.quiz-mark` check or × (`role="img"` with "The right
  answer", "Your answer, right" or "Your answer, not right", so shape and words
  carry the result, never colour alone; `--chalk-break` in dark). A mark writes on
  over 160 ms (`QUIZ_MARK_MS`, `pencil-draw`) only within 160 ms of the answer
  (`markedAt`), resumed by elapsed time, so a redraw never replays it.
- Edit proposals are marked like proof (2026-11-15): `leoMarkdown.wordDiff`
  (a word LCS, capped; null falls back to `.proposal-old`/`.proposal-new`) into
  `.proposal-marked` with real `<del>` (line-through, `DELETE_MARK` loop) and
  `<ins>` (`CARET_MARK`), no red or green. The study strip's ideas carry
  `.study-mark` circles with the same chalk check and × paths and
  `--chalk-break` mask, a heavier ring on the idea being asked; forced colours
  draw them in CanvasText without the mask.
- Routing (`routing::light`, widened 2026-11-21, the user's call): short lookups,
  greetings and small formatting edits of something named (turn/convert/make this
  into a checkbox, bullet, heading, list or table, fix typos, rename, bold, sort,
  tidy, each naming this/these/that/those/them, or a selection except for rename)
  go to the quick tier; study, teaching, quizzes,
  analysis, why/how, comparisons, plans, summaries, new lists (make/put together a list), other note changes,
  attachments, files, Auto access and >6 turns stay on the chosen model, and so do
  other languages. A plain English question the rules cannot place reuses the note
  search's query vector (`vectors::close_and_asked`; `routing::leans_light`: cosine
  to centroids of 40 light and 40 full examples, built once in the background,
  light only past `MARGIN`); no vector, no centroids yet or a close call keeps the
  chosen model. No AI call routes. A chat moves up once when needed, never down while warm
  (`routing::choose`, `Slot::light`); a failing light writer gives way on the last try, so `Auto`
  names who really answered. `Prefs.auto_model` turns it off.
- Pinned chats (2026-11-23, the user's call): `chats.pinned` (an added column, `VERSION` stays 7),
  PATCH `/api/chats/{id}` `{pinned}` (204, 404). They head the history in a Pinned group, newest first;
  a pin never moves `updated_at`, and Keeping's Chats choice and its count skip pinned chats.
- With a note open, a This note group follows Pinned (2026-11-24, the user's call): chats with a question
  asked on that note (`note` on the user message: the note Felix was reading), with it attached (`refs`) or about a
  passage in it (`selection`); never a search source. `chats.notes` is a JSON list of ids, added to by each
  save, worked out again when messages are dropped, and filled once from stored messages when the column is first added; `note` reaches no prompt or hash.
- Copy/Retry/Edit drop the last pair with POST `/api/chats/{id}/drop`
  `{count, total}` (409 unless the saved chat has `total` messages).
- Files for Felix (`chat_files`): any number per chat (2026-11-17, the owner's principle 1); a message's documents share `DOCS_CHARS` and only each share is read (`chat_files::texts` reads each head and `part` any part from the database; past ten, `documents_within` charges names and wrapping to the budget and counts the rest, reached by `read_document` `document: "list"`); text only, of any size (2026-11-18, the owner's principle 1): the file arrives as a streamed
  `PUT /api/chats/{id}/files/{name}` into `<data>/incoming/`, is read through `Powers.reader_to` into `chat_files::Intake`, and a text over
  `KEPT_WHOLE` (2,000,000 characters) is kept in pieces of at most `PIECE_CHARS` (64,000) in `chat_doc_parts` (`chat_docs.cuts` lists their lengths; `text` still holds the first 2,000,000 characters and the closing note, which is all an older leo reads); `part`, `texts` and the export read a piece at a time; a document's pieces go with its row (a trigger), and strays are swept by `tidy_orphans`; deleted with the
  chat, with its review marks. Add to note (`routes/insert.rs`) is version-checked and request ids
  dedupe. Chat titles are local; history shows titles only.
- Felix draws only when asked to see, draw or visualize something (2026-11-14,
  replacing the 2026-11-09 threshold): one ```svg, one picture unless it unfolds in order, then 2 to 5 steps (viewBox 320 wide,
  `var(--text|muted|accent|accent-soft)`, top-level `<g data-step data-caption>`),
  never Mermaid or text characters; margin reads never draw; `drawing::drawn`
  runs only when `drawing::asks_to_see` the user's message. `diagrams.js`
  `stepDrawings` parses it with `DOMParser` only to build step variants and fill
  in theme colours, and shows them as `<img>` data URLs, never in the page.
  User-written ```mermaid still renders. `BASE` stays under 4,500 bytes.
- `REPLY_STYLE` is not changed by prompt-size work. Escape belongs to the owner
  inside the page first; an open sheet always wins.

- His face reacts to the pointer (2026-11-21, the user's call, beside the tap reactions `TAPS`):
  a double click hugs him (`hugged`, click `detail` ≥ 2; `hug`: squeeze, blush, heart, a snuggle
  sway, `HUG_MS`), and from the `HUGGED_TOO_MUCH`th hug with less than `HUG_GAP_MS` between them
  he squints (`squint`: pixel `>_<` eyes, a sweat drop, leaning away) until he is left alone; the
  hug wins over a transformation started by the double click's first click. Under reduced motion
  both show the face without moving.

Decided: no modes, the agent decides (2026-11-05); marking never waits for
Felix (2026-10-19).

## Margin (`margin.rs`, `routes/margin.rs`, `web/margin.js`, `app/margin-notes.js`)

- Storage: `margin_threads`, `margin_modes`, `margin_state`, `margin_personas`,
  `margin_instructions` in leo.db, never note files or git. At most `MOST_OPEN`
  (24) open threads per note (held inside the INSERT and on reopening; a read-along saves up to 48) and
  `MOST_CLOSED` (48). Replies only while open, never two at once for a thread
  (`MOST_REPLY` 4,000, `MOST_MESSAGES`, the last 8 sent). `/reply` and asks (POST
  `.../threads`) take `request` (a uuid per typed message or question, kept on retry) and check
  `insert::remembered` first (a reply before the replying guard), so a resent reply or ask gets
  the first answer and one AI call; a reply resent while it runs is 409 `busy`.
- A thread whose passage is gone is not shown (2026-11-09; there is no lost-place
  box).
- A passage found only by its context (`marginKit.find` answers `how: 'context'`), or on
  one of several copies its stored context cannot tell apart (`how: 'guess'`: no copy has 6
  characters of its old surroundings matching, or another copy matches as well), keeps its
  note, with one muted line, "The passage changed since Felix wrote this."
  (`.margin-changed`, set by `marginFill`); an exact match shows nothing. A guess is never
  sent as reworded text. A kept note (not `margin::replaceable`: an ask, Read through,
  chat-made, pinned or answered note) found by context is re-anchored 2 s after editing
  stops (PATCH `.../threads/{id}` `{version, quote, prefix, suffix}`, context from the
  found offset, `margin_threads.reworded`, which keeps the muted line); untouched
  read-along notes never are, so the 2026-11-16 replace rule holds.
- An open thread shows its last 6 messages (`SHOWN_MESSAGES`); older ones sit behind a
  "Show N earlier" button (`.margin-earlier`) that shows them all for that note. From
  `LONG_THREAD` (6) messages it ends with "This has turned into its own question.
  Continue in chat or make it a note." (`.margin-long`; the links are the thread's own
  chat and Add to note actions).
- Notes the page cannot place do not fill the limit: `marginDraw` collects their ids
  (`marginPage.gone`, by `marginKit.find`) and asks, Read through and reopening send
  them as `gone`; the open limit is then `min(MOST_OPEN + n, 2 × MOST_OPEN)`
  (`margin::room`, `n` counts only this note's open ids), passed to `put` and
  `set_status` as `limit`.
- `marginDraw` keeps each thread's `find` answer (`marginPage.finds`, keyed by its
  anchor) and, while typing, re-finds only threads whose passage or context the edit
  touched (`marginKit.edited`, `carried`); a full pass runs 400 ms after typing stops,
  in idle slices (`marginSettle`). `marginLoad` returns before drawing when the fetched
  threads, resolved passages, mode, persona and instructions equal what is shown (a
  return to the window).
- Resolved passages stay findable (2026-11-14, the user's call: no Undo toast): GET
  `.../margin` adds `resolved: [{id, quote, prefix, suffix}]` (`margin::resolved_on`: status
  resolved, newest first, no messages, never dismissed). The page keeps them in
  `marginPage.resolved` (never in `threads`) and `marginRevisits` draws a dotted underline
  (`.margin-revisit`, text at 20%) for each it can place: a button on the first line (title
  "Reopen Felix's note", focusable) and aria-hidden spans on the others, a 5 px strip below
  the text line so the text stays editable. Click or Enter/Space runs `marginReopen`: POST status `open` with
  `gone`, then the returned thread opens. Resolving, and a read's tidied ids, join the list.
- Whole notes (2026-11-15, the user's call, replacing the first-line fold): a
  closed note shows Felix's first answer (`marginFill`; Copy, Add, print and chat keep
  the latest, `marginNow`, and an arriving reply never overwrites a closed note) and dims to .7 until lit. The layout is first
  computed with the text as a whole cut at 12 lines, the last shown block with a fade
  (`.margin-cut`) and the blocks after it hidden (`.margin-over`), both set by `marginCuts`
  (one read pass, then the writes) only when the text overflows; then `marginGrow` lets each
  cut note take the free room under it on its side (`marginKit.spare`: the next note's top
  less the gap, Infinity for the last, so a note alone shows whole and nothing moves), taking
  back the pixels paragraph margins add when hidden blocks return. A note that ends below the
  page adds the difference as `.note-columns` bottom padding, since `.note-layout`'s `contain`
  keeps absolute notes out of the page's scroll. Figures always show whole. Pins: `margin_threads.pinned`
  (guarded `ALTER`, db `VERSION` 7 (5 added `reworded`, 6 `closed_by`, 7 chat message rows), a revert keeps 7; a revert never lowers `VERSION`, in `margin.json`), PATCH
  `.../threads/{id}` `{pinned}` (or `{version, quote, prefix, suffix}` to re-anchor, below the body limit); a pinned note stays open in every layout
  (`marginPinnedOpen`: Escape and a click outside never fold it, several can be open;
  unpinning folds it unless it is the one open) and a newest read-along note
  (`marginPage.newest`, `data-newest`) stays undimmed (2026-11-16, the user's call). An open thread's
  messages sit right of a 1 px `--line` rule; its one action row is Resolve,
  Dismiss, Add to note, expand, then pin and chat in `.margin-tools` at the
  row's end. No ×: Esc (anywhere, a sheet or menu first) or a click outside closes, a typed reply staying put; `.margin-toggle` is the
  closed opener and focus target and takes no clicks while open. A closed note shows a round ⋯
  (`.margin-menu`, 2026-11-23, the user's call) at its outer top corner while hovered or focused:
  Open full screen (with a drawing), Resolve, Copy, Pin open, Add to note and Dismiss in `popupMenu`.
- Width: `marginLayout` sets `--margin-w` each pass from `marginKit.arrangement`
  and `marginWidth`: notes autosize to the room beside the note's `max-width`
  (Right 240–360 px beside a column of at least `LEAST_COLUMN` 600; Both 180–360 px
  per side). Both holds on as long as it stays readable (2026-11-15, the user's
  call): notes shrink to `LEAST_SHARED` (180 px, about 22 characters a line) first,
  then the column to `SHARED_COLUMN` (520 px, about 65 characters); below the chosen
  layout's room (`ROOM_BOTH` 992 px, `ROOM_RIGHT` 928 px of columns) it becomes marks,
  never one side (2026-11-16, the user's call). It never moves every note right to make them wider. A closed note
  in the left margin is right-aligned toward the page (`data-side`). Marks (2026-11-16)
  are a 13 px ink blot just after the passage's last word, never over the text (a thread with replies
  shows their count beside the blot, `9+` past nine, in the mark's aria-label too, and its slot is
  36 px wide, 2026-11-21, the user's call): `marginSlots`
  puts an empty aria-hidden `.margin-slot` (inline, 21 px of padding, hidden in print) into
  the rendered text at the passage's end (`doc.domPoint`, text nodes only, never in a line
  editor or a code, table or maths block) before anything is measured, and the blot sits on
  it; side by side when two end together. A slot that would make its block taller is hidden, so
  opening a line never moves a block or leaves a blank line. A passage without a usable slot (an open
  line, a quote, code, table or maths block, or a slot that hangs past the end of its line) has its blot
  in the 16 px right of the column (`data-aside`), level with its last line and stacked down when two
  meet; a read's blot sits just after the last word (`marginPassageEnd`), or beside the column when that
  would not fit in it. No blot or count is ever over text (2026-11-24, the user's call). A slot's fit is
  measured again only when its block's size or the fonts changed (`data-fit`). A window resize
  redraws in the same frame. Opening a line redraws (`focusin` on `#doc`). An open or pinned note sits
  under its block as a raised card (`--raised`, `--shadow-pop`), the block's `--margin-room`
  holding its height (pinned cards stack, placed in document order); it unfolds with a
  clip and the room over `OPEN_MS` only when the user opens it (not on load or a layout
  change) and folds away over `OPEN_MS × EXIT_SHARE` before it closes (`marginFoldAway`,
  tracked by thread, inert while leaving, still drawn open until the fold ends:
  `marginIsOpen`), none under `stillMotion()`. `marginKit.mostColumn` is the
  widest column the chosen margins allow (Infinity when they are marks). With Felix's panel open
  (2026-11-21, the user's call) the room left decides, never the panel itself: notes stay beside a
  wide window and become marks in a narrow one (`marginCovered` takes off what an overlaid panel
  covers; a docked one already narrows `.note-columns`). A block the doc replaces (its line editor
  closing) redraws at once (`marginPage.watch`, a childList observer on `#doc`), so a passage's slot
  is back before its blot is drawn.
- Deletes: a trashed note keeps its threads; deleting from the trash removes them
  (`finish_held_rows`) and its reading place, task details, recording sources and original uploads
  (`dependents::clear`), and so do trash expiry and the hourly sweep of notes gone
  outside leo (the sweep also tracks kept audio and the originals and transcript
  folders, counts only notes `Store::confirmed_gone` names, starts its clock again
  whenever it cannot run, and returns the missing ids to `dependents::clear`, which
  checks again under the notes lock and never follows a link); expiry and the sweep
  first move the files to `<data>/held/` (Trash rows "Files of a removed note", with
  Restore) and delete them with the rows a trash period later; a removal that fails is retried on the next delete and daily
  (the `deleting` set in `margin_state`), a recording in use is skipped, and nothing is
  removed for an id that is live or in the trash again, or while a note or trashed file
  cannot be read. `sweep_kept` copies the live ids and the trash index, checked against
  the files' sizes and times (`Store::trash_snapshot`, no file opened unless it changed), under
  the notes lock and `sweep_store` works outside it; it waits `SWEEP_AFTER_DAYS` (7) and never runs
  while no notes loaded or a file, trashed or live, is unreadable. Export adds `leo/margin.json`
  (personas and instructions are not exported).
- Modes `along` (default for notes leo made after `Prefs.since`; a v4 id, never a
  headerless file) or `ask`. A note's own row wins; turning the default off pins
  notes made while it was on (`margin::pin_modes`, before prefs are written). A
  fresh install stores `since = now` on its first `prefs()` read.
- A read-along sends only changed lines (2026-11-24, the user's call: a line whose words did not change is never
  read again): `marginKit.changedBlocks` compares sentences without list, checkbox and quote markers or whitespace,
  so Enter, an empty bullet, a ticked box, a renumbered list and split or joined lines send nothing; each run of
  changed lines (20 characters or more) is one block, and code, maths and tables go whole.
- Read-along POST `/api/notes/{id}/margin/read` `{version, blocks}`: the server
  refuses a stale version (409), a note not `along` or not edited in this engine
  run (403, `Reads::edited` set only by PATCH, so browsing never reads), a
  streaming reply, the gap, overlapping reads and the daily cap (429 with `why`).
  `Reads` takes its lock the way `Db::with` does, so a crash inside it fails one
  read, not every later one.
  The cap is checked before the model is asked and spent in one transaction only after it answered (silence counts, a failed read does not; the note is saved even if another read used the last one).
  A read-along never refuses for a full margin (2026-11-14, the user's call): it saves
  with `limit` 2 × `MOST_OPEN` (the storage guard, 48), and at 48 open it frees
  the oldest open note that is not pinned, not in `keep` (a list: the thread open on the
  page and every thread with typed text; a string or null is still accepted), not a note
  the new one replaces and not being replied to (`margin::free_oldest` picks it and holds
  its reply guard; `margin::put_replacing` resolves it in the transaction that stores the
  new note, only if the note would not fit otherwise, so nothing is freed unless the new
  note is stored), listing it in `resolved`; with none free it answers `{failed: true}` with `margin::FULL` (no Settings problem), before the model is asked or, if the last free note was taken meanwhile, after it (its blocks are forgotten, so nothing is spent and the text is read again). The page never drops the open thread or one with typed text from `resolved`: it POSTs status `open` for it, and deletes a new read note that overlaps a thread it holds that way (showing it if the delete fails).
  Only Read through still answers 409 `full`.
  Quiet tidy (2026-11-14, the user's call): after a read-along note is saved, while more
  than `TIDY_TO` (5) notes are open, the least recently updated ones are resolved
  (`margin::resolve_untouched`) if they have `source` `read` (not asks, chat-made or
  Read through notes, which are saved as `through`), no message from you, are not pinned,
  in `keep`, being replied to or the note just saved; their ids join `resolved` and the page
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
- A read-along also sends up to 1,500 characters of the note before the first changed block (`margin::earlier_text`: its last occurrence, cut at a block boundary, fenced as `earlier_text`) as context only; the model's raw quote must lie inside a changed block, else the read is silent; a read whose blocks left the note meanwhile keeps a quote only if it is unique. Read through sends none.
- The model's quote must be in the note now; reads grow it to the whole
  terminated sentence(s) it touches (`margin::whole_sentences`, else unchanged),
  then `margin::crowded` allows one open note per sentence; for read-along a dismissed note keeps its
  sentence too while its words are in the note, and one you resolved yourself (`margin_threads.closed_by`), not one Felix tidied away (`margin::declined_on`), never in Read through. A reworded passage still counts
  (2026-11-16, the user's call): read and Read through send `changed` (`marginChanged`: each
  open thread the page places only by context, as `{id, text}` of its current wording, at most
  2 × `MOST_OPEN`). The server places each text itself between that thread's stored context
  tails, as the page's `byContext` does (`margin::by_context`: its quote gone from the note, each
  stored tail cut to at least `LEAST_TAIL` (6) UTF-16 units, the text ≤ 2 × the quote + 120 UTF-16 units), else ignores it, and compares sentences by position
  (`margin::crowds_at`). A read-along note landing on such a sentence replaces every old one there
  (`margin::put_replacing`: one transaction that resolves them and stores the new note, freeing
  the oldest free note too when the margin is still full, listed in
  `resolved`, only if each is a read-along note, not pinned, not in `keep`, not being answered and
  has no message from you); else, and always in Read through, the new note is dropped and the
  old ones keep the sentence. A note that is still one sentence (`margin::one_sentence`) has no
  context to place a reworded comment by, so a read-along note there also replaces any open
  read-along comment whose passage was the whole note and is gone (no stored context, untouched,
  not pinned, not in `keep`); the page could only hide it. `marginRevisits` draws no reopen
  underline under text an open note covers. The server anchors on prefix + quote + suffix exactly and
  never falls back to another occurrence. An ask or re-anchored thread keeps
  the whole selected passage (at least 3 characters). Felix reads its first
  1,000 characters when answering, as the toolbar and reply area indicate.
  Read-along and `add_margin_note` still use `MOST_QUOTE` (600) for model-chosen
  anchors. The selection toolbar (`passages.js`) has Felix this
  (fixed ⌘↵/Ctrl+Enter), Ask Felix and Visualize; the two quick ones are asks
  with a preset question (`ASK_PRESETS`). It opens in the right margin beside the
  selection when the layout has one (`.ask-float.beside`), else above the selection (below
  its last word when there is no room above; 2026-11-21, the user's call); `marginDraw` places it
  again whenever the margin layout changes, so a resize never leaves it beside a margin that is gone.
  Ask Felix opens the margin draft without asking (`marginAskFirst`: Felix-blue box,
  Escape keeps a half-typed question per passage in `marginPage.drafted`); a passage
  that already has an open thread opens it instead (`marginSame`). Both margins puts
  a note on the side with fewer notes when that side is free at its height, else the other
  (ties right; `marginKit.sides`; 2026-11-22, the user's call, replacing right first).
  Margin icon buttons show the shared tooltip (their aria-label, `::after`), no `title`; the floating note toolbar's `.actions` rules are gone.
  Felix this on such a passage only opens it (Visualize still asks in it). The open
  thread's tools are Copy (Felix's text), pin and chat; a folded thread says "N replies".
  The ask box shows "Enter to ask · Esc to close"; a draw ask waits with "Sketching it…"
  (`MARGIN_DRAWS`). A silent read leaves a faint aria-hidden "Nothing to add." after the
  passage for 2.6 s (`marginQuiet`). Inside a margin note the stack steps by 8 px (open thread
  indent 12 px, reply and action rows 8 px) and Add to note is a small plain button on `--surface-2`
  with the control radius, not a pill (2026-11-21, the user's call). A resolved note stays folded to one line of muted text (13 px, no
  fade) under a small "Resolved" label until the note is left (`marginPage.faded`; the label and the
  fold exist only for `data-resolved` notes that are not open, 2026-11-21, the user's call); clicking it reopens it. A new stepped
  drawing plays once (`playDrawing`, 1.1 s a step plus 20 ms a caption character, none
  under Reduce Motion, never when its step is remembered); ↻ plays it again.
- Reads use `Powers.toolless`, never `quick` (`no_tools`: `--tools ""`,
  `web_search="disabled"`, chain cut to its first provider, so a read fails
  closed), under a `READ_LIMIT` stop; the engine waits `READ_LIMIT` + 5 s as a
  backstop. Replies and asks use `Powers.toolless_chat`; a disconnected ask,
  reply or weave request stops its writer. Every read is a fresh
  process, no warm session, one retry on a stall.
- Prompt safety: note text sits between tags with a random suffix and is never
  instructions; thread messages are one JSON record per line inside them.
  Plain messages use `textContent`; diagrams use the escaped renderer, strict
  Mermaid and inert SVG. Personas and note instructions go after the prompt's
  first paragraph (`margin::voiced`, `margin::instructed`), so the safety and
  format rules come last. The eight custom persona slots use engine-made IDs
  and keep full names and instructions. Per-note instructions are kept whole.
  Persona names are UI labels. Felix reads the first 500 characters of each
  instruction and the first 1,000 of a new margin question or reply. The boxes
  use the shared `data-reads` hint. `margin-notes.js` owns `updateReadHint` and
  the delegated listener in `uploads.js` uses it when available. `core.js` keeps
  the page's 500-character instruction budget with the note header. Threads keep
  full messages and tool-created margin notes. Prior thread messages still
  contribute up to 320 characters each to prompts. Ask, reply and re-anchor use
  Axum's 2 MiB JSON request limit.
- Settings says text stays here only for a loopback address and a model without
  `cloud` in its name (`ai::reads_stay_here`). Failure text is the writer's own
  sentence (`leo_core::writing_said`), `no_writer()` or `margin::PROBLEM`.
- Chat context: `margin::context` adds the open threads the page can place
  (`margin::shown`, the page's `find` rule; ≤3,000 characters and an
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
  while the note is hovered, focused or open, and always (fainter, 25%) for a pinned note and the newest read-along note (`path.tether.rest`, 2026-11-21, the user's call). A newly shown
  tether draws passage to note with `pencil-draw` (`pathLength` 1: tie 450 ms, then spine 350 ms),
  once per light-up; repaints keep the path, so they never restart it; none under `stillMotion()`.
  The solid underline (one per text line) and the spine are drawn with `marginWobble` (the pencil's jitter), phased by
  `marginSeed(thread id)` plus the line index, so a repaint, scroll or relayout never reshapes them; their
  endpoints sit exactly where a straight line's would, and the tie stays a plain cubic.
  `doc.spanRects` measures text nodes only, one rect per visual line (never a whole `li` box, and never KaTeX's hidden `.katex-mathml` copy); a display formula gives one rect as tall as it is drawn, clipped to its column, or one per row of a matrix or aligned block where the rows do not overlap (the resting mark takes that height through `--mark`), and a quote that falls inside a formula marks the whole formula (2026-11-22).
- Tests: `src/tests/margin.rs` drives the real engine over TCP with fake powers.

Decided: margin threads never in files (2026-11-04); read-along default for new
notes (2026-11-08); one note per sentence (2026-11-09); no lost-place box
(2026-11-09).

## Uploads, originals, recording

- POST `/api/import` runs the `Importer` on a thread; "What do you want" goes in
  `<what_the_user_wants>`. Several files make a note each (`run_each`); a failed
  file is listed in `error` and the rest are made. The box takes any length; the importer reads its first 2,000 characters (`MOST_WANTS`, a model budget) and the sheet says so under the box. `into` (a note id, 404 if
  gone, never with `each`) never touches the note: the job keeps `section` and its status shows only `section_chars`
  (a count: the text never reaches the page), and the page appends it through version-checked `/insert`, passing the job's id (`job`), not its text, so a section of any length is added (2026-11-18, the owner's principle 1; a job that is gone or not done is 404 with a sentence), so typing during an
  upload never makes a conflict copy.
- Originals live in `<data>/attachments/<note-id>/` (never synced), named by
  `free_original_name`; figures go to the AI via `leo_services::figures`.
- Uploads have no file count or size limit (2026-11-18, the owner's principle 1): each chosen file is
  sent as its own streamed request (`PUT /api/import/files/{upload}/{item}`, body limit off, two at a
  time from the page; `upload` and `item` are uuids the page makes, the file's name is not part of where
  it is kept) and written 64 KB at a time through a temporary file into `<data>/incoming/<upload>/<item>`
  (a second send of an item is 409; a full disk is 507; 60 s without a byte is 408). `POST /api/import`
  carries `{upload, files: [{item, name, type}], …}` and no bytes (8 MiB of JSON at most); the importer
  reads each `UploadFile.path`, and a note's originals are hard-linked (else copied) from there into
  `attachments/<note-id>/` before the job ends. The `incoming/<upload>` folder is removed when the job
  ends however it ends, and when `POST` refuses; any older than a day is removed at launch
  (`sweep_incoming`). Two imports run at a time; a third is `waiting` in a queue (`ImportSlots`) and the
  page says so. A PDF, Word or PowerPoint file is still read whole into memory by its reader.
- An original of any size can be read (2026-11-18, the owner's principle 1): `Originals::lines`
  asks `Powers.reader_to` (leo-services `import::read_to`, which streams a text file in 64 KB pieces) to
  write into a writer that holds 1 MB and spills the rest to `<attachments>/<note>/.read/<name>.txt`
  (first line: the original's length and modified time; reused while they match, written by temp file
  and rename only after the read succeeded, refused through a linked `.read`). A PDF or Office file is
  still loaded whole, one large one at a time. `Lines::iter` hands out lines of at most 64 KB, the same
  numbering held or spilled; `pack_part` keeps only the part asked for. The copy is leo's, not the
  person's: Keeping deletes it with its original, `forget_missing`/`forget_note` and the next read sweep
  one whose original is gone, and nothing counts, expires or exports it.
- Recording (`record.rs`, `recording.js`): capture runs in Rust through the
  listener; the page never opens a browser microphone or screen picker and the
  server takes no audio from the page. Native jobs have no page heartbeat.
  `capture_activity` is reserved before a job is created; its lease blocks
  updater installation through the durable save. Recovery lists only what the
  `Housekeeper` lists (`pending_page`, every unfinished recording newest first; recover
  and Delete look one up with `pending_recording`); a recovery job carries its session id.
- Live transcript (2026-11-21, the user's call): the 1 s poll redraws the live page
  around the kept nodes (`paintLive`/`graft` swap what surrounds them, at any depth; a kept node is never
  rebuilt), and `showTranscript` patches `#rec-transcript`, or on a call each
  turn's words (rows keyed by the turn's `n`, so a kept row is never rebuilt), with `tailMerge(shown, next)`: a trimmed head
  (`keep_tail`) is dropped from the front, the stable prefix stays, and only the changed
  tail (backed up to its word) is replaced, so selection and scroll survive. New words are
  a `.rec-new` span (`fade`, `--t-open`, none under reduced motion), folded back into the plain text on the next poll once its fade has ended and no selection touches the transcript, so a long recording keeps a few nodes (a call, a few per turn); text already said when
  the node is made arrives plain. Scrolled up, the view stays put and Back to latest
  (`#rec-latest`) scrolls down and resumes following. `aria-live="off"`: the node now
  persists, so polite would read every update. The `[data-keep]` nodes are the transcript
  panel `#rec-heard`, the tab list `#rec-tabs` (its count and selection are set in place by
  `syncTabs`, so keyboard focus stays) and Felix's column. A call's view carries `turns`
  (`live_turns.rs`, 2026-11-23, the user's call: live looks like the saved transcript; You and Others only,
  untimed): each side's whole text is aligned word by word with what it said before, so a rewrite keeps
  every word in its own turn; new words go on in the side's latest turn until their sentence ends or the
  other side has said 12 words, then begin a new one. `transcript` is the same turns as `You: …` /
  `Others: …` lines. Microphone and screen recordings stay plain text, since their speaker labels exist
  only once saved.
- Record page (2026-11-22, the owner's call: lectures and meetings share it; speaker UI only
  where speaker data exists; Start and the live controls stay red). Before Start: the title
  field first (`#rec-title`, note-title type, "Untitled recording"), then the event and folder
  selects as chips (`.rec-chip`, still `.select-control` selects), the Microphone/Screen/Call
  picture cards, What you want, Label speakers, the red Start and Felix's before-start questions
  under it. While recording and after Stop the page keeps one shell (`drawLive`): a note-like
  header (title, chips for event time, folder and kind, the hearing line) with one recorder bar
  on the right (`.rec-bar`: pulsing dot, clock, the wave at 120×24, Pause, red Stop and save);
  tabs Your notes (count, default) · Transcript · What you want (`kanban-tab` buttons, the tab
  in `s.tab`, panels toggled by `hidden`); Felix's three live questions in a 300 px `set-card`
  column beside the tabs from a 820 px `recroom` container, under them narrower. After Stop
  (`writing`) there is no bar; one progress line under the title (the leo mark in ink, "Writing
  the notes · step", the bar, Finish now = `rec-finish`), tabs Notes (disabled) · Your notes ·
  Transcript; the saved note opens as before. The view carries `title` and `directory`; the
  event is the page's own. The questions are chat `.starter` rows (`FELIX_ASKS`): before
  Start, shown once an event is chosen or What you want is filled, What should I listen
  for? and What should I ask? attach the event and the want (`briefAsk`); while recording,
  What did I just miss?, Sum up so far and What should I ask next? attach the typed points
  and the newest transcript (`liveAsk`, at most `MOST_ASK` 8,000 characters, the tail,
  since the server's `clip` keeps a passage's head). Each opens Felix's chat through
  `attachSelection({note: '', title: 'Live recording', text})` and `askNew` (`deps.chat`).
- Label speakers (2026-11-16, the user's call: per recording, for meetings,
  not lectures): a switch under the note fields, off by default, remembered as
  `leo-label-speakers`, shown for Call too (2026-11-22, the owner's call: a call
  splits only its Others track); it sends `profile.speakers`. With the speaker model missing it
  offers Download through `/api/speech` (`speakers` in the view, polled while
  downloading, redrawn only when it changes, focus kept); Start sends it only
  while the model is ready or downloading. The switch is `settingsSwitch()`,
  and the choice is kept in memory when storage is blocked. Settings → Speech
  models lists the speaker model under Downloaded with Delete.
- A note's transcript and original files open in the note's place (2026-11-22, the owner's
  call; the side pane is gone): `openAside` (sources.js) sets `.note-layout.aside-open`, which hides
  `#doc`, the margin, outline, related and `#note-facts` and shows `#note-aside` with its own facts
  line `#note-aside-facts` (the doc stays mounted; `closeAside` redraws the margin). The title stays;
  Back to note or Escape returns (refused with "Save your fixes first." while a fix is unsaved) to
  the scroll position and focus it opened from; after a redraw, focus goes to the same element by id
  or to the same original's chip (`focusAgain`).
  Transcript: facts "Transcript · <duration> · Back to note" (the duration once the last page is
  shown); Find in transcript (the server's matches and the page's row filter); `transcriptFlow`
  (recording.js) groups a page: with speakers (`speakersOf(head.speakers)`, not `Microphone`,
  `Computer audio` or empty) one turn per run of a speaker (tone dot, name, time once), else a
  paragraph a minute with its time; typed points sit at their time as "Your point · …"; a run of
  passages with `echo: true` (missing is false) folds into "Hidden: N echo lines · Show"; the fold keeps its own speaker (`who`) and shows it as "You · echo", and `sourceMatch` opens a fold (`showEcho`) before scrolling to a match inside it. Tones
  come from `head.speakers` order (`toneOf`: You in ink, others `--hue-0`, `-1`, `-4`, …), so
  paging never recolours. Each line is a `contenteditable="plaintext-only"` `.source-text` span;
  it pages through recordings (`?at=<id>&passages=<n|end>&points=<n|end>`, 512 KB of each kind a
  page, Earlier and Later; a pick list when a note has several; search matches across all of them
  open the page that holds each); a fix sends only the changed rows (`{source, base: revision,
  changes}`, batches under 1 MB); the speaker row renames a speaker for every line of that
  recording (`{source, base, rename: {from, to}}` → `recording::rename_speaker` through the same
  revision writer; an empty name, a track name (Microphone, Computer audio, You, Others) or a name another speaker has (compared ignoring case) is 422 with its own message, so first-seen order never changes, and a rename to the same name writes nothing; a name another speaker has instead asks "Merge A into B?" (`confirmSheet`), and only on Merge sends `rename: {from, to, merge: true}` → `recording::merge_speaker` (2026-11-22, the orchestrator's call: one voice can appear as both a call app's name and Others N), which writes the existing spelling through the same version-checked writer; the note
  text is not rewritten). What you want, Write the note again and "How this recording went" stay
  below. The ⋯ menu's Show transcript opens the same view with the selected text as the search.
  Originals: facts "<name> · Download · Back to note", the PDF (with its page), picture or text at
  the note's width; off the note the viewer sheet stays. Combine's Undo sends `{from}` and removes
  the recordings copied from that note (`copied_from`).
- What you want from the notes and the meeting details are kept whole, of any length (2026-11-18, the owner's principle 1); only what is sent is bounded: the AI reads the first `WANTS_READ_CHARS` (4,000) and `CONTEXT_READ_CHARS` (8,000) characters (`long::read_of`, also what `room_after_profile` charges to the word budget), and the Record page, its What you want tab and the note's transcript say so under the box once the brief is longer (`data-reads`). Felix's rename of a note keeps the whole title he gives; the file name is shortened by the usual rule.
- Pre-Start checks (`Housekeeper::checks`: `speech`, `writing`, `headset`) are
  cheap and never open the microphone.
- When no writing AI wrote the notes (`writer_failed`), the transcript is the
  note and leo.db `writeup_pending` marks it until PATCH `written: true`.
- Screenshots: pasted PNG/JPEG/GIF/WebP, MIME from bytes; late responses cannot
  attach to another recording.
- Share and PDF: Save .md is a Blob download (no route); print CSS hides every
  fixed overlay with fixed `@page` margins; a completed export cannot rename a
  newer page. Export as PDF with Felix's notes (2026-11-15; shown only when
  `marginPrintable()` finds open placed notes with Felix text) inserts empty
  `sup[data-print]` numbers drawn by `::after` (no text enters the note), a
  `.print-notes` list after `#doc` in Newsreader under a hairline and a
  `<style data-print>` footer with the title; every `[data-print]` is removed
  (parents normalized) at the start of every `share()`, on `afterprint`, a print
  error and the first pointerdown/keydown after printing; the cleanup is armed
  before the first mark is placed, inside `share()`'s error handling. The title
  footer needs `@page` margin boxes, which the macOS window's print engine does
  not draw (the page counter there is the engine's own).

Decided: one note per uploaded file (2026-10-20).

## Note connections (`graph.rs`; "map" in storage)

Embeddings, summaries, concepts and note links are shared by search, related
notes and Felix, and refresh when idle. A trashed note keeps its rows in `leo.db`,
out of memory, search and connections, and gets them back on restore with no AI
request; they go when it is deleted for good. The graph page, canvas renderer and
`/api/graph*` routes are removed; connection caches stay. A note the meaning model refuses waits 10 minutes, or until edited, while the rest index. Graph, vector and
caption maintenance starts after its first interval and defers while the window
is hidden or the capture activity gate is busy. The idle check compares version
stamps first (store fingerprint, caption generation, graph generation, meaning
version) and reads no note when nothing changed; pictures are looked for only in
the notes the store's changed-notes feed names (`Store::changes_since`, or
every note after "start over"); whether a build is due is asked every time.

The nearest-notes list (`Vectors::neighbours`, which decides which pairs of notes are worth asking the writing AI about) has no size cut-off (2026-11-17, the owner's principle 1): a library of 6,000 notes or fewer is finished in one call; a larger one is built `NEAR_SLICE` (256) notes per call, the rest are left out of the answer (every pair involving them is asked about, as before) until their slice comes, and after a change only the changed notes and the lists they enter are recomputed. Averages are kept at full precision (the owner chose speed over half-size vectors, 2026-11-18).

## Calendar and tasks (`calendar.rs`, `ics.rs`, `tasks.rs`, `routes/tasks.rs`, `app/calendar.js`, `schedule.js`)

- Calendars: any number of secret iCal links through `CalendarAccess`
  (`secret::default_store`), never sent to the page; leo-host fetches them
  (`fetch_public`). A feed is kept as `<data>/calendar/<feed>.<hash>.ics` (0600,
  the name carries `fnv64` of its bytes; a same-bytes refresh rewrites nothing; never
  in a note export) and read as a stream by `ics::read_from`, twice: first for the
  moved or cancelled occurrences' keys, then event by event for the window. No event
  count and no count of calendars; links are refreshed one at a time (the fetch's own
  10 s limit), each no sooner than `STALE_AFTER_MINUTES`; `CALENDAR_BYTES` (1 GB) only
  stops an endless response (2026-11-17, the owner's principle 1). One line
  of a feed is read up to 1 MiB (`MOST_LINE`). Event titles and locations keep
  the full parsed values. The repeat-rule guards stay. leo.db `calendar_feeds` keeps the times, ETag and Last-Modified (a missing file means an
  unconditional fetch); an old stored `body` is moved to a file on first read.
  GET `/api/schedule?from&to&live` (≤43 days).
- Offer to record: `calendar::offer_to_record`, a 60 s task (only with
  `Powers.app_settings`) over cached expansions after `catch_up` (same `Busy`
  guard): a timed event once when start−2 min ≤ now < start+10 min and nothing
  is recording (`record::live`); it never presses Start. leo.db `calendar_prompted(uid, start)` dedupes
  (pruned after 2 days); `offer` in calendars.json (absent = on).
- Standalone tasks: `<notes>/.leo/tasks/<id>.json` (`tasks::tasks_dir`), one
  JSON line per save through `jsonline.rs` (one generic reader/writer for any
  `Line`: `DIR`, `id`, `stamp`; shared with boards and cards), written atomically; the newest `updated_at`
  wins; deletes write a tombstone (`deleted`). Rows in the old leo.db `tasks`
  table are not read. Descriptions are optional plain text with no fixed length, and so are titles (2026-11-17, the owner's principle 1); a one-line file is read up to 64 MB (`MOST_FILE_BYTES`), a guard against a damaged or planted file that no typed text reaches.
  One mutex (`tasks::writing`) serialises task writes; task and board file work
  runs on blocking workers and never holds the notes store lock.
- No fixed number of tasks (2026-11-18, the owner's principle 1): tasks are kept in memory like boards and
  cards (`tasks::table`, a `jsonline::Index<Task>` per data folder with the same watcher rule as Boards "Tables").
  Every task is held whole, tombstones included. `tasks::snapshot` is the live tasks newest first, an `Arc`
  rebuilt only when the table's `generation` moves; `tasks::load` still reads the one file. A restore
  (`POST /api/tasks/import`) takes its tasks 500 at a time, each piece under one hold of `tasks::writing`, and
  Keeping sends the file 500 at a time, stopping at a failed piece and saying how many came back.
- The task list has no fixed length (2026-11-17, the owner's principle 1): `GET /api/tasks` takes
  `offset`, `limit` (500 by default), `q` (a word of the title) and `note` (a note id), and answers
  `{tasks, total}`; the Tasks card ends with Show more while it holds fewer than `total`, and a poll
  asks for as many as it already holds. Felix's task tools look through every task. The Calendar's
  schedule answer (`GET /api/schedule`) carries the first page of the list as `tasks` (`limit`, 500
  by default, the page asks for as many as it already holds), `total`, and as `window` every task
  past that page that is dated within a day of the asked dates or repeats (2026-11-18, no limit on
  data); an event's task link looks through every task. The page reads page and window together
  (`withWindow`); the Calendar's tray shows 50 rows of Earlier and No date and Show more reveals
  50 more, asking for the next page when every loaded row is shown. Quick Leo reads `/api/tasks`
  50 at a time until six due rows are known or the list passes today.
- Note tasks: checkboxes under Action items (or To do, Next steps, Deadlines) plus
  promoted ones (`note_tasks`, located by text and neighbours, never the wrong
  box). A date change is written into the checkbox (409 with a preview when the
  old date is gone); checkbox descriptions never rewrite note content.
- Repeating: `repeat` weekdays 0 (Mon)…6, `done_on` holds every day ticked, never
  overdue. `moved` maps an occurrence's original date to `{date, time, end}`
  (every move kept, absent when empty so old files are unchanged); `on` stays the
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
- The whole of a calendar item or tray row opens it (`.cal-open::after` covers it; the checkbox sits
  above, and a press on the resize edge without a drag opens it too). Every press on the grid or tray
  is a `cal.drag` (`press` mode for what cannot be dragged), so `drawCalendar` never replaces what is
  being clicked: it waits (`cal.redraw`) until the click has happened, and while a new task is being
  typed. A press elsewhere saves the typed task, and empty time starts one with a panel or day open.
  Words that could not be saved stay (`cal.compose.failed`): the box returns after redraws and other
  pages, a new box takes them, and only Enter or an edit tries again.
- Task blocks are as tall as their time (min 15 min, 18 px; `calLeast`): 2 to 3 PM
  ends at the 3 PM line and a resize shrinks the block live. Only a task with no end
  keeps a default span (`CAL_DUE_MIN`, 78 min). Under an hour a block takes `inline`:
  title, time on one line. Today is a pencil ring (`calRing`, inline SVG, one of
  `CAL_RINGS` by day of month, inside the old 26/24 px circle) in the mini
  calendar, week header and month. A month cell's "N more" opens that day in place
  (2026-11-21, the user's call): `cal.more` holds the day, the cell raises (`.cal-pop`,
  `--shadow-lift`), the label becomes Show less, and Escape, a click outside or the label
  closes it; the day number still opens Day, and no other cell moves.
- The Tasks card in `#dock` is folded to a small pill (the tick and the open count as plain text, danger-coloured when something is late; 2026-11-21, the user's call) at every start and opens only
  while hovered or holding keyboard focus (`tasksSync` sets `.open` from `:hover`/`:focus-within`;
  opening reveals the card from the pill with a `clip-path` over `MOTION_MS`, folding shrinks it
  back over the exit share before the pill returns, none under `stillMotion()`; the card is
  focusable); nothing is saved and a click never pins it (mouse presses on it do
  not take focus). A page swap that finds it hidden with tasks loaded redraws it (the `#app` childList
  observer), so leaving Home or the Calendar shows it at once rather than at the next poll. Home draws the date, one sentence
  (`schedule.dateline`, local data only, never "overdue"), the week ribbon
  (`#home-week`, 2026-11-15, 36 px reserved from first paint: the seven days from
  `S.weekStart(calLocale())`, today in `calRing(key, true)`, the end frame that
  never sets `cal.ringAt`; a 3 px dot on days with tasks (`tasksWin.tasks`) or
  events (`homeWeek`: one `/api/schedule` read of the week on mount, at midnight,
  when the tasks poll's events change and on a poll at least `HOME_WEEK_FRESH` (10 min)
  after the last read, never written into `tasksWin.events`);
  a day sets only `cal.selected` and opens the Calendar in its last view) and,
  under the quick bar, Earlier and Today rows (`#home-today`, the same rows;
  today's done rows stay until midnight; six at most, then "N more in Calendar";
  Today's done count is a pencil tally `S.tally`, "N of M done" kept as the title
  and hidden text) and Recently written (`#home-recent`: the three notes edited
  last, from the `x-total` of `/api/notes?show=pinned&limit=1` then
  `sort=updated&brief=true&limit=<pinned+3, at most 1000>` sorted by `updated_at` on the
  client, fetched from `loadSideFolders` and Felix's note changes while home is open,
  one load at a time (a call during one queues one more; a load hung past
  `HOME_RECENT_WAIT`, 20 s, gives way and its late answer is dropped), painted from
  cache by `drawHomeToday`, room reserved only once notes are known, hidden with no
  notes; the ribbon and this list skip a redraw that would change nothing, the
  ribbon reloads its week when the week rolls over, and is one Tab stop with
  arrow, Home and End keys) and hides the card. While home is open
  the Tasks poll reads `/api/schedule` (today to a week ahead, not `live`) for the
  cached events.
- Task dates say `S.dayWord` (Today, Tomorrow, a short weekday within six days
  either way, else "Nov 3"; the full date, `S.dayName`, as the title and as hidden
  `.said` text, the short word `aria-hidden`) in the Tasks card, home and
  the Calendar tray. The late group shows "Earlier" (`TASK_GROUP_NAMES`; the key
  stays `Overdue`, which Quick Leo filters on), and so do its count ("N from
  earlier · M to do"), the badge label, the tray ("Nothing from earlier.") and
  Quick Leo's late rows.
- Day and Week shade today's column from midnight to the now-line with `--wash`
  (`.cal-past`, under the events, no pointer events).
- POST `/api/tasks` takes `request` (uuid, kept across a retry of the same quick-add): a
  repeated one is answered with the first task (`insert::remembered_creation`, scope `task`, a queue of its own beside the margin's `insert::remembered`).
  POST `/api/notes` (scope `note`, answered 200 on a repeat) and POST `/api/boards` (scope
  `board`) take `request` the same way, so a resent first save or board create makes one.
- Export adds `tasks.json`; POST `/api/tasks/import` (ids dedupe; a newer local
  copy is kept).

## Boards (`boards.rs`, `routes/boards.rs`, `board_tools.rs`, `app/boards.js`)

- Files `<notes>/.leo/boards/<b…>.json` (name, columns `{id, name}`, 1–12) and
  `<notes>/.leo/cards/<c…>.json` (board, column, `rank`, title, subheading,
  description with no fixed length (2026-11-17, the owner's principle 1), priority `""|low|medium|high`), read and written through
  `jsonline.rs` like tasks: one JSON line per save, atomic, nofollow reads,
  newest `updated_at` wins, tombstone deletes; a file counts only when its name
  is its id. No fixed count of boards or cards (2026-11-17, the owner's principle 1). Not calendar tasks.
- Typing @ in a card's description links a note as `[[Title]]` (2026-11-21, the user's call).
- Tables (P-C52): boards and the faces of cards (`boards::Placed`: place, title, subheading,
  priority, the first 240 characters of the description and `description_more`) are kept in memory
  (`jsonline::Index`, one per kind per data folder, found by `boards::tables`). A file is trusted without
  being read only while its size, modified time and change time (unix) are as they were. The notes
  watcher also reports changes under `<notes>/.leo/` with their paths (`leo_core::watch::Leo`; at most
  4,096 paths, past that or on an error "everything"), kept per folder in `jsonline::Hub`; leo's own writes
  and removals add their path too. A request after a quiet moment lists no folder and reads no file; a
  reported path is looked at alone. The folder is listed, one stamp per file and only changed files read,
  when there is no watcher, it reported an error, on Windows (until `a_burst_of_changes_under_leos_folder_is_never_missed`
  passes on the runner) and once a minute. A board's answer (`GET /api/boards/{id}`) carries faces;
  `GET /api/cards/{id}` reads the whole card, and the card panel loads it before the description can be
  edited. Undo of a deleted card is `POST /api/cards/{id}/undelete`. Felix's board tools and export still
  read whole cards (`boards::all_cards`).
- Order is `rank` (f64): a move writes only the moved card (`boards::between`)
  unless the gap is gone, then `settle` renumbers that column. A card whose
  column vanished shows in the first column and is rewritten only when it moves.
  One mutex (`boards::writing`) serialises writes.
- API: GET/POST `/api/boards`, GET/PATCH/DELETE `/api/boards/{id}` (removed
  columns' cards go to `move_to` or the first column). DELETE returns
  `{board, undo}` and marks the board and its cards deleted or changes nothing;
  a board's own mark is written first, its cards' marks are finished later if
  interrupted (`boards::finish_deletes`, at each Keeping pass and before Undo),
  and neither is removed for good before the other. POST
  `/api/boards/{id}/undelete` (Undo of a delete, by id: the board and the cards
  deleted with it, for as long as the trash period keeps them);
  `/api/boards/restore` remains for undoing a column change saved in older
  chats. Felix's Undo of a board change is by id too: `board_undos` in leo.db
  keeps the board's name, columns and moved cards' places, used once by POST
  `/api/boards/{id}/undo/{undo}` and expired with the trash period (30 days when
  it is Forever) or when its board is gone. POST `/api/boards/{id}/cards`,
  PATCH/DELETE `/api/cards/{id}`, POST `/api/cards/{id}/move`
  `{board?, column?, after?}` (missing = end, null = top). Export adds
  `leo/boards.json`. Card create takes `request` (a uuid per user action,
  kept when the same card is submitted again after a failure): a repeated one is
  answered with the first result (`insert::remembered_creation`, scope
  `card:<board>`) and applies nothing twice. Moves take none (nothing retries them).
- `description` stays plain Markdown. Leaving the card panel settles every field;
  a failed save keeps the
  panel, address and text, so a 400 never costs text; Discard (shown with the refusal) is the only way
  out that drops it. A click outside the panel leaves it the same way (2026-11-21, the user's
  call), so a failed save keeps it open. A board fetched after a change is dropped if a write began
  meanwhile.
  A card move (keyboard, status menu), a priority, a delete and ticking a task done apply and redraw
  first, then save; a failed save puts the card, chip or tick back and shows the error toast.
  Each move carries a per-card number (a delete bumps it too, so an earlier pending move cannot bring the card back): only the latest move's answer or rollback is applied, and the board is fetched once when the last pending move settles.
- Dragging is pointer events (5 px, or a 300 ms touch hold), capture on `#app`.
- A right click on a card, Shift+F10 or the menu key opens `kanbanCardMenu` (2026-11-23, the user's
  call): Open, Move to…, Priority…, Move to board… (with more than one board) and Delete card, in
  `popupMenu` at the pointer; Priority goes through the open card's panel when it is open.
- Card priority (2026-11-15) is one to three slanted pencil strokes
  (`KANBAN_STROKES`, 14 px) before the title, coloured by level (2026-11-16: low `--hue-0`,
  medium `--hue-amber`, high `--hue-red`), the title indented by
  `text-indent`; the word stays as hidden text (`.kanban-priority-word`) plus a
  `title`, and the `.kanban-priority.<level>` classes remain.

Decided: Jira-style boards on their own page (2026-11-02); the card panel is not
a modal.

## Settings, Keeping, export

- Settings go through `SettingsApi` (`leo_host::WebSettings`); `Powers.speech`
  (`SpeechApi`, `/api/speech`), `Powers.ollama` (`OllamaApi`,
  `/api/writing/ollama`), `Powers.dictation` (`DictationApi`, `/api/dictation`,
  64 MiB input) and `Powers.app_settings` (`AppApi`, `/api/app`, `/api/health`)
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
  picture is `choiceCards` (theme, Felix, text size, lettering, margin
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
- Usage is a Settings page (`#/settings/usage`, under This app; 2026-11-21, the user's
  call, replacing the sheet): Plan limits on top, then Felix activity (moved from Writing
  AI), not the icon rail or the phone More sheet. Ordinary Settings reads never refresh
  coding-plan limits; opening the page POSTs
  `/api/settings/usage` (`{}` Codex only, its Refresh `{ "claude": true }` also Claude)
  behind one refresh slot; a failed refresh keeps saved meters. `usage::Usage`
  keeps content-free counts, `usage_days`, `usage_calls` and `usage_tools` (90 days), never prompt or reply
  text; `/api/processes` never sends arguments to the page.
- Every AI request carries its own `usage::Purpose` (2026-11-21, the user's call):
  leo-host's `felix::counted` records each one (graph Connections, Combine, Margin
  replies, Recording write-ups and drafts, Import, Pictures, Dictation); `OtherWriting`
  stays only to read rows already stored. `USAGE_KINDS` in chat.js labels every purpose
  (a usage.rs test reads it), and leo-host's `route_of` names every variant.
- Charts and legends show every model and kind, never an "Other" slice (2026-11-21, the
  user's call): kinds take `--chart-6`…`--chart-20` in `USAGE_KINDS` order, models their
  maker's `--fam-*` shades by rank, then the same shades mixed toward `--text`; each is
  at least 3:1 on `--tint` and `--raised` (tokens.test.js). Costs are estimates at API
  prices (2026-11-21, the user's call): the dated `api_cents` on each model in
  leo-services `choice.rs` reach the page as `prices`, the one place costs are worked out; the page
  prices each stored day by model (kind rows carry their model), cached input at the
  cached rate, models on this computer free, any other model "no price". Shown in the
  stat tiles and both legends as "≈ $x".
- Keeping (`keeping.rs`, `app/keeping.js`, `/api/keep`; its daily pass also runs trash expiry and its dependents): one list (7/30/90 days,
  1 year, Forever) for Trash, Chats, Uploaded files, Recording transcripts,
  Unfinished recording audio. Shortening asks first with a count from POST
  `/api/keep/preview` (the same list the deletion uses) (`keeping::would_delete`, deletes nothing); one save at a
  time. Forever deletes nothing. Expiry runs at `start_in`, daily and after POST
  `/api/keep` (`keeping::expire_now`): chats past the choice and orphan chat
  documents, uploads by file mtime (the folder and note
  never touched), transcripts in `recording-sources/`, unfinished audio through
  `Housekeeper::old_recordings` + `discard_recordings` (batches of 256) and
  `expire_kept_audio`, skipping the session in use (`RecordJob::session_id`) and
  locked sessions, and backups leo set aside (30 days, `expire_backups`: the
  `*.before-database` files and `<notes>/directories.json.corrupt`, dated by the
  newer of modified and change time, regular files only). DELETE `/api/record/{id}` also deletes a waiting recording
  without a job. The Storage page and `/api/storage` are gone; files only it
  managed stay untouched.
- Automatic note, picture and dependent cleanup runs on an idle tick and defers
  during recording or dictation. Save Keeping responds before applying expiry.
  Move records survive restart and name the held directories and audio sessions.
  A hold expires while its owner is gone. Directory digests verify the contents,
  and only row versions enrolled by that hold can be deleted. A first missing-note
  sweep after launch gives no time credit. Restore gives conflicts separate names
  and reports files that could only partly be restored.
- One rule for every move and deletion in that cleanup and in Delete for good: it is
  authorised only by a SHA-256 digest leo recorded earlier for exactly those bytes,
  checked again just before the step (the `pictures-made` ledger's fourth column, the
  trash index, a hold's folder and audio digests, an older unused picture's digest from
  the pass that listed it, a removed note's `created_at` and digest). Anything without
  one, or whose generation leo cannot prove (an id seen reused: two different
  `created_at` among the live notes, the trash and the recorded removals), is kept. Trash
  rows carry `revision` (held rows: their recorded folder and audio digests); Delete for
  good sends back what the page showed, and a changed item, or a picture whose bytes
  changed, answers 409 with a sentence so the page asks again. Held folders and audio are
  hashed outside the notes lock (`paths::digest_directory` returns the folder's stamps:
  size, times, file id); under the lock leo rechecks those stamps and the notes folder's
  fingerprint just before acting. Moves into the trash and `held/` never replace what is
  there (`paths::move_new`, `paths::write_new`). Held rows are deleted only while their
  content digest (`held_note_rows.digest`) is the one recorded. A file is hashed through
  its open handle (`paths::digest_stamped`: file id, size and times taken from the handle
  before and after, and the path checked to still be that file); the same stamp is
  checked again on the path just before it is unlinked. A Delete for good that has to
  wait (a recording running) queues the folder digests and the revision of any
  already held item (`margin_state` `deleting-shown`). Audio and row revisions for a note
  are captured before it is unlinked. Changes after confirmation and revisions
  first seen on retry enter the normal hold. Purge unlinks each verified entry and
  removes only empty directories, and manual Restore can return a changed folder
  whose identity is still the one leo held. A combine copies transcripts into a
  private staging folder, records each copy's digest from the staged bytes, publishes it
  without replacing anything, and records it only while it still matches (originals:
  the digest of the source bytes); a failed save or Undo removes only those that still
  match. Transcript corrections and Combine Undo share the library's transcript writer lock.
  The cleanup locks (`audio_work`, `clearing`, `picture_cleanup`) and
  the task and board writer locks belong to each library, never the process. Keeping's
  age-based expiry (uploads, transcripts, unfinished and kept audio, backups) is the
  person's own choice and is not under this rule.
- Recording snapshots take the picture destination from the loaded notes store. The request only supplies the image and recording metadata.
- Export: zip of notes, optional uploads, chats, trash; never config or keys.
  Held originals and transcripts are included when Trash and Uploads are selected.
  Restore tasks refuses a zip or file without task ids. An export has no item or size limit
  (2026-11-17, the owner's principle 1); it is streamed entry by entry into a temporary file
  the response then sends (a file in 64 KB pieces, a chat's messages and a board's cards a page
  or a card at a time, a chat document a million characters at a time, zip64 when needed), skips
  a folder it has already entered or one deeper than leo reads (`MOST_DEPTH`, 64), and stops, removing its
  file, when the request is abandoned. Tasks and margin threads are still held whole while written.
- Appearance keys are per browser (`leo-theme`, `leo-note-width`,
  `leo-text-size`, `leo-felix-letters`, `leo-page-width`) and set before paint by `/theme.js`;
  only reading text multiplies `--read`. Page width (2026-11-16, the user's call): two `.page-mark` separators at the note's top corners (drag.js) set
  `leo-page-width` (480–1600 px, symmetric about the column's centre, live px label, arrows
  ±20, double-click/Enter resets; 2 px marks at 38% ink); `showWidth` writes it as an inline
  `--note-width` unless the width is Full. A drag stops at `marginKit.mostColumn`, the widest
  column that leaves the chosen margins their least width, so the margins never give way.
  Settings → Note width only says the current width with a Reset (`width-reset`; no size
  cards since 2026-11-16, the user's call); an older Comfortable or Full choice still applies until reset.

Decided: Keeping replaces Storage (2026-11-05); Quit leo is not in Settings; Writing
AI uses a pen icon, never a sparkle.

## Idle work and freshness

- `read_store` takes an immutable Store callback and coalesces filesystem scans
  with core's Watch: it checks first, on relevant/rescan notifications, every
  60 s and after native foreground return; a missing watcher checks every access, a broken one at most every 2 s, until it reports events again.
  A save never absorbs an unread external edit: its fingerprint moves only by
  the files it wrote (per-file stamps). `store_now` always reconciles first;
  `with_store` reconciles when the watcher reported a change, the 60 s check is
  due or there is no watcher, and always on Windows (2026-11-17, the owner's
  call on ENG-D6: the most efficient and scalable method).
- GET `/api/rev[?foreground=1]` answers `{rev}`, 16 hex digits over the store's fingerprint and a
  the task, board and card tables' generation numbers (moved whenever an entry is added, replaced or
  dropped; a file rewritten in place moves its table's number once the watcher or the minute's listing sees it). `foreground=1` forces a disk check first.
  It is signed in like every API route and, as a GET, does not count as use.
- The watcher's `.leo` signal is separate from the notes' one: a change under `.leo` never marks the
  notes as changed (`leo_core::watch::Watch::leo`, taken by `jsonline::Hub` for the board and card tables).
- App-status polling stops while the document or window is hidden and drops late
  responses on visibility changes; return refreshes once. Automatic status GETs
  do not reset user-idle time. Dictation, model-download and recording display
  polls pause while hidden.
- On the Calendar the Tasks card does not poll `/api/tasks`: the schedule answer
  carries the tasks and stamps `tasksWin.at`; the tasks timer only re-arms (with a
  5 s grace so it never races the schedule poll) and loads again once the person
  leaves the Calendar.
- The task list is rebuilt only after a change: `AppState.generation` moves after every
  store refresh and every `with_store`/`store_now` call, and `tasks::ListedCache` keeps the
  last list (note tasks and standalone tasks, sorted once, with an id index for `find`) until
  the generation, the note-task records or the standalone tasks' snapshot (`tasks::snapshot`, compared
  by identity) differ; the generation is read inside the store closure, after any refresh. No task file
  is read for a repeat list.

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
- Fixture specs (boards.spec.js, calendar.spec.js, live-turns.spec.js) run the real CSS and page code
  over an in-memory API; flows.spec.js joins the page to the real routes.
  The manual search paging test disables the scroll observer in the signed-in
  document before changing the hash and moves its fixture notes to trash in one request.

The note API preserves the preexisting version hash for ordinary notes after removing the unused tags field.

Config and data file paths use the same `leo_core::paths::own` names as uninstall.
