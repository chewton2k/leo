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

#[cfg(test)]
mod tests {
    use super::*;

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
