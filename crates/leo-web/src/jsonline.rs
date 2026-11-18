use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use leo_core::watch::{Leo, LeoChanged};
use serde::de::DeserializeOwned;
use serde::Serialize;

const MOST_FILE_BYTES: u64 = 256 * 1024;
const MOST_FORGOTTEN_SCAN: usize = 100_000;
const MOST_WAITING: usize = 4096;
const LOOK_AGAIN: Duration = Duration::from_secs(60);

#[cfg(test)]
thread_local! {
    pub(crate) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static LISTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) trait Line: Serialize + DeserializeOwned {
    const DIR: &'static str;
    fn id(&self) -> &str;
    fn stamp(&self) -> DateTime<Utc>;
    fn deleted_at(&mut self, now: DateTime<Utc>);
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

pub(crate) type Stamp = (u64, u128, i64);

fn stamp_meta(meta: &std::fs::Metadata) -> Stamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        meta.ctime() * 1_000_000_000 + meta.ctime_nsec()
    };
    #[cfg(not(unix))]
    let changed = 0;
    (meta.len(), modified, changed)
}

#[derive(Default)]
enum Waiting {
    #[default]
    Nothing,
    Paths(HashSet<PathBuf>),
    All,
}

impl Waiting {
    fn add(&mut self, path: PathBuf) {
        match self {
            Waiting::Nothing => *self = Waiting::Paths(HashSet::from([path])),
            Waiting::Paths(paths) if paths.len() < MOST_WAITING => {
                paths.insert(path);
            }
            _ => *self = Waiting::All,
        }
    }
}

#[derive(Default)]
pub(crate) struct Hub {
    leo: Mutex<Option<Arc<Leo>>>,
    waiting: Mutex<HashMap<String, Waiting>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Hub {
    fn trusted(&self) -> bool {
        lock(&self.leo).as_ref().is_some_and(|leo| leo.trusted())
    }

    fn wrote(&self, kind: &str, path: PathBuf) {
        lock(&self.waiting)
            .entry(kind.to_string())
            .or_default()
            .add(path);
    }

    pub(crate) fn everything(&self) {
        for waiting in lock(&self.waiting).values_mut() {
            *waiting = Waiting::All;
        }
    }

    fn take(&self, kind: &str) -> Waiting {
        let said = lock(&self.leo).as_ref().map(|leo| leo.take());
        let mut waiting = lock(&self.waiting);
        match said {
            Some(LeoChanged::All) => {
                for waiting in waiting.values_mut() {
                    *waiting = Waiting::All;
                }
            }
            Some(LeoChanged::Paths(paths)) => {
                let name = |path: Option<&Path>| {
                    path.and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                };
                for path in paths {
                    let parent = path.parent();
                    if name(Some(&path)).as_deref() == Some(".leo") {
                        for waiting in waiting.values_mut() {
                            *waiting = Waiting::All;
                        }
                    } else if name(parent).as_deref() == Some(".leo") {
                        if let Some(kind) =
                            name(Some(&path)).and_then(|kind| waiting.get_mut(&kind))
                        {
                            *kind = Waiting::All;
                        }
                    } else if name(parent.and_then(Path::parent)).as_deref() == Some(".leo") {
                        if let Some(kind) = name(parent) {
                            waiting.entry(kind).or_default().add(path);
                        }
                    }
                }
            }
            _ => {}
        }
        std::mem::take(waiting.entry(kind.to_string()).or_default())
    }
}

fn hubs() -> &'static Mutex<HashMap<PathBuf, Arc<Hub>>> {
    static HUBS: OnceLock<Mutex<HashMap<PathBuf, Arc<Hub>>>> = OnceLock::new();
    HUBS.get_or_init(Default::default)
}

pub(crate) fn hub(chats: &Path) -> Arc<Hub> {
    hub_at(root(chats))
}

fn hub_at(data: &Path) -> Arc<Hub> {
    Arc::clone(lock(hubs()).entry(data.to_path_buf()).or_default())
}

pub(crate) fn watched(notes: &Path, leo: Arc<Leo>) {
    if let Some(data) = notes.parent() {
        *lock(&hub_at(data).leo) = Some(leo);
    }
}

struct Held<T> {
    items: HashMap<String, (Stamp, T)>,
    full_at: Option<Instant>,
}

pub(crate) struct Index<T> {
    held: Mutex<Held<T>>,
    generation: AtomicU64,
}

impl<T> Default for Index<T> {
    fn default() -> Self {
        Self {
            held: Mutex::new(Held {
                items: HashMap::new(),
                full_at: None,
            }),
            generation: AtomicU64::new(0),
        }
    }
}

impl<T: Line> Index<T> {
    pub(crate) fn view<R>(
        &self,
        chats: &Path,
        look: impl FnOnce(&HashMap<String, (Stamp, T)>) -> R,
    ) -> R {
        let mut held = lock(&self.held);
        self.refresh(&mut held, chats);
        look(&held.items)
    }

    pub(crate) fn generation(&self, chats: &Path) -> u64 {
        let mut held = lock(&self.held);
        self.refresh(&mut held, chats);
        self.generation.load(Ordering::Acquire)
    }

    fn refresh(&self, held: &mut Held<T>, chats: &Path) {
        let hub = hub(chats);
        let waiting = hub.take(T::DIR);
        let Ok(dir) = checked_dir(chats, T::DIR) else {
            if !held.items.is_empty() {
                held.items.clear();
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            return;
        };
        let everything = held.full_at.is_none_or(|at| at.elapsed() >= LOOK_AGAIN)
            || !hub.trusted()
            || matches!(waiting, Waiting::All);
        let mut changed = false;
        if everything {
            changed = self.list(held, &dir);
        } else if let Waiting::Paths(paths) = waiting {
            for path in paths {
                changed |= self.one(held, &dir, &path);
            }
        }
        if changed {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn list(&self, held: &mut Held<T>, dir: &Path) -> bool {
        #[cfg(test)]
        LISTED.with(|listed| listed.set(listed.get() + 1));
        held.full_at = Some(Instant::now());
        let Ok(entries) = std::fs::read_dir(dir) else {
            return !std::mem::take(&mut held.items).is_empty();
        };
        let mut seen = HashSet::with_capacity(held.items.len());
        let mut changed = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Some(meta) = entry.metadata().ok().filter(|meta| meta.is_file()) else {
                continue;
            };
            seen.insert(id.to_string());
            changed |= self.keep(held, dir, id, &entry.path(), stamp_meta(&meta));
        }
        let before = held.items.len();
        held.items.retain(|id, _| seen.contains(id));
        changed || held.items.len() != before
    }

    fn one(&self, held: &mut Held<T>, dir: &Path, path: &Path) -> bool {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
        else {
            return false;
        };
        let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) else {
            return false;
        };
        let path = dir.join(&name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => self.keep(held, dir, id, &path, stamp_meta(&meta)),
            _ => held.items.remove(id).is_some(),
        }
    }

    fn keep(&self, held: &mut Held<T>, dir: &Path, id: &str, path: &Path, stamp: Stamp) -> bool {
        if held.items.get(id).is_some_and(|(held, _)| *held == stamp) {
            return false;
        }
        match read_in::<T>(dir, path).filter(|item| item.id() == id) {
            Some(item) => {
                held.items.insert(id.to_string(), (stamp, item));
                true
            }
            None => held.items.remove(id).is_some(),
        }
    }
}

pub(crate) fn file_path(chats: &Path, kind: &str, name: &str) -> anyhow::Result<PathBuf> {
    let dir = checked_dir(chats, kind)?;
    leo_core::paths::contained_path(&dir, Path::new(name))
}

pub(crate) fn read_file<T: Line>(chats: &Path, path: &Path) -> Option<T> {
    let checked = file_path(chats, T::DIR, path.file_name()?.to_str()?).ok()?;
    if checked != path {
        return None;
    }
    read_checked(path)
}

fn read_in<T: Line>(dir: &Path, path: &Path) -> Option<T> {
    let checked = leo_core::paths::contained_path(dir, Path::new(path.file_name()?)).ok()?;
    if checked != path {
        return None;
    }
    read_checked(path)
}

fn read_checked<T: Line>(path: &Path) -> Option<T> {
    use std::io::Read;
    #[cfg(test)]
    READS.with(|reads| reads.set(reads.get() + 1));
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
    leo_core::paths::write_atomic(&path, line)?;
    hub(chats).wrote(T::DIR, path);
    Ok(())
}

pub(crate) fn write_gone<T: Line + Clone>(
    chats: &Path,
    item: &T,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut gone = item.clone();
    gone.deleted_at(now);
    write_file(chats, &gone)
}

/// Every file of one kind, tombstones included. A file counts only when its
/// name is its id, so a copied or renamed file cannot pose as another item.
/// Boards pass no cap on the scan: deletes leave a tombstone behind (so a backup
/// merge cannot bring the item back), and a cap counted before tombstones are
/// dropped could silently hide live items. Each file read is size-bounded.
pub(crate) fn read_all<T: Line>(chats: &Path, most: Option<usize>) -> Vec<T> {
    let Ok(dir) = checked_dir(chats, T::DIR) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
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
        .filter_map(|(id, path)| read_in::<T>(&dir, &path).filter(|item| item.id() == id))
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
        .inspect(|path| hub(chats).wrote(T::DIR, path.clone()))
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

    #[derive(Clone, Serialize, serde::Deserialize)]
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
        fn deleted_at(&mut self, now: DateTime<Utc>) {
            self.deleted = true;
            self.at = now;
        }
    }

    #[test]
    fn a_tombstone_keeps_everything_but_the_mark_and_the_time() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let made = Utc::now() - chrono::Duration::days(3);
        let now = Utc::now();
        let live = item("Keep these words", made, false);
        write_file(&chats, &live).unwrap();
        write_gone(&chats, &live, now).unwrap();
        let path = file_path(&chats, "items", "item-0001.json").unwrap();
        let back: Item = read_file(&chats, &path).unwrap();
        assert_eq!(
            (back.id.as_str(), back.title.as_str(), back.at, back.deleted),
            ("item-0001", "Keep these words", now, true)
        );
        assert!(!live.deleted && live.at == made);
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

    #[cfg(unix)]
    #[test]
    fn a_scan_skips_a_linked_file_and_a_dot_name() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        write_file(&chats, &item("Kept", Utc::now(), false)).unwrap();
        let folder = checked_dir(&chats, "items").unwrap();
        let outside = dir.path().join("outside.json");
        std::fs::write(
            &outside,
            serde_json::to_string(&Item {
                id: "item-0002".into(),
                ..item("Linked", Utc::now(), false)
            })
            .unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("item-0002.json")).unwrap();
        std::fs::copy(
            folder.join("item-0001.json"),
            folder.join(".item-0003.json"),
        )
        .unwrap();
        let found = read_all::<Item>(&chats, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Kept");
    }

    fn named(id: &str, title: &str) -> Item {
        Item {
            id: id.into(),
            ..item(title, Utc::now(), false)
        }
    }

    fn titles(index: &Index<Item>, chats: &Path) -> Vec<String> {
        let mut found: Vec<String> = index.view(chats, |held| {
            held.values().map(|(_, item)| item.title.clone()).collect()
        });
        found.sort();
        found
    }

    fn counted(work: impl FnOnce()) -> (usize, usize) {
        READS.with(|reads| reads.set(0));
        LISTED.with(|listed| listed.set(0));
        work();
        (READS.with(|r| r.get()), LISTED.with(|l| l.get()))
    }

    fn watching(dir: &Path) -> leo_core::watch::Watch {
        let notes = dir.join("notes");
        std::fs::create_dir_all(notes.join(".leo")).unwrap();
        let watch = leo_core::watch::Watch::start(&notes).unwrap();
        watched(&notes, watch.leo());
        watch
    }

    fn until(mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(start.elapsed() < Duration::from_secs(20), "never seen");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_quiet_folder_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let _watch = watching(dir.path());
        write_file(&chats, &named("item-0001", "One")).unwrap();
        let index = Index::<Item>::default();
        assert_eq!(counted(|| drop(titles(&index, &chats))), (1, 1));
        std::thread::sleep(Duration::from_millis(500));
        let _ = titles(&index, &chats);
        let (_, listed) = counted(|| {
            for _ in 0..3 {
                let _ = titles(&index, &chats);
            }
        });
        assert_eq!(listed, 0);
    }

    #[test]
    fn a_change_the_watcher_reports_is_read_without_listing_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let _watch = watching(dir.path());
        write_file(&chats, &named("item-0001", "One")).unwrap();
        let index = Index::<Item>::default();
        let _ = titles(&index, &chats);
        std::thread::sleep(Duration::from_millis(500));
        let _ = titles(&index, &chats);
        let folder = checked_dir(&chats, "items").unwrap();
        std::fs::write(
            folder.join("item-0002.json"),
            serde_json::to_string(&named("item-0002", "Two")).unwrap(),
        )
        .unwrap();
        let (mut reads, mut listed) = (0, 0);
        until(|| {
            let (r, l) = counted(|| assert!(titles(&index, &chats).len() <= 2));
            reads += r;
            listed += l;
            titles(&index, &chats).len() == 2
        });
        assert_eq!(listed, 0);
        assert_eq!(reads, 1);
    }

    #[test]
    fn an_unchanged_file_is_not_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        write_file(&chats, &named("item-0001", "One")).unwrap();
        write_file(&chats, &named("item-0002", "Two")).unwrap();
        let index = Index::<Item>::default();
        assert_eq!(counted(|| drop(titles(&index, &chats))), (2, 1));
        assert_eq!(counted(|| drop(titles(&index, &chats))), (0, 1));
    }

    #[test]
    fn a_file_rewritten_in_place_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        write_file(&chats, &named("item-0001", "Aaa")).unwrap();
        let index = Index::<Item>::default();
        assert_eq!(titles(&index, &chats), ["Aaa"]);
        let before = index.generation(&chats);
        std::thread::sleep(Duration::from_millis(20));
        let path = file_path(&chats, "items", "item-0001.json").unwrap();
        let mut text = std::fs::read_to_string(&path).unwrap();
        text = text.replace("Aaa", "Bbb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(titles(&index, &chats), ["Bbb"]);
        assert!(index.generation(&chats) > before);
    }

    #[test]
    fn a_removed_file_leaves_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        write_file(&chats, &named("item-0001", "One")).unwrap();
        write_file(&chats, &named("item-0002", "Two")).unwrap();
        let index = Index::<Item>::default();
        assert_eq!(titles(&index, &chats), ["One", "Two"]);
        std::fs::remove_file(file_path(&chats, "items", "item-0001.json").unwrap()).unwrap();
        assert_eq!(titles(&index, &chats), ["Two"]);
    }

    #[test]
    fn a_change_to_a_task_file_is_still_seen_after_the_cards_were_read() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let _watch = watching(dir.path());
        let leo = checked_dir(&chats, "cards")
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(leo.join("cards")).unwrap();
        std::fs::create_dir_all(leo.join("tasks")).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let hub = hub(&chats);
        let _ = hub.take("cards");
        let _ = hub.take("tasks");
        std::fs::write(leo.join("cards/c0000001.json"), "{}").unwrap();
        std::fs::write(leo.join("tasks/t0000001.json"), "{}").unwrap();
        until(|| {
            std::thread::sleep(Duration::from_millis(300));
            let cards = hub.take("cards");
            let tasks = hub.take("tasks");
            let said = |waiting: &Waiting, name: &str| match waiting {
                Waiting::Paths(paths) => paths.iter().any(|p| p.ends_with(name)),
                Waiting::All => true,
                Waiting::Nothing => false,
            };
            said(&cards, "c0000001.json") && said(&tasks, "t0000001.json")
        });
    }

    #[test]
    #[ignore = "Windows can lose events: run by hand, and on the Windows runner"]
    fn a_burst_of_changes_under_leos_folder_is_never_missed() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let _watch = watching(dir.path());
        let folder = checked_dir(&chats, "items").unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let index = Index::<Item>::default();
        assert!(titles(&index, &chats).is_empty());
        for number in 0..2000 {
            let id = format!("item-{number:05}");
            std::fs::write(
                folder.join(format!("{id}.json")),
                serde_json::to_string(&named(&id, "Burst")).unwrap(),
            )
            .unwrap();
        }
        until(|| titles(&index, &chats).len() == 2000);
    }
}
