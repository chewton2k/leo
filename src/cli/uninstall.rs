use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use std::collections::HashSet;

use leo_core::paths::own;
use leo_core::store::Store;
use leo_services::config::Config;
use leo_services::session::{self, Session};

const MARKER: &str = "# Added by the leo installer";

fn leftovers(dir: &Path, kept: &[PathBuf]) -> Vec<PathBuf> {
    gather(dir, kept, dir.file_name().is_some_and(|n| n == "leo"))
}

fn gather(dir: &Path, kept: &[PathBuf], all: bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if kept.contains(&path) {
            continue;
        }
        let mine = all || own::listed(&entry.file_name().to_string_lossy());
        if kept.iter().any(|k| k.starts_with(&path)) {
            if mine {
                out.extend(gather(&path, kept, true));
            }
            continue;
        }
        if mine {
            out.push(path);
        }
    }
    out
}

#[derive(Debug, Default, PartialEq)]
struct Recordings {
    kept: Vec<PathBuf>,
    owned: usize,
    unfinished: usize,
}

fn recordings(root: &Path, notes: &Path) -> Recordings {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Recordings::default();
    };
    let trashed = leo_core::store::trashed_ids(notes);
    let listed = session::kept_owned(root);
    let live: Option<HashSet<String>> = listed
        .iter()
        .any(|(_, note)| !trashed.contains(note))
        .then(|| {
            notes
                .is_dir()
                .then(|| Store::load_from(notes).ok())
                .flatten()
                .filter(|store| store.unreadable.is_empty())
                .map(|store| store.notes.into_iter().map(|note| note.id).collect())
        })
        .flatten();
    let unfinished: HashSet<PathBuf> = Session::unfinished_page(root, None, usize::MAX)
        .0
        .into_iter()
        .map(|session| session.dir)
        .collect();
    let mut owned = HashSet::new();
    let mut gone = HashSet::new();
    for (dir, note) in listed {
        let belongs =
            live.as_ref().is_none_or(|live| live.contains(&note)) || trashed.contains(&note);
        if belongs {
            owned.insert(dir);
        } else if !unfinished.contains(&dir) && !session::locked(&dir) {
            gone.insert(dir);
        }
    }
    let mut kept: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !gone.contains(path))
        .collect();
    kept.sort();
    Recordings {
        kept,
        owned: owned.len(),
        unfinished: unfinished.difference(&owned).count(),
    }
}

struct Plan {
    doomed: Vec<PathBuf>,
    recordings: Recordings,
}

fn plan(dirs: &[PathBuf], notes: &Path, home: &Path, root: &Path, temp: Option<&Path>) -> Plan {
    let mut dirs = dirs.to_vec();
    dirs.dedup();
    let recordings = recordings(root, notes);
    let mut kept = own::kept_with(notes);
    kept.extend(recordings.kept.iter().cloned());
    let holds_kept = |dir: &Path| kept.iter().any(|k| k.starts_with(dir));
    let mut out = Vec::new();
    for dir in dirs {
        let owned = dir.file_name().is_some_and(|n| n == "leo");
        if owned && !holds_kept(&dir) && dir.exists() {
            out.push(dir);
        } else {
            out.extend(leftovers(&dir, &kept));
        }
    }
    let models = home.join(".leo");
    if !home.as_os_str().is_empty() && models.is_dir() && !holds_kept(&models) {
        out.push(models);
    }
    if let Some(entries) = temp.and_then(|t| std::fs::read_dir(t).ok()) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if own::LEGACY_TEMP.iter().any(|p| name.starts_with(p)) && name.ends_with(".wav") {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out.dedup();
    Plan {
        doomed: out,
        recordings,
    }
}

fn kept_recordings(recordings: &Recordings, root: &Path, home: &Path) -> Option<String> {
    let count =
        |n: usize, kind: &str| format!("{n} {kind}recording{}", if n == 1 { "" } else { "s" });
    (recordings.owned + recordings.unfinished > 0).then(|| {
        format!(
            "  Kept {} that belong to your notes, and {} not yet saved as notes, in {}. Open leo again to save the unfinished ones.",
            count(recordings.owned, ""),
            count(recordings.unfinished, "unfinished "),
            pretty(root, home)
        )
    })
}

fn described(path: &Path, home: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.starts_with("leo.db.damaged-") && own::listed(&name) {
        format!(
            "{}: an old copy of leo's database (chats and margin notes from before it was set aside)",
            pretty(path, home)
        )
    } else {
        pretty(path, home)
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub fn run(yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("could not tell where leo is installed")?;
    let mut dirs: Vec<PathBuf> = [
        exe.parent(),
        exe.canonicalize().ok().as_deref().and_then(Path::parent),
    ]
    .into_iter()
    .flatten()
    .map(Path::to_path_buf)
    .collect();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    let notes = Store::notes_dir()?;
    let bundle = super::installation::installed_bundle(&exe)?;
    let fallback = if cfg!(windows) {
        super::installation::app_dir()?.join("bin")
    } else {
        home.join(".local/bin")
    };
    let record = leo_core::paths::config_dir()?.join(own::CLI_DIR);
    let command_dir = super::installation::cli_dir(&exe, &record, &fallback)?;
    if !dirs.contains(&command_dir) {
        dirs.push(command_dir.clone());
    }
    if let Some(bundle) = &bundle {
        anyhow::ensure!(!notes.starts_with(bundle), "Your notes are inside the app folder. Move them in Settings before uninstalling; nothing was removed.");
        super::installation::verify_installed_bundle(bundle)?;
    }
    let root = session::root()?;
    let data: Vec<PathBuf> = [leo_core::paths::config_dir()?, leo_core::paths::data_dir()?].into();
    let temp = std::env::var_os("LEO_HOME")
        .is_none()
        .then(std::env::temp_dir);
    let Plan { doomed, recordings } = plan(&data, &notes, &home, &root, temp.as_deref());
    let kept = kept_recordings(&recordings, &root, &home);

    if !yes {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("nothing removed; run `leo uninstall --yes` to confirm");
        }
        println!();
        println!("  This removes leo and everything it made, except your notes and what belongs to them:");
        println!("    {}", pretty(&exe, &home));
        for path in &doomed {
            println!("    {}", described(path, &home));
        }
        if let Some(bundle) = &bundle {
            println!("    {} and its login item", pretty(bundle, &home));
        }
        println!("    the PATH line the installer added, and any API keys leo stored");
        println!();
        println!(
            "  Your notes stay in {}, with their original files and recording transcripts.",
            pretty(&notes, &home)
        );
        if let Some(kept) = &kept {
            println!("{kept}");
        }
        let answer = super::prompt::ask("  Remove leo? [y/N] ")?;
        if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
            println!("  Nothing removed.");
            return Ok(());
        }
    }

    let providers: Vec<String> = Config::load().providers.keys().cloned().collect();

    let mut left = Left::default();
    println!();
    if let Some(bundle) = &bundle {
        left.step(
            "remove the login item",
            super::installation::remove_login_item(&home, bundle),
        );
    }
    let (later, now): (Vec<&PathBuf>, Vec<&PathBuf>) =
        doomed.iter().partition(|path| record.starts_with(path));
    for path in now {
        remove_listed(&mut left, path, &home);
    }
    for path in later.iter().filter(|path| **path != &record) {
        for inside in gather(path, std::slice::from_ref(&record), true) {
            remove_listed(&mut left, &inside, &home);
        }
    }

    let keychain_is_leos = std::env::var_os("LEO_HOME").is_none()
        || std::env::var("LEO_USE_KEYCHAIN").is_ok_and(|v| v != "0" && !v.is_empty());
    if keychain_is_leos {
        let gone = leo_services::config::secret::forget_keychain(&providers);
        if gone > 0 {
            println!(
                "  Removed {gone} key{} from the system keychain",
                if gone == 1 { "" } else { "s" }
            );
        }
    }

    if cfg!(windows) {
        for dir in &dirs {
            let what = format!("remove {} from your PATH", pretty(dir, &home));
            if let Ok(true) = left.attempt(&what, remove_from_windows_path(dir)) {
                println!("  Removed {} from your PATH", pretty(dir, &home));
            }
        }
    }

    for rc in [
        ".zshrc",
        ".bash_profile",
        ".bashrc",
        ".profile",
        ".config/fish/config.fish",
    ] {
        let path = home.join(rc);
        let what = format!("remove the PATH line from {}", pretty(&path, &home));
        if let Ok(true) = left.attempt(&what, clean_profile(&path, &dirs)) {
            println!("  Removed the PATH line from {}", pretty(&path, &home));
        }
    }

    let mut app_aside = None;
    if last_steps(&left) {
        let resolved = exe.canonicalize().unwrap_or_else(|_| exe.clone());
        match &bundle {
            Some(bundle) if resolved.starts_with(bundle) => {}
            _ => {
                left.step("remove the leo program", remove_program(&exe));
            }
        }
    }
    if let Some(bundle) = bundle.as_ref().filter(|_| last_steps(&left)) {
        let link = command_dir.join(if cfg!(windows) { "leo.cmd" } else { "leo" });
        if super::installation::is_cli_link(&link, bundle) {
            let what = format!("remove the terminal command {}", pretty(&link, &home));
            left.step(&what, remove(&link).map_err(Into::into));
        }
    }
    if last_steps(&left) {
        for path in later {
            remove_listed(&mut left, path, &home);
        }
    }
    if let Some(bundle) = bundle.as_ref().filter(|_| last_steps(&left)) {
        if !left.step("remove the app", super::installation::remove_bundle(bundle)) {
            app_aside = Some(super::installation::aside(bundle)).filter(|aside| aside.exists());
        }
    }
    if last_steps(&left) {
        println!("  Removed {}", pretty(&exe, &home));
    }

    println!();
    if !last_steps(&left) {
        match app_aside {
            Some(aside) => println!(
                "  Not finished: {}. Delete {} yourself.",
                left.0.join(", "),
                pretty(&aside, &home)
            ),
            None => println!(
                "  Not finished: {}. The app was left in place; fix that and run leo uninstall again.",
                left.0.join(", ")
            ),
        }
        println!();
        anyhow::bail!(
            "{} step(s) could not be finished; your notes were not touched.",
            left.0.len()
        );
    }
    println!(
        "  Your notes are still in {}, with their original files and recording transcripts.",
        pretty(&notes, &home)
    );
    if let Some(kept) = &kept {
        println!("{kept}");
    }
    println!("  Tools installed separately (cloudflared, Ollama) stay;");
    println!("  remove them with your package manager if you no longer want them.");
    println!("  Thank you for using leo!");
    println!();
    Ok(())
}

#[derive(Default)]
struct Left(Vec<String>);

impl Left {
    fn attempt<T>(&mut self, what: &str, result: Result<T>) -> Result<T, ()> {
        result.map_err(|error| {
            println!("  Could not {what}: {error:#}");
            self.0.push(what.to_string());
        })
    }

    fn step(&mut self, what: &str, result: Result<()>) -> bool {
        self.attempt(what, result).is_ok()
    }
}

fn last_steps(left: &Left) -> bool {
    left.0.is_empty()
}

fn remove_listed(left: &mut Left, path: &Path, home: &Path) {
    match remove(path) {
        Ok(()) => println!("  Removed {}", pretty(path, home)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            left.step(&format!("remove {}", pretty(path, home)), Err(e.into()));
        }
    }
}

fn clean_profile(path: &Path, dirs: &[PathBuf]) -> Result<bool> {
    use std::io::Write;
    if path.is_symlink() {
        return Ok(false);
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(false);
    };
    let Some(kept) = dirs
        .iter()
        .find_map(|dir| without_installer_lines(&text, dir))
    else {
        return Ok(false);
    };
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("no folder")?)?;
    temp.as_file()
        .set_permissions(std::fs::metadata(path)?.permissions())?;
    temp.write_all(kept.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(true)
}

fn remove_program(exe: &Path) -> Result<()> {
    if !cfg!(windows) {
        return match std::fs::remove_file(exe) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result.with_context(|| format!("could not remove {}", exe.display())),
        };
    }
    let parked = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&parked);
    std::fs::rename(exe, &parked).with_context(|| format!("could not remove {}", exe.display()))?;
    let _ = deferred_remove_command(&parked)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    Ok(())
}

fn deferred_remove_command(path: &Path) -> std::process::Command {
    let mut command = leo_core::children::quiet_command("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; Wait-Process -Id ([int]$env:LEO_REMOVE_PARENT) -Timeout 30 -ErrorAction SilentlyContinue; if(-not (Get-Process -Id ([int]$env:LEO_REMOVE_PARENT) -ErrorAction SilentlyContinue)){Remove-Item -LiteralPath $env:LEO_REMOVE_PROGRAM -Force}"])
        .env("LEO_REMOVE_PARENT", std::process::id().to_string()).env("LEO_REMOVE_PROGRAM", path);
    command
}

pub fn windows_path_without(path: &str, dir: &str) -> Option<String> {
    let same = |entry: &str| {
        entry
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(dir.trim_end_matches(['\\', '/']))
    };
    let entries: Vec<&str> = path.split(';').filter(|e| !e.is_empty()).collect();
    if !entries.iter().any(|e| same(e)) {
        return None;
    }
    Some(
        entries
            .into_iter()
            .filter(|e| !same(e))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

fn remove_from_windows_path(dir: &Path) -> Result<bool> {
    let read = leo_core::children::quiet_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','User')",
        ])
        .output()
        .context("your PATH could not be read")?;
    anyhow::ensure!(read.status.success(), "your PATH could not be read");
    let current = String::from_utf8_lossy(&read.stdout).trim().to_string();
    let Some(kept) = windows_path_without(&current, &dir.to_string_lossy()) else {
        return Ok(false);
    };
    let written = leo_core::children::quiet_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "[Environment]::SetEnvironmentVariable('Path','{}','User')",
                kept.replace('\'', "''")
            ),
        ])
        .status()
        .is_ok_and(|s| s.success());
    anyhow::ensure!(written, "your PATH could not be changed");
    Ok(true)
}

fn without_installer_lines(text: &str, dir: &Path) -> Option<String> {
    let dir = dir.to_string_lossy();
    let quoted = dir.replace('\'', "'\\''");
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let is_block = lines[i].trim_end() == MARKER
            && lines
                .get(i + 1)
                .is_some_and(|next| next.contains(dir.as_ref()) || next.contains(&quoted));
        if is_block {
            if kept.last().is_some_and(|l| l.trim().is_empty()) {
                kept.pop();
            }
            changed = true;
            i += 2;
        } else {
            kept.push(lines[i]);
            i += 1;
        }
    }
    changed.then(|| kept.concat())
}

fn pretty(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_windows_removal_treats_the_executable_path_as_data() {
        let path = Path::new(r"C:\Leo & another command\leo.exe.old");
        let command = super::deferred_remove_command(path);
        assert!(!command
            .get_args()
            .any(|arg| arg.to_string_lossy().contains("another command")));
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "LEO_REMOVE_PROGRAM" && value == Some(path.as_os_str())));
    }
    #[test]
    fn quoted_installer_blocks_are_removed_without_touching_other_shell_code() {
        let dir = Path::new("/home/me/bin with 'quote' and $dollar");
        let text = "keep\n# Added by the leo installer\nexport PATH='/home/me/bin with '\\''quote'\\'' and $dollar':\"$PATH\"\nafter\n";
        assert_eq!(
            without_installer_lines(text, dir).as_deref(),
            Some("keep\nafter\n")
        );
    }
    #[test]
    fn a_windows_path_loses_only_leos_folder() {
        let path = r"C:\Windows;C:\Users\me\AppData\Local\Programs\leo\;C:\Tools";
        assert_eq!(
            windows_path_without(path, r"C:\Users\me\AppData\Local\Programs\leo").as_deref(),
            Some(r"C:\Windows;C:\Tools")
        );
        assert_eq!(
            windows_path_without(r"C:\Windows;C:\Tools", r"C:\leo"),
            None
        );
        assert_eq!(
            windows_path_without(r"c:\users\me\leo;C:\Tools;", r"C:\Users\Me\leo").as_deref(),
            Some(r"C:\Tools")
        );
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    #[test]
    fn in_a_leo_folder_everything_but_the_notes_and_what_belongs_to_them_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let leo = tmp.path().join("leo");
        let notes = leo.join("notes");
        touch(&notes.join("a.md"));
        touch(&leo.join("config.toml"));
        touch(&leo.join("something-new.json"));
        touch(&leo.join("cache/x"));
        touch(&leo.join("attachments/note-a0001/source.pdf"));
        touch(&leo.join("recording-sources/note-a0001/source.json"));
        let mut found = leftovers(&leo, &own::kept_with(&notes));
        found.sort();
        assert_eq!(
            found,
            vec![
                leo.join("cache"),
                leo.join("config.toml"),
                leo.join("something-new.json")
            ]
        );
    }

    #[test]
    fn in_any_other_folder_only_leos_own_files_go() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("me");
        let notes = home.join("notes");
        touch(&notes.join("a.md"));
        for name in [
            "config.toml",
            "serve-token",
            "keep.json",
            "ui-prefs.json",
            "leo.db-wal",
            "graph.json.before-database",
            "recent.json",
            "taxes.pdf",
            "Documents/x",
            "attachments/x",
            "recording-sources/x",
        ] {
            touch(&home.join(name));
        }
        let mut found = leftovers(&home, &own::kept_with(&notes));
        found.sort();
        assert_eq!(
            found,
            [
                "config.toml",
                "graph.json.before-database",
                "keep.json",
                "leo.db-wal",
                "recent.json",
                "serve-token",
                "ui-prefs.json",
            ]
            .map(|name| home.join(name))
        );
    }

    #[test]
    fn notes_nested_deeper_keep_their_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let leo = tmp.path().join("leo");
        let notes = leo.join("data/notes");
        touch(&notes.join("a.md"));
        touch(&leo.join("data/recent.json"));
        let found = leftovers(&leo, &own::kept_with(&notes));
        assert_eq!(found, vec![leo.join("data/recent.json")]);
    }

    fn note(path: &Path, id: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("---\nid: {id}\ntitle: {id}\n---\nbody\n")).unwrap();
    }

    fn recording(root: &Path, saved: bool, note: Option<&str>, failed: bool) -> PathBuf {
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.saved = saved;
        manifest.note = note.map(String::from);
        let session = Session::create(root, manifest).unwrap();
        if failed {
            std::fs::write(session.dir.join("seg-00001.wav"), "audio").unwrap();
        }
        session.dir
    }

    #[test]
    fn recordings_of_kept_notes_and_unfinished_ones_stay() {
        let tmp = tempfile::tempdir().unwrap();
        for (data, notes_beside) in [
            (tmp.path().join("leo"), true),
            (tmp.path().join("mine"), true),
            (tmp.path().join("leo-outside"), false),
        ] {
            let data = if notes_beside { data } else { data.join("leo") };
            let notes = if notes_beside {
                data.join("notes")
            } else {
                data.parent().unwrap().join("notes")
            };
            note(&notes.join("One.md"), "note-n0001");
            note(&notes.join(".trash/Two.md"), "note-n0002");
            let root = data.join("recordings");
            let a = recording(&root, true, Some("note-n0001"), true);
            let b = recording(&root, true, None, false);
            Session::open(&b)
                .unwrap()
                .trace("saved", "Note note-n0002 saved");
            let c = recording(&root, true, Some("note-n0003"), false);
            let d = recording(&root, false, None, false);
            let e = recording(&root, true, Some("note-n0004"), false);
            std::fs::write(e.join("lock"), std::process::id().to_string()).unwrap();
            touch(&data.join("config.toml"));

            let planned = plan(
                std::slice::from_ref(&data),
                &notes,
                Path::new(""),
                &root,
                None,
            );
            assert_eq!(planned.recordings.kept, {
                let mut kept = vec![a.clone(), b.clone(), d.clone(), e.clone()];
                kept.sort();
                kept
            });
            assert_eq!(
                (planned.recordings.owned, planned.recordings.unfinished),
                (2, 1)
            );
            assert!(
                planned.doomed.contains(&c),
                "{data:?}: {:?}",
                planned.doomed
            );
            assert!(planned.doomed.contains(&data.join("config.toml")));
            for path in [&a, &b, &d, &e, &notes, &data, &root] {
                assert!(
                    !planned.doomed.iter().any(|p| path.starts_with(p)),
                    "{path:?} would go: {:?}",
                    planned.doomed
                );
            }

            std::fs::write(notes.join("Broken.md"), "---\nid: [\n---\n").unwrap();
            let planned = plan(
                std::slice::from_ref(&data),
                &notes,
                Path::new(""),
                &root,
                None,
            );
            let mut all = vec![a, b, c, d, e];
            all.sort();
            assert_eq!(planned.recordings.kept, all, "{data:?}");
            assert!(!planned
                .doomed
                .iter()
                .any(|p| root.starts_with(p) || p.starts_with(&root)));
        }
    }

    #[test]
    fn a_run_with_no_failed_step_reports_nothing_left() {
        let mut left = Left::default();
        assert!(left.step("remove the login item", Ok(())));
        assert_eq!(left.attempt("remove a PATH line", Ok(true)), Ok(true));
        assert!(left.0.is_empty());
        assert!(!left.step("remove the app", Err(anyhow::anyhow!("busy"))));
        assert_eq!(left.0, ["remove the app"]);
    }

    #[test]
    fn the_retry_steps_run_only_when_nothing_is_left() {
        let mut left = Left::default();
        assert!(last_steps(&left));
        left.step("remove ~/config.toml", Err(anyhow::anyhow!("denied")));
        assert!(!last_steps(&left));
    }

    #[test]
    fn only_the_installer_block_for_this_directory_is_removed() {
        let dir = Path::new("/home/me/.local/bin");
        let text = "alias ll='ls -l'\n\n# Added by the leo installer\nexport PATH=\"/home/me/.local/bin:$PATH\"\nexport EDITOR=nano\n";
        assert_eq!(
            without_installer_lines(text, dir).as_deref(),
            Some("alias ll='ls -l'\nexport EDITOR=nano\n")
        );
    }

    #[test]
    fn a_file_without_the_block_is_left_alone() {
        let dir = Path::new("/home/me/.local/bin");
        assert_eq!(
            without_installer_lines("export PATH=\"$HOME/bin:$PATH\"\n", dir),
            None
        );
        let other = "# Added by the leo installer\nexport PATH=\"/opt/leo:$PATH\"\n";
        assert_eq!(without_installer_lines(other, dir), None);
    }

    #[test]
    fn fish_blocks_are_removed_too() {
        let dir = Path::new("/home/me/.local/bin");
        let text = "\n# Added by the leo installer\nfish_add_path /home/me/.local/bin\n";
        assert_eq!(without_installer_lines(text, dir).as_deref(), Some(""));
    }
}
