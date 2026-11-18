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

pub fn stale_temp(name: &str, meta: &std::fs::Metadata) -> bool {
    name.len() == 10
        && name.starts_with(".tmp")
        && name.bytes().skip(4).all(|b| b.is_ascii_alphanumeric())
        && meta.is_file()
        && meta
            .modified()
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600)
}

pub fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("the file has no parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents.as_ref())?;
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
}
