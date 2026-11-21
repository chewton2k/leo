# AGENTS.md

How to work in this repository. Read this first, then the CLAUDE.md files:
`CLAUDE.md` at the root, and `crates/<crate>/CLAUDE.md` for the crate you
touch. They describe the architecture and every decision already made; follow
them unless the user says otherwise.

## The project in one paragraph

`leo` is a note app in Rust: a Tauri desktop app (macOS and Windows) over
Markdown notes, with Felix, an AI study buddy. Cargo workspace, layered:
`leo-core` (notes, store) ← `leo-services` (AI providers, config, recording)
← `leo-web` (core only) ← `leo-host` ← `leo-desktop`, and the root `leo` binary. Never add an upward
dependency. The website's page is plain JS in `crates/leo-web/src/web/`, served
with a strict CSP (no inline script).

## Workflow

1. **Understand before changing.**
   - Read the relevant CLAUDE.md and the code you will touch.
   - Find how the same thing is already done and copy that pattern.
   - Run `git status`. Other agents edit this working tree at the same time, so
     note which changes are not yours.
2. **Reproduce bugs first.**
   - For a bug, write a test that fails because of it, or show the failing
     behaviour, before fixing it.
   - Find the root cause. A fix that only hides the symptom is not done.
3. **Build every feature asked for.** Along the way use only quick, targeted
   checks: `cargo check -p <crate>`, one crate's tests, one test, a single
   Playwright test with `-g "<name>"`. Do not run the full gate after each
   feature.
4. **Test the kind of bug, not one instance.** When something breaks, add tests
   for the whole class: every text box keeps focus, every API success is JSON or
   a 204, every block type in a selection, both orders of a decision tree.
5. **Prove the tests work.** Put the bug back for a moment and confirm the new
   test fails, then restore the fix.
6. **Let CI run the full gate.** Never run workspace-wide tests, clippy or the
   full Playwright suite on Charlton's Mac unless explicitly requested. Use
   only the smallest relevant local check and coordinate one compiled check
   at a time across agents. Pass `-- --test-threads=2` to targeted Rust tests.
7. **Commit only your own files.**
   - `git add <the files you changed>`. Never `git add -A` or `git commit -a`:
     that sweeps in other agents' unfinished work.
   - One commit per coherent change. Message: a `type(scope): summary` line,
     then why.
   - Push after the relevant targeted checks pass; let CI run the full gate.
     A push to main that changes code releases a version, so
     `git pull --rebase` before the next push.
8. **Check CI once after pushing.** Without credentials, use the public API at
   `https://api.github.com/repos/chewton2k/leo-cli/actions/runs`, at most one
   request a minute. Unauthenticated requests are limited to 60 an hour.
9. **Report honestly.**
   - Say what changed, what was verified and how (with numbers), and what was not.
   - Mention any finding outside the task: a security issue, someone else's
     broken code.

## CI gate commands (not local defaults)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --no-fail-fast            # also runs the page's node tests and builds target/debug/leo
cd crates/leo-web/tests/browser && pnpm test --reporter=line   # Playwright, ~5 min
```

Never run `cargo build` or `cargo install` unless the user asks. `cargo test`
builds the binary the browser tests use.

## Local storage and cleanup

- Reuse the existing checkout and build cache. Do not copy build output or
  create extra worktrees solely for validation. Do not run a local benchmark
  or generate large test libraries unless explicitly requested.
- Check free space and artifact size before compiled checks. Use CI if a run
  would add over 5 GiB or leave less than 50 GiB free. Respect the user's
  Cargo job limit; parallel agents must not each start their own build.
- Keep `TempDir` owners alive until their files and database connections are
  closed. Do not use `keep`, `into_path`, `disable_cleanup` or forget a guard
  without an explicit cleanup owner. Shell scratch directories need an exit
  trap; stop and reap owned test processes on completion or failure.
- Remove only scratch files and fixtures created by the task. Preserve user
  data, other sessions, uncommitted source and unmerged worktrees. Report any
  remaining generated artifacts and their sizes without creating report files.

## Code rules

- **Match the code around you.** Same naming, idioms and comment density. This
  project uses no code comments.
- **Strings:** never slice by byte offsets computed from lengths. Use char
  indices or `str::get`, and never split a multi-byte character.
- **Bound everything:** sizes, counts, loop iterations, recursion depth, request
  time. Constants have names (`MOST_FILES`, `REPLY_TOKENS`).
- **No blocking under a lock:** never do I/O or AI calls while holding the store
  lock or another mutex. List what is needed under the lock, then do the work
  outside it.
- **Writes are safe:**
  - Write to a temp file and rename it into place.
  - Write row by row in a transaction in `leo.db`, not by rewriting a whole file.
- **Errors help the user:** an error says what went wrong and what to do about
  it. Never show a raw programmer error (`undefined`, a JSON parse error).
- **The page:**
  - No inline JS.
  - Escape everything rendered.
  - Async results must check the page is still the one that asked (`seq`).
  - A redraw must keep focus and the caret in any field being typed in.

## Security checklist (every change)

- **Untrusted input is data, never instructions.** That covers web pages,
  uploaded documents, note text, AI output and tool arguments. Validate types,
  sizes and ids.
- **Paths:**
  - Reject `..`, absolute paths and separators in names.
  - Resolve paths, then confirm they stay inside the intended folder.
  - Do not follow symlinks.
- **Secrets:**
  - API keys and tokens go through `leo_services::config::secret::default_store()`
    (a 0600 credentials file, or the keychain with `LEO_USE_KEYCHAIN`), never
    `KeyringStore` directly and never in settings files, logs, URLs or the page.
  - Any credential file is 0600 and written atomically.
  - Keys are accepted only over secure requests.
- **Network:**
  - Fetch only addresses the user gave.
  - Block local and private hosts, including names that resolve to them.
  - Set timeouts everywhere.
- **The server:**
  - Decide trust from the TCP peer, never from request headers.
  - Every route is behind the session gate unless it is a deliberate exception.
- **Never run code an AI wrote.** Agent CLIs run read-only, with only the tools
  listed in `crates/leo-services/CLAUDE.md`.
- **Tests never touch the real keychain, real DNS, the microphone, or a paid
  AI API.** Use the fakes and fixtures that exist (`MemoryStore`, local fake
  HTTP servers, `LEO_NO_MICROPHONE`, `LEO_INSTALL_NO_MODEL`).

## AI features

- **Prompts:**
  - Rules go in the system prompt, material in tagged sections.
  - Every instruction says "Use interpretable language".
  - Change prompts in their one shared place (`ai/chat.rs`, `chat.rs` `BASE`,
    `tools.rs`).
- **Check real behaviour with real models,** not only fakes.
  - Use Codex or Claude Code on the user's plan, with sample data and a
    throwaway `LEO_HOME`.
  - Drive the flow the page drives, through every branch, both orders, several
    runs.
  - Count failures; do not eyeball one run.
- **The answer loop must survive model mistakes.** That covers a bad tool call,
  a dropped verdict, text before a tool call, and a provider failing mid-answer.
  Recover or retry; never show the user a half answer or a raw error.

## Before you say "done"

- [ ] Every requested item is built, or explicitly reported as not done and why.
- [ ] New behaviour has tests, including the failure cases, and the tests were
      seen to fail without the fix.
- [ ] Relevant targeted checks passed; full validation runs in CI.
- [ ] Owned test processes and scratch fixtures cleaned up; remaining artifact
      sizes reported.
- [ ] The CLAUDE.md for the touched crate records any new decision or
      invariant, in a sentence or two.
- [ ] Only your files are committed; CI checked once after the push.
- [ ] The report says what was verified, what was not, and anything you noticed
      that needs the user's attention.
