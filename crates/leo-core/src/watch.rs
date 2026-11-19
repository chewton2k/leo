use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const MOST_LEO_PATHS: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub enum LeoChanged {
    Nothing,
    Paths(Vec<PathBuf>),
    All,
}

pub struct Leo {
    seen: Mutex<Option<HashSet<PathBuf>>>,
    broken: Arc<AtomicBool>,
}

impl Leo {
    fn add(&self, path: PathBuf) {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        match seen.as_mut() {
            Some(paths) if paths.len() < MOST_LEO_PATHS => {
                paths.insert(path);
            }
            _ => *seen = None,
        }
    }

    fn everything(&self) {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn take(&self) -> LeoChanged {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        match seen.replace(HashSet::new()) {
            None => LeoChanged::All,
            Some(paths) if paths.is_empty() => LeoChanged::Nothing,
            Some(paths) => LeoChanged::Paths(paths.into_iter().collect()),
        }
    }

    pub fn trusted(&self) -> bool {
        !cfg!(windows) && !self.broken.load(Ordering::Relaxed)
    }
}

pub struct Watch {
    _watcher: RecommendedWatcher,
    touched: Arc<AtomicBool>,
    broken: Arc<AtomicBool>,
    trash: Arc<AtomicBool>,
    leo: Arc<Leo>,
}

impl Watch {
    pub fn start(notes_dir: &Path) -> Option<Watch> {
        let touched = Arc::new(AtomicBool::new(false));
        let broken = Arc::new(AtomicBool::new(false));
        let trash = Arc::new(AtomicBool::new(false));
        let root = notes_dir.to_path_buf();
        let real = notes_dir.canonicalize().unwrap_or_else(|_| root.clone());
        let leo = Arc::new(Leo {
            seen: Mutex::new(Some(HashSet::new())),
            broken: Arc::clone(&broken),
        });
        let (seen, failed, told) = (Arc::clone(&touched), Arc::clone(&broken), Arc::clone(&leo));
        let binned = Arc::clone(&trash);
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| match event {
                Ok(event) => {
                    failed.store(false, Ordering::Relaxed);
                    if event.need_rescan() {
                        told.everything();
                        binned.store(true, Ordering::Relaxed);
                    }
                    let mut counted = event.need_rescan();
                    for path in &event.paths {
                        let path = rebased(&root, &real, path);
                        if path.starts_with(root.join(".leo")) {
                            told.add(path);
                        } else {
                            if path.starts_with(root.join(".trash")) {
                                binned.store(true, Ordering::Relaxed);
                            }
                            counted |= counts(&root, &path);
                        }
                    }
                    if counted {
                        seen.store(true, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    failed.store(true, Ordering::Relaxed);
                    seen.store(true, Ordering::Relaxed);
                    binned.store(true, Ordering::Relaxed);
                    told.everything();
                }
            },
            notify::Config::default().with_follow_symlinks(false),
        )
        .ok()?;
        watcher.watch(notes_dir, RecursiveMode::Recursive).ok()?;
        Some(Watch {
            _watcher: watcher,
            touched,
            broken,
            trash,
            leo,
        })
    }

    pub fn leo(&self) -> Arc<Leo> {
        Arc::clone(&self.leo)
    }

    pub fn take_leo(&self) -> LeoChanged {
        self.leo.take()
    }

    pub fn take(&self) -> bool {
        self.touched.swap(false, Ordering::Relaxed)
    }

    pub fn take_trash(&self) -> bool {
        self.trash.swap(false, Ordering::Relaxed)
    }

    pub fn broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }
}

fn rebased(root: &Path, real: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(real) {
        Ok(inside) if real != root => root.join(inside),
        _ => path.to_path_buf(),
    }
}

fn counts(root: &Path, path: &Path) -> bool {
    let Ok(inside) = path.strip_prefix(root) else {
        return true;
    };
    !inside.components().any(|part| match part {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watcher_error_clears_once_events_flow_again() {
        let root = tempfile::tempdir().unwrap();
        let watch = Watch::start(root.path()).unwrap();
        watch.broken.store(true, Ordering::Relaxed);
        watch.touched.store(true, Ordering::Relaxed);
        assert!(watch.take());
        std::fs::write(root.path().join("After.md"), "x").unwrap();
        let start = std::time::Instant::now();
        while !watch.take() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !watch.broken(),
            "after one Err the watch stays broken through later events and takes"
        );
    }

    #[test]
    fn hidden_folders_inside_the_notes_do_not_count_and_everything_else_does() {
        let root = Path::new("/notes");
        assert!(counts(root, Path::new("/notes/Ideas.md")));
        assert!(counts(root, Path::new("/notes/work/Plan.md")));
        assert!(counts(root, Path::new("/notes/work")));
        assert!(counts(root, Path::new("/elsewhere/Plan.md")));
        assert!(!counts(root, Path::new("/notes/.git/index")));
        assert!(!counts(root, Path::new("/notes/.trash/Old.md")));
        assert!(!counts(
            root,
            Path::new("/notes/work/.obsidian/workspace.json")
        ));
    }

    #[test]
    fn a_change_under_leos_folder_is_reported_with_its_path_and_does_not_mark_the_notes() {
        let root = tempfile::tempdir().unwrap();
        let cards = root.path().join(".leo/cards");
        std::fs::create_dir_all(&cards).unwrap();
        let watch = Watch::start(root.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = watch.take();
        let _ = watch.take_leo();
        std::fs::write(cards.join("c0000001.json"), "{}").unwrap();
        let start = std::time::Instant::now();
        let paths = loop {
            match watch.take_leo() {
                LeoChanged::Paths(paths) => break paths,
                LeoChanged::All => break Vec::new(),
                LeoChanged::Nothing => {}
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(
            paths
                .iter()
                .any(|path| path == &root.path().join(".leo/cards/c0000001.json")),
            "{paths:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!watch.take(), "a change under .leo marked the notes");
    }

    #[test]
    fn a_change_in_the_trash_is_reported_on_its_own_and_does_not_mark_the_notes() {
        let root = tempfile::tempdir().unwrap();
        let trash = root.path().join(".trash");
        std::fs::create_dir_all(&trash).unwrap();
        let watch = Watch::start(root.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = watch.take();
        let _ = watch.take_trash();
        std::fs::write(trash.join("Old.md"), "old").unwrap();
        let start = std::time::Instant::now();
        while !watch.take_trash() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!watch.take(), "a change in the trash marked the notes");
        assert_eq!(watch.take_leo(), LeoChanged::Nothing);
    }

    #[test]
    fn many_changes_at_once_are_reported_as_all() {
        let leo = Leo {
            seen: Mutex::new(Some(HashSet::new())),
            broken: Arc::default(),
        };
        assert_eq!(leo.take(), LeoChanged::Nothing);
        for index in 0..MOST_LEO_PATHS {
            leo.add(PathBuf::from(format!("/n/.leo/cards/c{index}.json")));
        }
        leo.add(PathBuf::from("/n/.leo/cards/one-more.json"));
        assert_eq!(leo.take(), LeoChanged::All);
        assert_eq!(leo.take(), LeoChanged::Nothing);
        leo.add(PathBuf::from("/n/.leo/cards/a.json"));
        assert_eq!(
            leo.take(),
            LeoChanged::Paths(vec![PathBuf::from("/n/.leo/cards/a.json")])
        );
    }
}
