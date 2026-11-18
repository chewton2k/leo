use std::path::Path;
use std::time::{Duration, Instant};

use leo_core::watch::Watch;

const RECONCILE: Duration = Duration::from_secs(60);
const BROKEN_RECHECK: Duration = Duration::from_secs(2);

pub struct Changes {
    watch: Option<Watch>,
    checked: Option<Instant>,
    #[cfg(test)]
    delayed: bool,
}

impl Changes {
    pub fn new(notes: &Path) -> Self {
        let watch = Watch::start(notes);
        if let Some(watch) = &watch {
            crate::jsonline::watched(notes, watch.leo());
        }
        Self {
            watch,
            checked: None,
            #[cfg(test)]
            delayed: false,
        }
    }

    pub fn due(&mut self) -> bool {
        #[cfg(test)]
        if self.delayed {
            return false;
        }
        let now = Instant::now();
        let (touched, healthy) = self
            .watch
            .as_ref()
            .map(|watch| signals(self.checked, now, watch.take(), watch.broken()))
            .unwrap_or((false, false));
        let due = needed(self.checked, now, touched, healthy);
        if due {
            self.checked = Some(now);
        }
        due
    }

    #[cfg(test)]
    pub fn delayed() -> Self {
        Self {
            watch: None,
            checked: None,
            delayed: true,
        }
    }
}

fn signals(checked: Option<Instant>, now: Instant, touched: bool, broken: bool) -> (bool, bool) {
    let stale = checked.is_none_or(|last| now.saturating_duration_since(last) >= BROKEN_RECHECK);
    (touched || (broken && stale), true)
}

fn needed(checked: Option<Instant>, now: Instant, touched: bool, healthy: bool) -> bool {
    !healthy
        || touched
        || checked.is_none_or(|last| now.saturating_duration_since(last) >= RECONCILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "benchmark: run by hand with --ignored"]
    fn benchmark_unchanged_note_scans() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..1000 {
            std::fs::write(
                root.path().join(format!("Note {index}.md")),
                "Benchmark fixture",
            )
            .unwrap();
        }
        let store = leo_core::store::Store::load_from(root.path()).unwrap();
        let mut changes = Changes::new(root.path());
        assert!(changes.watch.is_some());
        for trial in 0..5 {
            let start = Instant::now();
            for _ in 0..100 {
                assert!(!std::hint::black_box(store.changed_on_disk()));
            }
            let before = start.elapsed();
            let start = Instant::now();
            let mut scans = 0;
            for _ in 0..100 {
                if changes.due() {
                    scans += 1;
                    assert!(!std::hint::black_box(store.changed_on_disk()));
                }
            }
            println!("trial={trial} notes=1000 requests=100 before_us={} after_us={} after_scans={scans}", before.as_micros(), start.elapsed().as_micros());
        }
    }

    #[test]
    fn unchanged_request_bursts_scan_once_and_events_and_deadlines_reconcile() {
        let now = Instant::now();
        let mut checked = None;
        let mut scans = 0;
        for _ in 0..100 {
            if needed(checked, now, false, true) {
                scans += 1;
                checked = Some(now);
            }
        }
        assert_eq!(scans, 1);
        assert!(!needed(
            checked,
            now + RECONCILE - Duration::from_nanos(1),
            false,
            true
        ));
        assert!(needed(checked, now + RECONCILE, false, true));
        assert!(needed(checked, now, true, true));
        for touched in [false, true] {
            assert!(needed(checked, now, touched, false));
        }
    }

    #[test]
    fn a_broken_watcher_is_rechecked_at_most_every_two_seconds() {
        let now = Instant::now();
        let mut checked = None;
        let mut scans = 0;
        for _ in 0..100 {
            let (touched, healthy) = signals(checked, now, false, true);
            if needed(checked, now, touched, healthy) {
                scans += 1;
                checked = Some(now);
            }
        }
        assert_eq!(scans, 1);
        let later = now + BROKEN_RECHECK;
        let (touched, healthy) = signals(checked, later, false, true);
        assert!(needed(checked, later, touched, healthy));
        let (touched, healthy) = signals(checked, now, true, true);
        assert!(needed(checked, now, touched, healthy));
        let (touched, healthy) = signals(checked, now, false, false);
        assert!(!needed(checked, now, touched, healthy));
    }

    #[test]
    fn a_real_watcher_reports_external_edits_and_missing_roots_fall_back() {
        let root = tempfile::tempdir().unwrap();
        let mut changes = Changes::new(root.path());
        assert!(changes.due());
        assert!(changes.watch.is_some());
        std::fs::write(root.path().join("External.md"), "From another editor").unwrap();
        let start = Instant::now();
        while !changes.due() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut missing = Changes::new(&root.path().join("missing"));
        assert!(missing.watch.is_none());
        assert!(missing.due());
        assert!(missing.due());
    }
}
