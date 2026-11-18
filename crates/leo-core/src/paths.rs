//! Where leo keeps things on disk.
//!
//! `LEO_HOME`, when set, holds everything — notes, config, keys — in one
//! directory. That is what the end-to-end tests use to run the real binary
//! without touching the user's notes, and it lets someone keep leo's data
//! somewhere of their choosing. Unset, leo follows the platform: notes under the
//! data directory, config under the config directory, kept apart so that
//! machine-local settings are never pushed along with the notes.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// The directory holding `notes/`.
pub fn data_dir() -> Result<PathBuf> {
    choose(home(), dirs::data_dir(), "data")
}

/// The directory holding `config.toml`, keys, and recent notes.
pub fn config_dir() -> Result<PathBuf> {
    choose(home(), dirs::config_dir(), "config")
}

fn home() -> Option<PathBuf> {
    std::env::var_os("LEO_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn choose(home: Option<PathBuf>, platform: Option<PathBuf>, what: &str) -> Result<PathBuf> {
    match home {
        Some(home) => Ok(home),
        None => platform
            .map(|p| p.join("leo"))
            .with_context(|| format!("could not determine a {what} directory for this platform")),
    }
}

/// Whether `binary` is on the PATH. A plain scan with no subprocess, cheap
/// enough to call on every startup.
pub fn on_path(binary: &str) -> bool {
    find(binary).is_some()
}

pub fn find(binary: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        [".exe", ".cmd", ".bat", ".com", ""]
            .iter()
            .map(|ext| format!("{binary}{ext}"))
            .collect()
    } else {
        vec![binary.to_string()]
    };
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

pub fn validate_directory(directory: &str) -> Result<()> {
    if directory.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        !directory.starts_with('/')
            && !directory.contains('\\')
            && !directory.contains(':')
            && !directory.chars().any(char::is_control)
            && directory
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.')),
        "Folders must stay inside your notes: use a relative folder name without . or .."
    );
    Ok(())
}

pub fn contained_path(root: &std::path::Path, relative: &std::path::Path) -> Result<PathBuf> {
    anyhow::ensure!(
        relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Path must stay inside your notes"
    );
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                !meta.file_type().is_symlink(),
                "Notes cannot use symbolic links: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

fn temp_name(name: &str) -> bool {
    const SHAPES: [(&str, &str); 5] = [
        (".tmp", ""),
        (".leo-credentials-", ""),
        (".leo-download-", ".part"),
        (".leo-download-", ""),
        (".speaker-", ".wav"),
    ];
    SHAPES.iter().any(|(front, back)| {
        name.strip_prefix(front)
            .and_then(|rest| rest.strip_suffix(back))
            .is_some_and(|random| {
                random.len() == 6 && random.bytes().all(|b| b.is_ascii_alphanumeric())
            })
    })
}

pub fn stale_temp(name: &str, meta: &std::fs::Metadata) -> bool {
    temp_name(name)
        && meta.is_file()
        && meta
            .modified()
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600)
}

pub fn sweep_stale_temps(dir: &std::path::Path, depth: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 0 {
                removed += sweep_stale_temps(&entry.path(), depth - 1);
            }
            continue;
        }
        let name = entry.file_name();
        let stale = name.to_str().is_some_and(|name| {
            temp_name(name) && entry.metadata().is_ok_and(|meta| stale_temp(name, &meta))
        });
        if !stale {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) => crate::diag::warn(format!(
                "could not remove the old temporary file {}: {error}",
                entry.path().display()
            )),
        }
    }
    removed
}

pub fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> Result<()> {
    write_atomic_with(path, |w| Ok(w.write_all(contents.as_ref())?))
}

pub fn write_atomic_with(
    path: &std::path::Path,
    fill: impl FnOnce(&mut dyn std::io::Write) -> Result<()>,
) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("the file has no parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut out = std::io::BufWriter::new(temp.as_file_mut());
        fill(&mut out)?;
        out.flush()?;
    }
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn open_regular(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(file)
}

pub mod own {
    use std::path::{Path, PathBuf};

    pub const CONFIG: &str = "config.toml";
    pub const CREDENTIALS: &str = "credentials.json";
    pub const CREDENTIALS_LOCK: &str = ".credentials.lock";
    pub const SERVE_TOKEN: &str = "serve-token";
    pub const APP_SESSIONS: &str = "app-sessions.json";
    pub const DICTATION_LOCK: &str = ".dictation.lock";
    pub const SPEECH_MODEL: &str = "speech-model.json";
    pub const SPEECH_VERIFIED: &str = "speech-verified.json";
    pub const SPEECH_LOCK: &str = ".speech.lock";
    pub const SHADERS_READY: &str = "speech-shaders-ready";
    pub const USAGE: &str = "usage.json";
    pub const APP_PORT: &str = "app-port";
    pub const APP_LAUNCHED: &str = "app-launched";
    pub const APP_SETTINGS: &str = "app-settings.json";
    pub const DICTATION: &str = "dictation.json";
    pub const ACCESSIBILITY: &str = "accessibility-version";
    pub const UPDATE_FOREGROUND: &str = "update-foreground";
    pub const LOGIN_PATH: &str = "login-path";
    pub const CLI_DIR: &str = "app-cli-dir";

    pub const DATABASE: &str = "leo.db";
    pub const KEEP: &str = "keep.json";
    pub const UI_PREFS: &str = "ui-prefs.json";
    pub const CALENDARS: &str = "calendars.json";
    pub const CALENDAR: &str = "calendar";
    pub const CAPTIONS: &str = "captions.json";
    pub const GRAPH: &str = "graph.json";
    pub const MEANING: &str = "meaning.json";
    pub const RECORDINGS: &str = "recordings";
    pub const MODELS: &str = "models";
    pub const ENV: &str = ".env";

    pub const ORIGINALS: &str = "attachments";
    pub const SOURCES: &str = "recording-sources";

    pub const ALL: &[&str] = &[
        CONFIG,
        CREDENTIALS,
        CREDENTIALS_LOCK,
        SERVE_TOKEN,
        APP_SESSIONS,
        DICTATION_LOCK,
        SPEECH_MODEL,
        SPEECH_VERIFIED,
        SPEECH_LOCK,
        SHADERS_READY,
        USAGE,
        APP_PORT,
        APP_LAUNCHED,
        APP_SETTINGS,
        DICTATION,
        ACCESSIBILITY,
        UPDATE_FOREGROUND,
        LOGIN_PATH,
        CLI_DIR,
        DATABASE,
        KEEP,
        UI_PREFS,
        CALENDARS,
        CALENDAR,
        CAPTIONS,
        GRAPH,
        MEANING,
        RECORDINGS,
        MODELS,
        ENV,
    ];
    pub const MIGRATED: &[&str] = &[CAPTIONS, GRAPH, MEANING];
    pub const BEFORE: &str = ".before-database";
    pub const DATABASE_SIDES: &[&str] = &["-wal", "-shm"];
    pub const DATABASE_REWRITE: &str = "leo.db.rewrite";
    pub const DATABASE_REWRITE_READY: &str = "leo.db.rewrite-ready";
    pub const KEPT_WITH_NOTES: &[&str] = &[ORIGINALS, SOURCES];
    pub const LEGACY: &[&str] = &[
        "serve-sessions.json",
        "config.toml.new",
        "update-check.json",
        "recent.json",
        ".manual-installed",
        ".tour-completed",
        "notes.json",
        "notes.json.bak",
    ];
    pub const LEGACY_TEMP: &[&str] = &["leo-recording", "leo-live-", "leo-mic-probe-"];

    pub fn damaged_stamp(at: chrono::DateTime<chrono::Utc>) -> String {
        at.format("%Y%m%dT%H%M%SZ").to_string()
    }

    pub fn damaged_copy(database: &str, stamp: &str, n: usize) -> String {
        if n > 1 {
            format!("{database}.damaged-{stamp}-{n}")
        } else {
            format!("{database}.damaged-{stamp}")
        }
    }

    fn is_damaged_copy(name: &str) -> bool {
        let digits = |s: Option<&str>, len: usize| {
            s.is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_digit()))
        };
        let Some(rest) = name.strip_prefix("leo.db.damaged-") else {
            return false;
        };
        if !digits(rest.get(..8), 8)
            || rest.get(8..9) != Some("T")
            || !digits(rest.get(9..15), 6)
            || rest.get(15..16) != Some("Z")
        {
            return false;
        }
        let Some(rest) = rest.get(16..) else {
            return false;
        };
        let rest = DATABASE_SIDES
            .iter()
            .find_map(|side| rest.strip_suffix(side))
            .unwrap_or(rest);
        match rest.strip_prefix('-') {
            None => rest.is_empty(),
            Some(n) => {
                !n.starts_with('0')
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && n.parse::<usize>().is_ok_and(|n| n >= 2)
            }
        }
    }

    pub fn listed(name: &str) -> bool {
        let database = |base: &str| {
            name.strip_prefix(base)
                .is_some_and(|side| side.is_empty() || DATABASE_SIDES.contains(&side))
        };
        ALL.contains(&name)
            || LEGACY.contains(&name)
            || MIGRATED
                .iter()
                .any(|m| name.strip_prefix(m) == Some(BEFORE))
            || [DATABASE, DATABASE_REWRITE, DATABASE_REWRITE_READY]
                .into_iter()
                .any(database)
            || is_damaged_copy(name)
    }

    pub fn kept_with(notes: &Path) -> Vec<PathBuf> {
        let beside = notes.parent().unwrap_or(notes);
        std::iter::once(notes.to_path_buf())
            .chain(KEPT_WITH_NOTES.iter().map(|name| beside.join(name)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_and_its_copies_are_listed_and_nothing_like_them() {
        let stamp = own::damaged_stamp(chrono::Utc::now());
        for n in [1, 2, 3, 12] {
            for side in ["", "-wal", "-shm"] {
                let name = own::damaged_copy("leo.db", &stamp, n) + side;
                assert!(own::listed(&name), "{name}");
            }
        }
        for name in [
            "leo.db",
            "leo.db-wal",
            "leo.db.rewrite",
            "leo.db.rewrite-ready",
        ] {
            assert!(own::listed(name), "{name}");
        }
        for name in [
            "leo.db.damaged-",
            "leo.db.damaged-20261117T101500",
            "leo.db.damaged-20261117T101500Z-1",
            "leo.db.damaged-20261117T101500Z-02",
            "leo.db.damaged-20261117T101500Z.bak",
            "leo.dbx",
            "my-leo.db.damaged-20261117T101500Z",
        ] {
            assert!(!own::listed(name), "{name}");
        }
    }

    #[test]
    fn leo_home_holds_everything() {
        let home = PathBuf::from("/tmp/leo-home");
        assert_eq!(
            choose(Some(home.clone()), Some("/p".into()), "data").unwrap(),
            home
        );
    }

    #[test]
    fn without_it_the_platform_directory_is_used() {
        assert_eq!(
            choose(None, Some("/p".into()), "data").unwrap(),
            PathBuf::from("/p/leo")
        );
    }

    #[test]
    fn no_directory_at_all_is_an_error_not_a_panic() {
        assert!(choose(None, None, "data").is_err());
    }

    #[test]
    fn folder_names_stay_inside_the_notes() {
        for (directory, ok) in [
            ("", true),
            ("a", true),
            ("a/b", true),
            ("..", false),
            ("a/../b", false),
            ("/abs", false),
            ("a\\b", false),
            ("c:x", false),
            (".hidden", false),
            ("a//b", false),
            ("a/", false),
            ("a\nb", false),
        ] {
            assert_eq!(validate_directory(directory).is_ok(), ok, "{directory:?}");
        }
    }

    #[test]
    fn contained_paths_refuse_escapes_and_links_but_allow_missing_leaves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("real")).unwrap();
        assert_eq!(
            contained_path(root, std::path::Path::new("real/missing.md")).unwrap(),
            root.join("real/missing.md")
        );
        assert!(contained_path(root, std::path::Path::new("missing/leaf.md")).is_ok());
        for bad in ["../x", "real/../../x", "/etc/passwd"] {
            assert!(
                contained_path(root, std::path::Path::new(bad)).is_err(),
                "{bad}"
            );
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("middle")).unwrap();
            std::os::unix::fs::symlink(outside.path().join("f"), root.join("real/leaf.md"))
                .unwrap();
            assert!(contained_path(root, std::path::Path::new("middle/x.md")).is_err());
            assert!(contained_path(root, std::path::Path::new("real/leaf.md")).is_err());
        }
    }

    fn aged(path: &std::path::Path, seconds: u64) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(seconds))
            .unwrap();
    }

    const OWN_TEMPS: [&str; 5] = [
        ".tmpAbC123",
        ".leo-credentials-AbC123",
        ".leo-download-AbC123.part",
        ".leo-download-AbC123",
        ".speaker-AbC123.wav",
    ];

    #[test]
    fn leftover_temporary_files_of_every_shape_go_after_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        for name in OWN_TEMPS {
            let old = dir.path().join(name);
            std::fs::write(&old, "half").unwrap();
            aged(&old, 7200);
        }
        let young = dir.path().join(".tmpYoung1");
        std::fs::write(&young, "still being written").unwrap();
        assert_eq!(sweep_stale_temps(dir.path(), 0), OWN_TEMPS.len());
        for name in OWN_TEMPS {
            assert!(!dir.path().join(name).exists(), "{name}");
        }
        assert!(young.exists());
        assert_eq!(sweep_stale_temps(&dir.path().join("missing"), 3), 0);
    }

    #[test]
    fn names_that_only_look_like_temporary_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let near = [
            ".tmpABC12",
            ".tmpABCDEF.md",
            ".tmpABC-12",
            ".leo-download-ABCDE.part",
            ".leo-download-ABCDEF.partial",
            ".leo-credentials-ABCDEF.json",
            ".speaker-ABCDEF",
            ".speaker-ABCDEF.wav.bak",
            "tmpABCDEF",
            "seg-000001.wav",
            "seg-000001.part.wav",
            "note.md",
        ];
        for name in near {
            let path = dir.path().join(name);
            std::fs::write(&path, "x").unwrap();
            aged(&path, 7200);
        }
        assert_eq!(sweep_stale_temps(dir.path(), 0), 0);
        for name in near {
            assert!(dir.path().join(name).exists(), "{name}");
        }
    }

    #[test]
    fn a_folder_with_a_temporary_name_is_not_removed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(".tmpAbC123");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("kept.md"), "x").unwrap();
        assert_eq!(sweep_stale_temps(dir.path(), 1), 0);
        assert!(folder.join("kept.md").exists());
    }

    #[test]
    fn the_sweep_goes_only_as_deep_as_it_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        let levels = [
            dir.path().join(".tmpAbC123"),
            dir.path().join("a/.tmpAbC123"),
            deep.join(".tmpAbC123"),
        ];
        for path in &levels {
            std::fs::write(path, "x").unwrap();
            aged(path, 7200);
        }
        assert_eq!(sweep_stale_temps(dir.path(), 0), 1);
        assert_eq!(sweep_stale_temps(dir.path(), 1), 1);
        assert!(levels[2].exists());
        assert_eq!(sweep_stale_temps(dir.path(), 2), 1);
        assert!(!levels[2].exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_temporary_name_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join(".tmpAbC123");
        std::fs::write(&target, "not leo's").unwrap();
        aged(&target, 7200);
        std::os::unix::fs::symlink(&target, dir.path().join(".tmpAbC123")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("folder")).unwrap();
        assert_eq!(sweep_stale_temps(dir.path(), 2), 0);
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(dir.path().join(".tmpAbC123")).is_ok());
    }

    #[test]
    #[ignore = "benchmark: cargo test -p leo-core --release p_c34 -- --ignored --nocapture"]
    fn p_c34_benchmark_launch_sweep() {
        for folders in [1_000usize, 10_000] {
            let dir = tempfile::tempdir().unwrap();
            for index in 0..folders {
                let folder = dir.path().join(format!("note-{index}"));
                std::fs::create_dir(&folder).unwrap();
                std::fs::write(folder.join("a.pdf"), "x").unwrap();
                std::fs::write(folder.join("b.png"), "x").unwrap();
            }
            let started = std::time::Instant::now();
            let removed = sweep_stale_temps(dir.path(), 1);
            println!(
                "{folders} folders: {:?} ({removed} removed)",
                started.elapsed()
            );
        }
    }

    #[test]
    fn only_regular_files_open() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hi").unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut open_regular(&file).unwrap(), &mut text).unwrap();
        assert_eq!(text, "hi");
        assert!(open_regular(dir.path()).is_err());
        assert_eq!(
            open_regular(&dir.path().join("missing"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(open_regular(&link).is_err());
        }
    }

    #[test]
    fn an_atomic_write_replaces_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.json");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_atomic(&folder, "third").is_err());
        let mut left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["f.json", "folder"]);
        assert!(std::fs::read_dir(&folder).unwrap().next().is_none());
    }

    #[test]
    fn a_streamed_write_that_fails_leaves_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.jsonl");
        write_atomic_with(&path, |w| Ok(w.write_all(b"one\n")?)).unwrap();
        let failed = write_atomic_with(&path, |w| {
            w.write_all(b"two\n")?;
            anyhow::bail!("stopped")
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
