# User simulation: leo

## Summary

Six simulated first-time users, each shaped by a competing product, walked
leo's main flows on 2026-11-09. Two reached their goal cleanly (the margin-notes
writer, with detours; the evaluator, who lost no work), four did not. The top
problems:

1. **The writing AI is never set up** and every option needs a terminal (5 of 6).
2. **A failed write-up looks like success**: the note is a raw transcript under
   "Your recording is now a note." (3 of 6).
3. **Errors and Health advice point nowhere, or to the wrong place** (4 of 6).

Mode: **walkthrough**. Leo's test server was not built and the desktop shell
(permissions, global shortcut, native recording) cannot be driven from a
browser, so each user reconstructed the screens from the page code and
user-visible strings. Every finding is inferred; the serious ones were
reproduced by reading the code afterwards (marked "reproduced").

## Who we simulated

| Persona | Goal | Outcome | Where it broke |
|---|---|---|---|
| **W** Wispr Flow switcher, PM, impatient, no AI tools | Dictate clean Slack and email text from anywhere | Achieved with trouble, gave up on "clean" | Writing AI needs npm or a terminal; no tone per app |
| **G** Granola switcher, founder, call in 5 minutes, Claude Code installed but never signed in | Notes on a Zoom call, send a summary | Partial | Raw transcript instead of notes; sign-in needs a terminal; share is PDF only |
| **S** First-year student on Windows, never used a terminal | Record a lecture, get notes, get quizzed | Gave up | Same writing AI wall, then raw errors from Felix |
| **C** Grad student who lives in Claude and ChatGPT, Codex signed in | Ask questions across three PDFs | Partial, by luck | No chat on launch; file drop makes notes; default AI is Ollama, not the signed-in Codex |
| **R** Philosophy student who uses a margin-notes writing app, Claude Code signed in | Draft an essay with pushback in the margin | Achieved with detours | Default persona doesn't push back; no per-note instructions |
| **E** IT lead evaluating for a team; skips setup, denies the mic, quits mid-write | Decide on privacy, cost and data safety | No work lost; verdict "not yet" | Health contradicts the privacy promise; silent failures |

## Findings

### 1. No path to a working writing AI without a terminal
- **Severity**: Blocker. **Hit by** 5 of 6 (W, G, S, C, E).
- **Kind**: flow flaw.
- **Where**: first-run setup; Settings → Writing AI.
- **What happened**: Setup covers permissions and the speech model only. The default writing chain is `["ollama"]` (`crates/leo-services/src/config/mod.rs:25`); nothing detects an installed and signed-in Claude Code or Codex. In Settings each card says something like "`codex` is not installed: install Codex: npm install -g @openai/codex.", "If it asks, run `claude` once and sign in.", or "Ollama has no models yet: run `ollama pull qwen3:8b` on this computer." (`crates/leo-services/src/web_settings.rs:104-131`).
- **What they expected, and why**: AI built in (Granola, Wispr Flow), or "I'm signed in elsewhere, so it works" (C).
- **Evidence**: S: "I don't know what npm is." G: "That means a terminal, which I wasn't going to open mid-call." W: "'Claude plan' reads like my claude.ai subscription, but it isn't."
- **Basis**: inferred, reproduced (config default, no detection).

### 2. A failed write-up is reported as success
- **Severity**: Major (silent failure). **Hit by** 3 of 6 (G, S, E).
- **Kind**: defect.
- **Where**: Record → Stop and save → the note.
- **What happened**: When writing fails, leo saves the transcript and records warnings, but `accept()` navigates straight to the note and toasts "Your recording is now a note." (`crates/leo-web/src/web/recording.js:205-208`). The warnings stay on the writing screen that was just replaced.
- **What they expected, and why**: structured notes, or at least a reason.
- **Evidence**: S: "I assumed this was what 'notes' means." E: "I would think Leo writes poor notes."
- **Basis**: inferred, reproduced.

### 3. Errors don't lead to fixes; Health gives wrong advice
- **Severity**: Major. **Hit by** 4 of 6 (S, C, E, R).
- **Kind**: defect and flow flaw.
- **Where**: Felix chat, margin notes, uploads, Settings → Health.
- **What happened**: Felix can show the chain error ("every chat provider failed. Last error: …"). Health's fix for writing leads with `brew install ollama && ollama pull qwen3:8b` on every platform, and for speech says "leo update (downloads the speech model) … or: Settings in leo, choose OpenAI, Gemini or xAI, add its key" (`crates/leo-services/src/health.rs:236-244`). Neither is true for the app today.
- **What they expected, and why**: a button to the fix; advice that matches the README ("Speech never leaves your computer").
- **Basis**: inferred, reproduced (Health text).

### 4. Ollama is marked Ready with nothing to run
- **Severity**: Major (silent failure). **Hit by** 2 of 6 (E, W indirectly).
- **Kind**: defect.
- **Where**: Settings → Writing AI status, Health, dictation's AI writing warning.
- **What happened**: Health checks only that the Ollama port is open; `writing_ready()` treats any keyless provider as available without even that (`crates/leo-services/src/ai/mod.rs:261`, `ai/provider/openai.rs:528`). So the dictation warning "No writing AI is set up yet" never shows for the default, and an Ollama with no model shows Ready.
- **Basis**: inferred, reproduced.

### 5. Nothing starts the work for you
- **Severity**: Major. **Hit by** 3 of 6 (W, G, C).
- **Kind**: flow flaw / mental-model mismatch.
- **Where**: end of onboarding, home screen, Record.
- **What happened**: After setup the window shows "Pick a note on the left". The dictation shortcut is never shown (W held fn and nothing happened). There is no prompt when a calendar event starts (G). The chat is a small face icon; dropping PDFs on the home screen makes notes rather than starting a chat (C).
- **What they expected, and why**: Wispr's practice sandbox; Granola's meeting notification; a chat app's text box.
- **Basis**: inferred.

### 6. Output is hard to take elsewhere
- **Severity**: Minor to Major. **Hit by** 2 of 6 (G, C).
- **Kind**: flow flaw.
- **Where**: the note's share icon; Felix answers.
- **What happened**: Share is labelled "Export as PDF" and opens print. Felix answers offer "Save as note" and "Add to note", with no copy, retry or edit.
- **Basis**: inferred.

### 7. Recording finds problems only after the fact
- **Severity**: Major. **Hit by** 3 of 6 (G, S, E).
- **Kind**: flow flaw.
- **Where**: Record page.
- **What happened**: No check before Start. A missing speech model shows up as "Your note could not be saved" with the cause under Error details. A denied microphone shows as "No sound for a while: is the right microphone chosen, and not muted?". A headset on a Call may fail the second track only after Start.
- **Basis**: inferred (pre-flight absence confirmed by the code audit: `health::recording()` is used only in a test).

### 8. Felix in the margin is close to a margin-notes writing app, but its controls are hidden
- **Severity**: Minor. **Hit by** 1 of 6 (R).
- **Kind**: mental-model mismatch.
- **Where**: note meta line, selection pill.
- **What happened**: "Reader ▾" reads like a view mode, not who reads; Critic ("Pushes on weak spots") is the right persona but found by accident. "Ask Felix" on a selection answers at once with no chance to ask a pointed question. No way to give instructions for one note. Drawing works if asked, but nothing says so. Reads that produce nothing leave no trace.
- **Basis**: inferred.

### 9. Wording assumes students, Macs and AI vocabulary
- **Severity**: Minor. **Hit by** 4 of 6 (W, G, S, E).
- **Kind**: mental-model mismatch.
- **Where**: README, Record page, model cards, install guide.
- **What happened**: "classes", "exam", "menu bar" on Windows (reproduced, `recording.js:370`), "0.6B", "TDT", "× real time", "Coding accounts and remaining usage". The README leads with lectures, so a dictation user reads dictation as a side feature.
- **Basis**: inferred, partly reproduced.

### 10. Small speed bumps
- **Severity**: Polish. Download then a separate "Use and finish" for the speech model (W, S); "Write the note again" only behind "Show transcript" (G); Read through disabled until first save with the reason only in a tooltip (R); a 90-second pause between read-along comments with no cue (R); the export description narrower than the zip (E); quit while writing gives a generic confirmation (E).
- **Basis**: inferred.

## How users think about this app

| Users assume | Leo assumes |
|---|---|
| AI is part of the app, or follows the account they already pay for | AI is a separate program the user installed and signed in from a terminal |
| "Notes" means a written-up summary | A transcript with a heading is a valid note |
| "Claude plan" means their claude.ai subscription | It means a Claude Code sign-in |
| A meeting app knows the meeting is starting | The user opens Record and presses Start |
| The app opens on something to do (a box, a meeting, a practice field) | The app opens on the note explorer |
| Share means a link or a copy | Share means print to PDF |
| A persona menu says who is reading | "Reader ▾" looks like a view mode |
| Health tells the truth for this computer | Health text is shared across platforms and older versions |

## What worked

Every persona who reached these got through them cleanly; keep them as they are.

- The permissions page with live checkmarks and plain reasons.
- Recommended speech-model cards preselected.
- Recording that continues with the window closed, the pill and tray to return, and recovery that never loses audio ("Not saved yet" with Recover).
- The live notepad ("leo fills in the rest") during a recording.
- The dictation popup states and the "Copy last dictation" fallback.
- Margin reply threads (Reply, Resolve, Dismiss, Add to note) and Read through with Undo.
- "Save as note" from Felix in one click.
- Keeping rows that explain each kind of data and ask before deleting; a zip export without keys.

## Coverage and limits

- **Not run live.** No browser drove the app; the desktop shell can't be driven
  that way anyway. Timings (model download, write-up) are unknown.
- **Contamination.** leo-web's CLAUDE.md was loaded into several users' context
  automatically during their walk. They report trying not to use it; it may
  have made them find Felix's drop zone and chat list faster than a real user.
- **Not covered:** Windows install and SmartScreen in detail, the calendar page,
  boards, Felix's quiz quality, dictation accuracy, macOS system-audio prompts.
- **Simulated users are not real users.** They read every string; real users skim
  more and give up sooner. Confirm with the experiments in [README.md](README.md).

The cast was chosen without a confirmation round, since the task asked for the
whole pipeline. To check the fixes, rerun the same six personas after N1, N2,
Q2 and Q3 land.
