# leo — maintainer notes

Private notes for working on and releasing leo. This file is tracked; the
public guide for contributors is CONTRIBUTING.md.

## Before every push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <the crate you changed>     # or one test; never the whole workspace here
cargo fmt --all --check
```

Run only targeted checks locally (one crate, one test, one node test file, one
Playwright `-g` test); CI runs the full suite on every push.

CI (`.github/workflows/ci.yml`) runs on main pushes and pull requests without
duplicating branch pushes. `scripts/ci-base.cjs` selects the newest successful
push ancestor or newer release tag; a failed or cancelled push never advances
the baseline. Pull requests compare with their base. `scripts/ci-scope.sh`
selects Rust, Node and browser checks independently. Page-only changes
run Node and both browser suites, without workspace tests;
docs only run formatting and planner regression tests. Both browser jobs use
one test-server artifact, with separate servers and homes. Nightly, manual,
merge queue and unknown-input runs test everything. `CI passed` aggregates the
selected jobs and gates release; the nightly run never releases. See
CONTRIBUTING.md for the selection table and planner tests. Newer pushes cancel
older matching check jobs, but never a release in progress.
Check a run from the terminal, since `gh` is not installed here:

```sh
curl -s "https://api.github.com/repos/chewton2k/leo-cli/actions/runs?per_page=3" \
  | python3 -c "import json,sys; [print(r['name'], r['head_sha'][:7], r['status'], r['conclusion']) for r in json.load(sys.stdin)['workflow_runs']]"
```

The repository moved to `chewton2k/leo-cli`; `origin` already points there.

## CI audit, 2026-11-02

The audit sampled the 30 most recent CI runs and inspected the step timings of
[successful PR run 41542664119](https://github.com/chewton2k/leo-cli/actions/runs/41542664119).
There were 17 distinct commits in that sample; 13 had both push and PR runs.
The inspected run took 50m57s. Its Intel build started 41m22s after the workflow
was created, about 36m45s after change detection finished, then ran for 9m34s.
The Chromium integration step, including dependency/browser installation, took
13m20s. Two browser jobs independently built the same server. These observations
identify duplicate work and queue pressure; one run cannot establish median or
p95 performance, and job timestamps do not expose GitHub's scheduling decisions.

The changes remove duplicate push/PR runs, gate unrelated checks, run cheap
checks on Ubuntu, reuse one browser-server build and avoid PR Rust cache uploads.
Both browser engines remain in the PR path for page changes; Rust changes keep
macOS/Windows tests, MSRV and the Intel build. This is deterministic selection
by component, with full nightly validation and conservative fallback. It does
not predict which individual test cases can be omitted.

Published practices support this direction, at very different scales:

| Source | Reported practice | Application here |
|--------|-------------------|------------------|
| [Google, Software Engineering at Google, chapter 23](https://abseil.io/resources/swe-book/html/ch23.html) | Fast presubmit tests, asynchronous affected tests, dependency analysis and batching under resource limits | Select by affected component and measure queue time separately |
| [Meta, Predictive Test Selection, 2018](https://engineering.fb.com/2018/11/21/developer-tools/predictive-test-selection/) | A learned selector ran about one-third of transitively affected tests while catching over 99.9% of problematic changes in its reported deployment | Consider history-based selection only after collecting representative failure data; those historical results are not a guarantee for leo |
| [Anthropic, Test Impact Analysis, 2026](https://claude.com/blog/agentic-coding-is-straining-ci-heres-how-we-scaled-test-impact-analysis-at-anthropic?trk=public_post_comment-text) | Deterministic selection using result history and package relevance; the history listener itself became a scaling bottleneck | Keep the planner small, bounded, tested and independent of release completion |
| [OpenAI, Harness Engineering, 2026](https://openai.com/index/harness-engineering/) | Repository-enforced invariants and isolated application environments for agents | Keep executable CI rules and reproducible regression tests alongside the code |

These are first-party accounts, not independent comparisons of current company
pipelines. They support the principles rather than a promised speedup. Leo's
planner currently trusts completed successful workflow runs, including release
completion, or published release tags; decoupling release status is future work.

Next, compare at least 20 comparable page and Rust PR runs, separating cold and
warm caches, and report median/p95 queue and execution times plus runner minutes.
The first run of a CI configuration change deliberately selects everything.
If Chromium remains the long pole, isolate the mutable flow fixtures and then
measure sharding; simply adding workers would race the shared server state.
If Rust remains dominant, measure dependency-aware crate selection or compiler
caching before changing the build system. No Bazel migration is justified by
this sample. Instrumented line/branch coverage is not configured today: start
with scheduled collection and a measured baseline, then assess changed-line
coverage and flake/retry rates before introducing a blocking percentage.

## Releasing

Automatic. Push to `main`; when every CI job passes (tests on macOS and Windows,
lint, MSRV), `ci.yml` calls `release.yml`, which:

1. runs `scripts/next-version.sh`: the last `v*` tag with patch + 1, or
   `Cargo.toml`'s version if that is newer, or nothing when `src/`, `crates/`
   and `Cargo.*` are unchanged since the last tag (docs-only pushes);
2. sets it in `Cargo.toml` and `Cargo.lock` with `scripts/set-version.sh` and
   commits that to `main` as `release: vX.Y.Z` (github-actions[bot]; its
   token does not trigger another CI run). If `main` moved on meanwhile, it
   stops and the newer push's run releases instead;
3. builds, from that commit, `aarch64-apple-darwin`, `x86_64-apple-darwin`,
   `x86_64-unknown-linux-gnu` (Ubuntu 22.04, older glibc) and
   `aarch64-unknown-linux-gnu`;
4. publishes release `vX.Y.Z`, tagging the version commit, with a `.tar.gz` and
   `.sha256` per platform, `install.sh`, and notes listing the commit subjects
   since the previous release.

**After a push that releases, `git pull` before pushing again**: `main` has the
bot's version commit. For a minor or major release, set `version` in
`Cargo.toml` (e.g. `0.3.0`) in the commit. To re-run a release by hand: Actions tab → Release → Run workflow.

Check the real installer against a new release, in a throwaway home:

```sh
T=$(mktemp -d); mkdir -p $T/home
env -i HOME=$T/home SHELL=/bin/zsh PATH=/usr/bin:/bin \
  sh -c 'curl -fsSL https://raw.githubusercontent.com/chewton2k/leo-cli/main/install.sh | sh'
$T/home/.local/bin/leo --version
```

Your own copy updates itself from Settings → Updates & about.

**A broken release:** delete the release and its tag on GitHub (and locally with
`git tag -d vX.Y.Z`), fix the problem, then release a new patch version. Don't
reuse a version number: people may have already downloaded it.

`install.sh` is served from `main` on raw.githubusercontent.com, so a change to
it reaches users as soon as it is pushed, not when a release is made.

## The website's browser tests

```sh
cargo test -p leo-host --no-run           # builds target/debug/leo-test-server, which the tests serve
cd crates/leo-web/tests/browser
./node_modules/.bin/playwright test -g "<one test>"   # one test locally; CI runs them all, Chromium and WebKit
```

The flows include a contrast check (`readability`): every text on every page,
light and dark, must be at least 3:1 against what is behind it. There is no
screenshot suite.

## Tests that only run by hand

These reach the network, so they are `#[ignore]`d:

```sh
# HTTPS through rustls, against a public endpoint (no key needed).
cargo test -p leo-services tls -- --ignored

# Reading the latest release from GitHub's redirect.
cargo test -p leo-services latest -- --ignored

```

`LEO_FAKE_AUDIO=<wav>` also works on a normal run: recording then replays the
file instead of using the microphone.

## Useful for debugging

- `LEO_HOME=/tmp/leo-dev leo` gives a separate world of notes, config and keys.
- Settings → Health shows everything leo depends on, without a microphone or AI
  request.
- `LEO_CHAT_PROVIDER=ollama` pins a single chat provider, bypassing the
  fallback chain.
- My own data: `~/Library/Application Support/leo/` (notes, `config.toml`,
  `credentials.json`).
- With the lid closed, a MacBook's built-in microphone is off, so recording hears
  nothing. Use an external mic.

Never run against GitHub in CI. The app itself is tried by hand with
`pnpm --dir crates/leo-desktop tauri dev` (a throwaway `LEO_HOME` keeps your
notes out of it): the window opens signed in, closing hides it and the Dock icon brings it back (Windows: it minimizes),
a second launch focuses the first, Felix answers, Quit exits within seconds.

## Pitfalls learned the hard way

- **Never write a note file except through `Store::save`.** It checks the file
  still has the bytes leo saw, which is what keeps Obsidian's edits safe.
- **Never change `stable_id`.** Headerless notes' ids come from it; a golden
  test guards the values.
- **Never slice a `String` at a byte offset computed from a length.** Korean
  text crashed live transcription that way (v0.2.5). Use char indices or
  `str::get`.
- **Unit tests must not open the microphone.** `rec` hangs when no input device
  exists (lid closed), which hung the whole suite.
- **`Store::save` reconciles the notes directory with memory.** Any `.md` file
  it did not load gets deleted, except files listed in `Store::unreadable`. Keep
  that exception if you touch saving.
- **Never read the real keychain in tests.** An unsigned test binary blocks on a
  permission dialog nobody can click. Pass a `&dyn SecretStore` and use
  `MemoryStore`.
- **Never resolve DNS in health checks.** `to_socket_addrs` has no timeout;
  `health::port_open` only connects to literal addresses and localhost.
- **A provider's `max_tokens` in config.toml replaces what leo asks for**, it
  does not cap it. Raising leo's own constants does nothing for a provider with
  its own setting.
- **Anything user-facing that names a command must name a real one**, or
  `tests/wording.rs` fails.
- **Two programs, one notes folder.** The app and another editor (Obsidian)
  can both write at once. `Store.known` keeps a save from trashing notes it never had,
  and the web server reloads from disk per request. Keep both if you touch
  saving or the server.
- **Don't `cp` over a running binary on macOS**: the new copy gets killed.
  install.sh writes `.leo.new` and renames it; keep it that way.
- **After a release, `git pull`.** CI commits `release: vX.Y.Z` to main.
- **No code comments.** Reasoning goes in commit messages.
- **Commits:** no `Co-authored-by` lines; conventional type prefix; the body
  explains why.

## Ideas not done yet

- Resolve `[[wikilinks]]` by title in the app.
- An Obsidian plugin inside an existing vault.

- A Homebrew tap (`brew install chewton2k/tap/leo`). It needs a second
  repository, `homebrew-tap`, and a token for the release workflow to update the
  formula.
- Keep the audio file when a recording's transcription fails, so it can be
  retried.
- Flashcards or a quiz generated from a note.
