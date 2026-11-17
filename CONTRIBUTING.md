# Contributing to leo

Thanks for helping. This page covers getting the code running, how the project
is laid out, and what a change needs before it can be merged.

## Getting set up

You need Rust 1.88 or newer for the workspace, and 1.90 or newer for the app
(`crates/leo-desktop`), on macOS or Windows ([rustup.rs](https://rustup.rs)) and git. SoX
(`brew install sox` on macOS) is optional: without it the audio tests
skip themselves.

```sh
git clone https://github.com/chewton2k/leo-cli
cd leo-cli
cargo check
cargo test -p leo-core
```

Run one crate or one test locally; CI runs the whole suite on every push and
pull request.

To try your changes without touching your real notes, point leo at a scratch
directory. `LEO_HOME` keeps notes, settings and keys all in one place:

```sh
LEO_HOME=/tmp/leo-dev pnpm --dir crates/leo-desktop tauri dev
```

## How the code is organised

leo is a Cargo workspace. Each crate may only depend on the ones listed before
it, and the compiler enforces this, so please keep it that way rather than
reaching upward:

| Crate | Holds |
|-------|-------|
| `crates/leo-core` | Notes, the on-disk store, and retention choices. No terminal and no network. |
| `crates/leo-services` | AI providers and their fallback chains, prompts, config and credentials, recording, and health checks. |
| `crates/leo-web` | leo's pages and the engine behind them. |
| `crates/leo-host` | Gives the engine its AI, speech, settings, recording housekeeping and import powers; the Playwright test server. |
| `crates/leo-desktop` | The leo app: window, menu bar icon, single instance, open at login. |
| root `leo` | `leo` (opens the app), `leo update`, `leo uninstall`. |

A few ideas run through all of it:

- **One engine.** The app starts leo-web on loopback and supplies native powers
  through traits. The page stays plain JS and the web crate depends only on core.
- **One local speech model.** Recording and dictation share the catalog selection
  and supervised worker. Dictation goes first; recording pins the loaded model.
- **One terminal entry point.** The small bundled command opens the app, updates
  it or uninstalls it. Setup and diagnostics live in Settings.

## Tests

Every change comes with tests, and a bug fix comes with a test that failed
before the fix. Write the test first where you can.

```sh
cargo test -p leo-core                            # one crate
cargo test -p leo-core <name>                     # one test
cargo test --test e2e                             # end-to-end only
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

What each part covers:

- **Unit tests** sit beside the code in each crate.
- **`tests/e2e.rs`** runs the real `leo` binary against a throwaway `LEO_HOME`,
  with a cleared environment and a PATH of system tools.
- **`tests/install_app.rs`** runs the app installers into a throwaway home directory.
- **`tests/release.rs`** checks the scripts that pick and set the next
  version.
- **`crates/leo-web/tests/*.test.js`** are Node tests of the page code, such as
  `markdown.test.js` (the Markdown renderer) and `saving.test.js` (durable
  drafts, failed saves, concurrent edits, conflict recovery). CI's node job runs
  them all with `node --test crates/leo-web/tests/*.test.js`; locally run one
  file, e.g. `node --test crates/leo-web/tests/markdown.test.js`.
- **Browser flows** run against leo's real engine (`leo-test-server`) with isolated notes. After
  `cargo build -p leo-host --bin leo-test-server`, run `pnpm --dir crates/leo-web/tests/browser install`,
  `pnpm --dir crates/leo-web/tests/browser exec playwright install chromium`,
  then `pnpm --dir crates/leo-web/tests/browser test`. CI runs them on macOS.
- **`tests/wording.rs`** fails if any text a user can see names a command that no
  longer exists. If you rename or remove a command, update the text it points
  at, or add the old name to its list.

Tests must never reach the network, your real notes, or the OS keychain. AI
calls go through the `action::Ai` trait, which has a test double. For
credentials, use `MemoryStore`, which other crates get through the
`test-support` feature of `leo-services`. Two tests that do reach the network
are marked `#[ignore]` and are only run by hand.

## Style

- Run `cargo fmt`. CI rejects unformatted code, and clippy warnings are errors.
- Don't add code comments. Put the reasoning in the commit message instead.
- Use user-facing words in messages. A message that tells someone what to do
  should name a command or key that exists, and say it the way the help does.

## Commits and pull requests

Commit messages start with a type — `feat`, `fix`, `refactor`, `test`, `docs`,
`style`, `ci` or `release` — and a short summary in plain words:

```
fix: saving a note deleted any note file leo could not read

A note file with a header leo could not parse was skipped on load, and the
next save deleted every file it had not loaded. ...
```

The body says what was wrong or missing and why the change fixes it. Keep each
commit to one change.

Before opening a pull request:

- [ ] The tests for the crates you changed pass locally, and CI (the full
      suite) is green, including any new tests for your change
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all` has been run
- [ ] If the change is something a user would notice: the README says so

CI runs for pull requests and pushes to `main`; branch pushes do not duplicate
the pull request run. Branches without a pull request can use Actions → CI →
Run workflow. Main compares with the newest successful CI ancestor or newer
release tag, so failed or cancelled pushes remain covered. Pull requests compare
with their base. Unknown paths, CI configuration changes, manual runs, merge
queue runs and the weekly run (Mondays, 09:23 UTC) select every check.

| Change | Checks in addition to formatting and CI selection tests |
|--------|---------------------------------------------------------|
| Documentation only | None |
| README or CONTRIBUTING | Rust checks (the wording test reads them) |
| Page scripts (`app/*.js`, top-level `web/*.js`) | The Rust checks, all JavaScript unit tests, Chromium and WebKit integration suites |
| Other page assets | All JavaScript unit tests, Chromium and WebKit integration suites |
| JavaScript unit tests only | All JavaScript unit tests |
| Browser tests only | Chromium and WebKit integration suites (`tests/browser/server.js` also runs the JavaScript unit tests) |
| Core, services, host or web Rust | Workspace tests on macOS and Windows, clippy, Rust 1.88 check, Intel macOS compile check, both browser suites |
| Desktop, CLI or Rust integration tests | The same Rust checks, without browser suites |
| Installer or release helper scripts | Rust checks and installer validation, including PowerShell syntax on Windows |
| Dependency or CI configuration | All affected checks; dependencies and CI configuration select everything |

Chromium runs in three shards and WebKit in two; the first Chromium shard also
runs the phone test. All five jobs download the same test-server artifact built
once earlier in that run, and each installs only its own browser. Clippy, the
Rust 1.88 check and the Intel Mac compile check share one macOS job; the full
Intel release build runs in the weekly and manual runs and at release. A run
starts eight macOS jobs and needs at most five at once, the number GitHub runs
for this repository, so jobs do not wait on each other. Each job has a timeout. Pull
requests restore Rust caches from main without uploading another copy; browser
dependencies use pnpm's cache. Existing build scripts and release packaging stay
the same.

`CI passed` is the aggregate status check: selected checks must succeed,
intentionally unselected checks may be skipped, and failures or cancellations
block release. It is the recommended required check for branch protection.
The scheduled full run does not publish a release.

The speaker-label check (`speakers.yml`) builds its test once and shares it with
every model job. A pull request that touches labelling runs three models (the
default, a Whisper and a Canary), two at a time; the weekly run and a manual run
check every curated model, three at a time.

Run the CI planner's regression tests without compiling the app:

```sh
python3 scripts/test_ci_scope.py
node --test scripts/ci-base.test.cjs
pnpm --dir crates/leo-web/tests/browser run test:shards
```

Measure queue time, job duration and median/p95 time to `CI passed` separately
when comparing runs. The current pipeline has no instrumented line or branch
coverage gate. If adding one, collect coverage on scheduled runs first and
evaluate changed-line coverage against that baseline before adding a threshold.
Each browser shard starts its own server with a separate temporary data directory.
Tests within a shard keep one worker because they share mutable application state.
`fullyParallel` lets Playwright split individual tests, including the large flow
file, across machines; it does not increase workers. CI checks that the desktop and
WebKit shards are balanced and select every test exactly once. Each shard retains its HTML report, screenshots and
failure traces in an artifact named for its project and shard. Every shard must
pass the existing aggregate `CI passed` gate; a failing shard does not cancel
its siblings. Retries and test timeouts are unchanged.

## Releases

Nobody makes a release by hand. When a push to `main` passes CI and changes
anything under `src/`, `crates/` or `Cargo.*`, the release workflow publishes
it as the next patch version, and the install command picks it up. The
workflow commits the new number to `Cargo.toml` on `main` (`release: vX.Y.Z`),
so pull before pushing again. For a minor
or major release, raise `version` in `Cargo.toml` in your change; that version
is used instead. `scripts/next-version.sh` holds the rule and
`tests/release.rs` tests it.

## Reporting bugs

Open an issue with what you did, what you expected, and what happened. Settings, Health in
leo helps a lot. It checks leo, the notes, the AI and recording, and
never includes your API keys.
