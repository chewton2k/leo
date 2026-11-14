# leo-desktop: the leo app

The Tauri 2 app (`leo-app`, product "Leo", bundle id `io.github.chewton2k.leo`).
Rust 1.90 (Tauri's floor); not in `default-members`, so plain `cargo test` skips
it. Try it by hand with `pnpm --dir crates/leo-desktop tauri dev` (the one local
build allowed); point `LEO_HOME` at a throwaway folder to keep real notes out of
a test, or leave it unset to see the user's own data.

## Startup (`lib.rs`)

1. macOS: `PATH` = the login shell's PATH (`shell_path::login_path`, markers,
   3 s timeout, process group killed on timeout) merged with the current PATH and
   `usual_folders` (Homebrew, `/usr/local/bin`, `~/.local/bin`, `~/.cargo/bin`),
   so Codex, Claude Code and Ollama are found when Finder starts leo.
2. `leo_web::start` on `port::remembered` (`<config>/app-port`, default 47318,
   kept stable because localStorage and IndexedDB are per origin), with
   `fresh_token()`; the port actually used is remembered. A failure opens
   `dist/index.html?problem=…` instead.
3. Plugins: single instance (a second launch shows the window), autostart
   (enabled once, marker `<config>/app-launched`; never in debug builds),
   notification (Rust only; the page gets no notification permission).
4. Window `main`: the engine's sign-in URL, 1200×800, `--hidden` when started at
   login. macOS: overlay title bar, hidden title, window buttons at (18, 31) in
   the sidebar head; `window.__LEO_APP__ = 'mac'` makes `theme.js` set
   `html[data-app="mac"]`, which hides the sidebar wordmark and makes room for
   the buttons (2026-10-31, the user found the grey title strip ugly). `#bar`
   and `.side-head` are `data-tauri-drag-region="deep"`. The icon rail stays
   visible when the explorer is hidden. The sidebar toggle and the brand share one
   header row: to the right of the window controls when windowed, flush left in
   full screen (`lib.rs` tells the page on load and on every resize). Folded on
   a windowed Mac, the toggle drops into the rail below the window controls.

## Behaviour

- Close hides the window; in macOS full screen it first leaves full screen (`set_fullscreen(false)`) and hides on the first `Resized` that reports windowed, or after 1 s if none arrives, because hiding a full-screen window left a black Space (`LEAVING_FULLSCREEN`, `show` clears it); Dock click and "Open leo" show
  it; on macOS, Leo's Quit menu items or ⌘Q need a second attempt within three seconds
  (the first shows a prompt). Update restarts and error exits bypass this guard.
  `RunEvent::Exit` stops the engine (6 s cap), without committing or pushing notes.
  The macOS app menu replaces the predefined native Quit item with a regular
  ⌘Q item routed through `app.exit(0)`, so it reaches the guard and update actor.
  Tauri's default menu supplies Edit (⌘C/⌘V/⌘Z/⌘A)
  and binds none of the page's shortcuts.
- `links::stays_inside`: only `http://127.0.0.1|localhost:<port>` and the
  bundled page stay in the window; navigation and new windows elsewhere open in
  the default browser.
- Menu bar icon (`tray.rs`, 2026-11-02, the user's request): Open Leo, Record a
  lecture / Record a call (shows the window and calls the page's
  `window.leoFromApp('record', kind)`, which presses Start for that kind; a
  running recording is just shown), Copy last dictation (newest row of
  `leo_web::dictation::recent`, native clipboard), status rows (speech model
  `Library::selected_name`, writing AI and model plus 5h/7d plan use from
  `web_settings::writing_status`, AI use since launch `Running::usage_line`),
  Settings…, Check for updates… (queues a check, opens Updates & about), the
  version, Quit. A `leo-tray-status` thread refreshes the rows when the icon is
  clicked and otherwise every 5 min (2026-11-06, the user's call: no idle
  polling); Codex limits are asked from its app-server only after a click, at
  most every 5 min and only when Codex is the chosen writing AI and its saved
  use is stale (`writing_status(may_ask)`).
  Page commands are offered for 5 s (the page may still be loading) and run once
  per click (`__leoAsked`); copying runs off the main thread. A speech model that
  is chosen but not on disk reads "(not downloaded)".
- Calendar events (2026-11-09, `notify.rs`): `AppApi::event_starting` posts
  "<title> is starting" with the event uid and kind in `extra`; a tap (`on_action`, which
  2.8.0 reports on macOS and Windows) shows the window and navigates to
  `#/record?event=<uid>&kind=<call|microphone>` through
  `leoFromApp('event', '<kind>:<uid>')` (`page` JSON-quotes its argument). `call`
  when the event's location or description holds an http(s) link
  (`EventStarting.online`), else `microphone`. The page only opens Record ready;
  it never starts. macOS asks for notification permission on that first
  post, never during onboarding. The tray always adds "Get ready to record <title>" (`&` doubled, title cut at 40
  characters with an ellipsis) under Record a call until start+10 min, hidden while a recording is live
  (`Running::recording_live`), so a denied or missed notification still has a way in.
- The window disables Tauri's drag-drop handler (`disable_drag_drop_handler`):
  left on, it swallows the webview's HTML drag events, so moving notes and
  folders and combining notes did nothing in the app (2026-11-02). Dropped
  files reach the page's own handlers (`drag.js`).
- Sign in (2026-11-09, `sign_in.rs`, AppApi `{action:"sign_in",provider}`, only
  `claude_code`/`codex`): Codex runs `codex login` quietly (it opens the browser; one at
  a time, reaped, stopped after 10 min); Claude Code opens a terminal running the resolved
  `claude`: a private 0700 `.command` file (`#!/bin/zsh -l`, removes itself) opened with
  `open` on macOS, and on Windows `cmd /c start "" cmd /k claude`, the one intentional
  visible console. The command lines are pure functions with unit tests.
- Downloads save to `~/Downloads` under a free name (`links::download_path`)
  and are revealed when done.
- `window.print` calls `print_page` on macOS; Windows retains its original
  native function, since Tauri's print implementation calls back into JS.
  Calling the bridge on Windows would recurse instead of opening the dialog.
  The macOS bridge waits one task first: WebKit reads the print job's title,
  which names the saved PDF, from a copy that lags `document.title`.
  Custom commands have explicit permissions: only `main` at the loopback engine
  may print, and only the bundled `dictation` popup may call `dictation_popup`.
  Tauri treats loopback pages as remote origins and refuses ungranted commands.
- Capability `default`: remote `http://127.0.0.1:*`/`localhost:*`, core
  defaults, window dragging and double-click maximize.

The `leo-app` entry checks `speech::is_worker_invocation` before Tauri and
single-instance startup; native speech runs only in that child process.
Info.plist declares microphone and computer-audio capture uses. Recording
capture and its process activity guard live in services and continue when
`main` is hidden; the webview only controls and displays the job.


## Dictation

`Dictation` is managed as an Arc before the loopback engine starts and supplies
`Powers.dictation`. A bounded coordinator serializes shortcut inputs, handles
hold/tap and remembered input during decoding, and rejects stale completion by
generation. Its state is a tested `Queue`: a press after a cancel never starts a
later dictation, and a stop press after capture ended on its own is swallowed.
A panic in the pipeline (`catch_unwind`) ends with an error, never a dead
coordinator; a panic in the text rules pastes the raw transcript; a Recent
dictations (history) failure only logs. It opens the selected microphone only for capture. Lid-closed
microphone, trailing space and Press Enter after pasting were removed
(2026-11-02, the user's call); `dictation.json` files that still name them load
and drop them on the next save. Capture releases
its input before local VAD/transcription, word rules, local history and paste.
Start/stop WAVs are optional and excluded from lecture recordings by AudioGate;
they play on their own thread. The popup says "Starting microphone…" until the
first audio arrives, silence included; only then does it say Listening and
play the start chime (Handy's readiness cue). No audio at all in 8 s is an error (a
Bluetooth headset can take seconds to switch). A microphone that stalls after
audio, goes silent or stops transcribes what was heard and says why; empty or
meaningless text also says why. Notices carry a title and engine errors are
worded and logged. Errors and notices show as Minimal when the popup is Off.
At launch the app warms CoreAudio, checks the model stamp and, on Apple Silicon
once per app version (`<config>/speech-shaders-ready`), has a throwaway worker
compile the Metal shaders (about 4 s) so the first shortcut does not. No model is loaded at
launch (2026-11-04: warming measured 620 ms vs 612 ms for the first stream and
held about 900 MB on every launch). Audio queues while a model loads.
Debug-only replay/output controls require `LEO_DICTATION_TEST`; normal builds
never expose the test actions or write a test paste file.

`shortcut.rs` owns handy-keys on a dedicated thread, re-creating it after
Accessibility changes. Recording a binding pauses global registration and supports
modifier-only/Fn. The recorder reports the held keys, saves the largest chord
captured when the keys are released, Escape cancels, and it expires after 30 s
with "no keys were pressed". `configure` answers only after registration and
keeps the previous shortcut when the new one is taken; the bare paste modifier
(Cmd on macOS, Ctrl on Windows) is refused, and a saved invalid shortcut loads
(in memory) as the default with a problem, so Settings shows the one in use. A held binding (handy-keys or system
shortcut) is released before a rebuild; if the macOS key feed fails the system
shortcut takes over; key events are stamped when the feed sees them and dropped
inputs log. Secure Input uses the Carbon fallback
for representable chords (side-specific modifiers widen); modifier-only events
stay on handy-keys. Escape registers only while capturing (and through AI
writing when it is on), including fallback, and is reserved from being the
dictation binding. Not done (2026-11-11, the user's call): on Windows a
modifier-only hold can still end on a mouse click; the fix belongs upstream in
handy-keys. On Windows the same system
hotkey path carries any chord it can match exactly (2026-11-06, the user's
call), because handy-keys' Windows listener wakes every 10 ms and hooks the
mouse; handy-keys runs there only for modifier-only, Fn and one-side bindings
and while a shortcut is being recorded (`shortcut::tapped`). Status exposes missing
permissions, unavailable registration and uncovered Secure Input chords.

`paste_tx` publishes lazy clipboard data, waits up to 1 s for held modifiers
before the chord, waits for post-injection reads and a 200 ms quiet period, with
an 8 s cap. A failed chord is an error and leaves the text on the clipboard; a
paste no app read is reported as unsure. Restoration checks ownership again; a
newer copy wins. The clipboard/paste preference types are gone. Explicit copies
(Recent dictations, tray) write plain text; the manual-paste fallback and a
failed chord leave the text as Leo's own copy (`write_secret`, marked private,
on macOS), and a concealed or transient snapshot is not restored unless it is Leo's own copy (the pasteboard change count or clipboard
sequence number after the copy is compared). A pending restore is flushed on exit.
macOS snapshots every pasteboard item/type. Windows opens the clipboard with its
own owner window and retries, skips private formats, retains memory formats and
copies bitmap, metafile and palette handles; a format it cannot preserve causes
an error before publishing. Total snapshot bytes/items/types are bounded.
Enigo is taken out of its state slot for native I/O; layout-aware Cmd+V resolves
on the main thread. Without Accessibility or with an elevated Windows target,
text remains on the clipboard with a manual-paste instruction.

`overlay.rs` creates a bundled `dist/dictation.html` popup: a nonactivating NSPanel
on macOS, nonfocusable always-on-top window on Windows. It uses the focused
window's monitor, with cursor/primary fallbacks, and supports top/bottom and
Live/Minimal/Off. Its CSS token block comes directly from the web page's theme;
the main page sends Light/Dark/Match changes. Queued shows and updates check the
current generation. The popup's IPC is limited to its own window label. Its context menu is
blocked, the notice title shows, the timer and live words are `aria-hidden`, the
level bars are hidden outside listening, and on Windows it is placed in physical
pixels with the display and text scale. AX messaging has a 0.25 s timeout;
Accessibility is reset with `tccutil` only when it is denied after a version
change.
The popup follows Handy's live preview: state changes are pushed as a
`dictation` event (revision-stamped, skipped when the popup is Off except errors and notices, shown as Minimal; the page
polls only if listening fails), committed and tentative words render with a
caret, the text pins to its newest line, and 16 FFT buckets
(`dictation::visualizer`) drive nine smoothed bars. Dictation follows Handy (2026-11-04, the
user's call): audio passes Silero VAD at 0.3 in this process with the 1650 ms
streaming hangover (450 ms for models that do not stream) in ~32 ms feeds, the
stream's text is final, and the same filtered audio goes to a batch run only when
that text is empty or there was no stream. Measured on 73 read clips, Silero with
the stream gave 2.61% WER vs 2.96% unfiltered. Differences from Handy: streaming
models too slow for live text (`Selected.live`) skip the live pass (Voxtral: about
103 s for 60 s of speech, estimated); if Silero cannot load, dictation runs
unfiltered; and when a streaming model's filter kept nothing, the batch run gets
the raw audio, so quiet speech is not lost whole. Text stays
visible while the final pass runs. With AI writing on, Escape stays registered through the
polishing step (its own `escape` flag, separate from `capturing`), and a
fallback shows a four-second "Pasted without AI writing" notice with the
reason. Settings warns when no writing AI is set up (`writing_ready`, which is
`health::writing`'s cheap state; the tray's plan-use line reads the same state).
`CHORD_HOLD_MS` stays 100 (the user's call). Shutdown cancels dictation/shortcuts and stops the speech worker before the
loopback engine finishes.

## Settings and updates

`app_settings.rs` serializes atomic private app preferences through a bounded
actor; native login/tray failures restore the previous file and cached state.
First-launch microphone, Accessibility and model completion flags persist
independently; the page groups both permissions before the model choice.
`finish_step(3)` is the writing AI step (`writing_done = Some(true)`); any earlier
step first pins `writing_done` to its computed value, so new setups still see it.
The page reads `preferences.writing_done` as `writing_done()`.
`Preferences` ignores unknown keys, so a file from a newer build never resets
settings; `writing_done()` is true when the key is true, or absent and all three
older flags are done.
Permission status checks never open the microphone; requests are explicit.
A microphone request after a macOS denial opens the Microphone privacy pane,
since the system shows its dialog only once.
Health combines host doctor checks with native permission, model and shortcut
checks. macOS helper output is bounded, nonblocking and timed out by process group.

`updates.rs` owns updater checks, verified downloads and installation on one actor.
Automatic checks and install-on-quit default on; debug startup checks stay off.
Downloads use public HTTPS resolution, deadlines and a one-GiB cap. Signed bytes
stay in memory. Core activity leases cover capture through clipboard restoration;
installation takes an exclusive lease, unloads/reaps the speech worker, and keeps
the lease until exit. Failed installation leaves the engine actor usable.
macOS restart after an update (2026-11-06): Tauri's `restart` ran `open -n` while the
old process was alive, so a second Dock tile stayed. Now a detached `/bin/sh` (own
process group, null stdio, no interpolated strings) waits for this pid (30 s at most),
then `open <bundle>` with one retry, and the app exits with `RESTART_EXIT_CODE`; if the
spawn fails or the binary is not in a bundle, `app.restart()` runs as before. The first
update INTO this build still restarts with the old binary and may show two icons. Only
argv and the wait are tested; a real update needs a release.

Release-only `tauri.release.conf.json` includes the root CLI as `binaries/leo`,
so ordinary checks/dev need no prebuilt sidecar. CI builds that command first,
then Tauri bundles/signs it with the app and produces updater artifacts. The
committed updater key is public only. Stable private signing files live outside
this repository; release requires four repository Actions secrets. Microphone
entitlements accompany hardened runtime. Config omits version; workspace version
and scripts/set-version.sh remain authoritative.

Release assets are macOS arm64/x64 app archives and DMGs plus Windows x64 NSIS.
CI uses one stable self-signed identity and verifies both bundle and CLI signatures;
updater signatures cover the same app archive or NSIS used by terminal installation.
Windows builds set `TRANSCRIBE_CMAKE_ARGS` to an AVX2 floor (2026-11-04): transcribe.cpp
otherwise tunes to the build machine, which crashed with an illegal instruction when CI
reused a cache from another runner and would on users' processors too. Handy picks
instructions at run time with dynamic backends instead. Silero refuses a processor
below that floor, since it runs in the app process; the worker contains its own crash.
Windows uses the dynamic CRT to match transcribe-cpp; its release-only config
bundles the Visual C++ runtime DLLs next to the executables.
No Linux release/test rows remain. Chromium and targeted WebKit browser jobs run
on macOS. Main installer cutover happens only after published assets validate.

The bundle minimum is macOS 10.15, matching the native C++ filesystem floor.
The macOS build script links clang_rt.osx for Metal availability checks and
weak-links CoreAudio so older supported systems can load the app. Compiler
runtime discovery uses quiet_command with an 8 KiB output bound and five-second
deadlines; leo-core is a build dependency for the same child-process rules.

Release preflight signs a disposable probe with the updater key before the
version bump or native builds. Signer output is suppressed on failure so only
secret names and corrective steps appear; private values stay in the environment.

## Idle energy

`shortcut_events` owns the raw handy-keys listener and forwards relevant events
through a bounded queue. The coordinator waits on that queue; the old 10 ms poll
and handy-keys manager timer are gone. Permission/Secure
Input checks run every second while someone is at the keyboard or mouse and
stretch to ten seconds as the Mac sits idle (`secure_input::user_idle`), while
key/control messages wake immediately. A replaced feed is not joined: its
thread ends at its next key event or within thirty seconds.
Bindings preserve key/modifier release order, repeats, Fn and side-specific keys;
Escape eligibility wakes the coordinator explicitly. Feed shutdown clears native
blockers and cancels a producer waiting on a full queue.

The main window publishes visibility to the engine and page on hide/show,
minimize/restore and page-load completion. Hidden/minimized lecture previews pause;
native capture, final transcription and dictation remain active. A finished page
load resynchronizes visibility in case hide/show happened during navigation.

Foreground requests (single instance, Dock and tray page commands) are refused
while the core installation lease is held. Every successful update reopens Leo,
including install-on-quit. Installations leave a
private atomic one-use `update-foreground` marker, consumed only after the
single-instance check, so `--hidden` from login cannot hide the updated app.
Failed installation removes the marker and releases the opening restriction.

Automatic updater checks run at release startup and after an hour without an
updater command. Installation replaces the running bundle; the staged previous
bundle is removed, and unrelated application copies are never deleted.
Foreground requests run on the main thread, move the main window to the active
macOS Space and explicitly make it the main/key window. Initial activation runs
at Ready, never after an arbitrary page reload; hidden login launches stay hidden.
The macOS tray opens its menu through NSStatusItem directly for either mouse button
(`popUpStatusItemMenu`, not tray-icon's simulated click). Spike on a Mac (2026-11-06): a
Regular app shows no status menu over another app's full-screen Space by any path
(popUp, performClick, permanent menu, activating first); an Accessory app shows it, with
or without a window, and Regular flipped to Accessory for the life of the menu shows it
too. So `popup()` (`tray.rs`) sets Accessory before the menu and Regular after only when
Leo is not active, its main window is not on the active Space (`isOnActiveSpace`, false
when hidden, `needs_flip`); a click within a second of the last flip waits out the gap, so rapid changes never leave extra Dock icons; Leo in front or beside
you never flips. `setCanHide` was not needed in the spike. Only the gate is tested; the
real behaviour needs a release and a full-screen Space.
