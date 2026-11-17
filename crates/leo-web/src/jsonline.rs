use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;

const MOST_FILE_BYTES: u64 = 256 * 1024;
const MOST_FORGOTTEN_SCAN: usize = 100_000;

pub(crate) trait Line: Serialize + DeserializeOwned {
    const DIR: &'static str;
    fn id(&self) -> &str;
    fn stamp(&self) -> DateTime<Utc>;
}

pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn root(chats: &Path) -> &Path {
    chats.parent().unwrap_or(chats)
}

pub(crate) fn checked_dir(chats: &Path, kind: &str) -> anyhow::Result<PathBuf> {
    leo_core::paths::contained_path(root(chats), &Path::new("notes/.leo").join(kind))
}

pub(crate) fn file_path(chats: &Path, kind: &str, name: &str) -> anyhow::Result<PathBuf> {
    let dir = checked_dir(chats, kind)?;
    leo_core::paths::contained_path(&dir, Path::new(name))
}

pub(crate) fn read_file<T: Line>(chats: &Path, path: &Path) -> Option<T> {
    use std::io::Read;
    let checked = file_path(chats, T::DIR, path.file_name()?.to_str()?).ok()?;
    if checked != path {
        return None;
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MOST_FILE_BYTES {
        return None;
    }
    let file = leo_core::paths::open_regular(path).ok()?;
    let mut text = String::new();
    file.take(MOST_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MOST_FILE_BYTES {
        return None;
    }
    text.lines()
        .filter_map(|line| serde_json::from_str::<T>(line.trim()).ok())
        .max_by_key(|item| item.stamp())
}

pub(crate) fn write_file<T: Line>(chats: &Path, item: &T) -> anyhow::Result<()> {
    anyhow::ensure!(valid_id(item.id()), "That id is not valid.");
    let dir = checked_dir(chats, T::DIR)?;
    std::fs::create_dir_all(&dir)?;
    let line = serde_json::to_string(item)? + "\n";
    anyhow::ensure!(
        line.len() as u64 <= MOST_FILE_BYTES,
        "That is too large to save. Shorten its description and try again."
    );
    let path = file_path(chats, T::DIR, &format!("{}.json", item.id()))?;
    leo_core::paths::write_atomic(&path, line)
}

/// Every file of one kind, tombstones included. A file counts only when its
/// name is its id, so a copied or renamed file cannot pose as another item.
/// Boards pass no cap on the scan: deletes leave a tombstone behind (so a backup
/// merge cannot bring the item back), and a cap counted before tombstones are
/// dropped could silently hide live items. Each file read is size-bounded, and
/// the live limits (`MOST_BOARDS`, `MOST_CARDS`) apply after tombstones go.
pub(crate) fn read_all<T: Line>(chats: &Path, most: Option<usize>) -> Vec<T> {
    let Ok(dir) = checked_dir(chats, T::DIR) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".json")?.to_string();
            (!name.starts_with('.') && valid_id(&id)).then(|| (id, entry.path()))
        })
        .take(most.unwrap_or(usize::MAX))
        .filter_map(|(id, path)| read_file::<T>(chats, &path).filter(|item| item.id() == id))
        .collect()
}

pub(crate) fn forget_deleted<T: Line>(
    chats: &Path,
    before: DateTime<Utc>,
    deleted: impl Fn(&T) -> bool,
) -> usize {
    let Ok(entries) = checked_dir(chats, T::DIR).and_then(|dir| Ok(std::fs::read_dir(dir)?)) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .take(MOST_FORGOTTEN_SCAN)
        .filter(|path| {
            read_file::<T>(chats, path).is_some_and(|item| {
                deleted(&item)
                    && item.stamp() < before
                    && path.file_stem().and_then(|stem| stem.to_str()) == Some(item.id())
            })
        })
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count()
}

pub(crate) fn load<T: Line>(chats: &Path, id: &str) -> Option<T> {
    if !valid_id(id) {
        return None;
    }
    let path = file_path(chats, T::DIR, &format!("{id}.json")).ok()?;
    read_file::<T>(chats, &path).filter(|item| item.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, serde::Deserialize)]
    struct Item {
        id: String,
        title: String,
        at: DateTime<Utc>,
        #[serde(default)]
        deleted: bool,
    }

    impl Line for Item {
        const DIR: &'static str = "items";
        fn id(&self) -> &str {
            &self.id
        }
        fn stamp(&self) -> DateTime<Utc> {
            self.at
        }
    }

    fn item(title: &str, at: DateTime<Utc>, deleted: bool) -> Item {
        Item {
            id: "item-0001".into(),
            title: title.into(),
            at,
            deleted,
        }
    }

    fn lines(chats: &Path, items: &[&Item], tail: &str) {
        let path = file_path(chats, "items", "item-0001.json").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text: String = items
            .iter()
            .map(|i| serde_json::to_string(i).unwrap() + "\n")
            .collect();
        text.push_str(tail);
        std::fs::write(path, text).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_save_whose_folder_cannot_be_flushed_reports_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let folder = checked_dir(&chats, "items").unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o300)).unwrap();
        let saved = write_file(&chats, &item("Saved", Utc::now(), false));
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(saved.is_err());
    }

    #[test]
    fn a_torn_last_line_leaves_the_newest_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let newer = serde_json::to_string(&item("Torn", now, false)).unwrap();
        let torn = &newer[..newer.len() / 2];
        lines(
            &chats,
            &[&item("Whole", now - chrono::Duration::seconds(5), false)],
            torn,
        );
        assert_eq!(load::<Item>(&chats, "item-0001").unwrap().title, "Whole");
        assert_eq!(read_all::<Item>(&chats, None).len(), 1);
    }

    #[test]
    fn a_newer_tombstone_wins_over_a_live_line() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        lines(
            &chats,
            &[
                &item("Live", now - chrono::Duration::seconds(5), false),
                &item("Live", now, true),
            ],
            "",
        );
        assert!(load::<Item>(&chats, "item-0001").unwrap().deleted);
        write_file(
            &chats,
            &item("Back", now + chrono::Duration::seconds(5), false),
        )
        .unwrap();
        assert!(!load::<Item>(&chats, "item-0001").unwrap().deleted);
    }
}
