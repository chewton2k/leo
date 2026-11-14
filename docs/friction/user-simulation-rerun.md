# User simulation re-run: leo after the friction fixes

Same six personas and the same walkthrough method as `user-simulation-report.md`
(PR #42), run on 2026-11-09 against branch `feat/reduce-friction`. Walkthrough
mode: each user rebuilt the screens from the page code and user-visible strings.
Nobody ran the app, and every finding is inferred.

## Summary

The four findings from the first run that blocked people or failed silently
are gone:

| First run | Now |
|---|---|
| 1. No path to a writing AI without a terminal (5 of 6) | Setup step 3 shows what is on the computer, signs in from the app, and installs Ollama and a model that fits without a terminal. People with no account and no Ollama still have to install Ollama or skip (by design: leo does not ship its own AI). |
| 2. A failed write-up reported as success (3 of 6) | The toast says the transcript was saved and the notes were not written; the note shows a notice with Set up writing AI and Write the notes. |
| 3. Errors point nowhere; Health gives wrong advice (4 of 6) | One "leo can't write yet" message with a link, used only when setup can help; Health text is per platform and opens the right setting. |
| 4. Ollama shown Ready with nothing to run (2 of 6) | Fixed in Settings and setup; Health and the Record check are fixed in the follow-up batch below. |

## Who we simulated

| Persona | First run | Re-run |
|---|---|---|
| Wispr Flow switcher | Gave up on clean dictation (terminal) | Local dictation works; AI polish needs a writing AI, which setup now offers to set up |
| Granola switcher | Raw transcript, no share | Call recorded, honest note, Copy as Markdown; still no share link (out of scope) and calendar needs an iCal address (approved) |
| Student on Windows | Blocked at writing AI | Ollama path exists; an 8 GB laptop had no model that fit (fixed below) |
| ChatGPT/Claude user | Found chat by luck; no retry/edit | Ask field on home, Ask Felix for dropped files, Copy/Retry/Edit; first question with PDFs could not be retried (fixed below) |
| Margin-notes writer | Default persona didn't push back; no per-note instructions | Critic persona, per-note instructions, question field on a selection; silent reads left no trace (fixed below) |
| Evaluator | Health contradicted privacy; silent failures | Honest quit/relaunch, privacy lines, Keeping; Health still green for Ollama with no model (fixed below) |

## Follow-ups found and fixed in this round

- An 8 GB machine reports slightly under 8 GiB, so no Ollama model fit: the memory check has a 10% tolerance and a smaller model is in the list.
- Health and the Record pre-flight now use the full writing check (Ollama with no model is not green).
- Setup step 3: says AI dictation uses the writing AI too, preselects Ollama when nothing is installed, and no longer says "leo found these" when it found nothing.
- Ollama model cards select first and download from a separate button, like speech models.
- Writing AI notes in Settings no longer show npm or terminal commands.
- Retry and Edit work on a question that had files, @-notes or a selection.
- Quick Leo no longer turns "Make a table comparing…" into a task.
- Record warns before Start when microphone access is off; Health says Open System Settings when it is denied.
- Read-along says "Felix read it; nothing to add." when a read finds nothing.
- The dictation hint shows the shortcut the way the rest of leo does; the AI writing switch mentions app tone.
- README and the install guide put the downloads first and explain Windows SmartScreen.

## Still open (decided or out of scope)

- No share link, email or Slack for a note: leo is local-first; Share offers Copy as Markdown, .md and PDF.
- Calendar connection is an iCal address, not Google or Microsoft sign-in (approved; needs verified OAuth apps).
- The meeting prompt opens Record ready to start; it never starts recording by itself (approved).
- No writing AI is bundled (the user's decision, 2026-11-09).
- Felix has no rail entry or chat rename; Felix's starters lean toward studying.
- Default write-up fills gaps and adds review questions even for meetings (the user's 2026-10-10 decision).
- A silent recording with no speech model is reported as "no speech model", because silence is detected after transcription.

## Limits

Walkthroughs read code; real users are slower and more distracted. Some agents
had a CLAUDE.md file loaded into context automatically and said they didn't use
it. Real-device behaviour (macOS notifications, Terminal sign-in, Windows
SmartScreen, a real Ollama pull) needs a check in the running app.
