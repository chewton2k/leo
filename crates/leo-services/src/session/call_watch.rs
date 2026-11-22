use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use leo_core::call_names::{self, Look, Mark, Readers, Timeline};

type Anchor = Arc<Mutex<Option<(Instant, f64, bool)>>>;
type Heard = (u32, Instant, Look);

pub struct Watch {
    anchor: Anchor,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    pub fn start(readers: Readers, dir: PathBuf) -> Self {
        Self::start_with(readers, dir, call_names::EVERY, call_names::QUIET)
    }

    fn start_with(readers: Readers, dir: PathBuf, every: Duration, quiet: Duration) -> Self {
        let anchor: Anchor = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (anchor, stop) = (anchor.clone(), stop.clone());
            std::thread::Builder::new()
                .name("call-names".into())
                .spawn(move || watch(readers, &dir, &anchor, &stop, every, quiet))
                .ok()
        };
        Self {
            anchor,
            stop,
            thread,
        }
    }

    pub fn tick(&self, recorded_secs: f64, paused: bool) {
        if let Ok(mut anchor) = self.anchor.lock() {
            *anchor = Some((Instant::now(), recorded_secs, paused));
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn recorded_at(anchor: &Anchor, at: Instant) -> Option<f64> {
    let (then, secs, paused) = (*anchor.lock().ok()?)?;
    if paused {
        return None;
    }
    Some(match at.checked_duration_since(then) {
        Some(after) => secs + after.as_secs_f64(),
        None => secs - then.duration_since(at).as_secs_f64(),
    })
}

fn spawn_reader(
    readers: &Readers,
    generation: &Arc<AtomicU32>,
    stop: &Arc<AtomicBool>,
    tx: &Sender<Heard>,
    every: Duration,
) {
    let (readers, generation, stop, tx) = (
        readers.clone(),
        generation.clone(),
        stop.clone(),
        tx.clone(),
    );
    let mine = generation.load(Ordering::Relaxed);
    let _ = std::thread::Builder::new()
        .name("call-reader".into())
        .spawn(move || {
            let mut reader = readers();
            while !stop.load(Ordering::Relaxed) && generation.load(Ordering::Relaxed) == mine {
                let at = Instant::now();
                let look = reader.read();
                if tx.send((mine, at, look)).is_err() {
                    break;
                }
                let took = at.elapsed();
                std::thread::sleep(every.max(took * 2).saturating_sub(took));
            }
        });
}

fn write(file: &Path, marks: impl IntoIterator<Item = Mark>) {
    use std::io::Write;
    let text: String = marks
        .into_iter()
        .filter_map(|m| serde_json::to_string(&m).ok())
        .map(|line| line + "\n")
        .collect();
    if text.is_empty() {
        return;
    }
    if let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = out.write_all(text.as_bytes());
    }
}

fn watch(
    readers: Readers,
    dir: &Path,
    anchor: &Anchor,
    stop: &Arc<AtomicBool>,
    every: Duration,
    quiet: Duration,
) {
    let file = dir.join(call_names::FILE);
    let generation = Arc::new(AtomicU32::new(0));
    let (tx, rx) = mpsc::channel();
    spawn_reader(&readers, &generation, stop, &tx, every);
    let mut timeline = Timeline::default();
    let mut heard_at = Instant::now();
    let mut restarts = 0;
    let mut told: Vec<String> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(every) {
            Ok((from, at, look)) if from == generation.load(Ordering::Relaxed) => {
                heard_at = Instant::now();
                if let Look::Unreadable(why) = &look {
                    if !told.contains(why) {
                        super::trace_to(
                            dir,
                            "call-names",
                            &format!("could not read the call app: {why}"),
                        );
                        told.push(why.clone());
                    }
                }
                if let Some(secs) = recorded_at(anchor, at) {
                    let reading = call_names::read(&look);
                    write(
                        &file,
                        timeline.heard(secs - call_names::LAG_SECS, reading.as_ref()),
                    );
                }
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if heard_at.elapsed() > quiet && restarts < call_names::MOST_RESTARTS {
            restarts += 1;
            generation.fetch_add(1, Ordering::Relaxed);
            super::trace_to(
                dir,
                "call-names",
                "the call reader went quiet; started it again",
            );
            spawn_reader(&readers, &generation, stop, &tx, every);
            heard_at = Instant::now();
        }
    }
    write(&file, timeline.end());
}

#[cfg(test)]
mod tests {
    use super::*;
    use leo_core::call_names::{Node, Reader, Window};

    struct Fake {
        calls: Arc<AtomicU32>,
        hang_first: bool,
    }

    impl Reader for Fake {
        fn read(&mut self) -> Look {
            let n = self.calls.fetch_add(1, Ordering::Relaxed);
            if self.hang_first && n == 0 {
                std::thread::sleep(Duration::from_millis(600));
            }
            Look::Call {
                app: "us.zoom.xos".into(),
                windows: vec![Window {
                    title: "Zoom Meeting".into(),
                    id: String::new(),
                    tree: Node {
                        role: "AXWindow".into(),
                        children: [
                            "Juno Halvorsen (me), Computer audio muted",
                            "Rowan Ellery, Computer audio unmuted, Active speaker",
                        ]
                        .iter()
                        .map(|d| Node {
                            role: "AXTabGroup".into(),
                            description: Some(d.to_string()),
                            ..Node::default()
                        })
                        .collect(),
                        ..Node::default()
                    },
                }],
            }
        }
    }

    fn readers(calls: &Arc<AtomicU32>, made: &Arc<AtomicU32>, hang_first: bool) -> Readers {
        let (calls, made) = (calls.clone(), made.clone());
        Arc::new(move || {
            let first = made.fetch_add(1, Ordering::Relaxed) == 0;
            Box::new(Fake {
                calls: calls.clone(),
                hang_first: hang_first && first,
            }) as Box<dyn Reader>
        })
    }

    fn named(dir: &Path) -> Timeline {
        Timeline::parse(&std::fs::read_to_string(dir.join(call_names::FILE)).unwrap_or_default())
    }

    #[test]
    fn readings_land_in_recording_time_and_are_kept_in_the_session() {
        let temp = tempfile::tempdir().unwrap();
        let (calls, made) = (Arc::default(), Arc::default());
        let watch = Watch::start_with(
            readers(&calls, &made, false),
            temp.path().to_path_buf(),
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        watch.tick(100.0, false);
        std::thread::sleep(Duration::from_millis(300));
        drop(watch);
        let timeline = named(temp.path());
        assert_eq!(timeline.name(99.3, 99.5).as_deref(), Some("Rowan Ellery"));
        assert_eq!(timeline.name(10.0, 20.0), None);
    }

    #[test]
    fn nothing_is_kept_while_paused() {
        let temp = tempfile::tempdir().unwrap();
        let (calls, made) = (Arc::default(), Arc::default());
        let watch = Watch::start_with(
            readers(&calls, &made, false),
            temp.path().to_path_buf(),
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        watch.tick(100.0, true);
        std::thread::sleep(Duration::from_millis(200));
        drop(watch);
        assert!(calls.load(Ordering::Relaxed) > 0);
        assert!(!temp.path().join(call_names::FILE).exists());
    }

    #[test]
    fn a_quiet_reader_is_started_again() {
        let temp = tempfile::tempdir().unwrap();
        let (calls, made) = (Arc::default(), Arc::default());
        let watch = Watch::start_with(
            readers(&calls, &made, true),
            temp.path().to_path_buf(),
            Duration::from_millis(20),
            Duration::from_millis(150),
        );
        watch.tick(50.0, false);
        std::thread::sleep(Duration::from_millis(500));
        drop(watch);
        assert!(made.load(Ordering::Relaxed) >= 2);
        let trace = std::fs::read_to_string(temp.path().join("trace.jsonl")).unwrap();
        assert!(trace.contains("went quiet"));
        assert!(named(temp.path()).name(49.5, 49.7).is_some());
    }
}
