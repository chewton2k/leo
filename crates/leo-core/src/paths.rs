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

pub fn write_new(path: &std::path::Path, contents: impl AsRef<[u8]>) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("the file has no parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file_mut().write_all(contents.as_ref())?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
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
    open_regular_with(path, options)
}

pub fn open_append(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).create(true).append(true);
    open_regular_with(path, options)
}

pub(crate) fn open_regular_with(
    path: &std::path::Path,
    mut options: std::fs::OpenOptions,
) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::other("the file is a link"));
        }
    }
    if !metadata.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(file)
}

pub type FileDigest = [u8; 32];

pub fn digest(bytes: &[u8]) -> FileDigest {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}

pub fn digest_file(path: &std::path::Path, most_bytes: u64) -> std::io::Result<FileDigest> {
    digest_stamped(path, most_bytes).map(|(digest, _)| digest)
}

#[cfg(test)]
pub(crate) type WhileHashing = (std::path::PathBuf, Box<dyn FnOnce() + Send>);

#[cfg(test)]
pub(crate) static WHILE_HASHING: std::sync::Mutex<Vec<WhileHashing>> =
    std::sync::Mutex::new(Vec::new());

pub fn digest_stamped(
    path: &std::path::Path,
    most_bytes: u64,
) -> std::io::Result<(FileDigest, EntryStamp)> {
    use sha2::Digest;
    use std::io::Read;
    let changed = || std::io::Error::other("the file changed while leo read it");
    let file = open_regular(path)?;
    let before = handle_stamp(&file)?;
    if before.1 > most_bytes {
        return Err(std::io::Error::other("the file is too large to verify"));
    }
    let mut reading = (&file).take(most_bytes.saturating_add(1));
    let mut hash = sha2::Sha256::new();
    let mut chunk = [0; 32 * 1024];
    loop {
        let read = reading.read(&mut chunk)?;
        #[cfg(test)]
        {
            let hook = {
                let mut hooks = WHILE_HASHING.lock().unwrap_or_else(|e| e.into_inner());
                hooks
                    .iter()
                    .position(|(at, _)| at == path)
                    .map(|at| hooks.remove(at).1)
            };
            if let Some(hook) = hook {
                hook();
            }
        }
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    if reading.limit() == 0 {
        return Err(std::io::Error::other("the file is too large to verify"));
    }
    if handle_stamp(&file)? != before || !same_stamp(path, &before) {
        return Err(changed());
    }
    Ok((hash.finalize().into(), before))
}

pub fn same_stamp(path: &std::path::Path, stamp: &EntryStamp) -> bool {
    !stamp.0 && stamp_at(path).is_ok_and(|found| found == *stamp)
}

pub fn digest_text(digest: &FileDigest) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub fn parse_digest(text: &str) -> Option<FileDigest> {
    let mut digest = [0; 32];
    if text.len() != digest.len() * 2 {
        return None;
    }
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(digest)
}

pub type FileStamp = (u64, u128, i64);

pub fn file_stamp(meta: &std::fs::Metadata) -> FileStamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        meta.ctime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = 0;
    (meta.len(), modified, changed)
}

pub type EntryStamp = (bool, u64, u128, u128, u64, u128);

pub type FolderStamps = Vec<(String, EntryStamp)>;

fn handle_stamp(file: &std::fs::File) -> std::io::Result<EntryStamp> {
    #[cfg(not(windows))]
    {
        Ok(metadata_stamp(&file.metadata()?))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::*, Storage::FileSystem::*};
        let handle = HANDLE(file.as_raw_handle());
        let mut basic = FILE_BASIC_INFO::default();
        let mut id = FILE_ID_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&basic) as u32,
            )?;
        }
        let modern_id = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                std::mem::size_of_val(&id) as u32,
            )
        };
        let (volume, index) = match modern_id {
            Ok(()) => (
                id.VolumeSerialNumber,
                u128::from_le_bytes(id.FileId.Identifier),
            ),
            Err(error)
                if [
                    ERROR_INVALID_FUNCTION,
                    ERROR_NOT_SUPPORTED,
                    ERROR_INVALID_PARAMETER,
                ]
                .iter()
                .any(|code| code.to_hresult() == error.code()) =>
            {
                let mut legacy = BY_HANDLE_FILE_INFORMATION::default();
                unsafe { GetFileInformationByHandle(handle, &mut legacy) }?;
                let index =
                    (u64::from(legacy.nFileIndexHigh) << 32) | u64::from(legacy.nFileIndexLow);
                (u64::from(legacy.dwVolumeSerialNumber), u128::from(index))
            }
            Err(error) => return Err(error.into()),
        };
        if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(std::io::Error::other("the path is a link"));
        }
        if index == 0 {
            return Err(std::io::Error::other("the file has no identity"));
        }
        Ok((
            basic.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
            file.metadata()?.len(),
            basic.LastWriteTime as u128,
            basic.ChangeTime as u128,
            volume,
            index,
        ))
    }
}

#[cfg(not(windows))]
fn metadata_stamp(meta: &std::fs::Metadata) -> EntryStamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    #[cfg(unix)]
    let (changed, dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        let changed = u128::try_from(meta.ctime()).unwrap_or(0) * 1_000_000_000
            + u128::try_from(meta.ctime_nsec()).unwrap_or(0);
        (changed, meta.dev(), u128::from(meta.ino()))
    };
    #[cfg(not(unix))]
    let (changed, dev, ino) = (0, 0, 0);
    (meta.is_dir(), meta.len(), modified, changed, dev, ino)
}

pub fn stamp_at(path: &std::path::Path) -> std::io::Result<EntryStamp> {
    #[cfg(not(windows))]
    {
        let meta = std::fs::symlink_metadata(path)?;
        if !(meta.is_file() || meta.is_dir()) {
            return Err(std::io::Error::other("not a regular file or folder"));
        }
        Ok(metadata_stamp(&meta))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        };
        let file = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        handle_stamp(&file)
    }
}

pub fn folder_stamps(folder: &std::path::Path, left_out: &[&str]) -> std::io::Result<FolderStamps> {
    use std::path::Path;
    let refused = || std::io::Error::other("the folder contents could not be verified");
    const MOST_ENTRIES: usize = 10_000;
    const MOST_DEPTH: usize = 32;
    let mut stack = vec![(folder.to_path_buf(), 0)];
    let mut stamps = Vec::new();
    while let Some((dir, depth)) = stack.pop() {
        if depth > MOST_DEPTH || !stamp_at(&dir)?.0 {
            return Err(refused());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let key = path
                .strip_prefix(folder)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(refused)?;
            if left_out.contains(&key) {
                continue;
            }
            if stamps.len() >= MOST_ENTRIES {
                return Err(refused());
            }
            let stamp = stamp_at(&path)?;
            if stamp.0 {
                stack.push((path.clone(), depth + 1));
            }
            stamps.push((key.to_string(), stamp));
        }
    }
    stamps.sort();
    Ok(stamps)
}

pub fn digest_directory(
    folder: &std::path::Path,
    most_bytes: u64,
    left_out: &[&str],
) -> std::io::Result<(FileDigest, FolderStamps)> {
    let stamps = folder_stamps(folder, left_out)?;
    let digest = digest_listed(folder, &stamps, most_bytes, left_out)?;
    Ok((digest, stamps))
}

fn digest_listed(
    folder: &std::path::Path,
    stamps: &FolderStamps,
    most_bytes: u64,
    left_out: &[&str],
) -> std::io::Result<FileDigest> {
    let refused = || std::io::Error::other("the folder contents could not be verified");
    let total = stamps
        .iter()
        .filter(|(_, stamp)| !stamp.0)
        .try_fold(0u64, |sum, (_, stamp)| sum.checked_add(stamp.1));
    if total.is_none_or(|total| total > most_bytes) {
        return Err(refused());
    }
    let mut files = std::collections::BTreeMap::new();
    for (key, stamp) in stamps {
        let content = if stamp.0 {
            None
        } else {
            let (digest, found) = digest_stamped(&folder.join(key), stamp.1)?;
            if *stamp != found {
                return Err(refused());
            }
            Some(digest)
        };
        files.insert(key.clone(), content);
    }
    if folder_stamps(folder, left_out)? != *stamps {
        return Err(refused());
    }
    let bytes = serde_json::to_vec(&files).map_err(std::io::Error::other)?;
    Ok(digest(&bytes))
}

pub fn purge_folder(
    folder: &std::path::Path,
    stamps: &FolderStamps,
    last_file: Option<&str>,
) -> std::io::Result<()> {
    use std::{fs, io, path::Path};
    let changed = || io::Error::other("the folder changed before leo could remove it");
    let root = stamp_at(folder)?;
    if !root.0 {
        return Err(changed());
    }
    let directory_matches = |found: &EntryStamp, expected: &EntryStamp| {
        found.0 && found.4 == expected.4 && found.5 == expected.5
    };
    let path_for = |key: &str| {
        if !stamp_at(folder).is_ok_and(|found| directory_matches(&found, &root)) {
            return Err(changed());
        }
        let path = contained_path(folder, Path::new(key)).map_err(io::Error::other)?;
        for parent in Path::new(key)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let key = parent.to_str().ok_or_else(changed)?;
            let index = stamps
                .binary_search_by(|(name, _)| name.as_str().cmp(key))
                .map_err(|_| changed())?;
            if !stamp_at(&folder.join(parent))
                .is_ok_and(|found| directory_matches(&found, &stamps[index].1))
            {
                return Err(changed());
            }
        }
        Ok(path)
    };
    let files = stamps.iter().filter(|(_, stamp)| !stamp.0);
    let last = files
        .clone()
        .find(|(key, _)| last_file == Some(key.as_str()));
    for (key, stamp) in files.filter(|(key, _)| last_file != Some(key.as_str())) {
        let path = path_for(key)?;
        if !same_stamp(&path, stamp) {
            return Err(changed());
        }
        fs::remove_file(path)?;
    }
    for (key, stamp) in stamps.iter().rev().filter(|(_, stamp)| stamp.0) {
        let path = path_for(key)?;
        if !stamp_at(&path).is_ok_and(|found| directory_matches(&found, stamp)) {
            return Err(changed());
        }
        fs::remove_dir(path)?;
    }
    if !stamp_at(folder).is_ok_and(|found| directory_matches(&found, &root)) {
        return Err(changed());
    }
    if let Some((key, stamp)) = last {
        let mut entries = fs::read_dir(folder)?;
        let path = path_for(key)?;
        if entries
            .next()
            .transpose()?
            .is_none_or(|entry| entry.path() != path)
            || entries.next().is_some()
            || !same_stamp(&path, stamp)
        {
            return Err(changed());
        }
        fs::remove_file(path)?;
    }
    fs::remove_dir(folder)
}

pub fn move_new(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if !(meta.is_file() || meta.is_dir()) {
        return Err(std::io::Error::other("only files and folders are moved"));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(from.as_os_str().as_bytes())?;
        let to = std::ffi::CString::new(to.as_os_str().as_bytes())?;
        let moved = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if moved == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
        };
        let wide = |path: &std::path::Path| -> std::io::Result<Vec<u16>> {
            use std::path::{Component, Prefix};
            let path = std::path::absolute(path)?;
            let raw: Vec<_> = path.as_os_str().encode_wide().collect();
            if raw.contains(&0) {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
            }
            let (prefix, tail) = match path.components().next() {
                Some(Component::Prefix(p)) => match p.kind() {
                    Prefix::Disk(_) => (r"\\?\", raw.as_slice()),
                    Prefix::UNC(_, _) => (r"\\?\UNC\", &raw[2..]),
                    _ => ("", raw.as_slice()),
                },
                _ => ("", raw.as_slice()),
            };
            Ok(prefix
                .encode_utf16()
                .chain(tail.iter().copied())
                .chain([0])
                .collect())
        };
        let from = wide(from)?;
        let to = wide(to)?;
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        }?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        if !meta.is_file() {
            return Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
        }
        std::fs::hard_link(from, to)?;
        std::fs::remove_file(from)
    }
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
    pub const KEEPING_STATE: &str = ".leo-keeping";
    pub const UI_PREFS: &str = "ui-prefs.json";
    pub const CALENDARS: &str = "calendars.json";
    pub const CALENDAR: &str = "calendar";
    pub const INCOMING: &str = "incoming";
    pub const CAPTIONS: &str = "captions.json";
    pub const GRAPH: &str = "graph.json";
    pub const MEANING: &str = "meaning.json";
    pub const RECORDINGS: &str = "recordings";
    pub const MODELS: &str = "models";
    pub const ENV: &str = ".env";

    pub const ORIGINALS: &str = "attachments";
    pub const SOURCES: &str = "recording-sources";
    pub const HELD: &str = "held";

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
        KEEPING_STATE,
        UI_PREFS,
        CALENDARS,
        CALENDAR,
        INCOMING,
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
    pub const KEPT_WITH_NOTES: &[&str] = &[ORIGINALS, SOURCES, HELD, KEEPING_STATE];
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

    pub fn damaged_end(stamp: &str, n: usize) -> String {
        if n > 1 {
            format!(".damaged-{stamp}-{n}")
        } else {
            format!(".damaged-{stamp}")
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
    fn a_move_never_replaces_what_arrived_at_its_destination() {
        let temp = tempfile::tempdir().unwrap();
        let (file, taken) = (temp.path().join("a.png"), temp.path().join("b.png"));
        std::fs::write(&file, "mine").unwrap();
        std::fs::write(&taken, "synced").unwrap();
        assert!(move_new(&file, &taken).is_err());
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "synced");
        assert!(file.exists());
        let (folder, held) = (temp.path().join("folder"), temp.path().join("held"));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("x"), "x").unwrap();
        std::fs::create_dir(&held).unwrap();
        assert!(move_new(&folder, &held).is_err());
        assert!(folder.join("x").exists() && held.exists());
        std::fs::remove_dir(&held).unwrap();
        move_new(&folder, &held).unwrap();
        assert!(held.join("x").exists() && !folder.exists());
        let free = temp.path().join("c.png");
        move_new(&file, &free).unwrap();
        assert!(!file.exists() && std::fs::read_to_string(free).unwrap() == "mine");
    }

    #[test]
    fn a_folder_changed_while_it_is_hashed_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a"), "one").unwrap();
        let (_, stamps) = digest_directory(temp.path(), 1 << 20, &[]).unwrap();
        std::fs::write(temp.path().join("b"), "two").unwrap();
        assert!(digest_listed(temp.path(), &stamps, 1 << 20, &[]).is_err());
        assert!(digest_directory(temp.path(), 5, &[]).is_err());
    }

    #[test]
    fn purging_a_verified_folder_keeps_a_later_nested_file() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("held");
        let nested = folder.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("seen"), "seen").unwrap();
        let manifest = folder.join("session.json");
        std::fs::write(&manifest, "recording metadata").unwrap();
        let (_, stamps) = digest_directory(&folder, 1 << 20, &[]).unwrap();
        let synced = nested.join("synced");
        std::fs::write(&synced, "arrived after checking").unwrap();
        assert!(purge_folder(&folder, &stamps, Some("session.json")).is_err());
        assert!(manifest.exists());
        assert_eq!(
            std::fs::read_to_string(&synced).unwrap(),
            "arrived after checking"
        );
    }

    #[test]
    fn a_new_file_never_replaces_one_that_arrived() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Note.md");
        std::fs::write(&path, "synced").unwrap();
        assert!(write_new(&path, "leo").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "synced");
        let free = temp.path().join("Other.md");
        write_new(&free, "leo").unwrap();
        assert_eq!(std::fs::read_to_string(free).unwrap(), "leo");
    }

    #[cfg(windows)]
    #[test]
    fn an_exclusive_move_supports_windows_long_paths() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("a".repeat(140)).join("b".repeat(140));
        std::fs::create_dir_all(&folder).unwrap();
        let from = folder.join("note.md");
        let to = folder.join("trashed.md");
        std::fs::write(&from, "words").unwrap();
        move_new(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "words");
        assert!(!from.exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_stamps_distinguish_a_replacement_with_matching_timestamps() {
        use std::fs::{self, FileTimes, OpenOptions};
        use std::os::windows::fs::FileTimesExt;
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("note.md");
        let replacement = temp.path().join("replacement.md");
        fs::write(&original, "before").unwrap();
        let (_, stamp) = digest_stamped(&original, 100).unwrap();
        let times = fs::metadata(&original).unwrap();
        fs::write(&replacement, "edited").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_created(times.created().unwrap())
                    .set_modified(times.modified().unwrap()),
            )
            .unwrap();
        fs::rename(replacement, &original).unwrap();
        let replaced = fs::metadata(&original).unwrap();
        assert_eq!(replaced.created().unwrap(), times.created().unwrap());
        assert_eq!(replaced.modified().unwrap(), times.modified().unwrap());
        assert!(!same_stamp(&original, &stamp));
        assert_ne!(stamp_at(&original).unwrap().5, stamp.5);
    }

    #[cfg(windows)]
    #[test]
    fn regular_open_rejects_linked_windows_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.json");
        let link = temp.path().join("session.json");
        std::fs::write(&target, "{}").unwrap();
        if std::os::windows::fs::symlink_file(&target, &link).is_ok() {
            assert!(open_regular(&link).is_err());
        }
    }

    #[test]
    fn the_database_and_its_copies_are_listed_and_nothing_like_them() {
        let stamp = own::damaged_stamp(chrono::Utc::now());
        for n in [1, 2, 3, 12] {
            for side in ["", "-wal", "-shm"] {
                let name = format!("leo.db{}{side}", own::damaged_end(&stamp, n));
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
