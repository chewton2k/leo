# leo-services

This file holds the rules. Measurements, prompt content and longer explanations
are in `docs/behaviour.md` (not auto-loaded).

### Streaming goes through the provider trait

`ChatProvider::complete_streaming` defaults to `complete` and sinks the whole
answer once, so a provider that cannot stream works everywhere.
`run_chat_chain_with` holds the fallback policy for both paths, so they cannot
drift on which errors are fatal. The streaming chain takes an `on_restart`
callback: text from a provider that then failed is discarded, or a fallback
would glue half of one reply to all of another. OpenAI-compatible streams have
the agent CLIs' caps (`process::MOST_LINE` per line, `process::MOST_OUTPUT` in
total).

### AI pipeline

`record` → `session::mic` (cpal) captures 16 kHz mono →
`ai::transcribe_outcome()` runs the `[transcribe]` chain (the selected local
model) → `ai::long::structure_recording` runs the `[chat]` chain → note saved.
While recording, `recorder::Drafter` (injected `Request.draft`; tests pass None)
drafts each finished stretch into `drafts.json`, one call at a time, never while
speech is behind; Finish now cancels a running draft and Stop always joins it.
After Stop one closing call writes the rest around the drafts and every drafted
action item (`drafts_for` key must match, else the one-pass path; a rest too long
for the budget is drafted first). Details in docs/behaviour.md.

Each chain skips unavailable providers silently, advances past retryable errors
(402/408/429/5xx, refused connections) and aborts on fatal ones
(400/401/403/404). Every degradation taken is reported; a silent downgrade is
never invisible. `ai/mod.rs` holds the entry points (`transcribe_outcome`,
`chat_outcome`, `chat_streaming`, `quick_outcome`, `open_session`, …); nothing
above `ai/` knows about providers.

A writer failure a person sees starts with `leo_core::NO_WRITING_AI` ("leo
can't write yet: ") only when setup can fix it (not ready, sign-in, key or model
missing, not installed), so the page can add Set up writing AI; a rate limit,
timeout or outage reads "Your writing AI didn't answer just now
(busy|timed out|unavailable). Try again in a moment." with no prefix and no
provider body. Services use `ai::writing_error(&err)` (reads `health::writing`,
offline); leo-web uses `leo_core::writing_said` / `crate::no_writer()`. Never
write a per-site wording.

### The note prompts (`ai/chat.rs`)

Every chat prompt is `Prompt { system, user }`: rules in the system message,
material in the user message inside `<transcript>`, `<existing_notes>`,
`<typed_points>` or `<note>` tags, essentials restated after the transcript.
Always keep: fixing clearly misheard terms, filling gaps from the model's own
knowledge, and merging typed points. Typed points (`POINTS_RULE`) go into the
sections they belong to in the user's exact words: no section of their own, no
times. `long::points_kept` puts back any point the reply lost (85% of its words,
or the whole point) under `## Your points, as you wrote them`; long recordings
send each part only its own points (`long::points_for`, each point to exactly one
part; only a fallback with no AI answer lists them, as `## Key points`). Replies go through `clean_reply` and `split_title_body`. A reply that hits
`max_tokens` is kept and warned about; a provider's configured `max_tokens`
overrides leo's. Notes share one shape (`ai::chat::LEARNING`); uploads may
explain beyond the source but facts stay the source's.

### Recording sessions (`session/`)

A `Session` is `<data>/leo/recordings/<stamp>/` with `session.json`
(`Manifest`), a `lock` (pid + 5 s heartbeat; stale when old or the pid is dead)
and segments. `capture::Capture` reads s16 PCM from `mic::Mic`
(`Source::Microphone`, `Source::Screen`: macOS 14.6+ process tap or Windows
loopback, else BlackHole or `LEO_SCREEN_DEVICE`, see `mic::find_device`;
`Source::Replay` from `LEO_FAKE_AUDIO` at `LEO_FAKE_SPEED`; `Source::Synthetic`
in tests). `wav::Writer` writes `seg-NNNNN.part.wav`, renamed to `.wav` every
`SEGMENT_SECS` (300) with `OVERLAP_SECS` prefilled. Paused audio is never
written. `transcriber::Transcriber` writes `.txt` atomically, deletes the audio,
never gives up while recording, gives up after `attempts_after_stop` (writes
`.err`, keeps audio) unless rate-limited. `recover_parts` seals a crash's
`.part.wav`. `structure_recording` groups parts by `words_per_group`
(`long::groups`); every failure falls back to the raw transcript, so it cannot
fail. `Session::finish` deletes the folder unless a segment failed. Unfinished
sessions are resumed by the app (`recorder::resume`). `session/stress.rs`
simulates 3 h and (ignored) 24 h.

Live transcript (catalog models marked fast enough): every `ROLL_INTERVAL` (3 s;
`CLOUD_ROLL_INTERVAL` 8 s) re-transcribe from `settled_at`, at most
`LIVE_MOST_SECS` (20); at `SETTLE_AFTER_SECS` (12) settle at `live::quietest`
between 6 s and end − 2 s; only while no segment is waiting. Typed
points go through `Controls.points` (`recorder::points_of`) into the manifest.
Recording needs only speech and a microphone; the writing AI is optional.
`LEO_NO_MICROPHONE` turns the device off (tests, e2e, CI).

### Model configuration

Writing choices are Ollama, Codex and Claude Code; speech is local only. Provider
kinds are `openai` (Ollama and compatible servers), `parakeet`
(`ai/provider/local_speech.rs`), `claude_code` and `codex`. Loading a config
removes (`config/tidy.rs`, before deserialization) provider blocks of any other
kind and chain entries naming them, falling back to local defaults; a leftover
`[theme]` is ignored. `Config::load`/`load_from` never touch credentials.
Calendar secrets use the private credentials file, or the keychain with
`LEO_USE_KEYCHAIN=1`; tests use MemoryStore.

Config saves are atomic and serialised: `web_settings::apply` holds one `SAVING`
lock and `config::edit::save_document` writes through `leo_core::paths::write_atomic`
(a private temp file, 0600, renamed over), so no save interleaves and a crash
never truncates config.toml. `toml_edit` keeps the user's text; unrelated TOML
and legacy `[sync]` survive (tolerated, never migrated). `config/choice.rs`
keeps writing model/effort choices and the one local speech entry;
`web_settings::describe` never includes a key; `apply` validates every change
against `choice`. An override naming a provider with no `[providers]` block is
ignored with a warning. `.env` is loaded only from leo's data directory, never
the current directory.

### Speech (`speech/`)

`speech/catalog.json` is the pinned catalog (29 models); missing or invalid
accuracy stays unknown. Selection lives in `<config>/speech-model.json`, files in
`models_dir()/speech`; an empty selected ID means the user deleted the current
model. `Library` verifies exact size and SHA-256, keeps verified size/mtime
stamps in `<config>/speech-verified.json` (never re-hash on every launch),
downloads at most two models into owned temp files, checks cancellation while
awaiting the network and promotes atomically; linked and hard-linked models are
refused; selection/deletion/promotion use a private file lock.

`speech/engine.rs` admits eight jobs, gives dictation priority between chunks
and pauses lower work during a dictation reservation. A dictation cancel bumps a
per-dictation counter and kills the worker call only while a dictation is being
served, so recording segments and live passes keep running; waiting dictations
return Cancelled within 50 ms and the actor re-checks after model load and
between chunks. Custom words go to `RunOptions::vocabulary` when the model
supports it, and only those runs' text goes through the echo filter. A transcribe.cpp backend
error counts as a lost worker (one fresh GPU retry, then CPU); `loaded()` is
cleared when a worker is lost, and idle unload waits for a dictation. One actor owns supervisor
and model. Recording pins it; otherwise idle unload is five minutes. Stop and
updater unload share one five-second deadline; ordinary admission stays
immediate and bounded. Deletion waits behind inference, unloads first and
refuses an active recording/dictation model. Long audio cuts at VAD pauses (30 s
target, 40 s max). `vad::detector` is Silero at 0.3, Earshot at 0.5 if Silero
cannot load. Dictation filters while capturing; the engine only cuts and pads
clips under a second. Recordings are never trimmed: a chunk goes whole unless
VAD, on a copy raised to speech level (median frame level to 0.1, gain 1–50),
hears nothing. Slow catalog models skip live passes.

`speech/supervisor.rs` re-executes the binary with `--leo-speech-worker` before
normal startup. Only the worker initializes GPU backends, loads a speech model
or runs inference; the one exception is Silero VAD (CPU-only, in the app process,
written to `models_dir()/vad` from embedded bytes and compared before use, one
`Model` per detector). Framed JSON/PCM, queues, stderr and deadlines are bounded;
crashes/hangs are reaped and retried once (CPU after GPU). The Rust binding and
native sys crate are both pinned to exactly 0.4.2 (pre-1.0 patch versions are
not ABI-compatible). Filesystem/native work stays outside application mutexes.

### Ollama (`ollama.rs`)

leo helps download a model the user picks; it never picks or pulls one by
itself. `ollama_models.json` is the vendored catalog. `machine()` reads total
memory and free home-folder disk. `fit` is TooBig when memory needed exceeds
total by more than 10% or size + 2 GB exceeds free disk; unknown values never
block; no `recommended` while memory is unknown. `pull` streams loopback
`POST /api/pull` NDJSON with a 300 ms connect timeout, a six-hour total bound
(a blocking timeout covers the whole body) and the `stop` flag, and says disk and
memory errors in one sentence (`plain_error`).

### Agent CLIs (`ai/provider/agent_cli.rs`)

`claude_code` and `codex` write through the user's own signed-in CLI and plan:
`claude -p --safe-mode --tools WebSearch --allowedTools WebSearch
--no-session-persistence --system-prompt-file <private file>` (a file: Windows
refuses line breaks in arguments to `claude.cmd`, cmd.exe caps 8,191 characters)
and `codex exec --ephemeral --ignore-user-config --ignore-rules --sandbox
read-only -`, material on stdin, in a fresh private `leo-writing-*` temp dir
(0700, deleted after). Claude Code streams (`--output-format stream-json
--include-partial-messages --verbose`; a `result` with `is_error` fails); Codex's
stdout is the answer in one piece. Never `--bare` (it skips the OAuth login).

Every launch, native session, probe and usage poll clears inherited credentials,
provider/routing overrides and startup hooks, keeping only the signed-in HOME,
XDG, CODEX_HOME, CLAUDE_CONFIG_DIR, OS/PATH, certificate settings and the Linux
D-Bus address. An ambient API key cannot replace the CLI login.

Allowed tools: Claude Code gets WebSearch and nothing else (its Read tool is not
confined, and WebFetch could send notes to a URL); Codex keeps its web search and
gets `--disable` for every `CODEX_OFF` feature that `codex features list` reports
(unknown names make Codex fail; cached per program by canonical path and mtime),
including `memories` and `daemon_auto_start`. Never disable `code_mode_host`,
`unified_exec` or `shell_snapshot` (every reply came back empty). Watchdog:
Claude Code stops after `QUIET_LIMIT` (180 s) silent, either CLI after
`TOTAL_LIMIT` (15 min); the whole process group is killed and the run tried once
more. `Process::stop` reaps children after killing the group.

Usable = program on PATH (`locate`, `.cmd` on Windows); failures are retryable and
name the sign-in step. `health::writing` is SignInNeeded only from the last
sign-in probe's result (`agent_cli::last_signed_in`, ten minutes), never a new
probe. Sign-in probes accept only recognized status output, honor exit status and
share the bounded watchdog; raw output never leaves. `coding_plans` reports
availability, tri-state sign-in and saved usage, never changes the selected
writer, never makes a model call; an installed CLI alone is not sign-in;
availability and sign-in use the same normalized `AgentCli::bin`.

### Plan limits (`usage.rs`)

`<config>/usage.json` keeps the latest `Usage` (5h/7d `Window`, `seen_at`) per
provider. Claude Code's come free with every answer (`rate_limit_event` →
`from_claude`, saved even when the request failed); never send a request just to
check, except `refresh_claude` on an explicit Refresh (`POST /api/settings/usage`
with `{ "claude": true }`; `{}` reads Codex only): one tiny request on the first
Claude model with `--safe-mode` (never drop it: 184k tokens without, 3k with), no
tools, 20 s, plan logins only. Codex's come from a bare `codex app-server`
`account/rateLimits/read` (`ask_codex`, no thread, no tools, no model call);
failure falls back to bounded reads of recent local `token_count` events (links
rejected, no conversation text; older never replaces newer). Settings' writing
row shows `label` (age past `STALE_AFTER`; a window past its reset reads 0%).

### Vision and imports (`import.rs`, `import/office.rs`, `figures/`)

Native Markdown extraction precedes AI writing. Office XML is parsed with
roxmltree (`nodes_limit`, DTDs refused by default) plus a post-parse depth check
(`MOST_DEPTH`); parts, decompressed bytes and output are bounded; relationships
stay inside the ZIP and external links are never fetched. PDF text uses
`lopdf::extract_text_with_limit` per page; a page number must not hide a scan and
text on another page must not suppress one; an unreadable scan fails with its
page number. Scans over 16 million pixels get a 4,000 px reading copy (the PDF is
never rewritten). Source images ≤64 million pixels, 32,768 px per side, 20 MB
encoded; decoding under a 192 MiB allocation limit; reading images share 90 MB,
optional figures 64 MB. Photos and scans need `ai::see`; no local OCR. A tracking
file name (`filename::file_label`, never camera, scan, screenshot or date names)
is kept in front of the title (`filename::with_label`; `tracking_label` for
calendar events). Action items are the fewest that matter: one per deliverable or
date, never sub-steps, readings or study advice. `complete_with_images` defaults
to a clear "cannot read images" error; Codex's `-i` files go in the private dir
and are removed after.

`writing_budget(cfg)` takes the tightest budget over the chat chain (agents three
parts at once, Ollama one); context/output limits bound actual part sizes. `window_of`: Claude Code 200k, Codex 256k,
Ollama/local 8k (a valid `context` overrides; leo never sends `num_ctx`, so this assumes the server runs at least 8k, decided 2026-11-10). `note_tokens` reserves 45% (≤120k
for plan providers). `felix_room` caps Felix's notes at 32,000 characters after
`FELIX_PROMPT_TOKENS` (4,200); `felix_context` caps warm retention at 480,000.
Effort is passed only in the provider's own format (`--effort`,
`-c model_reasoning_effort=…`; letters and digits only).

### Native tools (`ai/provider/mcp.rs`, `ai/session.rs`)

Claude Code gets leo's tools through a `Bridge`: an MCP server over HTTP on
127.0.0.1, one per answer, a random 64-hex secret in its address, 404 for an
`Origin` header or another path; at most eight connections, bounded writes,
batches and requests; sockets closed on drop; shutdown checked during pending
calls. Native sessions isolate with `--setting-sources "" --disable-slash-commands
--strict-mcp-config` and allow only `WebSearch mcp__leo`. Ollama uses
OpenAI-compatible tool calls when the model supports them. Codex native sessions
are scoped by `codex_scope(program, sub, room)`: `--ignore-user-config
--ignore-rules` when `sub --help` lists both, else every enabled MCP server from
`codex mcp list --json` (never cached; a non-bare-key name is a named failure) is
switched off (`enabled=false` plus a dummy `url`/`command` of its listed transport:
plugin servers have no config table, and a table without a transport is refused), plus `notify=[]`, `skills.include_instructions=false`,
`model_provider="openai"`; a Codex that cannot be scoped is skipped for native
sessions. Every call goes through the validated `Turn.call`; malformed arguments
get a repair message, never execution. If a native path fails, the text protocol
is used. Turns check cancellation before more requests or tools; output goes
through bounded queues; budgets and a final no-tools request bound models that
ignore limits. `provider-contract.yml` (weekly, alert only, never a release gate)
runs the ignored `installed_programs_still_match_what_leo_relies_on`. Feature
discovery is bounded and fails closed (shell control and hooks included). CLI
probes, native turns and text runs share one process-group watchdog covering
stdin, stdout EOF, exit and pipe joins. Bridge request reads keep partial bodies
across polling timeouts and check cancellation on Windows and Unix. A failed
native start is logged through `diag` and shown in Health ("Felix and Codex"). Tests use
local fake servers, never paid APIs.

### What an answer used (`ai/spend.rs`)

Providers report `spent()` (`Spent { model, effort, input, output, estimated }`)
into `ChainOutcome.spent`. OpenAI-compatible providers read `usage` (asking while
streaming only from api.openai.com, openrouter.ai, api.x.ai), else estimate 4
characters a token; reasoning tokens are not added twice. `spend::answered`
prices from `choice.rs` (`rates`); agents are "on your plan", local models free.
Metering never records prompt or reply contents; the root wiring meters
writing/quick/summary outcomes before string-only callbacks drop token metadata.

### The quick model (`ai::quick_config`)

`quick_outcome` runs the `[chat]` chain with each provider's cheapest
`choice::WRITING` model and lowest effort (grading, shared quick reviews,
long-chat memory). Interview runtimes were removed 2026-11-05; downloaded caches
stay on disk.

### Meaning (`meaning.rs`)

BAAI bge-small-en-v1.5 on CPU via candle (all three Candle crates pinned to
0.10.0: 0.11 needs ARM FP16 types unavailable on the Rust 1.88 floor; model,
tokenizer and pooling unchanged). Files pinned by commit and SHA-256
(`MODEL_MANIFEST`) in `models_dir()/bge-small-en-v1.5`, re-hashed only when they
change, unloaded after 60 s idle once no caller holds them (loading outside the
cache mutex). `leo update` fetches it (`ensure_meaning_model`); the app fetches
it quietly when missing. `LEO_INSTALL_NO_MODEL` / `LEO_NO_MEANING_MODEL` turn
that off; `LEO_MEANING_URL` / `LEO_MEANING_MANIFEST` point tests at file://.

### Durable recording workflows

`Session::archive` keeps passages, points, event context, wants, warnings and
metadata-only diagnostics. `recorder::write_up` caches by input hash; only a
successful caller save permits `Session::commit` and audio cleanup. Finish
retries have a bounded window; Finish available cancels queued work but lets the
current request finish. `long::with_profile` adds `<recording_context>` and
`<what_the_user_wants>` (restated last) while keeping repair, gap-filling and
point merging. Track folders use exclusive `create_dir`; metadata uses unique
synced temp files. Capture opens only when requested, never because the page is
visible; `session::awake::Awake` holds App Nap off and native recording holds a
core activity lease until finished. Transcription attempts and filesystem
traversal are snapshotted outside their mutexes before disk I/O; temporary
write-up WAVs are guarded. A missing model leaves
segments pending without `.err`; the recorder waits for a model only the finish
window (120 s), then ends unsaved with audio kept, releasing its guards, and the
recording stays recoverable. `recorder::resume_with_controls` exposes the finish
control in recovery; leo-host refuses to write a note when recovery failed.
Capture queues are bounded; shutdown never waits on an active transcription;
lock heartbeats join on drop. Live previews pass PCM directly (never a temp
live.wav); `Controls.preview` false skips only new preview passes.
`web::fetch_public` is the one https fetch for user-given addresses: public hosts
only, capped size.

### Credentials and public fetch

Credential updates use one bounded writer queue (no I/O under a mutex), unique
synced 0600 temp files, bounded nofollow regular reads and atomic replace, and an
exclusive `.credentials.lock` (five-second deadline) through read/modify/write.
Corrupt credential files are reported and preserved; permission repair refuses
links. Private-file guards unlock before closing. Public fetch clients disable
proxies, check resolved addresses, reject private IPv4/IPv6 (including
site-local), bound bytes and redirects, never downgrade HTTPS; OS DNS lookups run
outside the HTTP runtime with a deadline and at most four lookup threads.
Downloads use owned temp paths, connection/total deadlines and checksums,
extraction has a process deadline, model checks go through the shared catalog
library; models
and toolchains are caches leo never expires.

Capability probes merge stdout and stderr into one bounded pipe and drop the
parent's writers after spawn; a present CLI that fails reports unverified safety
controls, never a missing installation. Health's notes check uses owned temp
files and keeps any existing `.leo-doctor-probe`. `Engine::unload_worker` reaps
the worker without closing the actor, so a failed update can resume.

### Tests

Synthetic capture tests use short segment intervals (twelve rolls), never an hour
of PCM. Saturation tests free queue capacity explicitly rather than relying on
actor scheduling. Cancellation tests wait for the fake CLI's startup marker, measure from
that signal, and require the child PID gone. Tests never touch the real keychain
or microphone.

## Dictation

`dictation/settings.rs` validates bounded preferences in private atomic
`<config>/dictation.json`; a failed live rebind restores the previous file;
malformed settings fall back without overwriting the damaged file. `RETIRED`
keys are stripped before parsing (the struct denies unknown fields).
`activation.rs`: hold, tap and the 300 ms hold-or-tap decision, ignoring repeats.
`generation.rs` gates completion after taking the state lock, so cancel wins.
The catalog refuses language "auto" for a model without `languageDetection`,
and a saved "auto" migrates to the system language, else `en`, else the first
language.

`dictation/text.rs`: custom words match fuzzily/phonetically at 0.18 (phonetic
is an internal soundex port of the old `natural` crate, golden-tested in
`soundex_codes_match_the_natural_crate_it_replaced`), then language-aware filler
rules (on by default; patterns compiled once per language and custom list,
`filler_patterns`), Unicode-safe punctuation and capitalization, clock times only
when marked (`230 pm` → `2:30 pm`; a bare `230` is left to the AI). Fillers on
also drop "you know" at a clause edge and "scratch that" with its sentence (the
next word is capitalized), never "like"; stutter collapse needs fillers on. The
filler language comes from a `whatlang` guess (`TextDetected`: confidence ≥ 0.9
and ≥ 20 characters) before the model's language. Custom-word matching has
guards (hyphens, plurals/possessives, casing); quoted text, code and URLs are
kept; `meaningful()` rejects text with no letter or digit. A vocabulary echo
(the model reciting the custom words) is stripped in `speech/engine.rs`, only
from runs that were given vocabulary; a whole-list drop needs at least eight
words, and text with any word outside the list is never dropped.

`polish.rs` is the optional AI step (`Writing`: off by default, `quick` 12 s,
`selected` 30 s). It is told only the frontmost app's name (never window titles
or screen text) and treats the dictation as text, never instructions. Replies are
only unwrapped (code fence, `<dictation>`), never `clean_reply`. A reply must keep
70% of the words (characters for unspaced scripts), add at most half new ones
(15% for a question) and keep the same number of negations; counts are
multisets, `FILLERS` don't count and number words/digits count as one token.
Added links, pipes, backticks, dollar signs, semicolons and stray line breaks
are refused, as are changed digits and am/pm; a spoken "no" self-correction is
honored. The prompt carries the custom words and spoken punctuation; the chain
has no tools; a leading `<think>` block is stripped. Failures are logged without
the provider's text. The call uses `ChatProvider::complete_until` so cancel or
deadline stops an agent CLI; every fallback returns a reason the popup shows.

`capture.rs` bounds dictation to 300 s and closes the input before draining and
transcribing. It returns `Captured { pcm, stalled }`: a capture that stalls 5 s
after first audio ends and transcribes what was heard, and `Mic::heard_signal`
tells a silent microphone from a quiet room. Dictation input takes the loudest
channel instead of the average (recordings keep their downmix); Windows opens a
500 ms buffer, retrying with the default config. `Mic::open_named` opens only the selected input; `microphones`
enumerates without opening. `AudioGate` makes lecture capture wait for a playing
feedback sound and blocks new ones until the recording ends (no I/O under its
mutex). Streaming dictation falls back to the retained batch PCM; queued live
audio is coalesced (≤16,000 samples a feed). `Engine::warm` loads the model at
keypress for non-streaming models. A reservation pauses segment/live passes; recording's pin keeps the worker
through dictation.
The model library lists damaged and uncatalogued GGUF files; deletion stays
contained and never follows links.

`leo update` fetches the release installer over HTTPS with time/size limits and
never pipes a failed curl into an empty successful shell. Git backup settings and
Health checks are retired.
