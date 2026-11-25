use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::filename;
use crate::notes::Note;
use crate::paths::write_atomic;

// ── Frontmatter serialization ───────────────────────────────────────────────

#[derive(Serialize)]
struct NoteFrontmatter {
    id: String,
    title: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
    #[serde(flatten)]
    extra: serde_yaml::Mapping,
}

fn note_to_markdown(note: &Note) -> Result<String> {
    let fm = NoteFrontmatter {
        id: note.id.clone(),
        title: note.title.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        pinned: note.pinned,
        extra: note.extra.clone(),
    };
    let yaml = serde_yaml::to_string(&fm)?;
    Ok(format!("---\n{}---\n\n{}", yaml, note.body))
}

fn fnv1a(data: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn stable_id(relative_path: &Path) -> String {
    let text = relative_path.to_string_lossy().replace('\\', "/");
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&fnv1a(text.as_bytes(), 0xcbf2_9ce4_8422_2325).to_be_bytes());
    bytes[8..].copy_from_slice(&fnv1a(text.as_bytes(), 0x8422_2325_cbf2_9ce4).to_be_bytes());
    uuid::Builder::from_custom_bytes(bytes)
        .into_uuid()
        .to_string()
}

fn split_front_matter(text: &str) -> Result<(Option<serde_yaml::Mapping>, String)> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok((None, text.to_string()));
    };
    let (yaml, body) = if let Some(after) = rest.strip_prefix("---\n") {
        ("", after)
    } else if rest == "---" {
        ("", "")
    } else if let Some(end) = rest.find("\n---\n") {
        (&rest[..=end], &rest[end + 5..])
    } else if let Some(yaml) = rest.strip_suffix("---").filter(|y| y.ends_with('\n')) {
        (yaml, "")
    } else {
        return Ok((None, text.to_string()));
    };
    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml).context("failed to parse frontmatter")?;
    let props = match value {
        serde_yaml::Value::Mapping(map) => map,
        serde_yaml::Value::Null => serde_yaml::Mapping::new(),
        _ => return Ok((None, text.to_string())),
    };
    Ok((Some(props), body.trim_start_matches('\n').to_string()))
}

fn date_from(value: serde_yaml::Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .map(|d| d.and_time(chrono::NaiveTime::MIN).and_utc())
        })
}

fn text_from(value: Option<serde_yaml::Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
}

fn normalised(text: &str) -> std::borrow::Cow<'_, str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains("\r\n") {
        std::borrow::Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

fn head_of(props: &mut serde_yaml::Mapping, relative_path: &Path) -> Head {
    head_and_renamed(props, relative_path).0
}

fn head_and_renamed(props: &mut serde_yaml::Mapping, relative_path: &Path) -> (Head, bool) {
    let directory = relative_path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(part) => part.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let stem = relative_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note")
        .to_string();

    let id = match props.shift_remove("id") {
        Some(serde_yaml::Value::Number(n)) => Some(n.to_string()),
        other => text_from(other),
    }
    .unwrap_or_else(|| stable_id(relative_path));
    let header_title = text_from(props.shift_remove("title")).unwrap_or_else(|| stem.clone());
    let named =
        uuid::Uuid::parse_str(&stem).is_ok() || filename::matches_title(&stem, &header_title);
    let title = if named { header_title } else { stem.clone() };
    (
        Head {
            id,
            title,
            directory,
            created_at: props.get("created_at").cloned().and_then(date_from),
        },
        !named,
    )
}

fn marker_end(bytes: &[u8]) -> Option<usize> {
    (0..bytes.len()).find_map(|at| {
        let rest = &bytes[at..];
        if rest.starts_with(b"\n---\n") {
            Some(at + 5)
        } else if rest.starts_with(b"\n---\r\n") {
            Some(at + 6)
        } else {
            None
        }
    })
}

fn read_more(file: &mut fs::File, bytes: &mut Vec<u8>) -> Option<bool> {
    use std::io::Read;
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match file.read(&mut chunk) {
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                return Some(n > 0);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

fn read_head(path: &Path, relative: &Path) -> Option<Head> {
    let mut file = crate::paths::open_regular(path).ok()?;
    let mut bytes = Vec::new();
    while bytes.len() < 8 && read_more(&mut file, &mut bytes)? {}
    let start = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let first = if bytes[start..].starts_with(b"---\n") {
        start + 3
    } else if bytes[start..].starts_with(b"---\r\n") {
        start + 4
    } else {
        return Some(head_of(&mut serde_yaml::Mapping::new(), relative));
    };
    let mut from = first;
    loop {
        if let Some(end) = marker_end(&bytes[from..]) {
            bytes.truncate(from + end);
            break;
        }
        from = bytes.len().saturating_sub(5).max(first);
        if !read_more(&mut file, &mut bytes)? {
            break;
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    let (front, _) = split_front_matter(&normalised(&text)).ok()?;
    Some(head_of(&mut front.unwrap_or_default(), relative))
}

fn parse_note_from_markdown(
    content: &str,
    relative_path: &Path,
    modified: DateTime<Utc>,
) -> Result<Note> {
    parse_note(content, relative_path, modified).map(|(note, _)| note)
}

fn parse_note(
    content: &str,
    relative_path: &Path,
    modified: DateTime<Utc>,
) -> Result<(Note, bool)> {
    let text = normalised(content);
    let (front, body) = split_front_matter(&text)?;
    let mut props = front.unwrap_or_default();
    let (
        Head {
            id,
            title,
            directory,
            ..
        },
        renamed,
    ) = head_and_renamed(&mut props, relative_path);
    let created_at = props
        .shift_remove("created_at")
        .and_then(date_from)
        .unwrap_or(modified);
    let updated_at = props
        .shift_remove("updated_at")
        .and_then(date_from)
        .unwrap_or(modified);
    let pinned = props
        .shift_remove("pinned")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok((
        Note {
            id,
            title,
            body,
            directory,
            created_at,
            updated_at,
            pinned,
            extra: props,
        },
        renamed,
    ))
}

pub const CONFLICT_SUFFIX: &str = " (conflict from leo)";

fn conflict_notice(title: &str) -> String {
    format!(
        "\"{title}\" was changed by another program while leo had it open, so leo kept your edit as \"{title}{CONFLICT_SUFFIX}\""
    )
}

#[derive(Debug, Clone)]
struct Seen {
    path: PathBuf,
    disk: u64,
    digest: crate::paths::FileDigest,
    model: u64,
    fields: u64,
    place: u64,
    copy: Option<(PathBuf, u64)>,
    stamp: Option<Stamp>,
    renamed: bool,
    pictures: Vec<String>,
    device: Option<u64>,
    created_at: DateTime<Utc>,
}

type Stamp = (u64, u128, i64);

fn stamp_of(meta: &fs::Metadata) -> Stamp {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        meta.ctime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = 0;
    (meta.len(), nanos, changed)
}

const TIDY_TRASH_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

fn tidy_due(tidied: Option<std::time::Instant>) -> bool {
    tidied.is_none_or(|at| at.elapsed() >= TIDY_TRASH_EVERY)
}

fn hash_note(note: &Note) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        &note.id,
        &note.title,
        &note.body,
        note.created_at,
        note.updated_at,
        note.pinned,
        serde_yaml::to_string(&note.extra).unwrap_or_default(),
    )
        .hash(&mut hasher);
    hasher.finish()
}

fn place_of(note: &Note) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (&note.title, &note.directory, note.created_at).hash(&mut hasher);
    hasher.finish()
}

fn hash_str(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn is_clean(path: &Path, expected: u64) -> bool {
    fs::read_to_string(path).is_ok_and(|text| hash_str(&text) == expected)
}

const FOLDER_LIST: &str = "directories.json";

fn stamp_term(relative: &Path, meta: Option<&fs::Metadata>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in relative.components() {
        if let std::path::Component::Normal(part) = part {
            part.to_string_lossy().hash(&mut hasher);
        }
    }
    let (len, nanos) = meta.map_or((0, 0), |m| {
        let nanos = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        (m.len(), nanos)
    });
    (len, nanos).hash(&mut hasher);
    hasher.finish()
}

fn fingerprint_of(notes_dir: &Path) -> u64 {
    let mut files = Vec::new();
    let _ = walk(notes_dir, 0, false, &mut files, &mut Vec::new());
    fingerprint_from(notes_dir, &files)
}

fn fingerprint_from(notes_dir: &Path, files: &[(PathBuf, fs::Metadata)]) -> u64 {
    let listed = fs::metadata(notes_dir.join(FOLDER_LIST))
        .ok()
        .map_or(0, |meta| stamp_term(Path::new(FOLDER_LIST), Some(&meta)));
    files.iter().fold(listed, |sum, (path, meta)| {
        let relative = path.strip_prefix(notes_dir).unwrap_or(path);
        sum.wrapping_add(stamp_term(relative, Some(meta)))
    })
}

fn path_key(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn unique_path(wanted: &Path) -> PathBuf {
    if !wanted.exists() {
        return wanted.to_path_buf();
    }
    let stem = wanted
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note");
    let ext = wanted.extension().and_then(|e| e.to_str()).unwrap_or("md");
    (2..)
        .map(|n| wanted.with_file_name(format!("{}.{ext}", filename::numbered(stem, n))))
        .find(|candidate| !candidate.exists())
        .expect("an unused name exists")
}

// ── Filesystem helpers ──────────────────────────────────────────────────────

/// notes directory: ~/Library/Application Support/leo/notes  (macOS)
fn notes_dir_path() -> Result<PathBuf> {
    crate::paths::contained_path(&crate::paths::data_dir()?, Path::new("notes"))
}

fn load_directories(notes_dir: &Path) -> Result<(Vec<String>, u64)> {
    let path = crate::paths::contained_path(notes_dir, Path::new(FOLDER_LIST))?;
    if !path.exists() {
        return Ok((Vec::new(), hash_str("")));
    }
    let raw = fs::read(&path)?;
    match serde_json::from_slice(&raw) {
        Ok(list) => Ok((list, hash_str(&String::from_utf8_lossy(&raw)))),
        Err(e) => {
            let kept = path.with_extension("json.corrupt");
            match fs::rename(&path, &kept) {
                Ok(()) => {
                    let _ = fs::File::options()
                        .write(true)
                        .open(&kept)
                        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
                    crate::diag::warn(format!(
                        "skipping {}: {e}; kept it as {}",
                        path.display(),
                        kept.display()
                    ))
                }
                Err(moved) => crate::diag::warn(format!(
                    "skipping {}: {e}; could not keep it: {moved}",
                    path.display()
                )),
            }
            Ok((Vec::new(), hash_str("")))
        }
    }
}

fn merged_folders(disk: Vec<String>, current: &[String], base: &[String]) -> Vec<String> {
    let current_set: HashSet<&str> = current.iter().map(String::as_str).collect();
    let base_set: HashSet<&str> = base.iter().map(String::as_str).collect();
    let mut out: Vec<String> = disk
        .into_iter()
        .filter(|dir| !base_set.contains(dir.as_str()) || current_set.contains(dir.as_str()))
        .collect();
    let mut have: HashSet<String> = out.iter().cloned().collect();
    for dir in current {
        if !base_set.contains(dir.as_str()) && have.insert(dir.clone()) {
            out.push(dir.clone());
        }
    }
    out
}

fn check_notes_root(notes_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(notes_dir) {
        Ok(metadata) => anyhow::ensure!(metadata.is_dir() && !metadata.file_type().is_symlink(), "Your notes folder must be a real directory; move it out of the symbolic link and try again"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

const MOST_DEPTH: usize = 64;

fn read_dir_or_empty(dir: &Path) -> std::io::Result<Option<fs::ReadDir>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn walk(
    dir: &Path,
    depth: usize,
    tidy: bool,
    files: &mut Vec<(PathBuf, fs::Metadata)>,
    errors: &mut Vec<(PathBuf, String)>,
) -> std::io::Result<()> {
    if depth > MOST_DEPTH {
        errors.push((dir.to_path_buf(), "folders nested too deep".to_string()));
        return Ok(());
    }
    let entries = match read_dir_or_empty(dir) {
        Ok(Some(entries)) => entries,
        Ok(None) => return Ok(()),
        Err(error) if depth > 0 => {
            errors.push((dir.to_path_buf(), error.to_string()));
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push((dir.to_path_buf(), error.to_string()));
                continue;
            }
        };
        let path = entry.path();
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) => {
                errors.push((path, error.to_string()));
                continue;
            }
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') {
                walk(&path, depth + 1, tidy, files, errors)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            #[cfg(windows)]
            let meta = fs::metadata(&path);
            #[cfg(not(windows))]
            let meta = entry.metadata();
            match meta {
                Ok(meta) => files.push((path, meta)),
                Err(error) => errors.push((path, error.to_string())),
            }
        } else if tidy {
            remove_if_stale(&entry);
        }
    }
    Ok(())
}

fn remove_if_stale(entry: &fs::DirEntry) {
    let name = entry.file_name();
    let Some(name) = name.to_str().filter(|name| name.starts_with(".tmp")) else {
        return;
    };
    if !entry
        .metadata()
        .is_ok_and(|meta| crate::paths::stale_temp(name, &meta))
    {
        return;
    }
    let path = entry.path();
    if let Err(error) = fs::remove_file(&path) {
        crate::diag::warn(format!(
            "could not remove the old temporary file {}: {error}",
            path.display()
        ));
    }
}

const TRASH: &str = ".trash";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    pub id: String,
    pub title: String,
    pub directory: String,
    pub deleted_at: DateTime<Utc>,
    pub readable: bool,
    pub revision: Option<crate::paths::FileDigest>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    pub titles: Vec<String>,
    pub unreadable: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub id: String,
    pub title: String,
    pub directory: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

pub struct TrashEntry {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub deleted_at: DateTime<Utc>,
    pub head: Option<Head>,
}

impl TrashEntry {
    pub fn id(&self) -> String {
        self.head
            .as_ref()
            .map_or_else(|| unreadable_id(&self.relative), |head| head.id.clone())
    }
}

fn unreadable_id(relative: &Path) -> String {
    stable_id(&Path::new(TRASH).join(relative))
}

const TRASH_INDEX: &str = "trash.json";
const TRASH_INDEX_VERSION: u32 = 2;
const MOST_TRASH_NOTE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Indexed {
    stamp: Stamp,
    #[serde(default)]
    digest: Option<crate::paths::FileDigest>,
    deleted_at: DateTime<Utc>,
    head: Option<Head>,
    #[serde(default)]
    pictures: Option<Vec<String>>,
    #[serde(default)]
    entered: Option<i64>,
    #[serde(default)]
    hidden: bool,
}

#[derive(Default)]
struct TrashIndex {
    files: std::collections::BTreeMap<PathBuf, Indexed>,
    changed: bool,
    incomplete: bool,
}

fn trash_index_path(notes_dir: &Path) -> Result<PathBuf> {
    keeping_state_path(notes_dir, TRASH_INDEX)
}

impl TrashIndex {
    fn load(notes_dir: &Path) -> TrashIndex {
        let read = || -> Result<Vec<(String, Indexed)>> {
            use std::io::Read;
            let mut bytes = Vec::new();
            match crate::paths::open_regular(&trash_index_path(notes_dir)?) {
                Ok(mut file) => file.read_to_end(&mut bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(e.into()),
            };
            let (version, files): (u32, Vec<(String, Indexed)>) = serde_json::from_slice(&bytes)?;
            Ok(if version == TRASH_INDEX_VERSION {
                files
            } else {
                Vec::new()
            })
        };
        let files = read().unwrap_or_else(|e| {
            crate::diag::warn(format!(
                "leo could not read its index of the trash, so it reads every trashed note again: {e:#}"
            ));
            Vec::new()
        });
        TrashIndex {
            files: files
                .into_iter()
                .map(|(relative, entry)| (relative.split('/').collect(), entry))
                .collect(),
            changed: false,
            incomplete: true,
        }
    }

    fn persist(&self, notes_dir: &Path) {
        let files: Vec<(String, &Indexed)> = self
            .files
            .iter()
            .filter_map(|(relative, entry)| {
                let parts = relative
                    .components()
                    .map(|part| match part {
                        std::path::Component::Normal(name) => name.to_str(),
                        _ => None,
                    })
                    .collect::<Option<Vec<&str>>>()?;
                Some((parts.join("/"), entry))
            })
            .collect();
        let write = || -> Result<()> {
            let path = trash_index_path(notes_dir)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomic(&path, serde_json::to_vec(&(TRASH_INDEX_VERSION, files))?)
        };
        if let Err(e) = write() {
            crate::diag::warn(format!("leo could not keep its index of the trash: {e:#}"));
        }
    }

    fn check(&mut self, notes_dir: &Path, clock: i64) {
        let mut files = Vec::new();
        let mut errors = Vec::new();
        let Some(trash) = trash_dir(notes_dir) else {
            self.incomplete = true;
            return;
        };
        if walk(&trash, 0, false, &mut files, &mut errors).is_err() {
            self.incomplete = true;
            return;
        }
        self.incomplete = !errors.is_empty();
        let mut known = std::mem::take(&mut self.files);
        for (path, meta) in files {
            let (Ok(modified), Ok(relative)) = (meta.modified(), path.strip_prefix(&trash)) else {
                self.incomplete = true;
                continue;
            };
            let stamp = stamp_of(&meta);
            let mut entry = match known.remove(relative) {
                Some(entry) if entry.stamp == stamp => entry,
                _ => {
                    self.changed = true;
                    Indexed {
                        stamp,
                        digest: crate::paths::digest_file(&path, MOST_TRASH_NOTE_BYTES).ok(),
                        deleted_at: modified.into(),
                        head: read_head(&path, relative),
                        pictures: None,
                        entered: Some(clock),
                        hidden: false,
                    }
                }
            };
            if entry.entered.is_none_or(|entered| entered > clock) {
                entry.entered = Some(clock);
                self.changed = true;
            }
            self.files.insert(relative.to_path_buf(), entry);
        }
        if self.incomplete {
            self.files.extend(known);
        } else {
            self.changed |= !known.is_empty();
        }
    }

    fn entries(&self, notes_dir: &Path) -> Vec<TrashEntry> {
        let Some(trash) = trash_dir(notes_dir) else {
            return Vec::new();
        };
        self.files
            .iter()
            .map(|(relative, entry)| TrashEntry {
                path: trash.join(relative),
                relative: relative.clone(),
                deleted_at: entry.deleted_at,
                head: entry.head.clone(),
            })
            .collect()
    }
}

const PICTURES: &str = "pictures.json";

fn pictures_path(notes_dir: &Path) -> Result<PathBuf> {
    keeping_state_path(notes_dir, PICTURES)
}

fn keeping_state_path(notes_dir: &Path, file: &str) -> Result<PathBuf> {
    let data = notes_dir
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = notes_dir
        .file_name()
        .context("the notes folder has no name")?;
    let library = crate::paths::digest_text(&crate::paths::digest(name.as_encoded_bytes()));
    crate::paths::contained_path(
        data,
        &Path::new(crate::paths::own::KEEPING_STATE)
            .join(library)
            .join(file),
    )
}

#[derive(Clone, Serialize, Deserialize)]
struct Grace {
    at: DateTime<Utc>,
    #[serde(default)]
    entered: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<(u64, u128)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    digest: Option<crate::paths::FileDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    returning: Option<String>,
}

impl Grace {
    fn from(at: DateTime<Utc>, entered: i64) -> Grace {
        Grace {
            at,
            entered,
            file: None,
            digest: None,
            returning: None,
        }
    }

    fn now() -> Grace {
        Grace::from(DateTime::<Utc>::UNIX_EPOCH, 0)
    }

    fn due(&self, clock: i64, period: Option<chrono::Duration>) -> bool {
        self.at == DateTime::<Utc>::UNIX_EPOCH
            || period
                .is_some_and(|period| clock.saturating_sub(self.entered) >= period.num_seconds())
    }
}

#[derive(Default, Clone, Copy, Serialize, Deserialize)]
struct Clock {
    secs: i64,
    last: Option<DateTime<Utc>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Left {
    path: String,
    disk: u64,
    twin: bool,
    #[serde(default)]
    device: Option<u64>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    digest: Option<crate::paths::FileDigest>,
}

fn device_of(path: &Path) -> Option<u64> {
    let stamp = crate::paths::stamp_at(path).ok()?;
    (stamp.0 && stamp.4 != 0).then_some(stamp.4)
}

fn folder_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

#[derive(Default, Serialize, Deserialize)]
struct Released {
    #[serde(default)]
    clock: Clock,
    #[serde(default)]
    binned: BTreeMap<String, Grace>,
    #[serde(default)]
    unused_files: BTreeMap<String, crate::attachments::SeenPicture>,
    #[serde(default)]
    gone: BTreeMap<String, Left>,
    #[serde(default)]
    expired: BTreeMap<String, Option<DateTime<Utc>>>,
    #[serde(default)]
    retired: BTreeMap<String, (DateTime<Utc>, crate::paths::FileDigest)>,
    #[serde(default)]
    reused: BTreeSet<String>,
    #[serde(default)]
    released: BTreeMap<String, Grace>,
    #[serde(default)]
    kept: BTreeSet<String>,
}

impl Released {
    fn load(notes_dir: &Path) -> Released {
        let read = || -> Result<Released> {
            use std::io::Read;
            let mut bytes = Vec::new();
            match crate::paths::open_regular(&pictures_path(notes_dir)?) {
                Ok(mut file) => file.read_to_end(&mut bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Released::default())
                }
                Err(e) => return Err(e.into()),
            };
            Ok(serde_json::from_slice(&bytes)?)
        };
        read().unwrap_or_else(|e| {
            crate::diag::warn(format!(
                "leo could not read which pictures no note links, so it starts their count again: {e:#}"
            ));
            Released::default()
        })
    }

    fn persist(&self, notes_dir: &Path) -> bool {
        let write = || -> Result<()> {
            let path = pictures_path(notes_dir)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomic(&path, serde_json::to_vec(self)?)
        };
        if let Err(e) = write() {
            crate::diag::warn(format!(
                "leo could not keep which pictures no note links: {e:#}"
            ));
            return false;
        }
        true
    }
}

#[derive(Default)]
struct Pictures {
    live: HashMap<String, usize>,
    saved: Released,
    changed: bool,
}

impl Pictures {
    fn counted(mut saved: Released, seen: &HashMap<String, Seen>) -> Pictures {
        let mut live: HashMap<String, usize> = HashMap::new();
        for name in seen.values().flat_map(|entry| &entry.pictures) {
            *live.entry(name.clone()).or_default() += 1;
        }
        let before = (saved.released.len(), saved.kept.len());
        saved.released.retain(|name, _| !live.contains_key(name));
        saved.kept.retain(|name| !live.contains_key(name));
        let changed = before != (saved.released.len(), saved.kept.len());
        Pictures {
            live,
            saved,
            changed,
        }
    }

    fn linked(&mut self, name: &str) {
        let count = self.live.entry(name.to_string()).or_default();
        *count += 1;
        if *count == 1 {
            let released = self.saved.released.remove(name).is_some();
            let kept = self.saved.kept.remove(name);
            self.changed |= released || kept;
        }
    }

    fn unlinked(&mut self, name: &str, release: Option<DateTime<Utc>>) {
        let Some(count) = self.live.get_mut(name) else {
            return;
        };
        *count -= 1;
        if *count > 0 {
            return;
        }
        self.live.remove(name);
        if let Some(at) = release.filter(|_| !self.saved.kept.contains(name)) {
            self.saved
                .released
                .entry(name.to_string())
                .or_insert(Grace::from(at, self.saved.clock.secs));
            self.changed = true;
        }
    }

    fn deleted(&mut self, names: &[String], asked: bool) {
        let grace = if asked {
            Grace::now()
        } else {
            Grace::from(Utc::now(), i64::MIN / 4)
        };
        for name in names {
            if !self.live.contains_key(name) && !self.saved.kept.contains(name) {
                self.saved.released.insert(name.clone(), grace.clone());
                self.changed = true;
            }
        }
    }
}

fn departures(
    gone: &mut BTreeMap<String, Left>,
    leftover: HashMap<String, Seen>,
    seen: &HashMap<String, Seen>,
) -> bool {
    let before = (gone.len(), gone.values().filter(|left| left.twin).count());
    for (id, entry) in leftover {
        if seen.contains_key(&id) {
            continue;
        }
        let parts: Option<Vec<&str>> = entry.path.iter().map(|part| part.to_str()).collect();
        if let Some(parts) = parts {
            gone.entry(id.clone()).or_insert(Left {
                path: parts.join("/"),
                disk: entry.disk,
                twin: id == stable_id(&entry.path),
                device: entry.device,
                created_at: Some(entry.created_at),
                digest: Some(entry.digest),
            });
        }
    }
    gone.retain(|id, _| !seen.contains_key(id));
    if gone.values().any(|left| !left.twin) {
        let disks: HashSet<u64> = seen.values().map(|entry| entry.disk).collect();
        for left in gone.values_mut() {
            left.twin |= disks.contains(&left.disk);
        }
    }
    before != (gone.len(), gone.values().filter(|left| left.twin).count())
}

fn read_regular_text(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    crate::paths::open_regular(path)?.read_to_string(&mut text)?;
    Ok(text)
}

fn pictures_read(path: &Path) -> Option<Vec<String>> {
    Some(crate::attachments::mentioned(
        &read_regular_text(path).ok()?,
    ))
}

pub struct Owed {
    pub path: PathBuf,
    relative: PathBuf,
    stamp: Stamp,
}

impl Owed {
    pub fn read(&self) -> Option<Vec<String>> {
        pictures_read(&self.path)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Emptied {
    pub deleted: usize,
    pub failed: usize,
    pub forgotten: Vec<String>,
    pub changed: bool,
}

fn trash_dir(notes_dir: &Path) -> Option<PathBuf> {
    crate::paths::contained_path(notes_dir, Path::new(TRASH)).ok()
}

pub fn trash_entries(notes_dir: &Path) -> Vec<TrashEntry> {
    let Some(trash) = trash_dir(notes_dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    if walk(&trash, 0, false, &mut files, &mut Vec::new()).is_err() {
        return Vec::new();
    }
    files
        .into_iter()
        .filter_map(|(path, meta)| {
            let deleted_at = DateTime::<Utc>::from(meta.modified().ok()?);
            let relative = path.strip_prefix(&trash).ok()?.to_path_buf();
            let head = read_head(&path, &relative);
            Some(TrashEntry {
                path,
                relative,
                deleted_at,
                head,
            })
        })
        .collect()
}

pub fn trashed_ids(notes_dir: &Path) -> HashSet<String> {
    trash_entries(notes_dir)
        .into_iter()
        .filter_map(|entry| entry.head.map(|head| head.id))
        .collect()
}

pub fn trash_days_now(notes_dir: &Path) -> Option<Option<u32>> {
    let text = match read_regular_text(&crate::keep::path_for(notes_dir)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some(Some(crate::keep::DEFAULT_TRASH_DAYS))
        }
        read => read.ok()?,
    };
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    match value.as_object()?.get("trash_days") {
        None => Some(Some(crate::keep::DEFAULT_TRASH_DAYS)),
        Some(serde_json::Value::Null) => Some(None),
        Some(days) => {
            let days = u32::try_from(days.as_u64()?).ok()?;
            crate::keep::CHOICES
                .contains(&Some(days))
                .then_some(Some(days))
        }
    }
}

pub const MOST_EMPTY_FOLDERS: usize = 10_000;

pub fn remove_emptied(top: &Path) {
    let mut found = vec![top.to_path_buf()];
    let mut i = 0;
    while i < found.len() && i < MOST_EMPTY_FOLDERS {
        if let Ok(entries) = std::fs::read_dir(&found[i]) {
            for entry in entries.take(MOST_EMPTY_FOLDERS).flatten() {
                if found.len() >= MOST_EMPTY_FOLDERS {
                    break;
                }
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    found.push(entry.path());
                }
            }
        }
        i += 1;
    }
    for dir in found.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

fn stamp_deleted(path: &Path) -> std::io::Result<()> {
    let stamp = || {
        fs::File::options()
            .write(true)
            .open(path)?
            .set_modified(std::time::SystemTime::now())
    };
    let Err(error) = stamp() else {
        return Ok(());
    };
    let original = fs::metadata(path)?.permissions();
    let mut writable = original.clone();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        writable.set_mode(original.mode() | 0o200);
    }
    #[cfg(not(unix))]
    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);
    if writable == original {
        return Err(error);
    }
    fs::set_permissions(path, writable)?;
    stamp().and(fs::set_permissions(path, original))
}

fn add_with_parents(directories: &mut Vec<String>, dir: &str) {
    let parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for i in 0..parts.len() {
        let dir = parts[..=i].join("/");
        if !directories.contains(&dir) {
            directories.push(dir);
        }
    }
}

struct Found {
    relative: PathBuf,
    note: Note,
    modified: std::time::SystemTime,
    renamed: bool,
    disk: u64,
    digest: crate::paths::FileDigest,
    stamp: Stamp,
    kept: Option<Seen>,
    pictures: Vec<String>,
}

fn skip(unreadable: &mut Vec<(PathBuf, String)>, path: &Path, error: &dyn std::fmt::Display) {
    crate::diag::warn(format!("skipping {}: {error}", path.display()));
    unreadable.push((path.to_path_buf(), error.to_string()));
}

fn read_found(
    path: &Path,
    relative: PathBuf,
    meta: &fs::Metadata,
    unreadable: &mut Vec<(PathBuf, String)>,
) -> Option<Found> {
    let content = match read_regular_text(path) {
        Ok(content) => content,
        Err(error) => {
            skip(unreadable, path, &error);
            return None;
        }
    };
    let modified = meta.modified().ok();
    let changed = modified.map_or_else(Utc::now, DateTime::<Utc>::from);
    match parse_note(&content, &relative, changed) {
        Ok((note, renamed)) => Some(Found {
            pictures: crate::attachments::mentioned(&content),
            disk: hash_str(&content),
            digest: crate::paths::digest(content.as_bytes()),
            relative,
            note,
            modified: modified.unwrap_or(std::time::UNIX_EPOCH),
            renamed,
            stamp: stamp_of(meta),
            kept: None,
        }),
        Err(e) => {
            skip(unreadable, path, &e);
            None
        }
    }
}

fn index_notes(
    mut found: Vec<Found>,
    mut feed: Option<&mut Feed>,
) -> (Vec<Note>, HashMap<String, Seen>) {
    let mut once = HashSet::new();
    let twins: HashSet<String> = found
        .iter()
        .filter(|item| !once.insert(item.note.id.as_str()))
        .map(|item| item.note.id.clone())
        .collect();
    drop(once);
    let last = |item: &Found| item.renamed && twins.contains(&item.note.id);
    found.sort_by(|a, b| {
        (last(a), a.modified, &a.relative).cmp(&(last(b), b.modified, &b.relative))
    });
    let mut notes = Vec::with_capacity(found.len());
    let mut seen = HashMap::with_capacity(found.len());
    for Found {
        relative,
        mut note,
        renamed,
        disk,
        digest,
        stamp,
        kept,
        pictures,
        ..
    } in found
    {
        let reassigned = seen.contains_key(&note.id);
        if reassigned {
            note.id = stable_id(&relative);
            if seen.contains_key(&note.id) {
                note.id = uuid::Uuid::new_v4().to_string();
            }
        }
        if let Some(feed) = feed.as_deref_mut().filter(|_| reassigned || kept.is_none()) {
            feed.record(note.id.clone());
        }
        let pictures = kept.as_ref().map_or(pictures, |kept| kept.pictures.clone());
        let (model, fields) = match &kept {
            _ if reassigned => (0, 0),
            Some(kept) => (kept.model, kept.fields),
            None => (
                note_to_markdown(&note)
                    .map(|text| hash_str(&text))
                    .unwrap_or(0),
                hash_note(&note),
            ),
        };
        seen.insert(
            note.id.clone(),
            Seen {
                path: relative,
                disk,
                digest,
                model,
                fields,
                place: kept.map_or_else(|| place_of(&note), |kept| kept.place),
                copy: None,
                stamp: Some(stamp),
                renamed,
                pictures,
                device: None,
                created_at: note.created_at,
            },
        );
        notes.push(note);
    }
    (notes, seen)
}

// ── Store ───────────────────────────────────────────────────────────────────

pub const MOST_LOGGED: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    Ids(Vec<String>),
    Everything,
}

#[derive(Debug)]
struct Feed {
    generation: u64,
    base: u64,
    log: std::collections::VecDeque<(u64, String)>,
}

impl Default for Feed {
    fn default() -> Self {
        Feed {
            generation: 1,
            base: 1,
            log: Default::default(),
        }
    }
}

impl Feed {
    fn record(&mut self, id: String) {
        if self.log.len() >= MOST_LOGGED {
            if let Some((gone, _)) = self.log.pop_front() {
                self.base = gone;
            }
        }
        self.generation += 1;
        self.log.push_back((self.generation, id));
    }

    fn everything(&mut self) {
        self.generation += 1;
        self.base = self.generation;
        self.log.clear();
    }
}

pub const MOST_FOLDER_NAME_CHARS: usize = 120;
pub const MOST_FOLDER_NAME_BYTES: usize = 240;

/// What Health reports about the loaded notes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Facts {
    pub notes: usize,
    pub directories: usize,
    pub unreadable: Vec<(PathBuf, String)>,
    pub conflict_copies: Vec<String>,
}

/// Persistent store backed by per-note .md files in a directory.
pub struct Store {
    pub notes: Vec<Note>,
    pub directories: Vec<String>,
    pub notes_dir: PathBuf,
    /// Note files that could not be read, and why. They are skipped, never
    /// deleted: `save` leaves them alone.
    pub unreadable: Vec<(PathBuf, String)>,
    seen: std::cell::RefCell<HashMap<String, Seen>>,
    fingerprint: std::cell::Cell<u64>,
    tidied: std::cell::Cell<Option<std::time::Instant>>,
    dirs_seen: std::cell::Cell<u64>,
    dirs_base: std::cell::RefCell<Vec<String>>,
    dirty: std::cell::RefCell<HashSet<String>>,
    removed: std::cell::RefCell<HashSet<String>>,
    full: std::cell::Cell<bool>,
    placed: std::cell::Cell<bool>,
    feed: std::cell::RefCell<Feed>,
    trash: std::cell::RefCell<Option<TrashIndex>>,
    trash_stale: std::cell::Cell<bool>,
    pictures: std::cell::RefCell<Pictures>,
    clock_sample: std::cell::Cell<std::time::Instant>,
    batching: std::cell::Cell<bool>,
    holding: std::cell::RefCell<Option<Option<HashMap<String, usize>>>>,
}

impl Store {
    /// Load notes from the platform data directory.
    pub fn load() -> Result<Self> {
        let notes_dir = notes_dir_path()?;
        Self::load_from(&notes_dir)
    }

    /// Load notes from a specific directory. Used directly in tests.
    /// Where the notes live, without loading them.
    pub fn notes_dir() -> Result<PathBuf> {
        notes_dir_path()
    }

    pub fn load_from(notes_dir: &Path) -> Result<Self> {
        Self::load_tidied(notes_dir, None, None)
    }

    fn load_tidied(
        notes_dir: &Path,
        tidied: Option<std::time::Instant>,
        old: Option<&mut Store>,
    ) -> Result<Self> {
        check_notes_root(notes_dir)?;
        fs::create_dir_all(notes_dir)?;
        let initial = old.is_none();
        let mut old = old;
        let clock_sample = old
            .as_deref()
            .map_or_else(std::time::Instant::now, |old| old.clock_sample.get());
        let (trash, trash_stale) = old.as_deref_mut().map_or((None, true), |old| {
            (old.trash.take(), old.trash_stale.get())
        });
        let released = old.as_deref_mut().map_or_else(
            || Released::load(notes_dir),
            |old| std::mem::take(&mut old.pictures.get_mut().saved),
        );
        let mut files = Vec::new();
        let mut errors = Vec::new();
        walk(notes_dir, 0, true, &mut files, &mut errors)?;
        let fingerprint = fingerprint_from(notes_dir, &files);
        let (mut directories, dirs_seen) = load_directories(notes_dir)?;
        let dirs_base = directories.clone();
        directories.retain(|dir| crate::paths::validate_directory(dir).is_ok());
        let files = files
            .into_iter()
            .map(|(path, meta)| {
                let relative = path
                    .strip_prefix(notes_dir)
                    .context("path outside notes_dir")?
                    .to_path_buf();
                Ok((path, relative, meta))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut unreadable = Vec::new();
        for (path, why) in &errors {
            skip(&mut unreadable, path, why);
        }
        let (mut kept, mut feed, leftover) = match old {
            Some(old) => (
                old.take_unchanged(&files),
                Some(old.feed.take()),
                std::mem::take(old.seen.get_mut()),
            ),
            None => Default::default(),
        };
        let found = files
            .into_iter()
            .filter_map(|(path, relative, meta)| match kept.remove(&relative) {
                Some((note, seen)) => Some(Found {
                    relative,
                    modified: meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    renamed: seen.renamed,
                    disk: seen.disk,
                    digest: seen.digest,
                    stamp: stamp_of(&meta),
                    note,
                    kept: Some(seen),
                    pictures: Vec::new(),
                }),
                None => read_found(&path, relative, &meta, &mut unreadable),
            })
            .collect();
        let (notes, mut seen) = index_notes(found, feed.as_mut());
        let mut devices = HashMap::new();
        for entry in seen.values_mut() {
            let folder = folder_of(&entry.path);
            entry.device = *devices
                .entry(folder.clone())
                .or_insert_with(|| device_of(&notes_dir.join(folder)));
        }

        // The directory list on disk only needs to remember empty directories:
        // every note's directory, and its parents, is known from where it is.
        for note in &notes {
            add_with_parents(&mut directories, &note.directory);
        }
        let kept = tidied.filter(|at| !tidy_due(Some(*at)));
        let mut released = released;
        let departed = departures(&mut released.gone, leftover, &seen);
        let mut pictures = Pictures::counted(released, &seen);
        pictures.changed |= departed;
        let store = Store {
            pictures: std::cell::RefCell::new(pictures),
            clock_sample: std::cell::Cell::new(clock_sample),
            batching: Default::default(),
            holding: Default::default(),
            seen: std::cell::RefCell::new(seen),
            fingerprint: std::cell::Cell::new(fingerprint),
            tidied: std::cell::Cell::new(Some(kept.unwrap_or_else(std::time::Instant::now))),
            dirs_seen: std::cell::Cell::new(dirs_seen),
            dirs_base: std::cell::RefCell::new(dirs_base),
            dirty: Default::default(),
            removed: Default::default(),
            full: std::cell::Cell::new(true),
            placed: Default::default(),
            feed: std::cell::RefCell::new(feed.unwrap_or_default()),
            trash: std::cell::RefCell::new(trash),
            trash_stale: std::cell::Cell::new(trash_stale),
            unreadable,
            notes,
            directories,
            notes_dir: notes_dir.to_path_buf(),
        };
        store.observe_generations();
        if kept.is_none() {
            store.tidy_trash(&store.notes.iter().map(|n| n.id.as_str()).collect(), false);
        }
        if initial {
            store.recover_picture_rows();
        }
        store.keep_pictures();
        Ok(store)
    }

    pub fn facts(&self) -> Facts {
        Facts {
            notes: self.notes.len(),
            directories: self.directories.len(),
            unreadable: self.unreadable.clone(),
            conflict_copies: self
                .notes
                .iter()
                .filter(|n| n.title.ends_with(CONFLICT_SUFFIX))
                .map(|n| n.title.clone())
                .collect(),
        }
    }

    pub fn save(&self) -> Result<()> {
        check_notes_root(&self.notes_dir)?;
        fs::create_dir_all(&self.notes_dir)?;
        let stamp = self.tidied.get();
        let everything = self.full.get() || tidy_due(stamp);
        let placed = self.placed.replace(false);
        let mut work = self.marked(everything);
        self.check_folders(work.iter().copied())?;
        let mut seen = self.seen.borrow_mut();
        let (targets, _shortcut) = self.place_work(&seen, &mut work, everything, placed)?;
        #[cfg(debug_assertions)]
        if !everything {
            self.check_marks(&seen, &work, &targets, _shortcut);
        }
        let mut failed: Option<anyhow::Error> = None;
        let mut fp = self.fingerprint.get();
        let mut moved = false;
        let mut hashed = Vec::with_capacity(work.len());

        for &note in &work {
            let target = &targets[&note.id];
            let fields = hash_note(note);
            hashed.push((note, fields, target));
            let previous = seen.get(&note.id).cloned();
            if let Some(previous) = &previous {
                if previous.fields == fields && self.notes_dir.join(&previous.path) == *target {
                    continue;
                }
            }
            let content = note_to_markdown(note)?;
            let model = hash_str(&content);

            let mut old_path = None;
            if let Some(previous) = &previous {
                if previous.model == model && self.notes_dir.join(&previous.path) == *target {
                    if let Some(entry) = seen.get_mut(&note.id) {
                        entry.fields = fields;
                        entry.place = place_of(note);
                    }
                    continue;
                }
                let old = crate::paths::contained_path(&self.notes_dir, &previous.path)?;
                if !is_clean(&old, previous.disk) {
                    if previous.model != model {
                        moved = true;
                        let copy = self.conflict_copy(note, previous.copy.as_ref())?;
                        if let Some(entry) = seen.get_mut(&note.id) {
                            entry.model = model;
                            entry.fields = fields;
                            entry.copy = Some(copy);
                        }
                    }
                    continue;
                }
                old_path = Some(old);
            }
            if previous.is_none() {
                self.tidied.set(None);
            }
            if old_path.as_ref() != Some(target) {
                moved = true;
            }

            let relative = target
                .strip_prefix(&self.notes_dir)
                .context("path outside notes_dir")?
                .to_path_buf();
            crate::paths::contained_path(&self.notes_dir, &relative)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let same = old_path.as_ref().is_some_and(|old| same_file(old, target));
            let replaced = match (&previous, &old_path) {
                (Some(previous), Some(old)) if same => fs::metadata(old)
                    .ok()
                    .map(|m| stamp_term(&previous.path, Some(&m))),
                _ => fs::metadata(target)
                    .ok()
                    .map(|m| stamp_term(&relative, Some(&m))),
            };
            if let (true, Some(old)) = (same, &old_path) {
                if old != target {
                    fs::rename(old, target)?;
                }
            }
            write_atomic(target, &content)?;
            if let Some(term) = replaced {
                fp = fp.wrapping_sub(term);
            }
            let written = fs::metadata(target)?;
            let added = stamp_term(&relative, Some(&written));
            fp = fp.wrapping_add(added);
            if let (Some(previous), Some(old_path)) = (&previous, old_path) {
                if !same {
                    let old_term = fs::metadata(&old_path)
                        .ok()
                        .map(|m| stamp_term(&previous.path, Some(&m)));
                    if let Err(e) = fs::remove_file(&old_path) {
                        if fs::remove_file(target).is_ok() {
                            fp = fp.wrapping_sub(added);
                        }
                        let folder = previous
                            .path
                            .parent()
                            .map(|parent| parent.display().to_string())
                            .filter(|parent| !parent.is_empty())
                            .unwrap_or_else(|| "the notes folder".to_string());
                        let said =
                            format!("leo could not move \"{}\" out of {folder}: {e}", note.title);
                        crate::diag::warn(said.clone());
                        failed.get_or_insert(anyhow::anyhow!(said));
                        continue;
                    }
                    if let Some(term) = old_term {
                        fp = fp.wrapping_sub(term);
                    }
                }
            }
            let back = parse_note(&content, &relative, Utc::now()).ok();
            let renamed = back.as_ref().is_some_and(|(_, renamed)| *renamed);
            let exact = back.is_some_and(|(back, _)| {
                hash_note(&back) == fields && back.directory == note.directory
            });
            self.feed.borrow_mut().record(note.id.clone());
            let pictures = crate::attachments::mentioned(&content);
            {
                let mut counts = self.pictures.borrow_mut();
                let before = previous.as_ref().map_or(&[][..], |p| &p.pictures[..]);
                for name in pictures.iter().filter(|name| !before.contains(name)) {
                    counts.linked(name);
                }
                for name in before.iter().filter(|name| !pictures.contains(name)) {
                    counts.unlinked(name, Some(Utc::now()));
                }
            }
            seen.insert(
                note.id.clone(),
                Seen {
                    path: relative,
                    disk: model,
                    digest: crate::paths::digest(content.as_bytes()),
                    model,
                    fields,
                    place: place_of(note),
                    copy: previous.and_then(|previous| previous.copy),
                    stamp: exact.then(|| stamp_of(&written)),
                    renamed,
                    pictures,
                    device: target.parent().and_then(device_of),
                    created_at: note.created_at,
                },
            );
        }
        let unsettled: HashSet<String> = hashed
            .into_iter()
            .filter(|(note, fields, target)| {
                !seen.get(&note.id).is_some_and(|s| {
                    s.fields == *fields
                        && s.place == place_of(note)
                        && self.notes_dir.join(&s.path) == **target
                })
            })
            .map(|(note, _, _)| note.id.clone())
            .collect();
        *self.dirty.borrow_mut() = unsettled;

        let removed = self.removed.borrow();
        let live: HashSet<&str> = if self.live_needed(everything) {
            self.notes.iter().map(|n| n.id.as_str()).collect()
        } else {
            HashSet::new()
        };
        let gone: Vec<String> = if everything {
            seen.keys()
                .filter(|id| !live.contains(id.as_str()))
                .cloned()
                .collect()
        } else {
            removed
                .iter()
                .filter(|id| seen.contains_key(*id) && !live.contains(id.as_str()))
                .cloned()
                .collect()
        };
        #[cfg(debug_assertions)]
        {
            let all: HashSet<&String> = seen
                .keys()
                .filter(|id| !live.contains(id.as_str()))
                .collect();
            assert_eq!(
                gone.iter().collect::<HashSet<_>>(),
                all,
                "a note left the store without passing a marking point"
            );
        }
        drop(removed);
        moved |= !gone.is_empty();
        for id in gone {
            self.feed.borrow_mut().record(id.clone());
            let Some(entry) = seen.get(&id) else {
                continue;
            };
            let path = crate::paths::contained_path(&self.notes_dir, &entry.path)?;
            if is_clean(&path, entry.disk) {
                let term = fs::metadata(&path)
                    .ok()
                    .map(|m| stamp_term(&entry.path, Some(&m)));
                self.move_to_trash(&path, &id)?;
                if let Some(term) = term {
                    fp = fp.wrapping_sub(term);
                }
            }
            if let Some(entry) = seen.remove(&id) {
                let mut counts = self.pictures.borrow_mut();
                for name in &entry.pictures {
                    counts.unlinked(name, None);
                }
            }
        }
        self.removed.borrow_mut().clear();
        drop(seen);
        if tidy_due(self.tidied.get()) {
            self.tidy_trash(&live, false);
            self.tidied.set(if everything {
                Some(std::time::Instant::now())
            } else {
                stamp
            });
        }

        self.keep_pictures();
        self.save_directories(&mut fp)?;
        self.fingerprint.set(fp);
        self.full.set(false);
        self.placed.set(!moved && failed.is_none());
        if let Some(e) = failed {
            return Err(e);
        }
        Ok(())
    }

    fn check_folders<'a>(&'a self, notes: impl Iterator<Item = &'a Note>) -> Result<()> {
        let mut checked = HashSet::new();
        for dir in notes
            .map(|n| n.directory.as_str())
            .chain(self.directories.iter().map(String::as_str))
        {
            if checked.insert(dir) {
                self.validate_directory(dir)?;
            }
        }
        Ok(())
    }

    fn marked(&self, everything: bool) -> Vec<&Note> {
        if everything {
            return self.notes.iter().collect();
        }
        let dirty = self.dirty.borrow();
        if dirty.is_empty() {
            return Vec::new();
        }
        self.notes
            .iter()
            .filter(|n| dirty.contains(&n.id))
            .collect()
    }

    fn place_work<'a>(
        &'a self,
        seen: &HashMap<String, Seen>,
        work: &mut Vec<&'a Note>,
        everything: bool,
        placed: bool,
    ) -> Result<(HashMap<String, PathBuf>, bool)> {
        let shortcut = !everything
            && placed
            && self.removed.borrow().is_empty()
            && work
                .iter()
                .all(|n| seen.get(&n.id).is_some_and(|s| s.place == place_of(n)));
        if shortcut {
            let targets = work
                .iter()
                .map(|n| (n.id.clone(), self.notes_dir.join(&seen[&n.id].path)))
                .collect();
            return Ok((targets, true));
        }
        let targets = self.assign_paths(seen);
        if !everything {
            let marked: HashSet<&str> = work.iter().map(|n| n.id.as_str()).collect();
            let shifted: Vec<&Note> = self
                .notes
                .iter()
                .filter(|n| {
                    !marked.contains(n.id.as_str())
                        && seen
                            .get(&n.id)
                            .is_none_or(|s| self.notes_dir.join(&s.path) != targets[&n.id])
                })
                .collect();
            self.check_folders(shifted.iter().copied())?;
            work.extend(shifted);
        }
        Ok((targets, false))
    }

    fn live_needed(&self, everything: bool) -> bool {
        everything
            || cfg!(debug_assertions)
            || !self.removed.borrow().is_empty()
            || tidy_due(self.tidied.get())
    }

    #[cfg(debug_assertions)]
    fn check_marks(
        &self,
        seen: &HashMap<String, Seen>,
        work: &[&Note],
        targets: &HashMap<String, PathBuf>,
        shortcut: bool,
    ) {
        let marked: HashSet<&str> = work.iter().map(|n| n.id.as_str()).collect();
        for note in &self.notes {
            if marked.contains(note.id.as_str()) {
                continue;
            }
            assert!(
                seen.get(&note.id)
                    .is_some_and(|s| s.fields == hash_note(note) && s.place == place_of(note)),
                "\"{}\" changed without passing a marking point",
                note.title
            );
        }
        if shortcut {
            let full = self.assign_paths(seen);
            for note in &self.notes {
                let used = targets
                    .get(&note.id)
                    .cloned()
                    .or_else(|| seen.get(&note.id).map(|s| self.notes_dir.join(&s.path)));
                assert_eq!(
                    used.as_ref(),
                    full.get(&note.id),
                    "\"{}\" was placed differently from a full name assignment",
                    note.title
                );
            }
        }
    }

    #[doc(hidden)]
    pub fn time_save_parts(&self) -> [std::time::Duration; 4] {
        let seen = self.seen.borrow();
        let everything = self.full.get() || tidy_due(self.tidied.get());
        let at = std::time::Instant::now();
        let mut work = self.marked(everything);
        let _ = std::hint::black_box(self.check_folders(work.iter().copied()));
        let folders = at.elapsed();
        let at = std::time::Instant::now();
        let _ =
            std::hint::black_box(self.place_work(&seen, &mut work, everything, self.placed.get()));
        let paths = at.elapsed();
        let at = std::time::Instant::now();
        let live: HashSet<&str> = if self.live_needed(everything) {
            self.notes.iter().map(|n| n.id.as_str()).collect()
        } else {
            HashSet::new()
        };
        std::hint::black_box(&live);
        let ids = at.elapsed();
        let at = std::time::Instant::now();
        for note in &work {
            std::hint::black_box(hash_note(note));
        }
        [folders, paths, ids, at.elapsed()]
    }

    pub fn touch_all(&mut self) {
        self.full.set(true);
        self.feed.get_mut().everything();
        self.dirty
            .get_mut()
            .extend(self.notes.iter().map(|n| n.id.clone()));
    }

    fn save_directories(&self, fp: &mut u64) -> Result<()> {
        let relative = Path::new(FOLDER_LIST);
        let path = crate::paths::contained_path(&self.notes_dir, relative)?;
        let disk = fs::read_to_string(&path).ok();
        let disk_text = disk.as_deref().unwrap_or("");
        let unread = hash_str(disk_text) != self.dirs_seen.get();
        let list = if unread {
            let base = self.dirs_base.borrow();
            let on_disk = serde_json::from_str(disk_text).unwrap_or_else(|_| base.clone());
            merged_folders(on_disk, &self.directories, &base)
        } else {
            self.directories.clone()
        };
        let text = serde_json::to_string_pretty(&list)?;
        if disk.as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        let before = fs::metadata(&path).ok();
        write_atomic(&path, &text)?;
        if !unread {
            if let Some(m) = before {
                *fp = fp.wrapping_sub(stamp_term(relative, Some(&m)));
            }
            *fp = fp.wrapping_add(stamp_term(relative, Some(&fs::metadata(&path)?)));
            self.dirs_seen.set(hash_str(&text));
            *self.dirs_base.borrow_mut() = list;
        }
        Ok(())
    }

    pub fn changed_on_disk(&self) -> bool {
        fingerprint_of(&self.notes_dir) != self.fingerprint.get()
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint.get()
    }

    pub fn changes_since(&self, seen: u64) -> (u64, Changed) {
        let feed = self.feed.borrow();
        if seen < feed.base || seen > feed.generation {
            return (feed.generation, Changed::Everything);
        }
        let from = feed.log.partition_point(|(at, _)| *at <= seen);
        let mut once = HashSet::new();
        let ids = feed
            .log
            .range(from..)
            .filter(|(_, id)| once.insert(id.as_str()))
            .map(|(_, id)| id.clone())
            .collect();
        (feed.generation, Changed::Ids(ids))
    }

    pub fn forget_changes(&self) {
        self.feed.borrow_mut().everything();
    }

    pub fn refresh(&mut self) -> Result<()> {
        let notes_dir = self.notes_dir.clone();
        let tidied = self.tidied.get();
        *self = Store::load_tidied(&notes_dir, tidied, Some(self))?;
        Ok(())
    }

    fn take_unchanged(
        &mut self,
        files: &[(PathBuf, PathBuf, fs::Metadata)],
    ) -> HashMap<PathBuf, (Note, Seen)> {
        let stamps: HashMap<&Path, Stamp> = files
            .iter()
            .map(|(_, relative, meta)| (relative.as_path(), stamp_of(meta)))
            .collect();
        let seen = self.seen.get_mut();
        let dirty = self.dirty.get_mut();
        let feed = self.feed.get_mut();
        let unchanged: Vec<bool> = self
            .notes
            .iter()
            .map(|note| {
                let Some(entry) = seen.get(&note.id) else {
                    return false;
                };
                let same = !dirty.contains(&note.id)
                    && !(entry.model == 0 && entry.fields == 0)
                    && entry.stamp.is_some()
                    && stamps.get(entry.path.as_path()) == entry.stamp.as_ref();
                #[cfg(debug_assertions)]
                if same {
                    assert!(
                        entry.fields == hash_note(note) && entry.place == place_of(note),
                        "\"{}\" changed without passing a marking point",
                        note.title
                    );
                }
                same
            })
            .collect();
        let mut taken = HashMap::with_capacity(self.notes.len());
        for (note, same) in std::mem::take(&mut self.notes).into_iter().zip(unchanged) {
            if !same {
                feed.record(note.id);
                continue;
            }
            if let Some(entry) = seen.remove(&note.id) {
                taken.insert(entry.path.clone(), (note, entry));
            }
        }
        taken
    }

    fn conflict_copy(&self, note: &Note, again: Option<&(PathBuf, u64)>) -> Result<(PathBuf, u64)> {
        self.validate_directory(&note.directory)?;
        let mut copy = note.clone();
        copy.title = format!("{}{CONFLICT_SUFFIX}", note.title);
        copy.pinned = false;
        let earlier = again.and_then(|(relative, hash)| {
            let path = crate::paths::contained_path(&self.notes_dir, relative).ok()?;
            if !is_clean(&path, *hash) {
                return None;
            }
            let text = fs::read_to_string(&path).ok()?;
            let id = parse_note_from_markdown(&text, relative, Utc::now())
                .ok()?
                .id;
            Some((path, id))
        });
        let fresh = earlier.is_none();
        let path = match earlier {
            Some((path, id)) => {
                copy.id = id;
                path
            }
            None => {
                copy.id = uuid::Uuid::new_v4().to_string();
                let dir = if copy.directory.is_empty() {
                    self.notes_dir.clone()
                } else {
                    self.notes_dir.join(&copy.directory)
                };
                unique_path(&dir.join(format!("{}.md", filename::file_name(&copy.title))))
            }
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = note_to_markdown(&copy)?;
        write_atomic(&path, &text)?;
        self.feed.borrow_mut().record(copy.id);
        if fresh {
            crate::diag::warn(conflict_notice(&note.title));
        }
        let relative = path
            .strip_prefix(&self.notes_dir)
            .context("path outside notes_dir")?
            .to_path_buf();
        Ok((relative, hash_str(&text)))
    }

    fn move_to_trash(&self, path: &Path, id: &str) -> Result<()> {
        let relative = path
            .strip_prefix(&self.notes_dir)
            .context("path outside notes_dir")?;
        let dest = unique_path(&crate::paths::contained_path(
            &self.notes_dir,
            &Path::new(TRASH).join(relative),
        )?);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let stamp = crate::paths::stamp_at(path)?;
        let text = read_regular_text(path)?;
        anyhow::ensure!(
            crate::paths::same_stamp(path, &stamp),
            "the note changed before it could be moved to the trash"
        );
        let mut note = parse_note_from_markdown(&text, relative, Utc::now())?;
        if note.id != id {
            note.id = id.to_string();
            let written = note_to_markdown(&note)?;
            let moved = crate::paths::write_new(&dest, &written);
            self.trashed_by_leo(&dest, &written, moved.is_ok());
            moved?;
            anyhow::ensure!(
                crate::paths::same_stamp(path, &stamp),
                "the note changed before it could be moved to the trash"
            );
            return Ok(fs::remove_file(path)?);
        }
        let moved = crate::paths::move_new(path, &dest)
            .map_err(anyhow::Error::from)
            .inspect(|()| {
                if let Err(error) = stamp_deleted(&dest) {
                    crate::diag::warn(format!(
                        "leo could not mark when a note entered the trash: {error}"
                    ));
                }
            });
        self.trashed_by_leo(&dest, &text, moved.is_ok());
        moved
    }

    fn trashed_by_leo(&self, dest: &Path, text: &str, moved: bool) {
        let clock = self.sample_clock();
        let mut slot = self.trash.borrow_mut();
        let Some(index) = slot.as_mut() else {
            return;
        };
        let known = (|| {
            let relative = dest.strip_prefix(trash_dir(&self.notes_dir)?).ok()?;
            let meta = fs::metadata(dest).ok().filter(|_| moved)?;
            let (head, pictures) = split_front_matter(&normalised(text))
                .ok()
                .map(|(front, _)| {
                    let head = head_of(&mut front.unwrap_or_default(), relative);
                    (head, crate::attachments::mentioned(text))
                })
                .unzip();
            let entry = Indexed {
                stamp: stamp_of(&meta),
                digest: Some(crate::paths::digest(text.as_bytes())),
                deleted_at: meta.modified().ok()?.into(),
                head,
                pictures,
                entered: Some(clock),
                hidden: false,
            };
            Some((relative.to_path_buf(), entry))
        })();
        match known {
            Some((relative, entry)) => {
                index.files.insert(relative, entry);
                index.changed = true;
            }
            None => self.trash_stale.set(true),
        }
    }

    fn clock(&self) -> i64 {
        self.pictures.borrow().saved.clock.secs
    }

    pub fn sample_clock(&self) -> i64 {
        let elapsed = self.clock_sample.get().elapsed().as_secs();
        if elapsed != 0 {
            let whole = std::time::Duration::from_secs(elapsed);
            self.clock_sample.set(self.clock_sample.get() + whole);
            let mut pictures = self.pictures.borrow_mut();
            pictures.saved.clock.secs = pictures.saved.clock.secs.saturating_add(elapsed as i64);
            pictures.changed = true;
        }
        self.clock()
    }

    pub fn observe(&self, now: DateTime<Utc>, seen: Option<chrono::Duration>) {
        self.clock_sample.set(std::time::Instant::now());
        {
            let mut pictures = self.pictures.borrow_mut();
            let clock = &mut pictures.saved.clock;
            let zero = chrono::Duration::zero();
            let passed = clock.last.map_or(zero, |last| (now - last).max(zero));
            let credit = seen
                .unwrap_or_else(|| passed.min(chrono::Duration::days(1)))
                .max(zero);
            clock.secs = clock.secs.saturating_add(credit.num_seconds());
            clock.last = Some(clock.last.map_or(now, |last| last.max(now)));
            pictures.changed = true;
        }
        self.keep_pictures();
    }

    pub fn would_expire(&self, days: u32) -> usize {
        let clock = self.sample_clock();
        let live: HashSet<&str> = self.notes.iter().map(|n| n.id.as_str()).collect();
        self.with_trash(true, |index| {
            if index.incomplete || !self.unreadable.is_empty() {
                return 0;
            }
            index
                .files
                .values()
                .filter(|entry| {
                    entry
                        .head
                        .as_ref()
                        .is_some_and(|head| !live.contains(head.id.as_str()))
                        && clock.saturating_sub(entry.entered.unwrap_or(clock))
                            >= i64::from(days) * 86_400
                })
                .count()
        })
    }

    fn with_trash<R>(&self, check: bool, work: impl FnOnce(&mut TrashIndex) -> R) -> R {
        let clock = self.sample_clock();
        let mut slot = self.trash.borrow_mut();
        let index = slot.get_or_insert_with(|| {
            self.trash_stale.set(true);
            TrashIndex::load(&self.notes_dir)
        });
        if self.trash_stale.replace(false) | check {
            index.check(&self.notes_dir, clock);
        }
        if let Ok(live) = self.seen.try_borrow() {
            for entry in index.files.values_mut() {
                let Some(head) = &entry.head else { continue };
                let hidden = live.contains_key(&head.id);
                if hidden != entry.hidden {
                    if !hidden {
                        entry.entered = Some(clock);
                    }
                    entry.hidden = hidden;
                    index.changed = true;
                }
            }
        }
        let out = work(index);
        if std::mem::take(&mut index.changed) {
            index.persist(&self.notes_dir);
        }
        out
    }

    pub fn trash_may_have_changed(&self) {
        self.trash_stale.set(true);
    }

    fn observe_heads(&self, index: &TrashIndex) {
        let mut first: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut reused = Vec::new();
        let mut note = |id: &str, created: DateTime<Utc>| {
            let id = id.to_ascii_lowercase();
            match first.get(&id) {
                Some(seen) if *seen != created => reused.push(id),
                Some(_) => {}
                None => {
                    first.insert(id, created);
                }
            }
        };
        for (id, entry) in self.seen.borrow().iter() {
            note(id, entry.created_at);
        }
        for head in index.files.values().filter_map(|entry| entry.head.as_ref()) {
            if let Some(created) = head.created_at {
                note(&head.id, created);
            }
        }
        let mut pictures = self.pictures.borrow_mut();
        let saved = &pictures.saved;
        for (id, created) in saved
            .gone
            .iter()
            .filter_map(|(id, left)| Some((id, left.created_at?)))
            .chain(
                saved
                    .retired
                    .iter()
                    .map(|(id, (created, _))| (id, *created)),
            )
            .chain(
                saved
                    .expired
                    .iter()
                    .filter_map(|(id, created)| Some((id, (*created)?))),
            )
        {
            note(id, created);
        }
        for id in reused {
            pictures.changed |= pictures.saved.reused.insert(id);
        }
    }

    pub fn observe_generations(&self) {
        self.with_trash(false, |index| self.observe_heads(index));
    }

    pub fn reused_id(&self, id: &str) -> bool {
        self.pictures
            .borrow()
            .saved
            .reused
            .contains(&id.to_ascii_lowercase())
    }

    pub fn trash_snapshot(&self) -> Vec<TrashEntry> {
        self.with_trash(true, |index| index.entries(&self.notes_dir))
    }

    pub fn trashed(&self) -> Vec<Trashed> {
        let live: HashSet<&str> = self.notes.iter().map(|n| n.id.as_str()).collect();
        let mut out: Vec<Trashed> = self.with_trash(false, |index| {
            index
                .files
                .iter()
                .filter_map(|(relative, entry)| {
                    let Some(head) = &entry.head else {
                        let place = head_of(&mut serde_yaml::Mapping::new(), relative);
                        return Some(Trashed {
                            id: unreadable_id(relative),
                            title: place.title,
                            directory: place.directory,
                            deleted_at: entry.deleted_at,
                            readable: false,
                            revision: entry.digest,
                        });
                    };
                    (!live.contains(head.id.as_str())).then(|| Trashed {
                        id: head.id.clone(),
                        title: head.title.clone(),
                        directory: head.directory.clone(),
                        deleted_at: entry.deleted_at,
                        readable: true,
                        revision: entry.digest,
                    })
                })
                .collect()
        });
        out.sort_by_key(|t| std::cmp::Reverse(t.deleted_at));
        out
    }

    pub fn restore(&mut self, id: &str) -> Option<String> {
        self.restore_many(&[id.to_string()]).titles.pop()
    }

    pub fn restore_many(&mut self, ids: &[String]) -> Restored {
        let entries = self.trash_snapshot();
        let mut by_id: HashMap<&str, &TrashEntry> = HashMap::new();
        let mut headless = HashSet::new();
        for entry in &entries {
            match &entry.head {
                Some(head) => {
                    by_id.entry(head.id.as_str()).or_insert(entry);
                }
                None => {
                    headless.insert(entry.id());
                }
            }
        }
        let mut taken: HashSet<String> = self.notes.iter().map(|n| n.id.clone()).collect();
        let mut restored = Restored::default();
        for id in ids {
            if taken.contains(id) {
                continue;
            }
            let Some(entry) = by_id.get(id.as_str()) else {
                if headless.remove(id) {
                    restored.unreadable += 1;
                }
                continue;
            };
            let note = read_regular_text(&entry.path).ok().and_then(|text| {
                parse_note_from_markdown(&text, &entry.relative, entry.deleted_at).ok()
            });
            taken.insert(id.clone());
            let Some(note) = note else {
                restored.unreadable += 1;
                continue;
            };
            add_with_parents(&mut self.directories, &note.directory);
            restored.titles.push(note.title.clone());
            self.dirty.get_mut().insert(note.id.clone());
            self.feed.get_mut().record(note.id.clone());
            self.notes.push(note);
        }
        if !restored.titles.is_empty() {
            self.notes.sort_by_key(|n| std::cmp::Reverse(n.created_at));
        }
        restored
    }

    pub fn delete_from_trash(&self, ids: &[String]) -> Emptied {
        let chosen: HashSet<&str> = ids.iter().map(String::as_str).collect();
        self.clear_trash(Some(&chosen), None)
    }

    pub fn delete_shown(
        &self,
        ids: Option<&[String]>,
        shown: &HashMap<String, crate::paths::FileDigest>,
    ) -> Emptied {
        let chosen: Option<HashSet<&str>> = ids.map(|ids| ids.iter().map(String::as_str).collect());
        self.clear_trash(chosen.as_ref(), Some(shown))
    }

    pub fn tidy_trash_now(&self) -> Vec<String> {
        self.tidy_trash(&self.notes.iter().map(|n| n.id.as_str()).collect(), true)
    }

    fn carried(index: &TrashIndex, entry: &TrashEntry) -> Option<Vec<String>> {
        index
            .files
            .get(&entry.relative)
            .and_then(|indexed| indexed.pictures.clone())
            .or_else(|| entry.head.as_ref().and_then(|_| pictures_read(&entry.path)))
    }

    fn tidy_trash(&self, live: &HashSet<&str>, expire: bool) -> Vec<String> {
        let clock = self.sample_clock();
        let period = trash_days_now(&self.notes_dir)
            .filter(|_| expire)
            .flatten()
            .map(|days| i64::from(days) * 86_400);
        let mut carried = Vec::new();
        let expired = self.with_trash(true, |index| {
            if index.incomplete || !self.unreadable.is_empty() {
                return Vec::new();
            }
            self.observe_heads(index);
            let seen = self.seen.borrow();
            let entries: Vec<_> = index
                .entries(&self.notes_dir)
                .into_iter()
                .filter_map(|entry| {
                    let head = entry.head.as_ref()?;
                    let indexed = &index.files[&entry.relative];
                    if live.contains(head.id.as_str()) {
                        let path = crate::paths::contained_path(
                            &self.notes_dir,
                            &seen.get(&head.id)?.path,
                        )
                        .ok()?;
                        let (digest, stamp) =
                            crate::paths::digest_stamped(&path, MOST_TRASH_NOTE_BYTES).ok()?;
                        return (indexed.digest == Some(digest))
                            .then_some((entry, Some((path, stamp))));
                    }
                    period
                        .is_some_and(|period| {
                            clock.saturating_sub(indexed.entered.unwrap_or(clock)) >= period
                        })
                        .then_some((entry, None))
                })
                .collect();
            {
                let mut pictures = self.pictures.borrow_mut();
                let mut has_expiry = false;
                for (entry, _) in &entries {
                    let Some(head) = entry
                        .head
                        .as_ref()
                        .filter(|head| !live.contains(head.id.as_str()))
                    else {
                        continue;
                    };
                    has_expiry = true;
                    let created = head.created_at.or_else(|| {
                        read_head(&entry.path, &entry.relative).and_then(|head| head.created_at)
                    });
                    pictures.saved.expired.insert(head.id.clone(), created);
                    if let (Some(created), Some(digest)) =
                        (created, index.files[&entry.relative].digest)
                    {
                        pictures
                            .saved
                            .retired
                            .insert(head.id.clone(), (created, digest));
                    }
                }
                if has_expiry && !pictures.saved.persist(&self.notes_dir) {
                    pictures.changed = true;
                    return Vec::new();
                }
            }
            let mut expired = Vec::new();
            let mut removed = false;
            for (entry, duplicate) in entries {
                let head = entry
                    .head
                    .as_ref()
                    .expect("only readable heads were selected");
                let is_live = live.contains(head.id.as_str());
                let pictures = if is_live {
                    Some(Vec::new())
                } else {
                    Self::carried(index, &entry)
                };
                let Some(pictures) = pictures else { continue };
                match self.unlink_trash(
                    &entry.relative,
                    &index.files[&entry.relative],
                    duplicate.as_ref(),
                ) {
                    Ok(()) => {
                        removed = true;
                        index.files.remove(&entry.relative);
                        index.changed = true;
                        carried.extend(pictures);
                        if !is_live {
                            expired.push(head.id.clone());
                        }
                    }
                    Err(e) => crate::diag::warn(format!("could not tidy the trash: {e}")),
                }
            }
            if removed {
                if let Some(trash) = trash_dir(&self.notes_dir) {
                    remove_emptied(&trash);
                }
            }
            expired
        });
        self.pictures.borrow_mut().deleted(&carried, false);
        self.keep_pictures();
        expired
    }

    fn unlink_trash(
        &self,
        relative: &Path,
        entry: &Indexed,
        duplicate: Option<&(PathBuf, crate::paths::EntryStamp)>,
    ) -> std::io::Result<()> {
        let path = crate::paths::contained_path(&self.notes_dir, &Path::new(TRASH).join(relative))
            .map_err(std::io::Error::other)?;
        let changed = || std::io::Error::other("the trashed file changed");
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_file() || stamp_of(&meta) != entry.stamp {
            return Err(changed());
        }
        let (digest, stamp) = crate::paths::digest_stamped(&path, MOST_TRASH_NOTE_BYTES)?;
        if entry.digest != Some(digest)
            || duplicate.is_some_and(|(live, stamp)| !crate::paths::same_stamp(live, stamp))
            || !crate::paths::same_stamp(&path, &stamp)
        {
            return Err(changed());
        }
        fs::remove_file(path)
    }

    pub fn expired_notes(&self) -> Vec<String> {
        self.pictures
            .borrow()
            .saved
            .expired
            .keys()
            .cloned()
            .collect()
    }

    pub fn removed_note_generation(
        &self,
        id: &str,
    ) -> Option<(DateTime<Utc>, crate::paths::FileDigest)> {
        if self.reused_id(id) {
            return None;
        }
        self.recorded_generation(id)
    }

    pub fn recorded_generation(
        &self,
        id: &str,
    ) -> Option<(DateTime<Utc>, crate::paths::FileDigest)> {
        let pictures = self.pictures.borrow();
        pictures.saved.retired.get(id).copied().or_else(|| {
            let gone = pictures.saved.gone.get(id)?;
            Some((gone.created_at?, gone.digest?))
        })
    }

    #[cfg(test)]
    pub fn empty_trash(&self) -> Emptied {
        self.clear_trash(None, None)
    }

    fn clear_trash(
        &self,
        chosen: Option<&HashSet<&str>>,
        shown: Option<&HashMap<String, crate::paths::FileDigest>>,
    ) -> Emptied {
        let mut emptied = Emptied::default();
        let mut removed: Vec<String> = Vec::new();
        let mut present: HashSet<String> = HashSet::new();
        let mut any = false;
        let mut carried = Vec::new();
        let mut kept: HashSet<PathBuf> = HashSet::new();
        self.with_trash(true, |index| {
            if let Some(shown) = shown {
                let entries = index.entries(&self.notes_dir);
                let digest_of = |entry: &TrashEntry| index.files[&entry.relative].digest;
                for entry in &entries {
                    let id = entry.id();
                    if chosen.is_some_and(|ids| !ids.contains(id.as_str())) {
                        continue;
                    }
                    let digest = digest_of(entry);
                    match (digest, shown.get(&id)) {
                        (Some(found), Some(seen)) if found == *seen => {}
                        (None, _) => {
                            emptied.failed += 1;
                            kept.insert(entry.relative.clone());
                        }
                        (Some(_), None) if chosen.is_none() => {
                            kept.insert(entry.relative.clone());
                        }
                        (Some(_), Some(seen))
                            if entries.iter().any(|other| {
                                other.id() == id && digest_of(other) == Some(*seen)
                            }) =>
                        {
                            emptied.failed += 1;
                            kept.insert(entry.relative.clone());
                        }
                        _ => emptied.changed = true,
                    }
                }
                if emptied.changed {
                    return;
                }
            }
            {
                let mut pictures = self.pictures.borrow_mut();
                let mut planned = false;
                for (relative, indexed) in &index.files {
                    if kept.contains(relative) {
                        continue;
                    }
                    if let (Some(head), Some(digest)) = (&indexed.head, indexed.digest) {
                        if let Some(created) = head
                            .created_at
                            .filter(|_| chosen.is_none_or(|ids| ids.contains(head.id.as_str())))
                        {
                            pictures
                                .saved
                                .retired
                                .insert(head.id.clone(), (created, digest));
                            planned = true;
                        }
                    }
                }
                if planned && !pictures.saved.persist(&self.notes_dir) {
                    pictures.changed = true;
                    emptied.failed += 1;
                    return;
                }
            }
            for entry in index.entries(&self.notes_dir) {
                let (wanted, counted) = match &entry.head {
                    Some(head) => (chosen.is_none_or(|c| c.contains(head.id.as_str())), true),
                    None => (chosen.is_none_or(|c| c.contains(entry.id().as_str())), true),
                };
                if !wanted || kept.contains(&entry.relative) {
                    continue;
                }
                let pictures = Self::carried(index, &entry).unwrap_or_default();
                let head_id = entry.head.map(|head| head.id);
                match self.unlink_trash(&entry.relative, &index.files[&entry.relative], None) {
                    Ok(()) => {
                        index.files.remove(&entry.relative);
                        index.changed = true;
                        any = true;
                        carried.extend(pictures);
                        emptied.deleted += usize::from(counted);
                        removed.extend(head_id);
                    }
                    Err(e) => {
                        crate::diag::warn(format!(
                            "could not delete {} from the trash: {e}",
                            entry.path.display()
                        ));
                        emptied.failed += 1;
                        present.extend(head_id);
                    }
                }
            }
        });
        let live: HashSet<&str> = self.notes.iter().map(|n| n.id.as_str()).collect();
        let mut listed = HashSet::new();
        for id in removed {
            if !present.contains(&id) && !live.contains(id.as_str()) && listed.insert(id.clone()) {
                emptied.forgotten.push(id);
            }
        }
        if any {
            if let Some(trash) = trash_dir(&self.notes_dir) {
                remove_emptied(&trash);
            }
        }
        self.pictures.borrow_mut().deleted(&carried, true);
        self.keep_pictures();
        emptied
    }

    pub fn picture_batch<R>(&self, work: impl FnOnce() -> R) -> R {
        let was = self.batching.replace(true);
        let out = work();
        self.batching.set(was);
        self.holding.take();
        self.keep_pictures();
        out
    }

    fn keep_pictures(&self) {
        if self.batching.get() {
            return;
        }
        let back: Vec<String> = {
            let pictures = self.pictures.borrow();
            pictures
                .saved
                .binned
                .keys()
                .filter(|name| {
                    pictures.live.contains_key(*name)
                        || pictures.saved.binned[*name].returning.is_some()
                })
                .cloned()
                .collect()
        };
        for name in back {
            self.unbin(&name, false);
        }
        let mut pictures = self.pictures.borrow_mut();
        if std::mem::take(&mut pictures.changed) {
            pictures.changed = !pictures.saved.persist(&self.notes_dir);
        }
    }

    fn bin_dir(&self) -> Result<PathBuf> {
        crate::paths::contained_path(
            &self.notes_dir,
            &Path::new(TRASH).join(crate::attachments::DIR),
        )
    }

    fn picture_dir(&self) -> Result<PathBuf> {
        crate::paths::contained_path(&self.notes_dir, Path::new(crate::attachments::DIR))
    }

    fn plain_file(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
    }

    fn free(path: &Path) -> bool {
        matches!(fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    }

    fn recover_picture_rows(&self) {
        const MOST_BIN_FILES: usize = 100_000;
        let Ok(entries) = self.bin_dir().and_then(|bin| Ok(fs::read_dir(bin)?)) else {
            return;
        };
        let mut pictures = self.pictures.borrow_mut();
        for entry in entries.take(MOST_BIN_FILES).flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !crate::attachments::own_name(&name) || pictures.saved.binned.contains_key(&name) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if !meta.is_file() {
                    continue;
                }
                let at = meta.modified().map_or_else(|_| Utc::now(), DateTime::from);
                pictures.saved.binned.insert(name, Grace::from(at, 0));
                pictures.changed = true;
            }
        }
    }

    pub fn binned_picture_names(&self) -> Vec<String> {
        self.pictures
            .borrow()
            .saved
            .binned
            .keys()
            .cloned()
            .collect()
    }

    fn binned_file_matches(&self, name: &str, path: &Path) -> bool {
        crate::attachments::own_name(name)
            && self
                .pictures
                .borrow()
                .saved
                .binned
                .get(name)
                .and_then(|grace| Self::picture_matches(path, grace))
                .is_some()
    }

    fn picture_matches(path: &Path, grace: &Grace) -> Option<crate::paths::EntryStamp> {
        let seen = crate::attachments::SeenPicture {
            stamp: grace.file?,
            digest: grace.digest?,
        };
        crate::attachments::same_bytes(path, &seen)
    }

    pub fn delete_binned_picture(&self, name: &str) -> std::io::Result<()> {
        let bin = self.bin_dir().map_err(std::io::Error::other)?;
        let path = bin.join(name);
        if self
            .pictures
            .borrow()
            .saved
            .binned
            .get(name)
            .is_some_and(|grace| grace.returning.is_some())
        {
            return Err(std::io::Error::other(
                "the picture in the trash could not be verified",
            ));
        }
        let stamp = self
            .pictures
            .borrow()
            .saved
            .binned
            .get(name)
            .filter(|_| crate::attachments::own_name(name))
            .and_then(|grace| Self::picture_matches(&path, grace));
        if !stamp.is_some_and(|stamp| crate::paths::same_stamp(&path, &stamp)) {
            return Err(std::io::Error::other(
                "the picture in the trash could not be verified",
            ));
        }
        fs::remove_file(path)?;
        self.binned_gone(name);
        Ok(())
    }

    fn unbin(&self, name: &str, manual: bool) -> bool {
        if !crate::attachments::own_name(name) {
            return false;
        }
        let (Ok(bin), Ok(home)) = (self.bin_dir(), self.picture_dir()) else {
            return false;
        };
        let from = bin.join(name);
        let Some(grace) = self.pictures.borrow().saved.binned.get(name).cloned() else {
            return false;
        };
        let exact = |path: &Path| Self::picture_matches(path, &grace).is_some();
        if let Some(to) = grace
            .returning
            .as_deref()
            .filter(|name| crate::attachments::own_name(name))
        {
            if Self::free(&from) && exact(&home.join(to)) {
                self.remove_bin_row(name);
                return true;
            }
        }
        if !(exact(&from) || manual && grace.digest.is_none() && Self::plain_file(&from)) {
            return false;
        }
        if fs::create_dir_all(&home).is_err() {
            return false;
        }
        let wanted = home.join(name);
        const MOST_HOME_PICTURES: usize = 100_000;
        let Ok(entries) = fs::read_dir(&home) else {
            return false;
        };
        let mut occupied = false;
        for (n, entry) in entries.enumerate() {
            let Ok(entry) = entry else { return false };
            if n >= MOST_HOME_PICTURES {
                return false;
            }
            if entry.file_name().eq_ignore_ascii_case(name) {
                occupied = true;
                break;
            }
        }
        let to = if !occupied && Self::free(&wanted) {
            wanted
        } else {
            let Some((stem, ext)) = name.rsplit_once('.') else {
                return false;
            };
            home.join(format!("{stem}-restored-{}.{ext}", uuid::Uuid::new_v4()))
        };
        let Ok(meta) = fs::symlink_metadata(&from) else {
            return false;
        };
        let stamp = crate::attachments::written(&meta);
        if !meta.is_file() || grace.file.is_some_and(|file| file != stamp) {
            return false;
        }
        let digest = match grace.digest {
            Some(digest) => digest,
            None => match crate::paths::digest_file(&from, crate::attachments::MOST_BYTES as u64) {
                Ok(digest) => digest,
                Err(_) => return false,
            },
        };
        {
            let mut pictures = self.pictures.borrow_mut();
            let row = pictures.saved.binned.get_mut(name).unwrap();
            row.file = Some(stamp);
            row.digest = Some(digest);
            row.returning = to
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            if manual {
                pictures.saved.released.remove(name);
                pictures.saved.kept.insert(name.to_string());
            }
            if !pictures.saved.persist(&self.notes_dir) {
                return false;
            }
        }
        if Self::free(&to)
            && self.binned_file_matches(name, &from)
            && crate::paths::move_new(&from, &to).is_ok()
        {
            self.remove_bin_row(name);
            return true;
        }
        false
    }

    fn remove_bin_row(&self, name: &str) {
        let mut pictures = self.pictures.borrow_mut();
        pictures.changed |= pictures.saved.binned.remove(name).is_some();
    }

    pub fn bin_pictures(
        &self,
        files: &[(String, PathBuf, crate::attachments::SavedPicture)],
    ) -> std::io::Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let refused = || std::io::Error::other("leo moves only an unchanged picture it saved");
        let home = self.picture_dir().map_err(|_| refused())?;
        let bin = self.bin_dir().map_err(|_| refused())?;
        fs::create_dir_all(&bin)?;
        const MOST_BATCH_BYTES: u64 = 64 * 1024 * 1024;
        let mut remaining = MOST_BATCH_BYTES;
        let mut ready = Vec::new();
        for (name, file, record) in files {
            let Some(digest) = record.digest else {
                continue;
            };
            let stamp = record.stamp;
            if !crate::attachments::own_name(name)
                || file.parent() != Some(home.as_path())
                || !file
                    .file_name()
                    .is_some_and(|file| file.eq_ignore_ascii_case(name))
                || !Self::free(&bin.join(name))
                || !fs::symlink_metadata(file)
                    .is_ok_and(|meta| meta.is_file() && crate::attachments::written(&meta) == stamp)
            {
                continue;
            }
            if stamp.0 > remaining {
                break;
            }
            ready.push((name, file, stamp, digest));
            remaining -= stamp.0;
        }
        let entered = self.sample_clock();
        {
            let mut pictures = self.pictures.borrow_mut();
            for (name, _, stamp, digest) in &ready {
                pictures.saved.binned.insert(
                    name.to_string(),
                    Grace {
                        at: Utc::now(),
                        entered,
                        file: Some(*stamp),
                        digest: Some(*digest),
                        returning: None,
                    },
                );
            }
            if !ready.is_empty() && !pictures.saved.persist(&self.notes_dir) {
                return Err(std::io::Error::other("leo could not save the picture move"));
            }
        }
        for (name, file, stamp, digest) in ready {
            let to = bin.join(name);
            let seen = crate::attachments::SeenPicture { stamp, digest };
            if Self::free(&to)
                && crate::attachments::same_bytes(file, &seen)
                    .is_some_and(|found| crate::paths::same_stamp(file, &found))
            {
                match crate::paths::move_new(file, &to) {
                    Ok(()) => {
                        let mut pictures = self.pictures.borrow_mut();
                        pictures.saved.released.remove(name);
                        pictures.changed = true;
                    }
                    Err(e) => crate::diag::warn(format!(
                        "could not move the picture {name} to the trash: {e}"
                    )),
                }
            }
        }
        self.keep_pictures();
        Ok(())
    }

    #[cfg(test)]
    fn bin_picture(&self, name: &str, file: &Path) -> std::io::Result<()> {
        let stamp = crate::attachments::written(&fs::symlink_metadata(file)?);
        let digest = crate::paths::digest_file(file, crate::attachments::MOST_BYTES as u64)?;
        self.bin_pictures(&[(
            name.to_string(),
            file.to_path_buf(),
            crate::attachments::SavedPicture {
                stamp,
                digest: Some(digest),
            },
        )])
    }

    pub fn picture_files(&self) -> Vec<(String, PathBuf, Option<crate::attachments::SeenPicture>)> {
        let (Ok(bin), Ok(home)) = (self.bin_dir(), self.picture_dir()) else {
            return Vec::new();
        };
        let pictures = self.pictures.borrow();
        let binned = pictures.saved.binned.iter().map(|(name, grace)| {
            let seen = grace
                .file
                .zip(grace.digest)
                .map(|(stamp, digest)| crate::attachments::SeenPicture { stamp, digest });
            (format!("picture:{name}"), bin.join(name), seen)
        });
        let unused = pictures
            .saved
            .unused_files
            .iter()
            .map(|(name, seen)| (format!("unused:{name}"), home.join(name), Some(*seen)));
        binned.chain(unused).collect()
    }

    pub fn picture_changed(&self, id: &str, seen: Option<crate::attachments::SeenPicture>) {
        {
            let mut pictures = self.pictures.borrow_mut();
            if let Some(grace) = id
                .strip_prefix("picture:")
                .and_then(|name| pictures.saved.binned.get_mut(name))
            {
                grace.digest = None;
                grace.file = None;
                pictures.changed = true;
            } else if let Some(name) = id.strip_prefix("unused:") {
                match seen {
                    Some(seen) => pictures.saved.unused_files.insert(name.to_string(), seen),
                    None => pictures.saved.unused_files.remove(name),
                };
                pictures.changed = true;
            }
        }
        self.keep_pictures();
    }

    pub fn binned_pictures(&self) -> Vec<(String, PathBuf)> {
        let Ok(bin) = self.bin_dir() else {
            return Vec::new();
        };
        self.pictures
            .borrow()
            .saved
            .binned
            .keys()
            .filter(|name| crate::attachments::own_name(name))
            .map(|name| (name.clone(), bin.join(name)))
            .filter(|(_, path)| Self::plain_file(path))
            .collect()
    }

    pub fn ripe_binned(&self, period: Option<chrono::Duration>) -> Vec<String> {
        let clock = self.clock();
        self.pictures
            .borrow()
            .saved
            .binned
            .iter()
            .filter(|(_, grace)| {
                grace.digest.is_some()
                    && grace.returning.is_none()
                    && grace.at != DateTime::<Utc>::UNIX_EPOCH
                    && grace.due(clock, period)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn binned_gone(&self, name: &str) {
        self.remove_bin_row(name);
        self.keep_pictures();
    }

    pub fn unbin_picture(&self, name: &str) -> bool {
        let back = self.unbin(name, false);
        self.keep_pictures();
        back
    }

    pub fn released_by_user(&self, name: &str) -> bool {
        self.pictures
            .borrow()
            .saved
            .released
            .get(name)
            .is_some_and(|grace| grace.at == DateTime::<Utc>::UNIX_EPOCH)
    }

    pub fn set_unused(&self, files: BTreeMap<String, crate::attachments::SeenPicture>) {
        {
            let mut pictures = self.pictures.borrow_mut();
            if pictures.saved.unused_files != files {
                pictures.saved.unused_files = files;
                pictures.changed = true;
            }
        }
        self.keep_pictures();
    }

    pub fn unused_files(&self) -> BTreeMap<String, crate::attachments::SeenPicture> {
        self.pictures.borrow().saved.unused_files.clone()
    }

    pub fn binned_digest(&self, name: &str) -> Option<crate::paths::FileDigest> {
        self.pictures
            .borrow()
            .saved
            .binned
            .get(name)
            .and_then(|grace| grace.digest)
    }

    pub fn unused_rows(&self) -> Vec<Trashed> {
        let (Ok(home), live) = (self.picture_dir(), &self.pictures.borrow().live) else {
            return Vec::new();
        };
        let pictures = self.pictures.borrow();
        pictures
            .saved
            .unused_files
            .iter()
            .filter(|(name, _)| !live.contains_key(*name) && Self::plain_file(&home.join(name)))
            .map(|(name, seen)| Trashed {
                id: format!("unused:{name}"),
                title: name.clone(),
                directory: crate::attachments::DIR.to_string(),
                deleted_at: DateTime::<Utc>::UNIX_EPOCH,
                readable: true,
                revision: Some(seen.digest),
            })
            .collect()
    }

    pub fn trashed_ids(&self) -> HashSet<String> {
        self.trash_snapshot()
            .into_iter()
            .filter_map(|entry| entry.head.map(|head| head.id))
            .collect()
    }

    pub fn trashed_picture_counts(&self) -> HashMap<String, usize> {
        self.with_trash(false, |index| {
            index
                .files
                .values()
                .filter_map(|entry| {
                    let head = entry.head.as_ref()?;
                    let count = entry.pictures.as_ref().map_or(0, Vec::len);
                    (count > 0).then(|| (head.id.clone(), count))
                })
                .collect()
        })
    }

    pub fn confirmed_gone(&self) -> HashSet<String> {
        if !self.unreadable.is_empty() || self.pictures.borrow().saved.gone.is_empty() {
            return HashSet::new();
        }
        let folders: HashSet<PathBuf> = self
            .seen
            .borrow()
            .values()
            .map(|entry| folder_of(&entry.path))
            .collect();
        let pictures = self.pictures.borrow();
        pictures
            .saved
            .gone
            .iter()
            .filter(|(_, left)| {
                let folder = folder_of(Path::new(&left.path));
                !left.twin
                    && left.digest.is_some()
                    && left.device.is_some_and(|device| device != 0)
                    && device_of(&self.notes_dir.join(&folder)) == left.device
                    && folders.contains(&folder)
                    && crate::paths::contained_path(&self.notes_dir, Path::new(&left.path))
                        .is_ok_and(|path| {
                            matches!(fs::symlink_metadata(&path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
                                && path.parent().is_some_and(|folder| {
                                    fs::symlink_metadata(folder).is_ok_and(|m| m.is_dir())
                                        && fs::read_dir(folder).is_ok()
                                })
                        })
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn forget_departed(&self, ids: &[String]) -> bool {
        {
            let mut pictures = self.pictures.borrow_mut();
            let before = pictures.saved.gone.len();
            pictures.saved.gone.retain(|id, _| !ids.contains(id));
            pictures.changed |= before != pictures.saved.gone.len();
            for id in ids {
                pictures.changed |= pictures.saved.expired.remove(id).is_some();
                pictures.changed |= pictures.saved.retired.remove(id).is_some();
            }
        }
        self.keep_pictures();
        !self.pictures.borrow().changed
    }

    pub fn revalidate_trash(&self) -> bool {
        self.with_trash(true, |index| {
            !index.incomplete && index.files.values().all(|entry| entry.head.is_some())
        })
    }

    pub fn trash_complete(&self) -> bool {
        self.with_trash(false, |index| !index.incomplete)
    }

    fn held_in_trash(&self, name: &str) -> Option<usize> {
        if !self.batching.get() {
            return self
                .trash_holds(false)
                .map(|held| held.get(name).copied().unwrap_or(0));
        }
        if self.holding.borrow().is_none() {
            *self.holding.borrow_mut() = Some(self.trash_holds(false));
        }
        self.holding
            .borrow()
            .as_ref()
            .and_then(|held| held.as_ref())
            .map(|held| held.get(name).copied().unwrap_or(0))
    }

    fn trash_holds(&self, check: bool) -> Option<HashMap<String, usize>> {
        self.with_trash(check, |index| {
            if index.incomplete {
                return None;
            }
            let mut held: HashMap<String, usize> = HashMap::new();
            for entry in index.files.values() {
                entry.head.as_ref()?;
                for name in entry.pictures.as_ref()? {
                    *held.entry(name.clone()).or_default() += 1;
                }
            }
            Some(held)
        })
    }

    pub fn trash_reads_owed(&self) -> Vec<Owed> {
        let Some(trash) = trash_dir(&self.notes_dir) else {
            return Vec::new();
        };
        self.with_trash(true, |index| {
            index
                .files
                .iter()
                .filter(|(_, entry)| entry.head.is_some() && entry.pictures.is_none())
                .map(|(relative, entry)| Owed {
                    path: trash.join(relative),
                    relative: relative.clone(),
                    stamp: entry.stamp,
                })
                .collect()
        })
    }

    pub fn settle_owed(&self, read: Vec<(Owed, Vec<String>)>) {
        self.with_trash(false, |index| {
            for (owed, pictures) in read {
                if let Some(entry) = index
                    .files
                    .get_mut(&owed.relative)
                    .filter(|entry| entry.stamp == owed.stamp)
                {
                    entry.pictures = Some(pictures);
                    index.changed = true;
                }
            }
        });
    }

    pub fn picture_pass(
        &self,
        listed: &[String],
        now: DateTime<Utc>,
        period: Option<chrono::Duration>,
    ) -> Option<Vec<String>> {
        let clock = self.sample_clock();
        let in_trash = self.trash_holds(false)?;
        let mut pictures = self.pictures.borrow_mut();
        let Pictures {
            live,
            saved,
            changed,
        } = &mut *pictures;
        let listed: HashSet<&str> = listed.iter().map(String::as_str).collect();
        let before = (saved.released.len(), saved.kept.len());
        saved
            .released
            .retain(|name, _| listed.contains(name.as_str()));
        saved.kept.retain(|name| listed.contains(name.as_str()));
        *changed |= before != (saved.released.len(), saved.kept.len());
        let owned = |name: &str| live.contains_key(name) || in_trash.contains_key(name);
        for &name in &listed {
            if owned(name) {
                let released = saved.released.remove(name).is_some();
                let kept = saved.kept.remove(name);
                *changed |= released || kept;
            } else if !saved.kept.contains(name) && !saved.released.contains_key(name) {
                saved
                    .released
                    .insert(name.to_string(), Grace::from(now, clock));
                *changed = true;
            }
        }
        let due = saved
            .released
            .iter()
            .filter(|(name, grace)| !owned(name.as_str()) && grace.due(clock, period))
            .map(|(name, _)| name.clone())
            .collect();
        drop(pictures);
        self.keep_pictures();
        Some(due)
    }

    pub fn picture_owners(&self, name: &str) -> usize {
        let live = self.pictures.borrow().live.get(name).copied().unwrap_or(0);
        live + self.held_in_trash(name).unwrap_or(0)
    }

    pub fn released_pictures(&self) -> Vec<String> {
        self.pictures
            .borrow()
            .saved
            .released
            .keys()
            .cloned()
            .collect()
    }

    pub fn picture_still_due(&self, name: &str, period: Option<chrono::Duration>) -> bool {
        let pictures = self.pictures.borrow();
        !pictures.live.contains_key(name)
            && !pictures.saved.kept.contains(name)
            && pictures
                .saved
                .released
                .get(name)
                .is_some_and(|grace| grace.due(self.clock(), period))
            && self.held_in_trash(name) == Some(0)
    }

    pub fn picture_gone(&self, name: &str) {
        {
            let mut pictures = self.pictures.borrow_mut();
            let released = pictures.saved.released.remove(name).is_some();
            let kept = pictures.saved.kept.remove(name);
            pictures.changed |= released || kept;
        }
        self.keep_pictures();
    }

    pub fn picture_rows(&self) -> Vec<Trashed> {
        let there: HashSet<String> = self.binned_pictures().into_iter().map(|(n, _)| n).collect();
        self.pictures
            .borrow()
            .saved
            .binned
            .iter()
            .filter(|(name, _)| there.contains(*name))
            .map(|(name, grace)| Trashed {
                id: format!("picture:{name}"),
                title: name.clone(),
                directory: crate::attachments::DIR.to_string(),
                deleted_at: grace.at,
                readable: true,
                revision: grace.digest,
            })
            .collect()
    }

    pub fn keep_picture(&self, name: &str) -> bool {
        let back = self.unbin(name, true);
        self.keep_pictures();
        back
    }

    fn assign_paths(&self, seen: &HashMap<String, Seen>) -> HashMap<String, PathBuf> {
        let mut order: Vec<&Note> = self.notes.iter().collect();
        order.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        let mut taken: HashSet<String> = HashSet::new();
        let mut targets = HashMap::new();
        for note in order {
            let dir = if note.directory.is_empty() {
                self.notes_dir.clone()
            } else {
                self.notes_dir.join(&note.directory)
            };
            let base = filename::file_name(&note.title);
            let own = seen
                .get(&note.id)
                .map(|s| path_key(&self.notes_dir.join(&s.path)));
            for n in 1.. {
                let candidate = dir.join(format!("{}.md", filename::numbered(&base, n)));
                let key = path_key(&candidate);
                if taken.contains(&key) {
                    continue;
                }
                let is_own = own.as_deref() == Some(key.as_str());
                if !is_own && candidate.exists() {
                    continue;
                }
                taken.insert(key);
                targets.insert(note.id.clone(), candidate);
                break;
            }
        }
        targets
    }

    /// Create and store a new note, returning a reference to it.
    pub fn create_note(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        directory: &str,
    ) -> Result<&Note> {
        self.validate_directory(directory)?;
        add_with_parents(&mut self.directories, directory);
        let note = Note::new(title, body, directory);
        self.dirty.get_mut().insert(note.id.clone());
        self.feed.get_mut().record(note.id.clone());
        self.notes.push(note);
        Ok(self.notes.last().unwrap())
    }

    pub fn list_notes(&self, dir: Option<&str>) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| dir.is_none_or(|dir| n.directory == dir))
            .collect();
        notes.sort_by_key(|n| (std::cmp::Reverse(n.pinned), std::cmp::Reverse(n.updated_at)));
        notes
    }

    /// Find a note by full ID or unique prefix.
    pub fn find_note(&self, id_prefix: &str) -> Option<&Note> {
        if let Some(note) = self.notes.iter().find(|n| n.id == id_prefix) {
            return Some(note);
        }
        let matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.id.starts_with(id_prefix))
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    /// Find a mutable note by full ID or unique prefix.
    pub fn find_note_mut(&mut self, id_prefix: &str) -> Option<&mut Note> {
        let at = match self.notes.iter().position(|n| n.id == id_prefix) {
            Some(at) => at,
            None => {
                let mut found = self
                    .notes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.id.starts_with(id_prefix));
                let (first, _) = found.next()?;
                if found.next().is_some() {
                    return None;
                }
                first
            }
        };
        self.dirty.get_mut().insert(self.notes[at].id.clone());
        self.feed.get_mut().record(self.notes[at].id.clone());
        Some(&mut self.notes[at])
    }

    /// Delete every note whose full ID is in `ids`.
    /// Returns how many were deleted.
    pub fn delete_notes(&mut self, ids: &[String]) -> usize {
        let before = self.notes.len();
        let removed = self.removed.get_mut();
        let feed = self.feed.get_mut();
        self.notes.retain(|n| {
            let keep = !ids.contains(&n.id);
            if !keep {
                removed.insert(n.id.clone());
                feed.record(n.id.clone());
            }
            keep
        });
        before - self.notes.len()
    }

    /// The one search: every directory, titles and bodies.
    ///
    /// Every word must appear somewhere in the note. Notes whose
    /// title holds all the words come first, then other matches, then notes
    /// whose title only fuzzy-matches (`grtrv` for "Graph traversals"). Newest
    /// first within each group. Case never matters.
    pub fn find(&self, query: &str) -> Vec<&Note> {
        let query = query.to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();

        let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
        let fuzzy = nucleo::pattern::Pattern::parse(
            &words.join(" "),
            nucleo::pattern::CaseMatching::Ignore,
            nucleo::pattern::Normalization::Smart,
        );

        let mut ranked: Vec<(u8, &Note)> = Vec::new();
        for note in &self.notes {
            let title = note.title.to_lowercase();
            let body = note.body.to_lowercase();
            let in_title = words.iter().all(|w| title.contains(w));
            let anywhere = words.iter().all(|w| title.contains(w) || body.contains(w));
            let group = if in_title {
                0
            } else if anywhere {
                1
            } else {
                let mut buf = Vec::new();
                let haystack = nucleo::Utf32Str::new(&note.title, &mut buf);
                if fuzzy.score(haystack, &mut matcher).is_none() {
                    continue;
                }
                2
            };
            ranked.push((group, note));
        }
        ranked.sort_by(|(ga, a), (gb, b)| ga.cmp(gb).then(b.updated_at.cmp(&a.updated_at)));
        ranked.into_iter().map(|(_, n)| n).collect()
    }

    // ── Directory operations ───────────────────────────────────────────────

    pub fn validate_directory(&self, directory: &str) -> Result<()> {
        crate::paths::validate_directory(directory)?;
        crate::paths::contained_path(&self.notes_dir, Path::new(directory))?;
        Ok(())
    }

    pub fn create_dir(&mut self, path: &str) -> bool {
        if self.validate_directory(path).is_err() {
            return false;
        }
        if path.is_empty() {
            return false;
        }
        let mut created = false;
        let parts: Vec<&str> = path.split('/').collect();
        let fresh = |i: usize| !self.directories.contains(&parts[..=i].join("/"));
        if (0..parts.len()).any(|i| fresh(i) && !crate::filename::portable_folder(parts[i])) {
            return false;
        }
        for i in 0..parts.len() {
            let dir = parts[..=i].join("/");
            if !self.directories.contains(&dir) {
                self.directories.push(dir);
                created = true;
            }
        }
        created
    }

    pub fn dir_exists(&self, path: &str) -> bool {
        if path.is_empty() {
            return true;
        }
        self.directories.contains(&path.to_string())
    }

    pub fn subdirs(&self, parent: &str) -> Vec<String> {
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let mut dirs: Vec<String> = self
            .directories
            .iter()
            .filter_map(|d| {
                if parent.is_empty() {
                    if !d.contains('/') {
                        Some(d.clone())
                    } else {
                        None
                    }
                } else if let Some(rest) = d.strip_prefix(&prefix) {
                    if !rest.is_empty() && !rest.contains('/') {
                        Some(rest.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Delete a directory and everything in it. Returns the counts removed.
    pub fn delete_dir_recursive(&mut self, path: &str) -> (usize, usize) {
        let path = path.trim_matches('/');
        if path.is_empty() {
            // Refuse the root. "Delete everything" is not what any single
            // keypress should mean.
            return (0, 0);
        }
        let prefix = format!("{path}/");

        let (notes, dirs) = (self.notes.len(), self.directories.len());
        let removed = self.removed.get_mut();
        let feed = self.feed.get_mut();
        self.notes.retain(|n| {
            let keep = n.directory != path && !n.directory.starts_with(&prefix);
            if !keep {
                removed.insert(n.id.clone());
                feed.record(n.id.clone());
            }
            keep
        });
        self.directories
            .retain(|d| d != path && !d.starts_with(&prefix));
        (notes - self.notes.len(), dirs - self.directories.len())
    }

    pub fn move_dir(&mut self, from: &str, into: &str) -> Option<String> {
        let from = from.trim_matches('/');
        let into = into.trim_matches('/');
        if from.is_empty() || !self.dir_exists(from) || !self.dir_exists(into) {
            return None;
        }
        let inside = format!("{from}/");
        if into == from || into.starts_with(&inside) {
            return None;
        }
        let name = from.rsplit('/').next()?;
        let to = if into.is_empty() {
            name.to_string()
        } else {
            format!("{into}/{name}")
        };
        self.relocate_dir(from, &to)
    }

    pub fn rename_dir(&mut self, from: &str, name: &str) -> Option<String> {
        if from.is_empty()
            || name.is_empty()
            || name.chars().count() > MOST_FOLDER_NAME_CHARS
            || name.len() > MOST_FOLDER_NAME_BYTES
            || name.contains(['/', '\\'])
            || !crate::filename::portable_folder(name)
            || self.validate_directory(from).is_err()
        {
            return None;
        }
        let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
        let to = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        };
        self.relocate_dir(from, &to)
    }

    fn relocate_dir(&mut self, from: &str, to: &str) -> Option<String> {
        if from.is_empty() || !self.dir_exists(from) {
            return None;
        }
        if to == from
            || self.dir_exists(to)
            || self.validate_directory(to).is_err()
            || self.notes_dir.join(to).exists()
        {
            return None;
        }
        if self
            .directories
            .iter()
            .any(|dir| dir.eq_ignore_ascii_case(to))
        {
            return None;
        }
        let inside = format!("{from}/");
        let renamed = |path: &str| -> Option<String> {
            if path == from {
                Some(to.to_string())
            } else {
                path.strip_prefix(&inside)
                    .map(|rest| format!("{to}/{rest}"))
            }
        };
        let dirty = self.dirty.get_mut();
        let feed = self.feed.get_mut();
        for note in &mut self.notes {
            if let Some(dir) = renamed(&note.directory) {
                note.directory = dir;
                dirty.insert(note.id.clone());
                feed.record(note.id.clone());
            }
        }
        let mut directories: Vec<String> = self
            .directories
            .iter()
            .map(|d| renamed(d).unwrap_or_else(|| d.clone()))
            .collect();
        add_with_parents(&mut directories, to);
        directories.dedup();
        self.directories = directories;
        Some(to.to_string())
    }

    pub fn move_note(&mut self, id_prefix: &str, new_dir: &str) -> Option<String> {
        self.validate_directory(new_dir).ok()?;
        let note = self.find_note_mut(id_prefix)?;
        note.directory = new_dir.to_string();
        Some(note.title.clone())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    use super::*;
    use crate::notes::Note;

    #[test]
    #[ignore = "benchmark"]
    fn tags_header_load_and_find_benchmark() {
        use std::time::Instant;
        let count: usize = std::env::var("LEO_BENCH_NOTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let words = [
            "graph", "queue", "heap", "lemma", "cell", "price", "river", "verb",
        ];
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..count {
            let body: String = (0..800)
                .map(|w| words[(i * 7 + w * 13) % words.len()])
                .collect::<Vec<_>>()
                .join(" ");
            fs::write(
                dir.join(format!("Note {i}.md")),
                format!(
                    "---\nid: {}\ntitle: Note {i}\ntags:\n  - '#exam'\n  - week{}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\naliases:\n- Short {i}\ncssclasses: [wide]\n---\n\n{body}\n",
                    uuid::Uuid::new_v4(),
                    i % 12
                ),
            )
            .unwrap();
        }
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let mut loads = Vec::new();
        let mut store = None;
        for _ in 0..5 {
            let t = Instant::now();
            store = Some(Store::load_from(&dir).unwrap());
            loads.push(ms(t.elapsed()));
        }
        let store = store.unwrap();
        assert_eq!(store.notes.len(), count);
        let finds: Vec<f64> = (0..9)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(store.find("queue lemma").len());
                ms(t.elapsed())
            })
            .collect();
        println!(
            "P-C02 notes={count} load_ms={:.1} find_ms={:.2} loads={loads:.1?}",
            median(loads.clone()),
            median(finds)
        );
    }

    fn make_note() -> Note {
        Note {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            title: "Test Note".to_string(),
            body: "Hello **world**".to_string(),
            directory: "".to_string(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            pinned: false,
            extra: Default::default(),
        }
    }

    fn store_with_tree() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        store.create_dir("cs162");
        store.create_note("Root note", "b", "").unwrap();
        store.create_note("In cs130", "b", "cs130").unwrap();
        store.create_note("In lec 1", "b", "cs130/lec").unwrap();
        store.create_note("In lec 2", "b", "cs130/lec").unwrap();
        store.create_note("In cs162", "b", "cs162").unwrap();
        store.save().unwrap();
        (store, dir)
    }

    #[test]
    fn renaming_a_folder_keeps_its_notes_edit_dates() {
        let (mut store, dir) = store_with_tree();
        let before: HashMap<String, DateTime<Utc>> = store
            .notes
            .iter()
            .map(|n| (n.id.clone(), n.updated_at))
            .collect();
        store.rename_dir("cs130", "cs131").unwrap();
        let stamped: Vec<String> = store
            .notes
            .iter()
            .filter(|n| before[&n.id] != n.updated_at)
            .map(|n| n.title.clone())
            .collect();
        assert!(stamped.is_empty(), "updated_at changed for {stamped:?}");
        store.save().unwrap();
        assert!(dir.path().join("notes/cs131/In cs130.md").exists());
        assert!(!dir.path().join("notes/cs130/In cs130.md").exists());
    }

    #[test]
    fn moving_a_note_keeps_its_edit_date_and_moves_its_file() {
        let (mut store, dir) = store_with_tree();
        let id = store.create_note("Mover", "b", "").unwrap().id.clone();
        store.save().unwrap();
        let before = store.notes.iter().find(|n| n.id == id).unwrap().updated_at;
        store.move_note(&id, "cs162").unwrap();
        assert_eq!(
            store.notes.iter().find(|n| n.id == id).unwrap().updated_at,
            before
        );
        store.save().unwrap();
        assert!(dir.path().join("notes/cs162/Mover.md").exists());
        assert!(!dir.path().join("notes/Mover.md").exists());
    }

    #[test]
    fn a_folder_moves_with_everything_in_it_and_survives_a_reload() {
        let (mut store, dir) = store_with_tree();
        assert_eq!(
            store.move_dir("cs130/lec", "cs162").as_deref(),
            Some("cs162/lec")
        );
        assert!(!store.dir_exists("cs130/lec"));
        assert!(store.dir_exists("cs162/lec"));
        store.save().unwrap();
        let again = Store::load_from(&dir.path().join("notes")).unwrap();
        let moved: Vec<&str> = again
            .notes
            .iter()
            .filter(|n| n.title.starts_with("In lec"))
            .map(|n| n.directory.as_str())
            .collect();
        assert_eq!(moved, vec!["cs162/lec", "cs162/lec"]);
        assert_eq!(again.subdirs("cs162"), vec!["lec".to_string()]);
        assert!(again.subdirs("cs130").is_empty());
        assert_eq!(store.move_dir("cs162/lec", "").as_deref(), Some("lec"));
        assert_eq!(store.subdirs(""), vec!["cs130", "cs162", "lec"]);
    }

    #[test]
    fn a_folder_never_moves_into_itself_onto_another_or_nowhere() {
        let (mut store, _d) = store_with_tree();
        store.create_dir("cs162/lec");
        assert_eq!(store.move_dir("cs130", "cs130"), None);
        assert_eq!(store.move_dir("cs130", "cs130/lec"), None);
        assert_eq!(store.move_dir("cs130/lec", "cs162"), None);
        assert_eq!(store.move_dir("cs130/lec", "cs130"), None);
        assert_eq!(store.move_dir("", "cs162"), None);
        assert_eq!(store.move_dir("nope", "cs162"), None);
        assert_eq!(store.move_dir("cs162", "nope"), None);
        assert!(store.dir_exists("cs130/lec"));
    }

    #[test]
    fn deleting_a_directory_recursively_removes_its_notes_and_subdirectories() {
        let (mut store, _d) = store_with_tree();

        let (notes, dirs) = store.delete_dir_recursive("cs130");
        assert_eq!((notes, dirs), (3, 2));

        assert!(!store.dir_exists("cs130"));
        assert!(!store.dir_exists("cs130/lec"));
        // Untouched neighbours.
        assert!(store.dir_exists("cs162"));
        assert_eq!(store.notes.len(), 2);
        assert!(
            store
                .notes
                .iter()
                .filter(|n| n.title == "Root note")
                .count()
                == 1
        );
        assert!(store.notes.iter().filter(|n| n.title == "In cs162").count() == 1);

        // And it survives a reload: the files are gone from disk.
        store.save().unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 2);
        assert!(!reloaded.dir_exists("cs130"));
    }

    #[test]
    fn deleting_a_leaf_directory_leaves_its_parent() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive("cs130/lec"), (2, 1));
        assert!(store.dir_exists("cs130"), "the parent must survive");
        assert_eq!(
            store.notes.iter().filter(|n| n.title == "In cs130").count(),
            1
        );
    }

    /// "Delete everything" is not something any keypress should be able to mean.
    #[test]
    fn the_root_cannot_be_deleted_recursively() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive(""), (0, 0));
        assert_eq!(store.delete_dir_recursive("/"), (0, 0));
        assert_eq!(store.notes.len(), 5, "nothing was removed");
        assert!(store.dir_exists("cs130"));
    }

    #[test]
    fn deleting_an_unknown_directory_removes_nothing() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive("ghost"), (0, 0));
        assert_eq!(store.notes.len(), 5);
    }

    #[test]
    fn test_note_roundtrip() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed = parse_note_from_markdown(
            &md,
            std::path::Path::new("Test Note.md"),
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(parsed.id, note.id);
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.body, note.body);
        assert_eq!(parsed.directory, "");
    }

    #[test]
    fn test_directory_derived_from_path() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed = parse_note_from_markdown(
            &md,
            std::path::Path::new("cs162/lec/550e8400.md"),
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(parsed.directory, "cs162/lec");
    }

    #[test]
    fn test_load_from_reads_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let note = make_note();
        std::fs::write(
            notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md"),
            note_to_markdown(&note).unwrap(),
        )
        .unwrap();

        let mut note2 = make_note();
        note2.id = "aaaabbbb-0000-0000-0000-000000000000".to_string();
        note2.title = "Subdir Note".to_string();
        note2.directory = "cs162".to_string();
        std::fs::create_dir_all(notes_dir.join("cs162")).unwrap();
        std::fs::write(
            notes_dir.join("cs162/aaaabbbb-0000-0000-0000-000000000000.md"),
            note_to_markdown(&note2).unwrap(),
        )
        .unwrap();

        let store = Store::load_from(&notes_dir).unwrap();
        assert_eq!(store.notes.len(), 2);
        let loaded = store.notes.iter().find(|n| n.id == note.id).unwrap();
        assert_eq!(loaded.directory, "");
        assert_eq!(loaded.title, "Test Note");
        assert_eq!(loaded.body, "Hello **world**");
        let loaded2 = store.notes.iter().find(|n| n.id == note2.id).unwrap();
        assert_eq!(loaded2.directory, "cs162");
        assert_eq!(loaded2.title, "Subdir Note");
    }

    #[test]
    fn a_change_to_any_field_of_a_saved_note_reaches_its_file() {
        type Change = fn(&mut Note);
        let changes: [Change; 6] = [
            |n| n.title = "Renamed".into(),
            |n| n.body = "new body".into(),
            |n| n.created_at -= chrono::Duration::days(1),
            |n| n.updated_at += chrono::Duration::days(1),
            |n| n.pinned = true,
            |n| {
                n.extra
                    .insert("rating".into(), serde_yaml::Value::from(1.5));
            },
        ];
        for change in changes {
            let tmp = tempfile::TempDir::new().unwrap();
            let notes_dir = tmp.path().join("notes");
            let mut store = Store::load_from(&notes_dir).unwrap();
            store.create_note("Kept", "body", "").unwrap();
            store.notes[0]
                .extra
                .insert("rating".into(), serde_yaml::Value::from(4.5));
            store.save().unwrap();
            let id = store.notes[0].id.clone();
            change(store.find_note_mut(&id).unwrap());
            store.save().unwrap();
            let saved = &Store::load_from(&notes_dir).unwrap().notes[0];
            let wanted = &store.notes[0];
            assert_eq!(
                (
                    &saved.title,
                    &saved.body,
                    saved.created_at,
                    saved.updated_at,
                    saved.pinned,
                    &saved.extra
                ),
                (
                    &wanted.title,
                    &wanted.body,
                    wanted.created_at,
                    wanted.updated_at,
                    wanted.pinned,
                    &wanted.extra
                )
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_restored_note_leaves_the_trash_even_when_its_save_failed_midway() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = temp_store();
        let b = store.create_note("B", "b", "sub").unwrap().id.clone();
        store.notes[0].created_at -= chrono::Duration::seconds(1);
        let a = store.create_note("A", "a", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&a));
        store.save().unwrap();
        assert_eq!(store.trashed().len(), 1);
        store.restore(&a).unwrap();
        store.find_note_mut(&b).unwrap().body = "b2".into();
        let sub = store.notes_dir.join("sub");
        let open = std::fs::metadata(&sub).unwrap().permissions();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = store.save();
        std::fs::set_permissions(&sub, open).unwrap();
        assert!(failed.is_err());
        store.save().unwrap();
        assert!(store.find_note(&a).is_some());
        assert!(store.trashed().is_empty());
    }

    fn on_disk_as_in_memory(store: &Store) {
        let key = |n: &Note| {
            (
                n.id.clone(),
                n.title.clone(),
                n.body.clone(),
                n.directory.clone(),
            )
        };
        let mut memory: Vec<_> = store.notes.iter().map(key).collect();
        let mut disk: Vec<_> = Store::load_from(&store.notes_dir)
            .unwrap()
            .notes
            .iter()
            .map(key)
            .collect();
        memory.sort();
        disk.sort();
        assert_eq!(disk, memory);
    }

    #[test]
    fn an_autosave_writes_a_note_changed_through_every_mutator() {
        let (mut store, _tmp) = temp_store();
        let ids = [("A", "- [ ] one", "f/g"), ("B", "b", "f"), ("C", "c", "")]
            .map(|(title, body, dir)| store.create_note(title, body, dir).unwrap().id.clone());
        assert!(store.create_dir("h"));
        store.save().unwrap();
        type Step = fn(&mut Store, &[String; 3]);
        let steps: [Step; 10] = [
            |s, ids| s.find_note_mut(&ids[0]).unwrap().body = "- [ ] edited".into(),
            |s, ids| assert!(s.move_note(&ids[1], "h").is_some()),
            |s, _| assert!(s.rename_dir("f", "renamed").is_some()),
            |s, _| assert!(s.move_dir("renamed", "h").is_some()),
            |s, _| {
                s.create_note("D", "d", "").unwrap();
            },
            |s, ids| assert_eq!(s.delete_notes(std::slice::from_ref(&ids[2])), 1),
            |s, ids| assert!(s.restore(&ids[2]).is_some()),
            |s, _| assert_eq!(s.delete_dir_recursive("h"), (2, 3)),
            |s, ids| assert_eq!(s.restore_many(&ids[..2]).titles.len(), 2),
            |s, _| {
                s.notes[0].body = "every note at once".into();
                s.touch_all();
            },
        ];
        for step in steps {
            step(&mut store, &ids);
            store.save().unwrap();
            on_disk_as_in_memory(&store);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "without passing a marking point")]
    fn a_change_made_without_marking_is_caught() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "body", "").unwrap();
        store.save().unwrap();
        store.notes[0].body = "unmarked".into();
        let _ = store.save();
    }

    #[test]
    fn the_hourly_save_writes_an_unmarked_change() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "body", "").unwrap();
        store.save().unwrap();
        store.notes[0].body = "unmarked".into();
        store.tidied.set(None);
        store.save().unwrap();
        on_disk_as_in_memory(&store);
    }

    #[test]
    fn a_new_note_does_not_put_off_the_hourly_check() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "body", "").unwrap();
        store.save().unwrap();
        let stamp = store.tidied.get();
        store.create_note("New", "body", "").unwrap();
        store.save().unwrap();
        assert_eq!(store.tidied.get(), stamp);
    }

    #[test]
    fn the_shortcut_targets_equal_a_full_name_assignment() {
        let (mut store, _tmp) = temp_store();
        let x = store.create_note("Same", "x", "f").unwrap().id.clone();
        store.find_note_mut(&x).unwrap().created_at -= chrono::Duration::seconds(5);
        let y = store.create_note("Same", "y", "f").unwrap().id.clone();
        store.save().unwrap();
        type Step = fn(&mut Store, &str, &str);
        let steps: [Step; 8] = [
            |s, x, _| s.find_note_mut(x).unwrap().title = "Other".into(),
            |s, _, y| s.find_note_mut(y).unwrap().body = "y2".into(),
            |s, _, y| assert!(s.move_note(y, "g").is_some()),
            |s, x, _| s.find_note_mut(x).unwrap().title = "Same".into(),
            |s, x, _| assert!(s.move_note(x, "g").is_some()),
            |s, x, _| s.find_note_mut(x).unwrap().body = "x2".into(),
            |s, _, y| s.find_note_mut(y).unwrap().title = "Other".into(),
            |s, _, y| s.find_note_mut(y).unwrap().body = "y3".into(),
        ];
        for step in steps {
            step(&mut store, &x, &y);
            store.save().unwrap();
            let full = store.assign_paths(&store.seen.borrow());
            store.save().unwrap();
            let seen = store.seen.borrow();
            for note in &store.notes {
                assert_eq!(
                    store.notes_dir.join(&seen[&note.id].path),
                    full[&note.id],
                    "{}",
                    note.title
                );
            }
        }
        assert_eq!(note_files(&store), ["g/Other.md", "g/Same.md"]);
    }

    #[test]
    fn saving_again_with_nothing_changed_rewrites_no_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        let mut store = Store::load_from(&notes_dir).unwrap();
        store.create_note("Kept", "body", "").unwrap();
        store.save().unwrap();
        let stamp = |name: &str| {
            std::fs::metadata(notes_dir.join(name))
                .unwrap()
                .modified()
                .unwrap()
        };
        set_mtime_to_the_past(&notes_dir.join("Kept.md"));
        set_mtime_to_the_past(&notes_dir.join("directories.json"));
        let (note, folders) = (stamp("Kept.md"), stamp("directories.json"));
        store.save().unwrap();
        assert_eq!(stamp("Kept.md"), note);
        assert_eq!(stamp("directories.json"), folders);
        let id = store.notes[0].id.clone();
        store.find_note_mut(&id).unwrap().body = "changed".into();
        store.save().unwrap();
        assert_ne!(stamp("Kept.md"), note);
        assert_eq!(stamp("directories.json"), folders);
        assert!(std::fs::read_to_string(notes_dir.join("Kept.md"))
            .unwrap()
            .ends_with("changed"));
    }

    #[test]
    fn test_save_writes_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let store = Store {
            unreadable: Vec::new(),
            seen: Default::default(),
            fingerprint: Default::default(),
            tidied: Default::default(),
            dirs_seen: Default::default(),
            dirs_base: Default::default(),
            dirty: Default::default(),
            removed: Default::default(),
            full: std::cell::Cell::new(true),
            placed: Default::default(),
            feed: Default::default(),
            trash: Default::default(),
            trash_stale: Default::default(),
            pictures: Default::default(),
            clock_sample: std::cell::Cell::new(std::time::Instant::now()),
            batching: Default::default(),
            holding: Default::default(),
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();
        assert!(notes_dir.join("Test Note.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_notes_root_symlink_is_refused_on_read_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let linked = dir.path().join("notes");
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        assert!(Store::load_from(&linked).is_err());
        let mut store = Store::load_from(&dir.path().join("real")).unwrap();
        store.notes_dir = linked.clone();
        assert!(store.save().is_err());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    /// An empty store in a temporary directory.
    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (store, dir)
    }

    #[test]
    fn a_stores_facts_name_unreadable_files_and_conflict_copies() {
        let (mut store, _d) = temp_store();
        store.create_note("A", "a", "").unwrap();
        store
            .create_note(format!("B{CONFLICT_SUFFIX}"), "b", "cs130")
            .unwrap();
        store.create_dir("cs130");
        store.save().unwrap();
        fs::write(
            store.notes_dir.join("broken.md"),
            "---\ntitle: [unclosed\n---\nx",
        )
        .unwrap();
        let store = Store::load_from(&store.notes_dir).unwrap();
        let facts = store.facts();
        assert_eq!(facts.notes, 2);
        assert_eq!(facts.directories, store.directories.len());
        assert!(facts.directories >= 1);
        assert_eq!(facts.unreadable.len(), 1);
        assert!(facts.unreadable[0].0.ends_with("broken.md"));
        assert_eq!(facts.conflict_copies, vec![format!("B{CONFLICT_SUFFIX}")]);
    }

    // ── find ────────────────────────────────────────────────────────────────

    fn titles(found: Vec<&Note>) -> Vec<String> {
        found.iter().map(|n| n.title.clone()).collect()
    }

    fn store_for_find() -> (Store, tempfile::TempDir) {
        let (mut store, d) = temp_store();
        store.create_dir("cs130");
        store
            .create_note("Rust ownership", "borrow checker rules", "")
            .unwrap();
        store
            .create_note("Graph traversals", "BFS explores level by level", "cs130")
            .unwrap();
        store
            .create_note("Lecture 4", "graphs: BFS, then DFS", "cs130")
            .unwrap();
        (store, d)
    }

    /// One search, everywhere: bodies and other directories included.
    #[test]
    fn find_looks_in_bodies_in_every_directory() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("borrow")), vec!["Rust ownership"]);
        let bfs = titles(store.find("bfs"));
        assert_eq!(bfs.len(), 2, "{bfs:?}");
    }

    #[test]
    fn a_title_match_ranks_ahead_of_a_body_match() {
        let (store, _d) = store_for_find();
        let found = titles(store.find("graph"));
        assert_eq!(found, vec!["Graph traversals", "Lecture 4"]);
    }

    #[test]
    fn every_word_has_to_match_somewhere() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("bfs dfs")), vec!["Lecture 4"]);
    }

    #[test]
    fn a_hash_word_is_an_ordinary_word() {
        let (store, _d) = temp_store();
        fs::write(
            store.notes_dir.join("Tagged.md"),
            "---\nid: tagged-1\ntitle: Tagged\ntags: [exam]\n---\n\nnothing to see\n",
        )
        .unwrap();
        fs::write(
            store.notes_dir.join("Revision.md"),
            "---\nid: revision-1\ntitle: Revision\n---\n\n#exam on Friday\n",
        )
        .unwrap();
        let store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(titles(store.find("#exam")), vec!["Revision"]);
        assert_eq!(titles(store.find("exam")), vec!["Revision"]);
        assert!(store.find("#exam borrow").is_empty());
    }

    /// Fuzzy title matching, which the Ctrl-P finder used to offer, still finds
    /// a note from a few letters of its title.
    #[test]
    fn a_few_letters_of_a_title_still_find_it() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("grtrv")), vec!["Graph traversals"]);
    }

    #[test]
    fn find_ignores_case() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("RUST")), vec!["Rust ownership"]);
    }

    #[test]
    fn deleting_notes_removes_only_the_chosen_ones() {
        let (mut store, _d) = temp_store();
        let a = store.create_note("A", "", "").unwrap().id.clone();
        let b = store.create_note("B", "", "").unwrap().id.clone();
        store.create_note("C", "", "").unwrap();
        assert_eq!(store.delete_notes(&["nope".to_string()]), 0);
        assert_eq!(store.delete_notes(&[a, b]), 2);
        assert_eq!(titles(store.notes.iter().collect()), vec!["C"]);
    }

    /// A file leo cannot read is skipped, never deleted: it is the user's text,
    /// probably with a header they edited by hand, and saving any other note
    /// used to erase it as an "orphan".
    #[test]
    fn a_note_leo_cannot_read_survives_a_save() {
        let (mut store, _d) = temp_store();
        store.create_note("Good", "fine", "").unwrap();
        store.save().unwrap();
        let broken = store.notes_dir.join("broken.md");
        std::fs::write(&broken, "---\ntitle: [unclosed\n---\nmy words").unwrap();

        let mut store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(store.notes.len(), 1);
        assert_eq!(
            store.unreadable.len(),
            1,
            "the skipped file is not reported"
        );
        assert!(store.unreadable[0].0.ends_with("broken.md"));

        store.create_note("Another", "x", "").unwrap();
        store.save().unwrap();
        assert!(broken.exists(), "saving deleted a note leo could not read");
        assert!(std::fs::read_to_string(&broken)
            .unwrap()
            .contains("my words"));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_listed_and_the_rest_still_load() {
        let (mut store, _d) = temp_store();
        store.create_note("Good", "fine", "").unwrap();
        store.save().unwrap();
        let binary = store.notes_dir.join("binary.md");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        #[cfg(unix)]
        let locked = store.notes_dir.join("locked");
        #[cfg(unix)]
        let blocked = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir(&locked).unwrap();
            std::fs::write(locked.join("Inside.md"), "x").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            std::fs::read_dir(&locked).is_err()
        };
        let loaded = Store::load_from(&store.notes_dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let loaded = loaded.unwrap();
        assert!(loaded.notes.iter().any(|n| n.title == "Good"));
        assert!(loaded.unreadable.iter().any(|(p, _)| p == &binary));
        #[cfg(unix)]
        if blocked {
            assert!(loaded.unreadable.iter().any(|(p, _)| p == &locked));
        }
        assert!(binary.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_does_not_hide_edits_in_other_folders() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _d) = temp_store();
        let locked = store.notes_dir.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let blocked = std::fs::read_dir(&locked).is_err();
        let folders: Vec<String> = (0..4).map(|i| format!("f{i}")).collect();
        for folder in &folders {
            store.create_note("Note", "x", folder).unwrap();
        }
        store.save().unwrap();
        let changed: Vec<bool> = folders
            .iter()
            .map(|folder| {
                let note = store.notes_dir.join(folder).join("Note.md");
                std::fs::write(note, "edited elsewhere").unwrap();
                let changed = store.changed_on_disk();
                store.fingerprint.set(fingerprint_of(&store.notes_dir));
                changed
            })
            .collect();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        if blocked {
            assert!(changed.iter().all(|c| *c), "{changed:?}");
        }
    }

    #[test]
    fn a_corrupt_folder_list_still_loads_the_notes() {
        let (mut store, _d) = temp_store();
        store.create_note("Deep", "x", "cs162").unwrap();
        store.save().unwrap();
        let corrupt = store.notes_dir.join("directories.json.corrupt");
        std::fs::write(&corrupt, "older").unwrap();
        let list = store.notes_dir.join("directories.json");
        std::fs::write(&list, "{not json").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&list)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
            .unwrap();
        let store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(store.notes.len(), 1);
        assert_eq!(store.directories, vec!["cs162"]);
        assert_eq!(std::fs::read_to_string(&corrupt).unwrap(), "{not json");
        let kept_at = std::fs::metadata(&corrupt).unwrap().modified().unwrap();
        assert!(kept_at.elapsed().is_ok_and(|age| age.as_secs() < 600));
    }

    #[cfg(unix)]
    #[test]
    fn a_note_whose_trash_move_failed_is_trashed_on_the_next_save() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _d) = temp_store();
        let id = store.create_note("Gone", "x", "").unwrap().id.clone();
        store.save().unwrap();
        let trash = store.notes_dir.join(TRASH);
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o500)).unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        let failed = store.save();
        std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed.is_err());
        store.save().unwrap();
        assert!(!store.notes_dir.join("Gone.md").exists());
        assert!(Store::load_from(&store.notes_dir)
            .unwrap()
            .find_note(&id)
            .is_none());
        assert_eq!(store.trashed().len(), 1);
    }

    /// A note's directory is shown even if the directory list on disk never
    /// heard of it — as after a backup brought in notes from another computer.
    #[test]
    fn directories_are_derived_from_where_notes_live() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_note("Deep", "x", "cs162/lec").unwrap();
        store.save().unwrap();
        std::fs::write(store.notes_dir.join("directories.json"), "[\"cs130\"]").unwrap();

        let store = Store::load_from(&store.notes_dir).unwrap();
        for dir in ["cs130", "cs162", "cs162/lec"] {
            assert!(
                store.dir_exists(dir),
                "{dir} is missing: {:?}",
                store.directories
            );
        }
    }

    fn trash_files(store: &Store) -> Vec<PathBuf> {
        let mut files = Vec::new();
        walk(
            &store.notes_dir.join(".trash"),
            0,
            false,
            &mut files,
            &mut Vec::new(),
        )
        .unwrap();
        files.into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn a_deleted_note_goes_to_the_trash() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture 4", "BFS", "cs130")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        assert_eq!(store.delete_notes(std::slice::from_ref(&id)), 1);
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded.notes.is_empty(), "the trash was loaded as notes");
        let trashed = reloaded.trashed();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].title, "Lecture 4");
        assert_eq!(trashed[0].directory, "cs130");
        assert_eq!(trashed[0].id, id);
    }

    #[test]
    fn a_moved_note_is_not_trashed() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("A", "a", "").unwrap().id.clone();
        store.save().unwrap();
        store.move_note(&id, "cs130").unwrap();
        store.save().unwrap();
        assert!(store.trashed().is_empty(), "{:?}", trash_files(&store));
    }

    #[test]
    fn a_restored_note_comes_back_where_it_was_and_leaves_the_trash() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture 4", "BFS", "cs130")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();

        assert_eq!(store.restore(&id).as_deref(), Some("Lecture 4"));
        store.save().unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let note = reloaded.find_note(&id).expect("not restored");
        assert_eq!(note.directory, "cs130");
        assert_eq!(note.body, "BFS");
        assert!(reloaded.trashed().is_empty(), "still in the trash");
        assert!(reloaded.directories.contains(&"cs130".to_string()));
    }

    #[test]
    fn deleting_a_directory_trashes_its_notes() {
        let (mut store, _tmp) = temp_store();
        store.create_note("One", "", "cs130/lec").unwrap();
        store.create_note("Two", "", "cs130").unwrap();
        store.save().unwrap();
        store.delete_dir_recursive("cs130");
        store.save().unwrap();
        let mut places: Vec<String> = store.trashed().into_iter().map(|t| t.directory).collect();
        places.sort();
        assert_eq!(places, ["cs130", "cs130/lec"]);
    }

    #[test]
    fn emptying_the_trash_deletes_for_good() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("A", "a", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(store.empty_trash().deleted, 1);
        assert!(store.trashed().is_empty());
        assert!(trash_files(&store).is_empty());
    }

    #[test]
    fn chosen_notes_leave_the_trash_for_good_and_the_rest_stay() {
        let (mut store, _tmp) = temp_store();
        let ids: Vec<String> = ["A", "B", "C"]
            .iter()
            .map(|t| store.create_note(*t, "x", "").unwrap().id.clone())
            .collect();
        store.save().unwrap();
        for id in &ids {
            store.delete_notes(std::slice::from_ref(id));
        }
        store.save().unwrap();
        let chosen = vec![ids[0].clone(), ids[2].clone(), "not-in-trash".to_string()];
        assert_eq!(store.delete_from_trash(&chosen).deleted, 2);
        let left: Vec<String> = store.trashed().into_iter().map(|t| t.title).collect();
        assert_eq!(left, ["B"]);
        assert_eq!(trash_files(&store).len(), 1);
    }

    fn dirs_under(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    out.push(
                        entry
                            .path()
                            .strip_prefix(root)
                            .unwrap()
                            .display()
                            .to_string(),
                    );
                    stack.push(entry.path());
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn p02_emptying_the_trash_leaves_no_empty_folders() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Deep", "x", "a/b").unwrap();
        store.create_note("Mid", "x", "a").unwrap();
        store.save().unwrap();
        store.delete_dir_recursive("a");
        store.save().unwrap();
        let trash = store.notes_dir.join(".trash");
        assert_eq!(store.empty_trash().deleted, 2);
        let left = dirs_under(&trash);
        assert!(left.is_empty(), "folders left under .trash: {left:?}");
    }

    #[test]
    fn p02_an_unparseable_trashed_file_is_shown_or_expired() {
        let (store, _tmp) = temp_store();
        let trash = store.notes_dir.join(".trash");
        fs::create_dir_all(&trash).unwrap();
        let broken = trash.join("Broken.md");
        fs::write(&broken, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
        let long_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(400 * 24 * 3600);
        fs::File::options()
            .write(true)
            .open(&broken)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        reloaded.tidy_trash_now();
        let listed = reloaded.trashed().len();
        reloaded.empty_trash();
        assert!(
            listed == 1 || !broken.exists(),
            "a 400-day-old unparseable trashed file is neither listed nor expired nor emptied"
        );
    }

    #[test]
    fn emptying_the_trash_counts_a_file_leo_cannot_read() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "x", "").unwrap();
        store.save().unwrap();
        let id = store.notes[0].id.clone();
        store.delete_notes(&[id]);
        store.save().unwrap();
        let trash = store.notes_dir.join(".trash");
        fs::write(trash.join("Broken.md"), "---\ntitle: [\n---\nbody").unwrap();
        assert_eq!(
            trash_entries(&store.notes_dir)
                .iter()
                .filter(|entry| entry.head.is_none())
                .count(),
            1
        );
        let result = store.empty_trash();
        assert_eq!(result.deleted, 2);
        assert_eq!(result.failed, 0);
        assert!(trash_files(&store).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn p02_one_failed_delete_does_not_stop_emptying_the_trash() {
        use std::os::unix::fs::PermissionsExt;
        let dirs = ["a", "b", "c", "d", "e", "f"];
        let (mut store, _tmp) = temp_store();
        for dir in dirs {
            for n in 0..3 {
                store.create_note(format!("{dir}{n}"), "x", dir).unwrap();
            }
        }
        store.save().unwrap();
        for dir in dirs {
            store.delete_dir_recursive(dir);
        }
        store.save().unwrap();
        let trash = store.notes_dir.join(".trash");
        let locked: Vec<PathBuf> = ["b", "d", "f"].iter().map(|d| trash.join(d)).collect();
        for dir in &locked {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
        }
        let result = store.empty_trash();
        let left: Vec<PathBuf> = trash_files(&store)
            .into_iter()
            .filter(|p| !locked.iter().any(|l| p.starts_with(l)))
            .collect();
        for dir in &locked {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(result.failed, 9);
        assert_eq!(result.deleted, 9);
        assert!(left.is_empty(), "{} deletable files left", left.len());
    }

    fn trashed_file(store: &Store, relative: &str, text: &str) -> PathBuf {
        let path = store.notes_dir.join(".trash").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        store.trash_may_have_changed();
        path
    }

    #[test]
    fn an_unreadable_trashed_file_is_listed_by_its_file_name() {
        let (store, _tmp) = temp_store();
        trashed_file(&store, "cs130/week 1/Broken.md", "---\ntitle: [\n---\nx");
        let listed = store.trashed();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (
                listed[0].title.as_str(),
                listed[0].directory.as_str(),
                listed[0].readable
            ),
            ("Broken", "cs130/week 1", false)
        );
        assert_eq!(
            listed[0].id,
            stable_id(Path::new(".trash/cs130/week 1/Broken.md"))
        );
    }

    #[test]
    fn an_unreadable_trashed_file_can_be_deleted_for_good_by_its_id() {
        let (store, _tmp) = temp_store();
        let path = trashed_file(&store, "Broken.md", "---\ntitle: [\n---\nx");
        trashed_file(&store, "Other.md", "---\ntitle: [\n---\ny");
        let id = store
            .trashed()
            .into_iter()
            .find(|t| t.title == "Broken")
            .unwrap()
            .id;
        let emptied = store.delete_from_trash(&[id]);
        assert_eq!((emptied.deleted, emptied.failed), (1, 0));
        assert!(emptied.forgotten.is_empty());
        assert!(!path.exists());
        assert_eq!(store.trashed().len(), 1);
    }

    #[test]
    fn restoring_an_unreadable_trashed_file_restores_nothing() {
        let (mut store, _tmp) = temp_store();
        let path = trashed_file(&store, "Broken.md", "---\ntitle: [\n---\nx");
        let id = store.trashed()[0].id.clone();
        let restored = store.restore_many(&[id.clone(), id]);
        assert_eq!(
            restored,
            Restored {
                titles: vec![],
                unreadable: 1
            }
        );
        assert!(store.notes.is_empty());
        assert!(path.exists());
    }

    #[test]
    fn a_trashed_file_leo_cannot_read_has_its_own_id() {
        let (store, _tmp) = temp_store();
        fs::create_dir_all(store.notes_dir.join("a")).unwrap();
        fs::write(store.notes_dir.join("a/B.md"), "plain words").unwrap();
        trashed_file(&store, "a/B.md", "---\ntitle: [unclosed\n---\nbody\n");
        let store = Store::load_from(&store.notes_dir).unwrap();
        let live = &store.notes[0].id;
        let entries = trash_entries(&store.notes_dir);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].head.is_none());
        assert_ne!(&entries[0].id(), live);
    }

    #[test]
    fn restore_reads_only_the_chosen_files() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Keep", "x", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let path = store.notes_dir.join(".trash").join("Bad.md");
        let mut bytes =
            format!("---\nid: {}\ntitle: Bad\n---\n", uuid::Uuid::new_v4()).into_bytes();
        bytes.extend([0xff, 0xfe, 0xfd]);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            store.restore_many(std::slice::from_ref(&id)).titles,
            ["Keep"]
        );
        assert!(store.find_note(&id).is_some());
    }

    #[test]
    fn deleting_a_deep_note_for_good_leaves_no_folder_in_the_trash() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Deep", "x", "a/b").unwrap().id.clone();
        store.create_note("Top", "x", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let trash = store.notes_dir.join(".trash");
        assert!(trash.join("a/b").is_dir());
        assert_eq!(
            store.delete_from_trash(std::slice::from_ref(&id)).deleted,
            1
        );
        assert!(!trash.join("a").exists(), "{:?}", dirs_under(&trash));
    }

    #[test]
    fn an_expired_note_takes_its_folder_out_of_the_trash() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Deep", "x", "a/b").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let trash = store.notes_dir.join(".trash");
        let_days_pass(&store, 40);
        assert_eq!(store.tidy_trash_now(), [id]);
        assert!(!trash.join("a").exists(), "{:?}", dirs_under(&trash));
    }

    #[test]
    fn restoring_many_brings_back_the_trashed_ones_and_skips_the_rest() {
        let (mut store, _tmp) = temp_store();
        let ids: Vec<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|t| store.create_note(*t, "x", "x/y").unwrap().id.clone())
            .collect();
        store.save().unwrap();
        store.delete_notes(&ids[..3]);
        store.save().unwrap();
        let asked = vec![
            ids[2].clone(),
            ids[3].clone(),
            ids[0].clone(),
            "unknown".to_string(),
            ids[1].clone(),
            ids[0].clone(),
        ];
        assert_eq!(store.restore_many(&asked).titles, ["C", "A", "B"]);
        assert_eq!(store.notes.len(), 4);
        assert!(store.directories.contains(&"x/y".to_string()));
    }

    #[test]
    fn deleting_a_trashed_copy_of_a_live_note_forgets_nothing() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Live", "x", "").unwrap().id.clone();
        store.save().unwrap();
        trashed_file(
            &store,
            "Live.md",
            &note_to_markdown(&store.notes[0]).unwrap(),
        );
        let emptied = store.delete_from_trash(std::slice::from_ref(&id));
        assert_eq!(emptied.deleted, 1);
        assert!(emptied.forgotten.is_empty(), "{:?}", emptied.forgotten);
    }

    #[test]
    fn the_trash_forgets_notes_after_thirty_days() {
        let (mut store, _tmp) = temp_store();
        let old = store.create_note("Old", "", "").unwrap().id.clone();
        let new = store.create_note("New", "", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&old));
        store.save().unwrap();
        let_days_pass(&store, 31);
        store.delete_notes(std::slice::from_ref(&new));
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(
            reloaded.trashed().len(),
            2,
            "a load never expires the trash"
        );
        assert_eq!(reloaded.tidy_trash_now(), [old]);
        let left: Vec<String> = reloaded.trashed().into_iter().map(|t| t.id).collect();
        assert_eq!(left, vec![new]);
    }

    #[test]
    fn the_trash_follows_how_long_it_was_asked_to_keep_notes() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        fn aged(store: &Store, days: i64) {
            let_days_pass(store, days);
        }
        let dir = store.notes_dir.clone();
        let keep = |days| {
            crate::keep::save(
                &dir,
                &crate::keep::Keep {
                    trash_days: days,
                    chat_days: None,
                    ..Default::default()
                },
            )
            .unwrap()
        };

        keep(None);
        aged(&store, 400);
        store.tidy_trash_now();
        assert_eq!(store.trashed().len(), 1, "kept forever");

        keep(Some(365));
        store.tidy_trash_now();
        assert!(store.trashed().is_empty(), "a year is up");

        let id = store.create_note("Recent", "", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        keep(Some(7));
        aged(&store, 5);
        store.tidy_trash_now();
        assert_eq!(store.trashed().len(), 1, "inside a week");
        aged(&store, 3);
        store.tidy_trash_now();
        assert!(store.trashed().is_empty());
    }

    #[test]
    fn restoring_a_note_that_is_not_in_the_trash_does_nothing() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("A", "a", "").unwrap().id.clone();
        store.save().unwrap();
        assert_eq!(store.restore(&id), None);
        assert_eq!(store.restore("nope"), None);
        assert_eq!(store.notes.len(), 1);
    }

    #[test]
    fn pinned_notes_come_first_and_stay_pinned_on_disk() {
        let (mut store, _tmp) = temp_store();
        let old = store
            .create_note("Syllabus", "", "cs130")
            .unwrap()
            .id
            .clone();
        store.create_note("Lecture 1", "", "cs130").unwrap();
        store.find_note_mut(&old).unwrap().updated_at -= chrono::Duration::days(30);
        store.find_note_mut(&old).unwrap().pinned = true;
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let titles: Vec<&str> = reloaded
            .list_notes(Some("cs130"))
            .iter()
            .map(|n| n.title.as_str())
            .collect();
        assert_eq!(titles, ["Syllabus", "Lecture 1"]);
        assert!(reloaded.find_note(&old).unwrap().pinned);
    }

    #[test]
    fn an_unpinned_note_has_no_pinned_line() {
        let text = note_to_markdown(&make_note()).unwrap();
        assert!(!text.contains("pinned"), "{text}");
        let mut pinned = make_note();
        pinned.pinned = true;
        assert!(note_to_markdown(&pinned).unwrap().contains("pinned: true"));
    }

    #[test]
    fn a_save_leaves_an_unread_outside_edit_visible() {
        let (mut store, _tmp) = temp_store();
        let a = store.create_note("A", "one", "").unwrap().id.clone();
        store.create_note("B", "two", "").unwrap();
        store.save().unwrap();
        let b = store.notes_dir.join("B.md");
        fs::write(
            &b,
            fs::read_to_string(&b)
                .unwrap()
                .replace("two", "two, edited elsewhere"),
        )
        .unwrap();
        store.find_note_mut(&a).unwrap().body = "one, edited here".to_string();
        store.save().unwrap();
        assert!(store.changed_on_disk());
    }

    #[test]
    fn a_save_that_writes_moves_and_trashes_notes_records_what_it_wrote() {
        let (mut store, _tmp) = temp_store();
        let edited = store.create_note("Edited", "x", "").unwrap().id.clone();
        let renamed = store.create_note("Renamed", "x", "").unwrap().id.clone();
        let moved = store.create_note("Moved", "x", "").unwrap().id.clone();
        let trashed = store.create_note("Trashed", "x", "").unwrap().id.clone();
        store.save().unwrap();
        assert!(!store.changed_on_disk());
        store.find_note_mut(&edited).unwrap().body = "y".to_string();
        store.find_note_mut(&renamed).unwrap().title = "Renamed again".to_string();
        store.create_dir("Inner");
        store.move_note(&moved, "Inner").unwrap();
        store.delete_notes(std::slice::from_ref(&trashed));
        store.create_note("Fresh", "x", "Inner").unwrap();
        store.save().unwrap();
        assert!(!store.changed_on_disk(), "{:?}", note_files(&store));
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 4);
    }

    #[test]
    fn a_title_changed_only_in_case_renames_its_file() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("apple", "x", "").unwrap().id.clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().title = "Apple".to_string();
        store.save().unwrap();
        assert_eq!(note_files(&store), vec!["Apple.md".to_string()]);
        assert!(!store.changed_on_disk());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().title, "Apple");
    }

    #[test]
    fn a_save_that_rewrites_only_the_folder_list_records_it() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Note", "x", "").unwrap();
        store.save().unwrap();
        store.create_dir("Empty");
        store.save().unwrap();
        assert!(!store.changed_on_disk());
        store.create_dir("Another");
        store.save().unwrap();
        assert!(!store.changed_on_disk());
    }

    fn folder_list(store: &Store) -> Vec<String> {
        serde_json::from_str(&fs::read_to_string(store.notes_dir.join("directories.json")).unwrap())
            .unwrap()
    }

    fn change_folder_list(store: &Store, add: &str, remove: &str) {
        let mut list = folder_list(store);
        list.retain(|dir| dir != remove);
        if !add.is_empty() {
            list.push(add.to_string());
        }
        fs::write(
            store.notes_dir.join("directories.json"),
            serde_json::to_string(&list).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn an_unread_outside_edit_to_the_folder_list_survives_a_save() {
        for outside_first in [true, false] {
            let (mut store, _tmp) = temp_store();
            store.create_dir("Kept");
            store.save().unwrap();
            if outside_first {
                change_folder_list(&store, "Outside", "");
                store.create_dir("Inside");
            } else {
                store.create_dir("Inside");
                change_folder_list(&store, "Outside", "");
            }
            store.save().unwrap();
            let list = folder_list(&store);
            for dir in ["Kept", "Outside", "Inside"] {
                assert!(list.contains(&dir.to_string()), "{outside_first} {list:?}");
            }
            assert!(store.changed_on_disk());
            store.refresh().unwrap();
            assert!(store.dir_exists("Outside") && store.dir_exists("Inside"));

            let (mut store, _tmp) = temp_store();
            for dir in ["Kept", "Theirs", "Mine"] {
                store.create_dir(dir);
            }
            store.save().unwrap();
            if outside_first {
                change_folder_list(&store, "", "Theirs");
                store.delete_dir_recursive("Mine");
            } else {
                store.delete_dir_recursive("Mine");
                change_folder_list(&store, "", "Theirs");
            }
            store.save().unwrap();
            assert_eq!(
                folder_list(&store),
                vec!["Kept".to_string()],
                "{outside_first}"
            );
            assert!(store.changed_on_disk());
            store.refresh().unwrap();
            assert!(!store.dir_exists("Theirs") && !store.dir_exists("Mine"));
            assert!(store.dir_exists("Kept"));
        }
    }

    #[test]
    fn a_save_leaves_notes_another_program_added_alone() {
        let (mut app, _tmp) = temp_store();
        let mine = app.create_note("Mine", "", "").unwrap().id.clone();
        app.save().unwrap();

        let mut phone = Store::load_from(&app.notes_dir).unwrap();
        let theirs = phone
            .create_note("From the phone", "", "")
            .unwrap()
            .id
            .clone();
        phone.save().unwrap();

        app.delete_notes(std::slice::from_ref(&mine));
        app.save().unwrap();

        let now = Store::load_from(&app.notes_dir).unwrap();
        assert!(
            now.find_note(&theirs).is_some(),
            "the other program's note was removed"
        );
        let trashed: Vec<String> = now.trashed().into_iter().map(|t| t.id).collect();
        assert_eq!(trashed, vec![mine], "trashed a note it never had");
    }

    fn set_mtime_to_the_past(path: &Path) {
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
    }

    #[test]
    fn saving_rewrites_only_the_notes_that_changed() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "same", "").unwrap();
        let edited = store
            .create_note("Edited", "before", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let dir = store.notes_dir.clone();
        let file_of = |title: &str| dir.join(format!("{title}.md"));
        set_mtime_to_the_past(&file_of("Kept"));
        set_mtime_to_the_past(&file_of("Edited"));
        let modified = |title: &str| fs::metadata(file_of(title)).unwrap().modified().unwrap();
        let kept_time = modified("Kept");
        let edited_time = modified("Edited");

        store.find_note_mut(&edited).unwrap().body = "after".to_string();
        store.save().unwrap();

        assert_eq!(
            modified("Kept"),
            kept_time,
            "an unchanged note was rewritten"
        );
        assert!(
            modified("Edited") > edited_time,
            "an edited note was not written"
        );
        assert!(fs::read_to_string(file_of("Edited"))
            .unwrap()
            .contains("after"));
    }

    #[test]
    fn saving_one_note_leaves_another_programs_edit_to_a_different_note_alone() {
        let (mut store, _tmp) = temp_store();
        let mine = store.create_note("Mine", "a", "").unwrap().id.clone();
        store.create_note("Theirs", "b", "").unwrap();
        store.save().unwrap();
        let theirs_file = store.notes_dir.join("Theirs.md");
        let edited = fs::read_to_string(&theirs_file)
            .unwrap()
            .replace("\n\nb", "\n\nb edited elsewhere");
        fs::write(&theirs_file, &edited).unwrap();

        store.find_note_mut(&mine).unwrap().body = "a2".to_string();
        store.save().unwrap();

        assert_eq!(fs::read_to_string(&theirs_file).unwrap(), edited);
    }

    #[test]
    fn deleting_a_note_that_was_edited_elsewhere_keeps_the_edit() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "one", "").unwrap().id.clone();
        store.save().unwrap();
        let file = store.notes_dir.join("Shared.md");
        fs::write(
            &file,
            fs::read_to_string(&file)
                .unwrap()
                .replace("one", "one and more"),
        )
        .unwrap();

        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();

        assert!(file.exists(), "the other program's edit was moved away");
        assert!(store.trashed().is_empty());
    }

    #[test]
    fn test_save_handles_directory_move() {
        let (mut store, _tmp) = temp_store();
        store.create_dir("ideas");
        let id = store.create_note("Moving", "body", "").unwrap().id.clone();
        store.save().unwrap();
        let root_file = store.notes_dir.join("Moving.md");
        assert!(root_file.exists());

        store.move_note(&id, "ideas").unwrap();
        store.save().unwrap();

        assert!(store.notes_dir.join("ideas/Moving.md").exists());
        assert!(
            !root_file.exists(),
            "old location should be removed after move"
        );
    }

    #[test]
    fn test_save_trashes_a_note_it_no_longer_has() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "body", "").unwrap().id.clone();
        store.save().unwrap();
        let file = store.notes_dir.join("Old.md");

        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();

        assert!(!file.exists(), "the deleted note's file should be gone");
        assert_eq!(store.trashed().len(), 1, "and kept in the trash");
    }

    fn note_files(store: &Store) -> Vec<String> {
        let mut paths = Vec::new();
        walk(&store.notes_dir, 0, false, &mut paths, &mut Vec::new()).unwrap();
        let mut names: Vec<String> = paths
            .iter()
            .map(|(p, _)| {
                p.strip_prefix(&store.notes_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn notes_are_saved_under_their_titles() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Lecture 4", "BFS", "cs130").unwrap();
        store.create_note("Reading list", "", "").unwrap();
        store.save().unwrap();
        assert_eq!(
            note_files(&store),
            ["Reading list.md", "cs130/Lecture 4.md"]
        );
    }

    #[test]
    fn notes_with_the_same_title_are_numbered_oldest_first() {
        let (mut store, _tmp) = temp_store();
        let older = store.create_note("Same", "first", "").unwrap().id.clone();
        let newer = store.create_note("Same", "second", "").unwrap().id.clone();
        store.find_note_mut(&newer).unwrap().created_at += chrono::Duration::seconds(5);
        store.save().unwrap();

        assert_eq!(note_files(&store), ["Same (2).md", "Same.md"]);
        let first = fs::read_to_string(store.notes_dir.join("Same.md")).unwrap();
        let second = fs::read_to_string(store.notes_dir.join("Same (2).md")).unwrap();
        assert!(first.contains(&format!("id: {older}")), "{first}");
        assert!(second.contains(&format!("id: {newer}")), "{second}");
    }

    #[test]
    fn saving_again_changes_no_file_name() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Same", "a", "").unwrap();
        let newer = store.create_note("Same", "b", "").unwrap().id.clone();
        store.find_note_mut(&newer).unwrap().created_at += chrono::Duration::seconds(5);
        store.save().unwrap();
        let first = note_files(&store);
        store.save().unwrap();
        assert_eq!(first, note_files(&store));
    }

    #[test]
    fn a_file_another_program_put_in_the_way_is_never_overwritten() {
        let (mut store, _tmp) = temp_store();
        fs::write(
            store.notes_dir.join("Taken.md"),
            "written by another program",
        )
        .unwrap();
        store.create_note("Taken", "mine", "").unwrap();
        store.save().unwrap();
        assert_eq!(
            fs::read_to_string(store.notes_dir.join("Taken.md")).unwrap(),
            "written by another program"
        );
        assert!(store.notes_dir.join("Taken (2).md").exists());
    }

    #[test]
    fn renaming_a_note_moves_its_file() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old title", "x", "").unwrap().id.clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().title = "New title".to_string();
        store.save().unwrap();
        assert_eq!(note_files(&store), ["New title.md"]);
    }

    #[test]
    fn a_rename_that_only_changes_case_leaves_one_file() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("lower", "x", "").unwrap().id.clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().title = "Lower".to_string();
        store.save().unwrap();
        assert_eq!(note_files(&store).len(), 1, "{:?}", note_files(&store));
        assert!(Store::load_from(&store.notes_dir)
            .unwrap()
            .find_note(&id)
            .is_some());
    }

    #[test]
    fn titles_that_differ_only_by_case_get_separate_files() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Notes", "a", "").unwrap();
        store.create_note("notes", "b", "").unwrap();
        store.save().unwrap();
        assert_eq!(note_files(&store).len(), 2, "{:?}", note_files(&store));
        assert_eq!(Store::load_from(&store.notes_dir).unwrap().notes.len(), 2);
    }

    #[test]
    fn old_id_named_files_are_renamed_to_titles_on_the_next_save() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        fs::create_dir_all(dir.join("cs130")).unwrap();
        let root = Note::new("Graphs", "BFS", "");
        let nested = Note::new("Lecture", "DFS", "cs130");
        fs::write(
            dir.join(format!("{}.md", root.id)),
            note_to_markdown(&root).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("cs130").join(format!("{}.md", nested.id)),
            note_to_markdown(&nested).unwrap(),
        )
        .unwrap();

        let store = Store::load_from(&dir).unwrap();
        store.save().unwrap();

        assert_eq!(note_files(&store), ["Graphs.md", "cs130/Lecture.md"]);
        let reloaded = Store::load_from(&dir).unwrap();
        assert_eq!(reloaded.find_note(&root.id).unwrap().body, "BFS");
        assert_eq!(reloaded.find_note(&nested.id).unwrap().directory, "cs130");
    }

    #[test]
    fn a_second_deleted_note_with_the_same_name_does_not_replace_the_first_in_the_trash() {
        let (mut store, _tmp) = temp_store();
        let first = store.create_note("Draft", "one", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&first));
        store.save().unwrap();
        let second = store.create_note("Draft", "two", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&second));
        store.save().unwrap();

        let ids: HashSet<String> = store.trashed().into_iter().map(|t| t.id).collect();
        assert_eq!(ids, HashSet::from([first, second]));
    }

    fn parse(name: &str, text: &str) -> Note {
        parse_note_from_markdown(text, Path::new(name), chrono::Utc::now()).unwrap()
    }

    #[test]
    fn a_file_with_no_header_is_a_note_named_after_the_file() {
        let note = parse(
            "cs130/Office hours.md",
            "Tuesday 3-5pm\n\n- [ ] bring questions\n",
        );
        assert_eq!(note.title, "Office hours");
        assert_eq!(note.directory, "cs130");
        assert_eq!(note.body, "Tuesday 3-5pm\n\n- [ ] bring questions\n");
        assert!(note.extra.is_empty());
    }

    #[test]
    fn a_headerless_note_keeps_the_same_id_on_every_computer() {
        assert_eq!(
            parse("Office hours.md", "x").id,
            "dd6664c0-1761-8255-a94a-68bb2041c68e"
        );
        assert_eq!(
            parse("cs130/Office hours.md", "x").id,
            "2fc81c8a-8874-8e34-9b89-2740ad5e0b53"
        );
        assert_eq!(
            parse("Office hours.md", "x").id,
            parse("Office hours.md", "y").id
        );
        assert_ne!(
            parse("Syllabus.md", "x").id,
            parse("Office hours.md", "x").id
        );
    }

    #[test]
    fn obsidian_properties_become_leo_fields() {
        let text = "---\ntags:\n  - '#exam'\n  - graphs\npinned: true\n---\nBody\n";
        let note = parse("Notes.md", text);
        assert_eq!(
            note.extra["tags"],
            serde_yaml::from_str::<serde_yaml::Value>("['#exam', graphs]").unwrap()
        );
        assert!(note.pinned);
        assert_eq!(note.body, "Body\n");
    }

    #[test]
    fn tags_written_as_one_string_are_kept_as_written() {
        let note = parse("A.md", "---\ntags: \"exam, graphs #bfs\"\n---\nx");
        assert_eq!(note.extra["tags"], "exam, graphs #bfs");
    }

    #[test]
    fn a_header_missing_leos_fields_still_loads() {
        let note = parse("Real title.md", "---\ntitle: Real title\n---\nx");
        assert_eq!(note.title, "Real title");
        assert_eq!(
            parse("Notes.md", "---\ntitle: Real title\n---\nx").title,
            "Notes",
            "Obsidian shows a note by its file name"
        );
    }

    #[test]
    fn empty_properties_and_a_header_with_no_body_load() {
        assert_eq!(parse("A.md", "---\n---\nbody").body, "body");
        assert_eq!(parse("B.md", "---\ntitle: T\n---").body, "");
    }

    #[test]
    fn windows_line_endings_and_a_byte_order_mark_are_handled() {
        let text = "\u{feff}---\r\nid: abc\r\ntitle: Hello\r\n---\r\n\r\nline one\r\nline two\r\n";
        let note = parse("Hello.md", text);
        assert_eq!(note.id, "abc");
        assert_eq!(note.title, "Hello");
        assert_eq!(note.body, "line one\nline two\n");
    }

    #[test]
    fn a_leading_rule_with_no_closing_one_is_just_text() {
        let note = parse("Rule.md", "---\nnot a header\n");
        assert_eq!(note.body, "---\nnot a header\n");
    }

    #[test]
    fn a_header_that_is_not_yaml_is_still_unreadable() {
        let result = parse_note_from_markdown(
            "---\ntitle: [unclosed\n---\nx",
            Path::new("A.md"),
            chrono::Utc::now(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn the_header_reader_agrees_with_the_note_parser() {
        let tmp = tempfile::tempdir().unwrap();
        let big = "x".repeat(70 * 1024);
        let near = format!(
            "---\nid: abc\ntitle: {}é\n---\nbody",
            "t".repeat(8 * 1024 - 20)
        );
        let mut shapes: Vec<(&str, String)> = vec![
            ("cs130/Office hours.md", "Tuesday 3-5pm\n".into()),
            (
                "Notes.md",
                "---\ntags:\n  - '#exam'\n  - graphs\npinned: true\n---\nBody\n".into(),
            ),
            ("A.md", "---\ntags: \"exam, graphs #bfs\"\n---\nx".into()),
            ("Real title.md", "---\ntitle: Real title\n---\nx".into()),
            ("Notes.md", "---\ntitle: Real title\n---\nx".into()),
            ("A.md", "---\n---\nbody".into()),
            ("B.md", "---\ntitle: T\n---".into()),
            (
                "Hello.md",
                "\u{feff}---\r\nid: abc\r\ntitle: Hello\r\n---\r\n\r\nline one\r\n".into(),
            ),
            ("Hello.md", "\u{feff}plain".into()),
            ("Rule.md", "---\nnot a header\n".into()),
            ("A.md", "---\ntitle: [unclosed\n---\nx".into()),
            ("A.md", "---\njust words\n---\nx".into()),
            ("A.md", "---\n- a\n- b\n---\nx".into()),
            ("A.md", "---\nid: 42\ntitle: A\n---\nx".into()),
            ("Renamed.md", "---\nid: abc\ntitle: Old name\n---\nx".into()),
            (
                "d/e/5d8f8c3e-9a4b-4c1e-8f7a-2b3c4d5e6f70.md",
                "---\ntitle: Old style\n---\nx".into(),
            ),
            ("A.md", "---\n---\n".into()),
            ("A.md", "---\n---".into()),
            ("A.md", "---\ntitle: A\n---\n---\nmore\n---\n".into()),
            ("A.md", "---\r\ntitle: A\r\n---\r\nbody".into()),
            ("A.md", "---\r\n---\r\nbody".into()),
            ("A.md", "---\ntitle: A\r\r\n---\r\r\nbody\n---\n".into()),
            ("A.md", "--".into()),
            ("A.md", "".into()),
            ("A.md", "-- -\n---\n".into()),
            ("A.md", format!("---\ntitle: A\nlong: {big}\n---\nbody")),
            (
                "A.md",
                format!("---\ntitle: A\nlong: {}\n---\nbody", "y".repeat(9 * 1024)),
            ),
            ("A.md", format!("---\ntitle: A\nlong: {big}\n")),
            ("A.md", near),
        ];
        for n in 8 * 1024 - 8..8 * 1024 + 8 {
            shapes.push((
                "A.md",
                format!("---\ntitle: A\nlong: {}\n---\nbody", "z".repeat(n - 21)),
            ));
            shapes.push((
                "A.md",
                format!(
                    "---\r\ntitle: A\r\nlong: {}\r\n---\r\nbody",
                    "z".repeat(n - 24)
                ),
            ));
        }
        for (n, (relative, text)) in shapes.iter().enumerate() {
            let path = tmp.path().join(n.to_string()).join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            let parsed = parse_note_from_markdown(text, Path::new(relative), Utc::now())
                .ok()
                .map(|note| (note.id, note.title, note.directory));
            let read = read_head(&path, Path::new(relative))
                .map(|head| (head.id, head.title, head.directory));
            let shown: String = text.chars().take(60).collect();
            assert_eq!(read, parsed, "{relative} {shown:?}");
        }
    }

    #[test]
    fn a_trashed_note_is_listed_without_reading_its_body() {
        let (mut store, _tmp) = temp_store();
        let id = uuid::Uuid::new_v4().to_string();
        let path = store.notes_dir.join(".trash").join("Big.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = format!("---\nid: {id}\ntitle: Big\n---\n").into_bytes();
        bytes.extend([0xff, 0xfe]);
        bytes.extend(std::iter::repeat_n(b'w', 20 * 1024 * 1024));
        fs::write(&path, bytes).unwrap();
        let head = read_head(&path, Path::new("Big.md")).unwrap();
        assert_eq!(
            (head.id.as_str(), head.title.as_str()),
            (id.as_str(), "Big")
        );
        store.trash_may_have_changed();
        assert_eq!(store.trashed().len(), 1);
        let restored = store.restore_many(std::slice::from_ref(&id));
        assert!(restored.titles.is_empty());
        assert_eq!(restored.unreadable, 1);
        assert!(store.notes.is_empty());
    }

    fn read_in_full(store: &Store) -> Vec<Trashed> {
        let live: HashSet<&str> = store.notes.iter().map(|n| n.id.as_str()).collect();
        let mut out: Vec<Trashed> = trash_entries(&store.notes_dir)
            .into_iter()
            .filter_map(|entry| match &entry.head {
                None => {
                    let place = head_of(&mut serde_yaml::Mapping::new(), &entry.relative);
                    Some(Trashed {
                        id: entry.id(),
                        title: place.title,
                        directory: place.directory,
                        deleted_at: entry.deleted_at,
                        readable: false,
                        revision: crate::paths::digest_file(&entry.path, MOST_TRASH_NOTE_BYTES)
                            .ok(),
                    })
                }
                Some(head) => (!live.contains(head.id.as_str())).then(|| Trashed {
                    id: head.id.clone(),
                    title: head.title.clone(),
                    directory: head.directory.clone(),
                    deleted_at: entry.deleted_at,
                    readable: true,
                    revision: crate::paths::digest_file(&entry.path, MOST_TRASH_NOTE_BYTES).ok(),
                }),
            })
            .collect();
        out.sort_by(|a, b| (a.deleted_at, &a.id, &a.title).cmp(&(b.deleted_at, &b.id, &b.title)));
        out
    }

    fn listed(store: &Store) -> Vec<Trashed> {
        let mut out = store.trashed();
        out.sort_by(|a, b| (a.deleted_at, &a.id, &a.title).cmp(&(b.deleted_at, &b.id, &b.title)));
        out
    }

    fn trash_files_of(store: &Store) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(trash) = trash_dir(&store.notes_dir) {
            walk(&trash, 0, false, &mut files, &mut Vec::new()).unwrap();
        }
        let mut paths: Vec<PathBuf> = files.into_iter().map(|(path, _)| path).collect();
        paths.sort();
        paths
    }

    #[test]
    fn the_trash_index_lists_what_a_full_read_lists() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        let mut seed: u64 = 0x05ee_d1e0;
        let mut next = |most: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % most.max(1)
        };
        let folders = ["", "work", "work/deep", "home"];
        let titles = ["Plan", "Plan", "Notes on things", "A", "Lecture 4"];
        for step in 0..160 {
            let files = trash_files_of(&store);
            let pick = |n: usize| files.get(n % files.len().max(1)).cloned();
            match next(12) {
                0..=2 => {
                    let title = titles[next(titles.len())];
                    let folder = folders[next(folders.len())];
                    let id = store.create_note(title, "body", folder).unwrap().id.clone();
                    store.save().unwrap();
                    store.delete_notes(&[id]);
                    store.save().unwrap();
                }
                3 => {
                    if let Some(t) = store.trashed().get(next(8)).cloned() {
                        store.restore_many(&[t.id]);
                        store.save().unwrap();
                    }
                }
                4 => {
                    if let Some(t) = store.trashed().get(next(8)).cloned() {
                        store.delete_from_trash(&[t.id]);
                    }
                }
                5 => {
                    if next(4) == 0 {
                        store.empty_trash();
                    }
                }
                6 => {
                    if let Some(path) = pick(next(64)) {
                        let old = std::time::SystemTime::now()
                            - std::time::Duration::from_secs(40 * 24 * 60 * 60);
                        fs::File::options()
                            .write(true)
                            .open(&path)
                            .unwrap()
                            .set_modified(old)
                            .unwrap();
                        store.tidy_trash_now();
                    }
                }
                7 => {
                    let id = uuid::Uuid::new_v4();
                    let folder = folders[next(folders.len())];
                    let path = notes
                        .join(TRASH)
                        .join(folder)
                        .join(format!("Outside {step}.md"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(
                        &path,
                        format!("---\nid: {id}\ntitle: Outside {step}\n---\nhi\n"),
                    )
                    .unwrap();
                    store.trash_may_have_changed();
                }
                8 => {
                    if let Some(path) = pick(next(64)) {
                        let text = fs::read_to_string(&path).unwrap();
                        fs::write(&path, text.replacen("title: ", "title: Edited ", 1)).unwrap();
                        store.trash_may_have_changed();
                    }
                }
                9 => {
                    if let Some(path) = pick(next(64)) {
                        let id = uuid::Uuid::new_v4();
                        fs::write(
                            &path,
                            format!("---\nid: {id}\ntitle: Changed id {step}\n---\nnew\n"),
                        )
                        .unwrap();
                        store.trash_may_have_changed();
                    }
                }
                10 => {
                    if let Some(path) = pick(next(64)) {
                        if next(2) == 0 {
                            fs::remove_file(&path).unwrap();
                        } else {
                            fs::write(&path, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
                        }
                        store.trash_may_have_changed();
                    }
                }
                _ => {
                    store = Store::load_from(&notes).unwrap();
                }
            }
            assert_eq!(listed(&store), read_in_full(&store), "step {step}");
        }
        assert!(trash_index_path(&notes).unwrap().exists());
    }

    #[test]
    fn an_outside_edit_to_a_trashed_file_is_read_again() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Before", "body", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert!(store.trashed()[0].readable);
        let path = trash_files_of(&store).remove(0);
        fs::write(&path, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
        store.trash_may_have_changed();
        let listed = store.trashed();
        assert!(!listed[0].readable);
        assert_ne!(listed[0].id, id);
        let again = Store::load_from(&store.notes_dir).unwrap();
        assert!(!again.trashed()[0].readable);
    }

    #[test]
    fn an_unreadable_trashed_file_stays_unreadable_in_the_index() {
        let (store, _tmp) = temp_store();
        trashed_file(&store, "Broken.md", "---\ntitle: [unclosed\n---\nbody\n");
        let listed = store.trashed();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].readable);
        assert!(store.trash_snapshot()[0].head.is_none());
        let again = Store::load_from(&store.notes_dir).unwrap();
        let listed = again.trashed();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].readable);
        assert_eq!(listed[0].id, unreadable_id(Path::new("Broken.md")));
        assert!(again.trash_snapshot()[0].head.is_none());
    }

    #[test]
    fn a_damaged_index_file_starts_empty_and_warns() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Kept", "body", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(store.trashed().len(), 1);
        let index = trash_index_path(&store.notes_dir).unwrap();
        fs::write(&index, "{ not the index").unwrap();
        let again = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(listed(&again), read_in_full(&again));
        assert_eq!(again.trashed()[0].id, id);
        assert!(crate::diag::recent()
            .iter()
            .any(|said| said.contains("could not read its index of the trash")));
        assert!(serde_json::from_slice::<serde_json::Value>(&fs::read(&index).unwrap()).is_ok());
    }

    #[test]
    fn a_restart_trusts_the_index_for_unchanged_files() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Real title", "body", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(store.trashed().len(), 1);
        let index = trash_index_path(&store.notes_dir).unwrap();
        let mut kept: serde_json::Value =
            serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
        kept[1][0][1]["head"]["title"] = "Only in the index".into();
        fs::write(&index, serde_json::to_vec(&kept).unwrap()).unwrap();
        let again = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(again.trashed()[0].title, "Only in the index");
    }

    #[cfg(unix)]
    #[test]
    fn opening_the_trash_with_nothing_changed_opens_no_file() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Locked later", "body", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert!(store.trashed()[0].readable);
        let path = trash_files_of(&store).remove(0);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let quiet = store.trashed();
        store.trash_may_have_changed();
        let checked = store.trashed();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(quiet[0].readable, "a quiet trash opened its file");
        assert!(!checked[0].readable, "a changed file was not read again");
    }

    #[test]
    fn deletions_always_check_the_trash_first() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old id", "body", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(store.trashed()[0].id, id);
        let path = trash_files_of(&store).remove(0);
        let other = uuid::Uuid::new_v4().to_string();
        fs::write(
            &path,
            format!("---\nid: {other}\ntitle: New id\n---\nbody\n"),
        )
        .unwrap();
        let emptied = store.delete_from_trash(std::slice::from_ref(&id));
        assert_eq!(emptied.deleted, 0);
        assert!(path.exists());
        let emptied = store.delete_from_trash(std::slice::from_ref(&other));
        assert_eq!((emptied.deleted, emptied.forgotten), (1, vec![other]));
        assert!(!path.exists());
    }

    fn unreadable_under(root: &Path) -> Vec<String> {
        let mut bad = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "md") {
                    let relative = path.strip_prefix(root).unwrap();
                    let readable = fs::read_to_string(&path).is_ok_and(|text| {
                        parse_note_from_markdown(&text, relative, Utc::now()).is_ok()
                    });
                    if !readable {
                        bad.push(relative.display().to_string());
                    }
                }
            }
        }
        bad
    }

    #[test]
    fn every_note_leo_writes_reads_back() {
        let titles = [
            "---",
            "...",
            "a: b",
            "#x",
            " lead",
            "trail ",
            "\"q\"",
            "'q'",
            "[x]",
            "{x}",
            "&a",
            "*a",
            "!t",
            "%x",
            "@x",
            "`x",
            "|",
            ">",
            "? x",
            "- x",
            "yes",
            "null",
            "~",
            "123",
            "1e3",
            "0x1F",
            "2026-01-01",
            "a\tb",
            "\u{feff}bom",
            "a\u{85}b",
            "a\u{2028}b",
            "🦉 owl",
            "a\nb",
            "a\rb",
        ];
        let long = "t".repeat(300);
        let mut extras: Vec<serde_yaml::Mapping> = vec![serde_yaml::Mapping::new()];
        for yaml in [
            "nested:\n  a: 1\n  b: [x, y]\n",
            "list:\n- one\n- two\n",
            "tagged: !thing 5\n",
            "text: \"line\\n---\\nmore\\n\"\n",
            "'---': odd key\n",
        ] {
            extras.push(serde_yaml::from_str(yaml).unwrap());
        }
        let bodies = [
            "",
            "---\n",
            "---\nid: fake\n---\nlook-alike",
            "\n\nlead",
            "a\r\nb",
        ];
        let mut findings = Vec::new();
        let mut damaged = Vec::new();
        let mut check = |title: &str, extra: &serde_yaml::Mapping, body: &str| {
            let mut note = Note::new(title, body, "");
            note.extra = extra.clone();
            let text = note_to_markdown(&note).unwrap();
            let relative = format!("{}.md", filename::file_name(title));
            match parse_note_from_markdown(&text, Path::new(&relative), Utc::now()) {
                Err(e) => damaged.push(format!("{title:?} {extra:?} {body:?}: {e}")),
                Ok(back) => {
                    if back.id != note.id || back.title != note.title {
                        damaged.push(format!("{title:?}: came back as {:?}", back.title));
                    }
                    for (key, value) in &note.extra {
                        let came = back.extra.get(key);
                        match (value, came) {
                            (
                                serde_yaml::Value::String(sent),
                                Some(serde_yaml::Value::String(got)),
                            ) if sent != got => {
                                findings.push(format!("property {sent:?} reads back as {got:?}"))
                            }
                            _ if came != Some(value) => {
                                damaged.push(format!("{key:?} came back as {came:?}"))
                            }
                            _ => {}
                        }
                    }
                    let expected = body.replace("\r\n", "\n");
                    if back.body != expected.trim_start_matches('\n') {
                        damaged.push(format!(
                            "{title:?} body {body:?} came back as {:?}",
                            back.body
                        ));
                    } else if back.body != body {
                        findings.push(format!("body {body:?} reads back as {:?}", back.body));
                    }
                }
            }
        };
        for title in titles.iter().copied().chain([long.as_str()]) {
            check(title, &extras[0], "x");
        }
        for extra in &extras {
            check("Extras", extra, "x");
        }
        for body in bodies {
            check("Body", &extras[0], body);
        }
        assert!(damaged.is_empty(), "{damaged:#?}");
        assert_eq!(
            findings,
            [
                "body \"\\n\\nlead\" reads back as \"lead\"",
                "body \"a\\r\\nb\" reads back as \"a\\nb\"",
            ]
        );
    }

    #[test]
    fn a_trashed_copy_written_under_a_new_id_reads_back() {
        let (store, _tmp) = temp_store();
        let text = "---\nid: same\ntitle: One\n---\nx";
        fs::write(store.notes_dir.join("One.md"), text).unwrap();
        fs::write(
            store.notes_dir.join("Two.md"),
            "---\nid: same\ntitle: Two\n---\ny",
        )
        .unwrap();
        set_mtime_to_the_past(&store.notes_dir.join("One.md"));
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        let second = store
            .notes
            .iter()
            .find(|n| n.title == "Two")
            .unwrap()
            .id
            .clone();
        assert_ne!(second, "same");
        store.delete_notes(std::slice::from_ref(&second));
        store.save().unwrap();
        assert!(unreadable_under(&store.notes_dir).is_empty());
        assert_eq!(trashed_ids(&store.notes_dir), HashSet::from([second]));
    }

    #[cfg(unix)]
    #[test]
    fn the_stamp_fallback_rewrite_reads_back() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Locked", "x", "").unwrap().id.clone();
        store.save().unwrap();
        let path = store.notes_dir.join("Locked.md");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert!(unreadable_under(&store.notes_dir).is_empty());
        assert_eq!(trashed_ids(&store.notes_dir), HashSet::from([id]));
    }

    #[test]
    fn a_conflict_copy_reads_back() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "mine", "a").unwrap().id.clone();
        store.save().unwrap();
        let path = store.notes_dir.join("a/Shared.md");
        let theirs = fs::read_to_string(&path).unwrap().replace("mine", "theirs");
        fs::write(&path, theirs).unwrap();
        store.find_note_mut(&id).unwrap().body = "mine, edited".into();
        store.save().unwrap();
        assert!(unreadable_under(&store.notes_dir).is_empty());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 2);
    }

    #[test]
    fn a_restored_note_is_written_readable() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Back: again", "x", "a/b")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(store.restore(&id).as_deref(), Some("Back: again"));
        store.save().unwrap();
        assert!(unreadable_under(&store.notes_dir).is_empty());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().title, "Back: again");
    }

    #[test]
    fn a_save_racing_an_outside_rename_leaves_every_file_readable() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Moving", "x", "").unwrap().id.clone();
        store.save().unwrap();
        fs::rename(
            store.notes_dir.join("Moving.md"),
            store.notes_dir.join("Moved elsewhere.md"),
        )
        .unwrap();
        store.find_note_mut(&id).unwrap().body = "changed".into();
        store.save().unwrap();
        assert!(unreadable_under(&store.notes_dir).is_empty());
    }

    #[test]
    fn an_outside_edit_mid_save_leaves_every_file_leo_wrote_readable() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Half", "x", "").unwrap().id.clone();
        store.save().unwrap();
        let path = store.notes_dir.join("Half.md");
        fs::write(&path, "---\ntitle: [\n---\nbody").unwrap();
        store.find_note_mut(&id).unwrap().body = "edited".into();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(unreadable_under(&store.notes_dir), ["Half.md"]);
        assert!(trash_files(&store).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_no_partial_md() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = temp_store();
        store.create_note("First", "x", "locked").unwrap();
        store.save().unwrap();
        let locked = store.notes_dir.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        store.create_note("Second", "y", "locked").unwrap();
        let saved = store.save();
        let names: Vec<String> = fs::read_dir(&locked)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(saved.is_err());
        assert_eq!(names, ["First.md"]);
        assert!(unreadable_under(&store.notes_dir).is_empty());
    }

    #[test]
    fn a_file_dropped_into_the_folder_loads_with_its_modified_time() {
        let (store, _tmp) = temp_store();
        let file = store.notes_dir.join("Dropped in.md");
        fs::write(&file, "from another app").unwrap();
        let ten_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(864_000);
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(ten_days_ago)
            .unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let note = reloaded
            .notes
            .iter()
            .find(|n| n.title == "Dropped in")
            .expect("not loaded");
        assert!(note.updated_at < Utc::now() - chrono::Duration::days(9));
        assert!(reloaded.unreadable.is_empty());
    }

    #[test]
    fn saving_does_not_rewrite_a_note_leo_did_not_change() {
        let (store, _tmp) = temp_store();
        let file = store.notes_dir.join("Plain.md");
        fs::write(&file, "from another app").unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        store.create_note("Other", "x", "").unwrap();
        store.save().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "from another app");
    }

    #[test]
    fn editing_a_headerless_note_gives_it_a_header_and_keeps_its_id() {
        let (store, _tmp) = temp_store();
        fs::write(store.notes_dir.join("Plain.md"), "before").unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        let id = store
            .notes
            .iter()
            .find(|n| n.title == "Plain")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&id).unwrap().body = "after".to_string();
        store.save().unwrap();

        assert_eq!(note_files(&store), ["Plain.md"]);
        let text = fs::read_to_string(store.notes_dir.join("Plain.md")).unwrap();
        assert!(text.starts_with("---\n"), "{text}");
        assert!(text.contains(&format!("id: {id}")), "{text}");
        assert!(text.contains("after"), "{text}");
    }

    #[test]
    fn a_tags_entry_is_kept_through_save_trash_and_conflict() {
        let id = "0c9d1f4e-6b1a-4d55-9a0e-2f3c4b5a6d7e";
        let shapes = [
            "tags:\n  - '#exam'\n  - graphs\n",
            "tags: [exam, graphs]\n",
            "tags: exam, graphs #bfs\n",
            "tags: []\n",
            "tags:\n- exam\n- graphs\n",
            "tags:\n  - exam\n# later\n  - graphs\n",
        ];
        for (shape, newline) in shapes
            .iter()
            .flat_map(|shape| [(shape, "\n"), (shape, "\r\n")])
        {
            let file = format!(
                "---\nid: {id}\ntitle: Graphs\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-02T00:00:00Z\naliases:\n- Short\n{shape}cssclasses:\n- wide\nnotes: |\n  one\n  two\n---\n\nBFS by layers\n"
            )
            .replace('\n', newline);
            let wanted: serde_yaml::Mapping = serde_yaml::from_str(shape).unwrap();
            let (store, _tmp) = temp_store();
            let path = store.notes_dir.join("Graphs.md");
            fs::write(&path, &file).unwrap();
            let mut store = Store::load_from(&store.notes_dir).unwrap();
            assert_eq!(store.notes[0].extra["tags"], wanted["tags"], "{shape:?}");
            store.save().unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), file, "{shape:?}");

            store.delete_notes(&[id.to_string()]);
            store.save().unwrap();
            assert_eq!(store.restore(id).as_deref(), Some("Graphs"));
            store.save().unwrap();
            let mut store = Store::load_from(&store.notes_dir).unwrap();
            assert_eq!(store.notes[0].extra["tags"], wanted["tags"], "{shape:?}");

            fs::write(&path, fs::read_to_string(&path).unwrap() + "elsewhere\n").unwrap();
            store.find_note_mut(id).unwrap().body = "leo's own edit\n".into();
            store.save().unwrap();
            let copy = Store::load_from(&store.notes_dir).unwrap();
            let copy = copy
                .notes
                .iter()
                .find(|n| n.title.ends_with(CONFLICT_SUFFIX))
                .unwrap();
            assert_eq!(copy.extra["tags"], wanted["tags"], "{shape:?}");
            assert_eq!(copy.extra["notes"], "one\ntwo\n", "{shape:?}");
            assert_eq!(copy.body, "leo's own edit\n");
        }
    }

    #[test]
    fn a_last_multiline_entry_keeps_its_final_newline() {
        let note = parse("A.md", "---\ntitle: A\nnotes: |\n  one\n  two\n---\nbody");
        assert_eq!(note.extra["notes"], "one\ntwo\n");
        let fallback = "---\nid: fall-1\ntitle: A\nbase: &b x\nother: *b\nnotes: |\n  one\n  two\n---\n\nbody\n";
        let note = parse("A.md", fallback);
        assert_eq!(note.extra["notes"], "one\ntwo\n");
        let (store, _tmp) = temp_store();
        fs::write(store.notes_dir.join("A.md"), fallback).unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        store.find_note_mut("fall-1").unwrap().body = "edited\n".into();
        store.save().unwrap();
        let back = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(back.notes[0].extra["notes"], "one\ntwo\n");
    }

    #[test]
    fn a_header_leo_cannot_split_keeps_its_values() {
        let text = "---\nid: anchor-1\ntitle: A\nbase: &shared\n  - one\n  - two\nother: *shared\ntags: [exam]\n---\n\nbody\n";
        let (store, _tmp) = temp_store();
        fs::write(store.notes_dir.join("A.md"), text).unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        let extra = store.notes[0].extra.clone();
        store.find_note_mut("anchor-1").unwrap().body = "edited\n".into();
        store.save().unwrap();
        let back = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(back.notes[0].extra, extra);
        assert_eq!(back.notes[0].body, "edited\n");
    }

    #[test]
    fn unknown_properties_keep_their_file_order() {
        let text = "---\nzeta: 1\nid: order-1\nalpha: 2\ntitle: A\nmiddle: [x]\ncreated_at: 2026-01-01T00:00:00Z\nbeta: 3\n---\n\nbody\n";
        let note = parse("A.md", text);
        let keys: Vec<&str> = note.extra.keys().filter_map(|k| k.as_str()).collect();
        assert_eq!(keys, ["zeta", "alpha", "middle", "beta"]);
        let written = note_to_markdown(&note).unwrap();
        let order: Vec<usize> = ["zeta:", "alpha:", "middle:", "beta:"]
            .iter()
            .map(|k| written.find(k).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{written}");
    }

    #[test]
    fn properties_leo_does_not_know_survive_a_round_trip() {
        let text = "---\nid: abc\ntitle: T\ntags: []\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\naliases:\n- Short\ncssclasses:\n- wide\nrating: 5\n---\n\nbody\n";
        let note = parse("T.md", text);
        let written = note_to_markdown(&note).unwrap();
        for expected in ["aliases:", "- Short", "cssclasses:", "- wide", "rating: 5"] {
            assert!(written.contains(expected), "no {expected:?} in:\n{written}");
        }
        assert_eq!(parse("T.md", &written).extra, note.extra);
    }

    #[test]
    fn a_save_keeps_another_apps_properties_in_the_file() {
        let (store, _tmp) = temp_store();
        let text = "---\nid: keep1\ntitle: Keep\ntags: []\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\naliases:\n- Other name\n---\n\nold\n";
        fs::write(store.notes_dir.join("Keep.md"), text).unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        store.find_note_mut("keep1").unwrap().body = "new".to_string();
        store.save().unwrap();

        let saved = fs::read_to_string(store.notes_dir.join("Keep.md")).unwrap();
        assert!(saved.contains("aliases:"), "{saved}");
        assert!(saved.contains("Other name"), "{saved}");
        assert!(saved.contains("new"), "{saved}");
    }

    fn leo_text(title: &str) -> String {
        format!(
            "---\nid: abc\ntitle: {title}\ntags: []\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\nbody\n"
        )
    }

    #[test]
    fn a_file_renamed_in_another_app_takes_the_new_name_as_its_title() {
        assert_eq!(
            parse("New name.md", &leo_text("Old name")).title,
            "New name"
        );
    }

    #[test]
    fn numbered_and_sanitised_names_keep_the_header_title() {
        let text = leo_text("'Lecture 4: BFS'");
        assert_eq!(parse("Lecture 4 - BFS.md", &text).title, "Lecture 4: BFS");
        assert_eq!(
            parse("Lecture 4 - BFS (2).md", &text).title,
            "Lecture 4: BFS"
        );
    }

    #[test]
    fn a_file_named_after_its_id_keeps_the_header_title() {
        let name = "550e8400-e29b-41d4-a716-446655440000.md";
        assert_eq!(parse(name, &leo_text("Whatever")).title, "Whatever");
    }

    #[test]
    fn a_rename_made_in_another_app_is_not_undone_by_a_save() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old name", "x", "").unwrap().id.clone();
        store.save().unwrap();
        fs::rename(
            store.notes_dir.join("Old name.md"),
            store.notes_dir.join("New name.md"),
        )
        .unwrap();

        let mut reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().title, "New name");
        reloaded.create_note("Other", "y", "").unwrap();
        reloaded.save().unwrap();

        assert_eq!(note_files(&reloaded), ["New name.md", "Other.md"]);
    }

    #[test]
    fn a_note_changed_elsewhere_and_edited_here_keeps_both_versions() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "one", "").unwrap().id.clone();
        store.save().unwrap();
        let file = store.notes_dir.join("Shared.md");
        fs::write(
            &file,
            fs::read_to_string(&file)
                .unwrap()
                .replace("one", "one, plus a line from Obsidian"),
        )
        .unwrap();

        store.find_note_mut(&id).unwrap().body = "one, plus a line from leo".to_string();
        store.save().unwrap();

        assert!(fs::read_to_string(&file).unwrap().contains("from Obsidian"));
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 2);
        let copy = reloaded
            .notes
            .iter()
            .find(|n| n.title == "Shared (conflict from leo)")
            .expect("no conflict copy");
        assert!(copy.body.contains("from leo"));
        assert_ne!(copy.id, id);
    }

    #[test]
    fn saving_again_does_not_pile_up_conflict_copies() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "one", "").unwrap().id.clone();
        store.save().unwrap();
        let file = store.notes_dir.join("Shared.md");
        fs::write(
            &file,
            "---\nid: other\ntitle: Shared\n---\n\nchanged elsewhere\n",
        )
        .unwrap();
        store.find_note_mut(&id).unwrap().body = "edited here".to_string();
        store.save().unwrap();
        store.save().unwrap();
        store.save().unwrap();
        assert_eq!(note_files(&store).len(), 2, "{:?}", note_files(&store));
    }

    #[test]
    fn saving_after_a_conflict_keeps_one_copy_and_the_outside_edit() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "one", "").unwrap().id.clone();
        store.save().unwrap();
        let file = store.notes_dir.join("Shared.md");
        fs::write(
            &file,
            fs::read_to_string(&file)
                .unwrap()
                .replace("one", "one, from another program"),
        )
        .unwrap();
        for n in 1..=3 {
            store.find_note_mut(&id).unwrap().body = format!("edited here {n}");
            store.save().unwrap();
        }
        let files = note_files(&store);
        let changed = store.changed_on_disk();
        assert!(
            files.len() == 2 && changed,
            "files={files:?} changed_on_disk={changed}"
        );
        assert!(fs::read_to_string(&file)
            .unwrap()
            .contains("one, from another program"));
        let copy =
            fs::read_to_string(store.notes_dir.join("Shared (conflict from leo).md")).unwrap();
        assert!(copy.contains("edited here 3"), "{copy}");
    }

    #[test]
    fn a_note_changed_elsewhere_but_not_here_is_left_alone() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Shared", "one", "").unwrap();
        store.save().unwrap();
        let file = store.notes_dir.join("Shared.md");
        let edited = fs::read_to_string(&file).unwrap().replace("one", "two");
        fs::write(&file, &edited).unwrap();
        store.create_note("Unrelated", "x", "").unwrap();
        store.save().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), edited);
        assert_eq!(note_files(&store).len(), 2);
    }

    #[test]
    fn a_note_deleted_elsewhere_and_edited_here_comes_back_as_a_copy() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Shared", "one", "").unwrap().id.clone();
        store.save().unwrap();
        fs::remove_file(store.notes_dir.join("Shared.md")).unwrap();
        store.find_note_mut(&id).unwrap().body = "still writing".to_string();
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 1);
        assert_eq!(reloaded.notes[0].title, "Shared (conflict from leo)");
        assert!(reloaded.notes[0].body.contains("still writing"));
    }

    #[test]
    fn the_notice_names_the_note() {
        assert!(conflict_notice("Shared").contains("Shared"));
    }

    #[test]
    fn the_store_notices_another_programs_change_but_not_its_own_saves() {
        let (mut store, _tmp) = temp_store();
        assert!(!store.changed_on_disk());
        store.create_note("Mine", "a", "").unwrap();
        store.save().unwrap();
        assert!(
            !store.changed_on_disk(),
            "its own save looked like someone else's"
        );

        fs::write(store.notes_dir.join("Theirs.md"), "from elsewhere").unwrap();
        assert!(store.changed_on_disk());
        store.refresh().unwrap();
        assert!(!store.changed_on_disk());
        assert_eq!(store.notes.len(), 2);
    }

    #[test]
    fn refreshing_tidies_the_trash_at_most_hourly() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "a", "").unwrap();
        store.save().unwrap();
        let trashed = store.notes_dir.join(".trash/Kept.md");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::copy(store.notes_dir.join("Kept.md"), &trashed).unwrap();
        store.refresh().unwrap();
        assert!(trashed.exists(), "a refresh within the hour tidied again");
        assert!(
            store.trashed().is_empty(),
            "a live note was listed in the trash"
        );
        store.tidied.set(None);
        store.refresh().unwrap();
        assert!(!trashed.exists(), "a live note stayed in the trash");
    }

    #[test]
    fn a_note_duplicated_in_another_app_is_its_own_note_and_both_survive_a_save() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture 4", "the original", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let original = store.notes_dir.join("Lecture 4.md");
        let copy = store.notes_dir.join("Lecture 4 1.md");
        let copied_text = fs::read_to_string(&original)
            .unwrap()
            .replace("the original", "the copy, edited");
        fs::write(&copy, &copied_text).unwrap();
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&original)
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();

        let mut store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(store.notes.len(), 2);
        let ids: HashSet<&str> = store.notes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "two notes share one id");

        store.find_note_mut(&id).unwrap().body = "the original, edited in leo".to_string();
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let bodies: HashSet<String> = reloaded.notes.iter().map(|n| n.body.clone()).collect();
        assert!(bodies.contains("the original, edited in leo"), "{bodies:?}");
        assert!(bodies.contains("the copy, edited"), "{bodies:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_move_that_cannot_remove_the_old_file_fails_and_leaves_one_file() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = temp_store();
        store.create_dir("locked");
        let id = store
            .create_note("Moving", "body", "locked")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let locked = store.notes_dir.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        store.move_note(&id, "").unwrap();
        let saved = store.save();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(saved.is_err());
        let files = note_files(&store);
        let holding: Vec<&String> = files
            .iter()
            .filter(|f| {
                fs::read_to_string(store.notes_dir.join(f))
                    .unwrap()
                    .contains(&format!("id: {id}"))
            })
            .collect();
        assert_eq!(holding.len(), 1, "files holding the id: {holding:?}");
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().directory, "locked");
    }

    #[test]
    fn a_copy_with_the_same_mtime_leaves_the_original_its_id() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture 4", "the original", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let original = store.notes_dir.join("Lecture 4.md");
        let copy = store.notes_dir.join("Lecture 4 copy.md");
        fs::write(
            &copy,
            fs::read_to_string(&original)
                .unwrap()
                .replace("the original", "the copy"),
        )
        .unwrap();
        let when = fs::metadata(&original).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&copy)
            .unwrap()
            .set_modified(when)
            .unwrap();
        let store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(
            store.find_note(&id).unwrap().body,
            "the original",
            "the copy took the original's id"
        );
    }

    #[test]
    fn editing_the_original_keeps_its_id_on_the_next_launch() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture 4", "the original", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let original = store.notes_dir.join("Lecture 4.md");
        let copy = store.notes_dir.join("Lecture 4 1.md");
        fs::write(
            &copy,
            fs::read_to_string(&original)
                .unwrap()
                .replace("the original", "the copy, edited"),
        )
        .unwrap();
        set_mtime_to_the_past(&original);
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(store.find_note(&id).unwrap().body, "the original");
        store.find_note_mut(&id).unwrap().body = "the original, edited in leo".to_string();
        store.save().unwrap();
        store.save().unwrap();
        let copy_keeps_old_id = fs::read_to_string(&copy)
            .unwrap()
            .contains(&format!("id: {id}"));
        assert!(!copy_keeps_old_id, "the copy file still holds the old id");
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(
            reloaded.find_note(&id).unwrap().body,
            "the original, edited in leo",
            "next launch gave the id to the copy; copy file still holds the old id: {copy_keeps_old_id}"
        );
    }

    #[test]
    fn a_deleted_copy_sharing_its_originals_id_waits_in_the_trash_and_comes_back() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Lecture 4", "the original", "").unwrap();
        store.save().unwrap();
        let original = store.notes_dir.join("Lecture 4.md");
        let copied_text = fs::read_to_string(&original)
            .unwrap()
            .replace("the original", "the copy, edited");
        fs::write(store.notes_dir.join("Lecture 4 1.md"), copied_text).unwrap();
        set_mtime_to_the_past(&original);

        let mut store = Store::load_from(&store.notes_dir).unwrap();
        let copy = store
            .notes
            .iter()
            .find(|n| n.body == "the copy, edited")
            .unwrap()
            .id
            .clone();
        store.delete_notes(std::slice::from_ref(&copy));
        store.save().unwrap();
        store.tidy_trash_now();

        let mut reloaded = Store::load_from(&store.notes_dir).unwrap();
        let trashed: Vec<String> = reloaded.trashed().into_iter().map(|t| t.id).collect();
        assert_eq!(trashed, vec![copy.clone()]);
        assert!(reloaded.restore(&copy).is_some());
        reloaded.save().unwrap();
        let bodies: HashSet<String> = Store::load_from(&store.notes_dir)
            .unwrap()
            .notes
            .into_iter()
            .map(|n| n.body)
            .collect();
        assert!(bodies.contains("the original"), "{bodies:?}");
        assert!(bodies.contains("the copy, edited"), "{bodies:?}");
    }

    #[test]
    fn a_read_only_note_last_edited_long_ago_keeps_its_days_in_the_trash() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "keep me", "").unwrap().id.clone();
        store.save().unwrap();
        let path = store.notes_dir.join("Old.md");
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 24 * 3600),
            )
            .unwrap();
        let mut read_only = fs::metadata(&path).unwrap().permissions();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();

        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        store.tidy_trash_now();

        let trashed: Vec<String> = store.trashed().into_iter().map(|t| t.id).collect();
        assert_eq!(trashed, vec![id]);
        let left = trash_files(&store);
        assert!(fs::metadata(&left[0]).unwrap().permissions().readonly());
    }

    #[test]
    fn an_id_that_begins_another_id_still_finds_its_own_note() {
        let (mut store, _tmp) = temp_store();
        for id in ["2023", "20231"] {
            let mut note = make_note();
            note.id = id.into();
            store.notes.push(note);
        }
        assert_eq!(store.find_note("2023").unwrap().id, "2023");
        assert_eq!(store.find_note_mut("2023").unwrap().id, "2023");
        assert_eq!(store.find_note("20231").unwrap().id, "20231");
        assert!(store.find_note("202").is_none());
    }

    #[test]
    fn a_date_only_creation_date_and_a_numeric_id_survive_an_edit_in_leo() {
        let (store, _tmp) = temp_store();
        fs::write(
            store.notes_dir.join("Imported.md"),
            "---\nid: 12345\ntitle: Imported\ncreated_at: 2024-01-05\n---\n\nbody\n",
        )
        .unwrap();
        let mut store = Store::load_from(&store.notes_dir).unwrap();
        store.find_note_mut("12345").unwrap().body = "edited".into();
        store.save().unwrap();

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let note = reloaded.find_note("12345").unwrap();
        assert_eq!(note.body, "edited");
        assert_eq!(note.created_at.to_rfc3339(), "2024-01-05T00:00:00+00:00");
    }
    #[test]
    fn folders_cannot_escape_notes_or_use_reserved_hidden_paths() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&temp.path().join("notes")).unwrap();
        for dir in [
            "../outside",
            "a/../../outside",
            "/outside",
            "a\\..\\outside",
            "C:outside",
            ".trash",
            ".git",
            "a/.hidden",
            "a//b",
        ] {
            assert!(store.create_note("Escape", "body", dir).is_err(), "{dir}");
            assert!(!store.create_dir(dir), "{dir}");
        }
        store.save().unwrap();
        assert!(store.notes.is_empty());
        assert!(!temp.path().join("outside").exists());
    }

    #[test]
    fn changing_a_public_note_directory_is_checked_before_any_file_is_written() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&temp.path().join("notes")).unwrap();
        store.create_note("Escape", "body", "").unwrap();
        store.notes[0].directory = "../outside".to_string();
        assert!(store.save().is_err());
        assert!(!temp.path().join("outside").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_neither_load_outside_notes_nor_allow_writes_outside() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&notes).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("Private.md"), "private").unwrap();
        std::os::unix::fs::symlink(&outside, notes.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("Private.md"), notes.join("Alias.md")).unwrap();
        let mut store = Store::load_from(&notes).unwrap();
        assert!(store.notes.is_empty());
        assert!(store.create_note("Escape", "body", "linked").is_err());
        assert!(!store.create_dir("linked/nested"));
        assert_eq!(
            fs::read_to_string(outside.join("Private.md")).unwrap(),
            "private"
        );
    }

    #[test]
    fn a_new_empty_folder_on_disk_changes_the_store_fingerprint() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&temp.path().join("notes")).unwrap();
        store.save().unwrap();
        let mut other = Store::load_from(&store.notes_dir).unwrap();
        other.create_dir("empty");
        other.save().unwrap();
        assert!(store.changed_on_disk());
        store.refresh().unwrap();
        assert!(store.dir_exists("empty"));
    }

    #[test]
    #[ignore = "benchmark: run by hand with --ignored --nocapture"]
    fn verify_benchmark_refresh_costs() {
        use std::time::Instant;
        let counts: Vec<usize> = std::env::var("LEO_BENCH_NOTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .map_or_else(|| vec![1000, 5000, 10000], |n| vec![n]);
        let big = std::env::var("LEO_BENCH_BIG").is_ok_and(|v| v == "1");
        let body = "word ".repeat(1000);
        for count in counts {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("notes");
            fs::create_dir_all(&dir).unwrap();
            let built = Instant::now();
            for i in 0..count {
                fs::write(
                    dir.join(format!("Note {i}.md")),
                    format!("---\nid: {}\ntitle: Note {i}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n{body}\n", uuid::Uuid::new_v4()),
                )
                .unwrap();
            }
            let target = if big {
                let path = dir.join("Big.md");
                fs::write(
                    &path,
                    format!("---\nid: {}\ntitle: Big\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n{}\n", uuid::Uuid::new_v4(), "word ".repeat(4_000_000)),
                )
                .unwrap();
                path
            } else {
                dir.join("Note 0.md")
            };
            let built = built.elapsed();
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut store = Store::load_from(&dir).unwrap();
            let load = t.elapsed();
            let mut walks = Vec::new();
            for _ in 0..5 {
                let t = Instant::now();
                assert!(!store.changed_on_disk());
                walks.push(ms(t.elapsed()));
            }
            let mut reconciles = Vec::new();
            for round in 0..3 {
                let mut file = fs::OpenOptions::new().append(true).open(&target).unwrap();
                std::io::Write::write_all(&mut file, format!("edit {round}\n").as_bytes()).unwrap();
                drop(file);
                let t = Instant::now();
                assert!(store.changed_on_disk());
                store.refresh().unwrap();
                reconciles.push(ms(t.elapsed()));
            }
            assert!(store.notes.iter().any(|n| n.body.ends_with("edit 2\n")));
            let t = Instant::now();
            walk(&dir, 0, false, &mut Vec::new(), &mut Vec::new()).unwrap();
            let walk = t.elapsed();
            assert_eq!(store.notes.len(), count + usize::from(big));
            println!(
                "notes={count} big={big} fixture_s={:.1} load_ms={:.1} changed_on_disk_ms={walks:.1?} one_file_reconcile_ms={reconciles:.1?} walk_ms={:.1}",
                built.as_secs_f64(),
                ms(load),
                ms(walk),
            );
        }
    }

    fn set_mtime(path: &Path, when: std::time::SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    fn mtime_of(path: &Path) -> std::time::SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    type Snapshot = (
        std::collections::BTreeSet<(String, String, String, String, String)>,
        Vec<String>,
        Vec<String>,
        BTreeMap<String, usize>,
    );

    fn snapshot(store: &Store) -> Snapshot {
        let seen = store.seen.borrow();
        let notes = store
            .notes
            .iter()
            .map(|note| {
                let path = seen[&note.id].path.clone();
                let in_file = read_head(&store.notes_dir.join(&path), &path).map(|head| head.id);
                let id = if in_file.as_deref() != Some(note.id.as_str())
                    && note.id != stable_id(&path)
                {
                    "loser".to_string()
                } else {
                    note.id.clone()
                };
                (
                    path.to_string_lossy().replace('\\', "/"),
                    id,
                    note.title.clone(),
                    note.body.clone(),
                    note.directory.clone(),
                )
            })
            .collect();
        let mut unreadable: Vec<String> = store
            .unreadable
            .iter()
            .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
            .collect();
        unreadable.sort();
        let mut directories = store.directories.clone();
        directories.sort();
        let pictures = store.pictures.borrow().live.clone().into_iter().collect();
        (notes, unreadable, directories, pictures)
    }

    fn same_as_fresh(store: &mut Store, step: &str) {
        store.refresh().unwrap();
        let fresh = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(snapshot(store), snapshot(&fresh), "after {step}");
    }

    fn headed(title: &str, body: &str) -> String {
        format!(
            "---\nid: {}\ntitle: {title}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\n{body}\n",
            uuid::Uuid::new_v4()
        )
    }

    fn append(path: &Path, text: &str) {
        let old = fs::read_to_string(path).unwrap_or_else(|error| {
            let folder = path.parent().unwrap();
            let mut names: Vec<String> = fs::read_dir(folder)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            panic!(
                "appending {text:?}: {} is missing ({error}); {} holds {names:?}",
                path.display(),
                folder.display()
            )
        });
        fs::write(path, format!("{old}{text}\n")).unwrap();
    }

    fn copy_with_mtime(from: &Path, to: &Path, when: std::time::SystemTime) {
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::write(to, fs::read(from).unwrap()).unwrap();
        set_mtime(to, when);
    }

    #[test]
    fn refresh_equals_a_fresh_load_after_outside_changes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("notes");
        let folders = ["", "m", "m/deep"];
        for folder in folders {
            fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let file = |i: usize| dir.join(folders[i % 3]).join(format!("Note {i}.md"));
        for i in 0..50 {
            fs::write(
                file(i),
                headed(
                    &format!("Note {i}"),
                    &format!("body {i} ![p](attachments/20261117-101500-p{}.png)", i % 3),
                ),
            )
            .unwrap();
        }
        let mut store = Store::load_from(&dir).unwrap();

        fs::write(dir.join("m/Fresh.md"), headed("Fresh", "new")).unwrap();
        same_as_fresh(&mut store, "an outside create");
        append(&file(1), "edited");
        same_as_fresh(&mut store, "an outside edit");
        fs::rename(file(2), dir.join("m/deep/Renamed.md")).unwrap();
        same_as_fresh(&mut store, "an outside rename");
        fs::remove_file(file(3)).unwrap();
        same_as_fresh(&mut store, "an outside delete");

        let good = fs::read_to_string(file(4)).unwrap();
        fs::write(file(4), "---\nid: [unclosed\n---\nbroken\n").unwrap();
        same_as_fresh(&mut store, "a file made unparseable");
        fs::write(file(5), "---\nid: [unclosed\n---\nbroken\n").unwrap();
        fs::write(file(4), good).unwrap();
        same_as_fresh(&mut store, "one file broken and another fixed");

        let original = file(7);
        let tied = mtime_of(&original);
        copy_with_mtime(&original, &dir.join("a/Note 7.md"), tied);
        same_as_fresh(&mut store, "a copy in an earlier folder with a tied time");
        same_as_fresh(&mut store, "a second refresh after that copy");
        copy_with_mtime(&original, &dir.join("z/Note 7.md"), tied);
        same_as_fresh(&mut store, "a copy in a later folder with a tied time");
        same_as_fresh(&mut store, "a second refresh after the later copy");

        let named = file(8);
        copy_with_mtime(
            &named,
            &named.with_file_name("Note 8 copy.md"),
            mtime_of(&named),
        );
        same_as_fresh(&mut store, "a copy under another name with a tied time");

        let restarted = file(10);
        let id = read_head(&restarted, restarted.strip_prefix(&dir).unwrap())
            .unwrap()
            .id;
        let holder = |store: &Store| store.seen.borrow()[&id].path.clone();
        let before = holder(&store);
        copy_with_mtime(
            &restarted,
            &dir.join("a/Note 10.md"),
            mtime_of(&restarted) + std::time::Duration::from_secs(10),
        );
        store.refresh().unwrap();
        let after = holder(&store);
        let again = holder(&Store::load_from(&dir).unwrap());
        assert_eq!(
            before, after,
            "a refresh before the first save moved the id"
        );
        assert_eq!(
            before, again,
            "a restart before the first save moved the id"
        );
        same_as_fresh(&mut store, "a copy before the first save");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = file(11);
            let length = fs::metadata(&locked).unwrap().len();
            let when = mtime_of(&locked);
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(fs::metadata(&locked).unwrap().len(), length);
            assert_eq!(mtime_of(&locked), when);
            append(&file(12), "another change");
            same_as_fresh(&mut store, "a file made unreadable by permission");
            assert!(
                store.unreadable.iter().any(|(path, _)| *path == locked),
                "the locked file was not listed"
            );
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
            append(&file(13), "and another");
            same_as_fresh(&mut store, "the permission given back");
            assert!(store.unreadable.iter().all(|(path, _)| *path != locked));
        }

        for (title, body) in [
            ("Twin", "first"),
            ("Twin", "second"),
            ("Odd: name? <with> *stars*", "odd"),
            (&*"Long title ".repeat(30), "long"),
            ("Windows lines", "one\r\ntwo\r\n"),
            ("Blank first", "\n\nafter blank lines"),
        ] {
            store.create_note(title, body, "m").unwrap();
        }
        store.save().unwrap();
        append(&file(14), "after leo saved");
        same_as_fresh(&mut store, "notes leo saved itself");

        let theirs = file(16);
        let id = read_head(&theirs, theirs.strip_prefix(&dir).unwrap())
            .unwrap()
            .id;
        store.find_note_mut(&id).unwrap().body = "mine, in memory".to_string();
        append(&theirs, "theirs, on disk");
        store.save().unwrap();
        assert!(theirs
            .with_file_name(format!("Note 16{CONFLICT_SUFFIX}.md"))
            .exists());
        same_as_fresh(&mut store, "a conflict copy");
    }

    #[cfg(windows)]
    #[test]
    fn an_unchanged_stamp_is_not_read_again() {
        let (mut store, _d) = temp_store();
        let id = store
            .create_note("Same size", "first text", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let path = store.notes_dir.join("Same size.md");
        let when = mtime_of(&path);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replace("first text", "other text")).unwrap();
        set_mtime(&path, when);
        store.refresh().unwrap();
        assert_eq!(store.find_note(&id).unwrap().body, "first text");
    }

    #[test]
    fn a_stale_temp_file_is_removed_at_load() {
        let (store, _tmp) = temp_store();
        let stale = store.notes_dir.join(".tmpAbC123");
        fs::write(&stale, "half a note").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        set_mtime(&stale, long_ago);
        let nested = store.notes_dir.join("cs130/.tmpXyZ789");
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        fs::write(&nested, "half another").unwrap();
        set_mtime(&nested, long_ago);
        Store::load_from(&store.notes_dir).unwrap();
        assert!(!stale.exists(), "a two-hour-old temp file survived load");
        assert!(
            !nested.exists(),
            "a two-hour-old temp file in a folder survived load"
        );
    }

    #[test]
    fn a_young_or_differently_named_temp_file_is_kept_at_load() {
        let (store, _tmp) = temp_store();
        let dir = &store.notes_dir;
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let young = dir.join(".tmpAbC123");
        fs::write(&young, "a save still running").unwrap();
        fs::create_dir_all(dir.join(".leo")).unwrap();
        let mut old = vec![dir.join(".leo/.tmpAbC123")];
        for name in [".tmpAbC12", ".tmpAbC1234", ".tmpAb-123", "x.tmpAbC1"] {
            old.push(dir.join(name));
        }
        for path in &old {
            fs::write(path, "someone else's").unwrap();
            set_mtime(path, long_ago);
        }
        Store::load_from(dir).unwrap();
        assert!(young.exists(), "a young temp file was removed");
        for path in &old {
            assert!(path.exists(), "{} was removed", path.display());
        }
    }

    #[test]
    fn a_note_keeps_its_file_when_its_path_is_spelled_another_way() {
        let (mut store, _d) = temp_store();
        let id = store.create_note("Kept", "a", "m/deep").unwrap().id.clone();
        store.save().unwrap();
        store.seen.borrow_mut().get_mut(&id).unwrap().path =
            Path::new("m").join(".").join("deep").join("Kept.md");
        store.touch_all();
        store.save().unwrap();
        assert_eq!(note_files(&store), ["m/deep/Kept.md"]);
    }

    #[test]
    fn a_note_changed_in_memory_is_read_again_on_refresh() {
        let (mut store, _d) = temp_store();
        let id = store.create_note("Kept", "on disk", "").unwrap().id.clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "only in memory".to_string();
        store.refresh().unwrap();
        assert_eq!(store.find_note(&id).unwrap().body, "on disk");
    }

    type Scanned = HashMap<String, (String, String, String, u64)>;

    fn scan(store: &Store) -> Scanned {
        store
            .notes
            .iter()
            .map(|n| {
                (
                    n.id.clone(),
                    (
                        n.title.clone(),
                        n.body.clone(),
                        n.directory.clone(),
                        hash_note(n),
                    ),
                )
            })
            .collect()
    }

    #[derive(Default)]
    struct Reader {
        seen: u64,
        map: Scanned,
    }

    impl Reader {
        fn catch_up(&mut self, store: &Store) -> bool {
            let (now, changed) = store.changes_since(self.seen);
            self.seen = now;
            match changed {
                Changed::Everything => {
                    self.map = scan(store);
                    true
                }
                Changed::Ids(ids) => {
                    for id in ids {
                        match store.notes.iter().find(|n| n.id == id) {
                            Some(n) => {
                                self.map.insert(
                                    id,
                                    (
                                        n.title.clone(),
                                        n.body.clone(),
                                        n.directory.clone(),
                                        hash_note(n),
                                    ),
                                );
                            }
                            None => {
                                self.map.remove(&id);
                            }
                        }
                    }
                    false
                }
            }
        }
    }

    #[test]
    fn a_reader_given_everything_after_the_list_overflows_sees_every_change() {
        let (mut store, _d) = temp_store();
        for i in 0..MOST_LOGGED + 10 {
            store.create_note(format!("Note {i}"), "first", "").unwrap();
        }
        let mut reader = Reader::default();
        assert!(reader.catch_up(&store));
        let ids: Vec<String> = store.notes.iter().map(|n| n.id.clone()).collect();
        for id in &ids {
            store.find_note_mut(id).unwrap().body = "second".to_string();
        }
        assert_eq!(store.changes_since(reader.seen).1, Changed::Everything);
        assert!(reader.catch_up(&store));
        assert!(reader.map.values().all(|(_, body, _, _)| body == "second"));
        assert_eq!(reader.map, scan(&store));
        store.find_note_mut(&ids[0]).unwrap().body = "third".to_string();
        assert_eq!(
            store.changes_since(reader.seen).1,
            Changed::Ids(vec![ids[0].clone()])
        );
    }

    #[test]
    fn a_reader_that_never_looked_gets_everything() {
        let (mut store, _d) = temp_store();
        assert_eq!(store.changes_since(0).1, Changed::Everything);
        store.create_note("One", "a", "").unwrap();
        assert_eq!(store.changes_since(0).1, Changed::Everything);
    }

    #[test]
    fn a_cold_load_says_everything() {
        let (mut store, _d) = temp_store();
        for title in ["One", "Two", "Three"] {
            store.create_note(title, "a", "").unwrap();
        }
        store.save().unwrap();
        let (seen, changed) = store.changes_since(0);
        assert_eq!(changed, Changed::Everything);
        assert_eq!(store.changes_since(seen).1, Changed::Ids(vec![]));
        let fresh = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(fresh.changes_since(seen).1, Changed::Everything);
        assert_eq!(fresh.changes_since(0).1, Changed::Everything);
    }

    #[test]
    fn refresh_keeps_the_feed_and_adds_what_it_read() {
        let (mut store, _d) = temp_store();
        for title in ["One", "Two", "Three"] {
            store.create_note(title, "a", "").unwrap();
        }
        store.save().unwrap();
        let mut reader = Reader::default();
        reader.catch_up(&store);
        let edited = store
            .notes
            .iter()
            .find(|n| n.title == "Two")
            .unwrap()
            .id
            .clone();
        append(&store.notes_dir.join("Two.md"), "from elsewhere");
        fs::write(store.notes_dir.join("Four.md"), headed("Four", "new")).unwrap();
        store.refresh().unwrap();
        let made = store
            .notes
            .iter()
            .find(|n| n.title == "Four")
            .unwrap()
            .id
            .clone();
        let Changed::Ids(ids) = store.changes_since(reader.seen).1 else {
            panic!("a refresh forgot the feed");
        };
        let ids: HashSet<String> = ids.into_iter().collect();
        assert_eq!(ids, HashSet::from([edited, made]));
        assert!(!reader.catch_up(&store));
        assert_eq!(reader.map, scan(&store));
    }

    #[test]
    #[ignore = "benchmark: run by hand with --ignored --nocapture"]
    fn p_c79_benchmark_change_feed() {
        use std::time::Instant;
        let count: usize = std::env::var("LEO_BENCH_NOTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let big = std::env::var("LEO_BENCH_BIG").is_ok_and(|v| v == "1");
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        fs::create_dir_all(&dir).unwrap();
        let body = "word ".repeat(1000);
        for i in 0..count {
            fs::write(
                dir.join(format!("Note {i}.md")),
                headed(&format!("Note {i}"), &body),
            )
            .unwrap();
        }
        if big {
            fs::write(
                dir.join("Big.md"),
                headed("Big", &"word ".repeat(4_000_000)),
            )
            .unwrap();
        }
        let mut store = Store::load_from(&dir).unwrap();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let mut full = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let mut sum = 0u64;
            for note in &store.notes {
                sum ^= hash_note(note);
            }
            std::hint::black_box(sum);
            full.push(ms(t.elapsed()));
        }
        let target = if big { "Big" } else { "Note 0" };
        let id = store
            .notes
            .iter()
            .find(|n| n.title == target)
            .unwrap()
            .id
            .clone();
        let mut one = Vec::new();
        for _ in 0..5 {
            let (seen, _) = store.changes_since(0);
            store.find_note_mut(&id).unwrap().body.push('x');
            let t = Instant::now();
            let (_, changed) = store.changes_since(seen);
            one.push(ms(t.elapsed()));
            assert_eq!(changed, Changed::Ids(vec![id.clone()]));
        }
        let ids: Vec<String> = store.notes.iter().map(|n| n.id.clone()).collect();
        let mut feed = Feed::default();
        let rounds = MOST_LOGGED * 16;
        let t = Instant::now();
        for i in 0..rounds {
            feed.record(ids[i % ids.len()].clone());
        }
        let record_us = t.elapsed().as_secs_f64() * 1e6 / rounds as f64;
        let mut whole = Vec::new();
        for _ in 0..5 {
            let (seen, _) = store.changes_since(0);
            for id in ids.iter().cycle().take(MOST_LOGGED) {
                store.feed.borrow_mut().record(id.clone());
            }
            let t = Instant::now();
            let (_, changed) = store.changes_since(seen);
            whole.push(ms(t.elapsed()));
            assert!(matches!(changed, Changed::Ids(_)));
        }
        let feed = store.feed.borrow();
        let bytes = feed.log.capacity() * std::mem::size_of::<(u64, String)>()
            + feed.log.iter().map(|(_, id)| id.capacity()).sum::<usize>();
        println!(
            "P-C79 notes={count} big={big} full_pass_ms={full:.2?} changes_since_one_ms={one:.4?} changes_since_full_log_ms={whole:.3?} record_us={record_us:.3} log_bytes={bytes} logged={}",
            feed.log.len()
        );
    }

    #[test]
    fn the_change_feed_matches_a_full_scan() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("notes");
        let folders = ["", "a", "b"];
        for folder in folders {
            fs::create_dir_all(dir.join(folder)).unwrap();
        }
        for i in 0..200 {
            fs::write(
                dir.join(folders[i % 3]).join(format!("Note {i}.md")),
                headed(&format!("Note {i}"), &format!("body {i}")),
            )
            .unwrap();
        }
        let mut store = Store::load_from(&dir).unwrap();
        let mut reader = Reader::default();
        reader.catch_up(&store);
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n.max(1) as u64) as usize
        };
        let titles = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"];
        for step in 0..2000 {
            let kind = next(14);
            let pick = next(store.notes.len());
            let id = store.notes.get(pick).map(|n| n.id.clone());
            let path = id
                .as_ref()
                .map(|id| store.notes_dir.join(&store.seen.borrow()[id].path));
            let folder = {
                let mut dirs = store.directories.clone();
                dirs.push(String::new());
                dirs[next(dirs.len())].clone()
            };
            match (kind, id, path) {
                (0, Some(id), _) => {
                    store.find_note_mut(&id).unwrap().body = format!("edit {step}");
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                (2, Some(id), _) => {
                    store.delete_notes(&[id]);
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                (3, Some(id), _) => {
                    store.find_note_mut(&id).unwrap().title = titles[next(titles.len())].into();
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                (4, Some(id), _) => {
                    if next(5) == 0 {
                        if store.dir_exists("b") {
                            store.rename_dir("b", "c");
                        } else if store.dir_exists("c") {
                            store.rename_dir("c", "b");
                        }
                    } else {
                        store.move_note(&id, &folder);
                    }
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                (5, Some(id), _) => {
                    let other = store.notes[next(store.notes.len())].id.clone();
                    store.delete_notes(&[id, other]);
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                (6, _, _) => {
                    let trashed = store.trashed();
                    if let Some(entry) = trashed.get(next(trashed.len())) {
                        store.restore_many(std::slice::from_ref(&entry.id));
                        agrees(&mut reader, &store, step);
                        store.save().unwrap();
                    }
                }
                (7, Some(_), Some(path)) => {
                    append(&path, &format!("outside {step}"));
                    store.refresh().unwrap();
                }
                (8, _, _) => {
                    let made = dir.join(&folder).join(format!("Outside {step}.md"));
                    fs::create_dir_all(made.parent().unwrap()).unwrap();
                    fs::write(&made, headed(&format!("Outside {step}"), "from elsewhere")).unwrap();
                    store.refresh().unwrap();
                }
                (9, Some(_), Some(path)) => {
                    fs::remove_file(&path).unwrap();
                    store.refresh().unwrap();
                }
                (10, Some(_), Some(path)) => {
                    fs::rename(&path, path.with_file_name(format!("Moved {step}.md"))).unwrap();
                    store.refresh().unwrap();
                }
                (11, Some(id), Some(path)) => {
                    store.find_note_mut(&id).unwrap().body = format!("mine {step}");
                    agrees(&mut reader, &store, step);
                    append(&path, &format!("theirs {step}"));
                    store.save().unwrap();
                }
                (12, Some(id), _) => {
                    store.find_note_mut(&id).unwrap().body = format!("lost {step}");
                    agrees(&mut reader, &store, step);
                    store.forget_changes();
                    store.refresh().unwrap();
                }
                (13, _, _) => {
                    let gone = format!("gone{step}");
                    store
                        .create_note(titles[next(titles.len())], "doomed", &gone)
                        .unwrap();
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                    agrees(&mut reader, &store, step);
                    store.delete_dir_recursive(&gone);
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
                _ => {
                    store
                        .create_note(titles[next(titles.len())], "made", &folder)
                        .unwrap();
                    agrees(&mut reader, &store, step);
                    store.save().unwrap();
                }
            }
            agrees(&mut reader, &store, step);
        }
    }

    fn agrees(reader: &mut Reader, store: &Store, step: usize) {
        reader.catch_up(store);
        assert_eq!(reader.map, scan(store), "step {step}");
    }
    const PICTURE: &str = "20261117-101500-slide.png";
    const OTHER: &str = "20261117-101500-board.png";

    fn picture_store() -> (Store, tempfile::TempDir) {
        let (store, tmp) = temp_store();
        let folder = store.notes_dir.join("attachments");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(PICTURE), b"png").unwrap();
        fs::write(folder.join(OTHER), b"png").unwrap();
        (store, tmp)
    }

    fn linking(name: &str) -> String {
        format!("Words\n\n![a picture](attachments/{name})\n")
    }

    fn own_pictures() -> Vec<String> {
        vec![PICTURE.to_string(), OTHER.to_string()]
    }

    fn due(store: &Store, now: DateTime<Utc>) -> Vec<String> {
        let target = (now - Utc::now()).num_seconds();
        if target > store.clock() {
            store.observe(now, Some(chrono::Duration::seconds(target - store.clock())));
        }
        store
            .picture_pass(&own_pictures(), now, Some(chrono::Duration::days(30)))
            .unwrap()
    }

    #[test]
    fn a_picture_is_counted_for_every_note_that_links_it() {
        let (mut store, _tmp) = picture_store();
        store.create_note("A", linking(PICTURE), "").unwrap();
        store
            .create_note("B", format!("![[{}]]", PICTURE.to_uppercase()), "Course")
            .unwrap();
        store.save().unwrap();
        assert_eq!(store.picture_owners(PICTURE), 2);
        assert_eq!(store.picture_owners(OTHER), 0);
    }

    #[test]
    fn a_picture_follows_its_note_into_the_trash_and_back() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert_eq!(
            store.picture_owners(PICTURE),
            1,
            "the trashed note still holds it"
        );
        assert_eq!(store.trashed_picture_counts()[&id], 1);
        let later = Utc::now() + chrono::Duration::days(400);
        assert!(!due(&store, later).contains(&PICTURE.to_string()));
        store.restore(&id).unwrap();
        store.save().unwrap();
        assert_eq!(store.picture_owners(PICTURE), 1);
        assert!(store.trashed_picture_counts().is_empty());
        assert!(!due(&store, later).contains(&PICTURE.to_string()));
    }

    #[test]
    fn deleting_a_note_for_good_makes_its_picture_due_now() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        store.delete_from_trash(std::slice::from_ref(&id));
        assert_eq!(store.picture_owners(PICTURE), 0);
        assert_eq!(due(&store, Utc::now()), [PICTURE]);
        store.picture_gone(PICTURE);
        assert!(!store.picture_still_due(PICTURE, Some(chrono::Duration::days(30))));
    }

    #[test]
    fn a_shared_picture_waits_for_its_last_owner() {
        let (mut store, _tmp) = picture_store();
        let a = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        let b = store
            .create_note("B", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(&[a.clone(), b.clone()]);
        store.save().unwrap();
        store.delete_from_trash(std::slice::from_ref(&a));
        assert!(
            due(&store, Utc::now()).is_empty(),
            "B in the trash still holds it"
        );
        store.restore(&b).unwrap();
        store.save().unwrap();
        assert_eq!(store.picture_owners(PICTURE), 1);
        assert!(due(&store, Utc::now()).is_empty());
    }

    #[test]
    fn removing_a_link_releases_the_picture_and_undo_takes_it_back() {
        let (mut store, _tmp) = picture_store();
        fs::remove_file(store.notes_dir.join("attachments").join(OTHER)).unwrap();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        assert_eq!(store.picture_owners(PICTURE), 0);
        assert!(
            due(&store, Utc::now()).is_empty(),
            "kept for the trash period"
        );
        assert!(
            store.picture_rows().is_empty(),
            "it shows in the trash only once it was moved there"
        );
        assert_eq!(
            due(&store, Utc::now() + chrono::Duration::days(31)),
            [OTHER, PICTURE]
        );
        store.find_note_mut(&id).unwrap().body = linking(PICTURE);
        store.save().unwrap();
        assert_eq!(
            due(&store, Utc::now() + chrono::Duration::days(31)),
            [OTHER]
        );
        assert!(store.picture_rows().is_empty());
    }

    #[test]
    fn restoring_a_picture_row_keeps_the_file_and_it_is_not_trashed_again() {
        let (mut store, _tmp) = picture_store();
        fs::remove_file(store.notes_dir.join("attachments").join(OTHER)).unwrap();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        let file = store.notes_dir.join("attachments").join(PICTURE);
        store.bin_picture(PICTURE, &file).unwrap();
        assert!(store.keep_picture(PICTURE));
        assert!(file.exists());
        assert!(store.picture_rows().is_empty());
        assert!(due(&store, Utc::now() + chrono::Duration::days(400)).is_empty());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded.picture_rows().is_empty());
        assert!(reloaded
            .picture_pass(
                &own_pictures(),
                Utc::now() + chrono::Duration::days(400),
                Some(chrono::Duration::days(30))
            )
            .unwrap()
            .iter()
            .all(|name| name != PICTURE));
    }

    #[test]
    fn the_picture_counts_survive_a_reload() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        store.delete_from_trash(std::slice::from_ref(&id));
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.picture_owners(PICTURE), 0);
        assert_eq!(
            due(&reloaded, Utc::now()),
            [PICTURE],
            "the deletion is remembered"
        );
    }

    #[test]
    fn a_damaged_picture_file_starts_empty_and_warns() {
        let (store, _tmp) = picture_store();
        let keeping = pictures_path(&store.notes_dir).unwrap();
        fs::create_dir_all(keeping.parent().unwrap()).unwrap();
        fs::write(keeping, "{not json").unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(crate::diag::recent()
            .iter()
            .any(|w| w.contains("pictures no note links")));
        assert!(reloaded.picture_rows().is_empty());
    }

    #[test]
    fn reconcile_adds_a_trashed_file_it_did_not_know_and_releases_a_vanished_one() {
        let (mut store, _tmp) = picture_store();
        store.create_note("Other", "", "").unwrap();
        let gone = store
            .create_note("Gone", linking(OTHER), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&gone));
        store.save().unwrap();
        trashed_file(
            &store,
            "Outside.md",
            &format!(
                "---\nid: outside\ntitle: Outside\n---\n{}",
                linking(PICTURE)
            ),
        );
        let now = Utc::now();
        assert_eq!(
            store.picture_pass(&own_pictures(), now, Some(chrono::Duration::days(30))),
            None,
            "an unread trashed file makes the pass unsure"
        );
        let owed = store.trash_reads_owed();
        assert_eq!(owed.len(), 1);
        let read = owed
            .into_iter()
            .map(|o| {
                let p = o.read().unwrap();
                (o, p)
            })
            .collect();
        store.settle_owed(read);
        assert!(due(&store, now).is_empty());
        assert_eq!(store.picture_owners(PICTURE), 1);
        for path in trash_files(&store) {
            if fs::read_to_string(&path).unwrap().contains(&gone) {
                fs::remove_file(path).unwrap();
            }
        }
        store.trash_may_have_changed();
        assert!(
            due(&store, now).is_empty(),
            "a vanished trashed file gives its pictures the period"
        );
        assert_eq!(due(&store, now + chrono::Duration::days(31)), [OTHER]);
    }

    #[test]
    fn a_trashed_file_with_an_unreadable_head_stops_the_picture_pass() {
        let (mut store, _tmp) = picture_store();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        trashed_file(&store, "Broken.md", "---\ntitle: [unclosed\n---\nbody\n");
        assert_eq!(
            store.picture_pass(
                &own_pictures(),
                Utc::now() + chrono::Duration::days(400),
                Some(chrono::Duration::days(30))
            ),
            None
        );
    }

    #[test]
    fn a_refresh_keeps_the_counts_of_unchanged_files() {
        let (mut store, _tmp) = picture_store();
        store.create_note("A", linking(PICTURE), "").unwrap();
        store.create_note("B", linking(OTHER), "").unwrap();
        store.save().unwrap();
        fs::write(
            store.notes_dir.join("B.md"),
            "---\nid: b\ntitle: B\n---\nno pictures\n",
        )
        .unwrap();
        store.refresh().unwrap();
        let fresh = Store::load_from(&store.notes_dir).unwrap();
        for name in [PICTURE, OTHER] {
            assert_eq!(
                store.picture_owners(name),
                fresh.picture_owners(name),
                "{name}"
            );
        }
        assert_eq!(store.picture_owners(PICTURE), 1);
        assert_eq!(store.picture_owners(OTHER), 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_trash_folder_keeps_its_notes_and_stops_picture_deletion() {
        use std::os::unix::fs::PermissionsExt;
        let (mut store, _tmp) = picture_store();
        store.create_note("Other", "", "").unwrap();
        let id = store
            .create_note("Deep", linking(PICTURE), "course")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        assert!(store.trashed_ids().contains(&id));
        assert!(store.trash_complete());
        let folder = store.notes_dir.join(".trash/course");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o000)).unwrap();
        store.trash_may_have_changed();
        let ids = store.trashed_ids();
        let complete = store.trash_complete();
        let pass = store.picture_pass(
            &own_pictures(),
            Utc::now(),
            Some(chrono::Duration::days(30)),
        );
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            ids.contains(&id),
            "a folder leo cannot read keeps its notes in the trash"
        );
        assert!(!complete);
        assert_eq!(pass, None);
        store.trash_may_have_changed();
        assert!(store.trashed_ids().contains(&id));
        assert!(store.trash_complete());
    }

    #[test]
    fn a_picture_named_in_front_matter_or_html_or_a_plain_link_is_counted() {
        let (mut store, _tmp) = picture_store();
        store
            .create_note("Html", format!("<img src=\"attachments/{PICTURE}\">"), "")
            .unwrap();
        store
            .create_note("Plain", format!("[see](attachments/{OTHER})"), "")
            .unwrap();
        store.save().unwrap();
        fs::write(
            store.notes_dir.join("Front.md"),
            format!("---\nid: front\ntitle: Front\ncover: attachments/{OTHER}\n---\nbody\n"),
        )
        .unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.picture_owners(PICTURE), 1);
        assert_eq!(reloaded.picture_owners(OTHER), 2);
    }

    #[test]
    fn a_clock_that_jumps_forward_does_not_ripen_a_picture() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        let period = Some(chrono::Duration::days(30));
        store.observe(Utc::now(), None);
        let jumped = Utc::now() + chrono::Duration::days(400);
        store.observe(jumped, None);
        let due = store.picture_pass(&own_pictures(), jumped, period).unwrap();
        assert!(
            !due.contains(&PICTURE.to_string()),
            "only a day was counted"
        );
        for _ in 0..30 {
            let_days_pass(&store, 1);
        }
        assert!(store.picture_still_due(PICTURE, period));
        assert!(
            !store.picture_still_due(PICTURE, None),
            "Forever stops timed expiry"
        );
    }

    #[test]
    fn a_link_put_back_outside_leo_takes_the_picture_off_the_trash_at_once() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        assert!(store.released_pictures().contains(&PICTURE.to_string()));
        let path = store.notes_dir.join("A.md");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{text}\n{}", linking(PICTURE))).unwrap();
        store.refresh().unwrap();
        assert!(!store.released_pictures().contains(&PICTURE.to_string()));
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(!reloaded.released_pictures().contains(&PICTURE.to_string()));
    }

    #[test]
    fn a_keeping_file_leo_cannot_read_expires_nothing() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "", "").unwrap().id.clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let_days_pass(&store, 400);
        fs::write(crate::keep::path_for(&store.notes_dir), "{not json").unwrap();
        store.trash_may_have_changed();
        assert_eq!(trash_days_now(&store.notes_dir), None);
        assert!(store.tidy_trash_now().is_empty());
        assert!(store.trashed_ids().contains(&id));
        fs::remove_file(crate::keep::path_for(&store.notes_dir)).unwrap();
        assert_eq!(trash_days_now(&store.notes_dir), Some(Some(30)));
        assert_eq!(store.tidy_trash_now(), [id]);
    }

    #[cfg(unix)]
    #[test]
    fn a_note_removed_outside_leo_is_gone_only_while_its_folder_is_still_there() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Lecture", "", "course")
            .unwrap()
            .id
            .clone();
        store.create_note("Kept", "", "course").unwrap();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        assert!(store.confirmed_gone().is_empty());
        fs::remove_file(store.notes_dir.join("course/Lecture.md")).unwrap();
        store.refresh().unwrap();
        assert!(store.confirmed_gone().contains(&id));
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(
            reloaded.confirmed_gone().contains(&id),
            "remembered across a restart"
        );
        fs::rename(
            store.notes_dir.join("course"),
            store.notes_dir.join(".away"),
        )
        .unwrap();
        store.refresh().unwrap();
        assert!(
            !store.confirmed_gone().contains(&id),
            "its folder is missing, as when a disk is unplugged"
        );
        fs::rename(
            store.notes_dir.join(".away"),
            store.notes_dir.join("course"),
        )
        .unwrap();
        store.refresh().unwrap();
        assert!(store.confirmed_gone().contains(&id));
        fs::write(
            store.notes_dir.join("Broken.md"),
            "---\ntitle: [unclosed\n---\n",
        )
        .unwrap();
        store.refresh().unwrap();
        assert!(
            store.confirmed_gone().is_empty(),
            "nothing is gone while a file cannot be read"
        );
        fs::remove_file(store.notes_dir.join("Broken.md")).unwrap();
        store.refresh().unwrap();
        store.forget_departed(std::slice::from_ref(&id));
        assert!(store.confirmed_gone().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_note_that_comes_back_or_was_only_renamed_is_never_gone() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        fs::write(store.notes_dir.join("Plain.md"), "no header here\n").unwrap();
        store.refresh().unwrap();
        let plain = store
            .notes
            .iter()
            .find(|n| n.title == "Plain")
            .unwrap()
            .id
            .clone();
        fs::rename(
            store.notes_dir.join("Plain.md"),
            store.notes_dir.join("Renamed.md"),
        )
        .unwrap();
        store.refresh().unwrap();
        assert!(store.notes.iter().all(|n| n.id != plain));
        assert!(
            !store.confirmed_gone().contains(&plain),
            "a renamed file is the same note"
        );
        let id = store
            .notes
            .iter()
            .find(|n| n.title == "Other")
            .unwrap()
            .id
            .clone();
        let text = fs::read_to_string(store.notes_dir.join("Other.md")).unwrap();
        fs::remove_file(store.notes_dir.join("Other.md")).unwrap();
        store.refresh().unwrap();
        assert!(store.confirmed_gone().contains(&id));
        fs::write(store.notes_dir.join("Other.md"), text).unwrap();
        store.refresh().unwrap();
        assert!(!store.confirmed_gone().contains(&id));
    }

    fn let_days_pass(store: &Store, days: i64) {
        store.trash_snapshot();
        store.observe(Utc::now(), Some(chrono::Duration::days(days)));
    }

    #[cfg(unix)]
    #[test]
    fn a_departure_keeps_the_device_seen_while_the_note_existed() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Removed", "", "").unwrap().id.clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        let current = device_of(&store.notes_dir).unwrap();
        store.seen.get_mut().get_mut(&id).unwrap().device = Some(current.wrapping_add(1));
        fs::remove_file(store.notes_dir.join("Removed.md")).unwrap();
        store.refresh().unwrap();
        assert!(!store.confirmed_gone().contains(&id));
        store
            .pictures
            .get_mut()
            .saved
            .gone
            .get_mut(&id)
            .unwrap()
            .device = Some(0);
        assert!(!store.confirmed_gone().contains(&id));
    }

    #[test]
    fn a_note_enters_trash_at_the_current_observed_time() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Removed", "", "").unwrap().id.clone();
        store.save().unwrap();
        store.trash_snapshot();
        store
            .clock_sample
            .set(std::time::Instant::now() - std::time::Duration::from_secs(3_600));
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        store.with_trash(false, |index| {
            assert!(index.files.values().next().unwrap().entered.unwrap() >= 3_600);
        });
        store.observe(
            Utc::now(),
            Some(chrono::Duration::days(30) - chrono::Duration::minutes(1)),
        );
        assert!(store.tidy_trash_now().is_empty());
    }

    #[test]
    fn a_divergent_trash_copy_survives_even_when_its_id_is_live() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "Original", "").unwrap().id.clone();
        store.save().unwrap();
        let path = store.notes_dir.join("Old.md");
        let original = fs::read_to_string(&path).unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let binned = trash_files(&store).pop().unwrap();
        fs::write(path, original.replace("Original", "New version")).unwrap();
        store.refresh().unwrap();
        let_days_pass(&store, 31);
        assert!(store.tidy_trash_now().is_empty());
        assert_eq!(fs::read_to_string(binned).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn an_incomplete_trash_scan_never_expires_known_paths() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "Original", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let path = trash_files(&store).pop().unwrap();
        let_days_pass(&store, 31);
        let unreadable = path.parent().unwrap().join("unreadable");
        fs::create_dir(&unreadable).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        store.tidy_trash_now();
        fs::set_permissions(unreadable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn a_trash_replacement_with_the_indexed_stamp_does_not_expire() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "old body", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let path = trash_files(&store).pop().unwrap();
        let_days_pass(&store, 31);
        let text = fs::read_to_string(&path).unwrap();
        let replacement = text.replace("old body", "new body");
        fs::write(&path, &replacement).unwrap();
        store.with_trash(false, |index| {
            index.files.values_mut().next().unwrap().stamp =
                stamp_of(&fs::metadata(&path).unwrap());
        });
        assert!(store.tidy_trash_now().is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), replacement);
    }

    #[test]
    fn automatic_expiry_keeps_a_trashed_note_with_an_unreadable_body() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "Original", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let path = trash_files(&store).pop().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(0xff);
        fs::write(&path, bytes).unwrap();
        let_days_pass(&store, 31);
        assert!(store.tidy_trash_now().is_empty());
        assert!(path.exists());
    }

    #[test]
    fn a_clock_that_jumps_forward_expires_no_trashed_note() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "", "").unwrap().id.clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        for path in trash_files(&store) {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(400 * 86_400),
                )
                .unwrap();
        }
        store.trash_may_have_changed();
        store.observe(Utc::now(), None);
        store.observe(Utc::now() + chrono::Duration::days(400), None);
        assert!(
            store.tidy_trash_now().is_empty(),
            "one day at most after a jump"
        );
        assert!(store.would_expire(30) == 0);
        for _ in 0..30 {
            let_days_pass(&store, 1);
        }
        assert_eq!(store.would_expire(30), 1);
        assert_eq!(store.tidy_trash_now(), [id]);
    }

    #[test]
    fn a_trash_choice_leo_does_not_offer_expires_nothing() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Old", "", "").unwrap().id.clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let_days_pass(&store, 400);
        for text in [
            r#"{"trash_days": 14}"#,
            r#"{"trash_days": "30"}"#,
            r#"{"trash_days": -1}"#,
        ] {
            fs::write(crate::keep::path_for(&store.notes_dir), text).unwrap();
            assert_eq!(trash_days_now(&store.notes_dir), None, "{text}");
            assert!(store.tidy_trash_now().is_empty(), "{text}");
        }
        fs::write(
            crate::keep::path_for(&store.notes_dir),
            r#"{"trash_days": null}"#,
        )
        .unwrap();
        assert_eq!(trash_days_now(&store.notes_dir), Some(None));
        assert!(store.tidy_trash_now().is_empty());
        fs::write(
            crate::keep::path_for(&store.notes_dir),
            r#"{"trash_days": 7}"#,
        )
        .unwrap();
        assert_eq!(store.tidy_trash_now(), [id]);
    }

    #[test]
    fn an_unlinked_picture_waits_in_the_trash_and_comes_back_when_linked() {
        let (mut store, _tmp) = picture_store();
        let id = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        let period = Some(chrono::Duration::days(30));
        let_days_pass(&store, 31);
        let due = store
            .picture_pass(&own_pictures(), Utc::now(), period)
            .unwrap();
        assert!(due.contains(&PICTURE.to_string()));
        assert!(!store.released_by_user(PICTURE));
        let file = store.notes_dir.join("attachments").join(PICTURE);
        store.bin_picture(PICTURE, &file).unwrap();
        let binned = store.notes_dir.join(".trash/attachments").join(PICTURE);
        assert!(!file.exists() && binned.exists());
        assert_eq!(
            store
                .picture_rows()
                .iter()
                .map(|r| r.title.as_str())
                .collect::<Vec<_>>(),
            [PICTURE]
        );
        assert!(
            store.ripe_binned(period).is_empty(),
            "the trash period starts again"
        );
        let_days_pass(&store, 31);
        assert_eq!(store.ripe_binned(period), [PICTURE]);
        assert!(store.ripe_binned(None).is_empty(), "Forever keeps it");
        store.find_note_mut(&id).unwrap().body = linking(PICTURE);
        store.save().unwrap();
        assert!(file.exists() && !binned.exists(), "a link brings it back");
        assert!(store.picture_rows().is_empty());
        store.find_note_mut(&id).unwrap().body = "Words".into();
        store.save().unwrap();
        store.bin_picture(PICTURE, &file).unwrap();
        fs::write(
            store.notes_dir.join("B.md"),
            format!("---\nid: b\ntitle: B\n---\n{}", linking(PICTURE)),
        )
        .unwrap();
        store.refresh().unwrap();
        assert!(
            file.exists(),
            "a link written outside leo brings it back on refresh"
        );
        store.find_note_mut("b").unwrap().body = "x".into();
        store.find_note_mut(&id).unwrap().body = "y".into();
        store.save().unwrap();
        store.bin_picture(PICTURE, &file).unwrap();
        assert!(store.keep_picture(PICTURE), "Restore from the Trash page");
        assert!(file.exists());
    }

    #[test]
    fn only_a_deletion_the_person_asked_for_lets_a_picture_go_in_one_step() {
        let (mut store, _tmp) = picture_store();
        let a = store
            .create_note("A", linking(PICTURE), "")
            .unwrap()
            .id
            .clone();
        let b = store
            .create_note("B", linking(OTHER), "")
            .unwrap()
            .id
            .clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        store.delete_notes(&[a.clone(), b.clone()]);
        store.save().unwrap();
        store.delete_from_trash(std::slice::from_ref(&a));
        assert!(store.released_by_user(PICTURE));
        let_days_pass(&store, 31);
        assert_eq!(store.tidy_trash_now(), [b]);
        assert!(!store.released_by_user(OTHER));
        let due = store
            .picture_pass(
                &own_pictures(),
                Utc::now(),
                Some(chrono::Duration::days(30)),
            )
            .unwrap();
        assert!(
            due.contains(&OTHER.to_string()),
            "it goes to the trash first"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_left_empty_as_by_an_unplugged_disk_ages_none_of_its_notes() {
        let (mut store, _tmp) = temp_store();
        let a = store.create_note("A", "", "drive").unwrap().id.clone();
        let b = store.create_note("B", "", "drive").unwrap().id.clone();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        fs::remove_file(store.notes_dir.join("drive/A.md")).unwrap();
        fs::remove_file(store.notes_dir.join("drive/B.md")).unwrap();
        store.refresh().unwrap();
        let gone = store.confirmed_gone();
        assert!(!gone.contains(&a) && !gone.contains(&b), "{gone:?}");
        let c = store.create_note("C", "", "drive").unwrap().id.clone();
        store.save().unwrap();
        store.refresh().unwrap();
        assert!(
            store.confirmed_gone().contains(&a),
            "the folder holds a note again"
        );
        assert!(!store.confirmed_gone().contains(&c));
    }

    fn replace_while_hashing(path: &Path, text: &'static str) {
        let replaced = path.to_path_buf();
        crate::paths::WHILE_HASHING.lock().unwrap().push((
            path.to_path_buf(),
            Box::new(move || {
                let temp = replaced.with_extension("synced");
                fs::write(&temp, text).unwrap();
                fs::rename(&temp, &replaced).unwrap();
            }),
        ));
    }

    #[test]
    fn a_trashed_note_replaced_while_it_is_hashed_is_kept() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Gone soon", "old", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        store.delete_notes(std::slice::from_ref(&id));
        store.save().unwrap();
        let trashed = store.notes_dir.join(".trash/Gone soon.md");
        assert_eq!(store.trashed().len(), 1);
        replace_while_hashing(&trashed, "synced while leo read the old one");
        assert_eq!(
            store.delete_from_trash(std::slice::from_ref(&id)).deleted,
            0
        );
        assert_eq!(
            fs::read_to_string(&trashed).unwrap(),
            "synced while leo read the old one"
        );
    }

    #[test]
    fn a_binned_picture_replaced_while_it_is_hashed_is_kept() {
        let (store, _tmp) = picture_store();
        let file = store.notes_dir.join("attachments").join(PICTURE);
        store.bin_picture(PICTURE, &file).unwrap();
        let binned = store.notes_dir.join(".trash/attachments").join(PICTURE);
        replace_while_hashing(&binned, "png");
        assert!(store.delete_binned_picture(PICTURE).is_err());
        assert!(binned.exists());
    }

    #[test]
    fn a_hidden_copy_survives_an_edit_during_its_final_verification() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Double", "original text", "").unwrap();
        store.save().unwrap();
        let live = store.notes_dir.join("Double.md");
        let trash = store.notes_dir.join(".trash/Double.md");
        fs::create_dir_all(trash.parent().unwrap()).unwrap();
        let text = fs::read_to_string(&live).unwrap();
        fs::write(&trash, &text).unwrap();
        store.trash_snapshot();
        let edited = text.replace("original text", "updated text");
        crate::paths::WHILE_HASHING.lock().unwrap().push((
            trash.clone(),
            Box::new(move || fs::write(live, edited).unwrap()),
        ));
        store.tidy_trash_now();
        assert_eq!(fs::read_to_string(trash).unwrap(), text);
    }

    #[test]
    fn a_synced_clock_cannot_age_a_local_trash_entry_on_restart() {
        let (mut store, _tmp) = temp_store();
        let id = store.create_note("Local", "", "").unwrap().id.clone();
        store.save().unwrap();
        store.delete_notes(&[id]);
        store.save().unwrap();
        store.trash_snapshot();
        let synced = store.notes_dir.join(".leo/pictures.json");
        fs::create_dir_all(synced.parent().unwrap()).unwrap();
        fs::write(synced, r#"{"clock":{"secs":3456000,"last":null}}"#).unwrap();
        let restarted = Store::load_from(&store.notes_dir).unwrap();
        assert!(restarted.tidy_trash_now().is_empty());
        assert!(store.notes_dir.join(".trash/Local.md").exists());
    }

    #[test]
    fn a_hidden_trash_copy_starts_its_clock_when_its_note_leaves() {
        let (mut store, _tmp) = temp_store();
        let id = store
            .create_note("Kept", "live text", "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let live = store.notes_dir.join("Kept.md");
        let trash = store.notes_dir.join(".trash");
        fs::create_dir_all(&trash).unwrap();
        let divergent = fs::read_to_string(&live)
            .unwrap()
            .replace("live text", "older text");
        fs::write(trash.join("Kept.md"), divergent).unwrap();
        store.trash_may_have_changed();
        store.refresh().unwrap();
        assert!(store.trashed().is_empty());
        let_days_pass(&store, 40);
        assert!(store.tidy_trash_now().is_empty());
        fs::remove_file(&live).unwrap();
        store.refresh().unwrap();
        assert_eq!(store.trashed().len(), 1);
        assert!(store.tidy_trash_now().is_empty(), "its clock starts now");
        assert!(trash.join("Kept.md").exists());
        let_days_pass(&store, 31);
        assert_eq!(store.tidy_trash_now(), [id]);
    }

    #[test]
    fn two_generations_first_seen_in_the_trash_mark_their_id_reused() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Live", "", "").unwrap();
        store.save().unwrap();
        let id = "a1b2c3d4-0000-4000-8000-0000000000aa";
        let trash = store.notes_dir.join(".trash");
        fs::create_dir_all(&trash).unwrap();
        for (file, created) in [
            ("One.md", "2024-01-01T00:00:00Z"),
            ("Two.md", "2025-01-01T00:00:00Z"),
        ] {
            fs::write(
                trash.join(file),
                format!("---\nid: {id}\ntitle: {file}\ncreated_at: {created}\n---\nbody\n"),
            )
            .unwrap();
        }
        store.trash_may_have_changed();
        store.refresh().unwrap();
        assert!(store.reused_id(id));
        assert!(store.removed_note_generation(id).is_none());
    }

    #[test]
    fn an_outside_edit_never_marks_a_note_reused() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Kept", "first", "").unwrap();
        store.save().unwrap();
        let id = store.notes[0].id.clone();
        let file = store.notes_dir.join("Kept.md");
        let text = fs::read_to_string(&file)
            .unwrap()
            .replace("first", "edited outside");
        fs::write(&file, text).unwrap();
        fs::write(store.notes_dir.join("Loose.md"), "one").unwrap();
        store.refresh().unwrap();
        let loose = store
            .notes
            .iter()
            .find(|n| n.title == "Loose")
            .unwrap()
            .id
            .clone();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(store.notes_dir.join("Loose.md"), "one, edited outside").unwrap();
        store.refresh().unwrap();
        assert!(!store.reused_id(&id) && !store.reused_id(&loose));
    }

    #[cfg(unix)]
    #[test]
    fn an_edited_headerless_note_moved_to_another_folder_is_unresolved() {
        let (mut store, _dir) = temp_store();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        let original = store.notes_dir.join("Loose.md");
        fs::write(&original, "First version").unwrap();
        store.refresh().unwrap();
        let id = store
            .notes
            .iter()
            .find(|note| note.title == "Loose")
            .unwrap()
            .id
            .clone();
        let folder = store.notes_dir.join("Moved");
        fs::create_dir(&folder).unwrap();
        let relocated = folder.join("New name.md");
        fs::rename(original, &relocated).unwrap();
        fs::write(relocated, "An edited version").unwrap();
        store.refresh().unwrap();
        assert!(!store.confirmed_gone().contains(&id));
    }

    #[test]
    fn a_headerless_note_renamed_and_edited_in_one_refresh_is_never_gone() {
        let (mut store, _tmp) = temp_store();
        store.create_note("Other", "", "").unwrap();
        store.save().unwrap();
        fs::write(store.notes_dir.join("Plain.md"), "first words\n").unwrap();
        store.refresh().unwrap();
        let plain = store
            .notes
            .iter()
            .find(|n| n.title == "Plain")
            .unwrap()
            .id
            .clone();
        fs::remove_file(store.notes_dir.join("Plain.md")).unwrap();
        fs::write(store.notes_dir.join("Renamed.md"), "first words, edited\n").unwrap();
        store.refresh().unwrap();
        assert!(!store.confirmed_gone().contains(&plain));
    }

    #[test]
    fn a_batch_keeps_the_picture_list_once_at_its_end() {
        let (mut store, _tmp) = picture_store();
        store.create_note("A", linking(PICTURE), "").unwrap();
        store.save().unwrap();
        let kept = pictures_path(&store.notes_dir).unwrap();
        let _ = fs::remove_file(&kept);
        store.picture_batch(|| {
            store.picture_gone(PICTURE);
            store.set_unused(BTreeMap::from([(
                OTHER.to_string(),
                crate::attachments::SeenPicture {
                    stamp: (1, 1),
                    digest: crate::paths::digest(b"png"),
                },
            )]));
            assert!(!kept.exists(), "nothing written inside the batch");
        });
        assert!(kept.exists());
    }
}
