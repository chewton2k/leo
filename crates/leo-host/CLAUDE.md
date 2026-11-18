# leo-host

The service wiring between leo-web and leo-services. `powers()` supplies AI,
speech, settings, imports and the recording `Housekeeper`; web keeps its core-only dependency.
`leo-test-server` runs the same engine for targeted browser checks. It stores "When you ask" as its
disposable home's default reading mode at startup, so browser suites don't depend on when the
Read along default's `since` was first set (2026-11-08); the product default is covered by leo-web's unit tests. Every worker
entry point dispatches `--leo-speech-worker` before normal startup.

`health::scan` converts the local doctor checks (`leo_services::doctor::scan`;
there are no probes: no network or AI requests, and the microphone is never
opened) to the native Health view. Writing CLI sign-in status is bounded and parsed to a boolean; its
raw output never reaches the page. The host exposes no backup setup or push
capability; existing user repositories are never changed by app lifecycle events.
The browser-test engine has a real startup/session-gate integration test, so
Cargo tests also produce the executable used by browser checks.

The Usage sheet receives both coding accounts and their saved usage windows;
detecting a signed-in CLI never changes the user's selected writer. Codex limit
refresh does not depend on which writing provider is selected. General Settings
loads only saved limits; the explicit Usage action performs the bounded CLI
refresh and preserves saved readings when that CLI cannot return limits.

Lecture listeners pass the engine's shared preview-visibility flag into recorder
controls, including both call tracks. This flag never pauses capture or final
transcription, and recovery continues while the main window is hidden.

`housekeeping.rs` holds `Housekeeping`, leo-web's `record::Housekeeper`: `pending_page`
(unfinished native sessions, unlocked ones only, each with its manifest's `started`, newest first a
page at a time, every folder reachable; metadata is remembered until its file changes),
`pending_recording` (one by id), `old_recordings` (older than the Keeping cutoff) and
`discard_recording` (validated id, contained paths, refuses a locked session). Settings →
Keeping's audio expiry calls it, so a recording that is running or being recovered (its
session is locked) is never discarded. `discard_recordings` deletes a batch, finding each recording by id and rereading its metadata before removing it (expiry uses it). A saved session that kept audio names its note (`Manifest.note`);
`discard_kept_for_notes` removes it when that note is deleted for good (for the trash's removal
pass); a session saved before `note` existed is matched through its trace line `Note {id} saved`.
`retarget_kept` moves a combined note's sessions to the note that remains (all or none, refused
while one is locked) and `restore_kept` gives them back on Undo; every delete or move rereads the
manifest from disk. The Storage page, its areas and actions were removed
on 2026-11-05; files they managed stay on disk.
