# Installing leo

leo is a desktop app for macOS (Apple silicon and Intel) and Windows x64. Linux
is not supported.

## Install

### From a download (easiest)

Download Leo from [Releases](https://github.com/chewton2k/leo-cli/releases/latest):

| Platform | File |
|---|---|
| macOS, Apple silicon | [`leo-aarch64-apple-darwin.dmg`](https://github.com/chewton2k/leo-cli/releases/latest/download/leo-aarch64-apple-darwin.dmg) |
| macOS, Intel | [`leo-x86_64-apple-darwin.dmg`](https://github.com/chewton2k/leo-cli/releases/latest/download/leo-x86_64-apple-darwin.dmg) |
| Windows x64 | [`leo-x86_64-pc-windows-msvc-setup.exe`](https://github.com/chewton2k/leo-cli/releases/latest/download/leo-x86_64-pc-windows-msvc-setup.exe) |

On macOS, open the `.dmg` and drag Leo into Applications. Leo uses a stable
self-signed certificate and is not notarized, so on macOS 15 the first launch of
a browser download is blocked: open System Settings → Privacy & Security and
choose **Open Anyway**.

On Windows, run the `-setup.exe`. If SmartScreen says "Windows protected your
PC", choose **More info**, then **Run anyway**.

### From a terminal

macOS, in Terminal:

```sh
curl -fsSL https://github.com/chewton2k/leo-cli/releases/latest/download/install.sh | sh
```

Windows, in PowerShell:

```powershell
irm https://github.com/chewton2k/leo-cli/releases/latest/download/install.ps1 | iex
```

The macOS installer puts Leo in `/Applications`; Windows uses
`%LOCALAPPDATA%\Leo`. Both also add a `leo` terminal command that points into the
app, so app updates update it too. `LEO_INSTALL_DIR` chooses the terminal
command's folder; `LEO_INSTALL_SKIP_PATH=1` leaves your shell or user PATH
unchanged.

## First launch

Open Leo like any other app, or type `leo` in a terminal. First launch offers
three skippable steps: microphone access (for recording), Accessibility access
(for pasting dictation into other apps) and a local speech model.

Dictation defaults to Option+Space on macOS and Ctrl+Space on Windows; rebind it
in Settings. Speech and models stay on this computer. Writing AI (notes from
recordings, Felix) uses your signed-in Claude Code or Codex plan, or a local
Ollama model; choose it in Settings when you need it. Dictation uses writing AI
only if you turn on its AI writing switch.

## Updates and removal

Settings → Updates & about checks for updates, shows what changed, and installs
when you quit by default. Updates wait for recording and dictation to finish.

`leo uninstall` removes the app, its login item and its local settings and
models, and keeps your notes.

Settings → Health lists setup problems and how to fix each one.

## Moving from the terminal version

macOS users with a standard installation can quit their old Leo session, run
the install command above (an older copy's `leo update` does the same), then
open Leo in Applications. The installer replaces the terminal command with the
app launcher, so typing `leo` opens the app. The terminal interface and
browser-server workflow are replaced by the desktop window.

Your notes stay in the same folder. Leo reads the existing database and writing
settings; there is no export or import step. Built-in GitHub backup is retired;
existing repositories and remotes stay untouched. Keep the same operating-system
account and data location when switching.

Download a speech model in Settings → Speech models; the previous speech model
cannot be reused. Previous cloud speech choices fall back to local speech.
Stored keys, writing AI sign-ins and calendar credentials stay in leo's
credentials file.

Two cases are not automatic yet:

- **Windows:** an older copy's `leo update` command keeps its executable running while
  the installer tries to replace it. Quit old Leo, run the downloaded Windows
  installer, then open the app from Start. An existing terminal command may
  still point to the old version.
- **Custom `LEO_HOME`:** a value set only in your shell is not remembered by an
  app opened from Finder or Start, so that launch may show a different notes
  folder. Keep your existing setup until custom-location migration is
  supported.

Linux users can keep their existing terminal installation and notes.
