use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use leo_core::call_names::{self, Ask, Look, Mark, Names, Readers, Seat, Timeline};

#[derive(Debug, Clone, Copy)]
struct Moment {
    at: Instant,
    secs: f64,
    paused: bool,
    resumed: Instant,
    epoch: u32,
    held: f64,
}

type Anchor = Arc<Mutex<Option<Moment>>>;
type Seated = Arc<Mutex<Option<Seat>>>;
type Alive = Arc<AtomicUsize>;
type Heard = (u32, Instant, Look);

const STOP_WAIT: Duration = Duration::from_secs(1);
const STEP: Duration = Duration::from_millis(50);
const MOST_READERS: usize = 2;
const NO_ROOM: &str =
    "two call readers stopped answering and are still waiting; names stay off until leo is opened again";

static ALIVE: OnceLock<Alive> = OnceLock::new();

#[derive(Clone)]
struct Pace {
    every: Duration,
    quiet: Duration,
    alive: Alive,
}

pub struct Watch {
    anchor: Anchor,
    stop: Arc<AtomicBool>,
    done: Option<mpsc::Receiver<()>>,
}

impl Watch {
    pub fn start(names: Names, dir: PathBuf) -> Self {
        let pace = Pace {
            every: call_names::EVERY,
            quiet: call_names::QUIET,
            alive: ALIVE.get_or_init(Alive::default).clone(),
        };
        Self::start_with(names, dir, pace)
    }

    fn start_with(names: Names, dir: PathBuf, pace: Pace) -> Self {
        let anchor: Anchor = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (done_tx, done) = mpsc::channel();
        let started = {
            let (anchor, stop) = (anchor.clone(), stop.clone());
            std::thread::Builder::new()
                .name("call-names".into())
                .spawn(move || {
                    watch(names, &dir, &anchor, &stop, &pace);
                    let _ = done_tx.send(());
                })
                .is_ok()
        };
        Self {
            anchor,
            stop,
            done: started.then_some(done),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn tick(&self, recorded_secs: f64, paused: bool) {
        if let Ok(mut anchor) = self.anchor.lock() {
            *anchor = Some(moved(*anchor, Instant::now(), recorded_secs, paused));
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop();
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(STOP_WAIT);
        }
    }
}

fn moved(last: Option<Moment>, now: Instant, secs: f64, paused: bool) -> Moment {
    match last {
        Some(last) if last.paused == paused => Moment {
            at: now,
            secs,
            ..last
        },
        last => Moment {
            at: now,
            secs,
            paused,
            resumed: now,
            epoch: last.map_or(0, |last| last.epoch + 1),
            held: secs,
        },
    }
}

fn paused_at(seen: &mut Option<u32>, moment: Moment) -> Option<f64> {
    let changed = seen.is_some_and(|epoch| epoch != moment.epoch);
    *seen = Some(moment.epoch);
    changed.then_some(moment.held)
}

fn now_of(anchor: &Anchor) -> Option<Moment> {
    *anchor.lock().ok()?
}

fn recorded_at(moment: Moment, read: Instant, epoch: u32, lag: Duration) -> Option<f64> {
    let said = read.checked_sub(lag)?;
    if moment.paused || moment.epoch != epoch || said < moment.resumed {
        return None;
    }
    Some(match said.checked_duration_since(moment.at) {
        Some(after) => moment.secs + after.as_secs_f64(),
        None => moment.secs - moment.at.duration_since(said).as_secs_f64(),
    })
}

fn rest(stop: &AtomicBool, wait: Duration) {
    let until = Instant::now() + wait;
    while !stop.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(STEP));
    }
}

struct Slot(Alive);

impl Slot {
    fn take(alive: &Alive) -> Option<Slot> {
        let slot = Slot(alive.clone());
        (alive.fetch_add(1, Ordering::SeqCst) < MOST_READERS).then_some(slot)
    }

    fn wait(alive: &Alive, stop: &AtomicBool, most: Duration) -> Option<Slot> {
        let until = Instant::now() + most;
        loop {
            if let Some(slot) = Slot::take(alive) {
                return Some(slot);
            }
            if stop.load(Ordering::Relaxed) || Instant::now() >= until {
                return None;
            }
            std::thread::sleep(STEP);
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn_reader(
    slot: Slot,
    readers: &Readers,
    anchor: &Anchor,
    seat: &Seated,
    stop: &Arc<AtomicBool>,
    tx: &Sender<Heard>,
    pace: &Pace,
) -> Option<JoinHandle<()>> {
    let (readers, anchor, seat, stop, tx, pace) = (
        readers.clone(),
        anchor.clone(),
        seat.clone(),
        stop.clone(),
        tx.clone(),
        pace.clone(),
    );
    std::thread::Builder::new()
        .name("call-reader".into())
        .spawn(move || {
            let _slot = slot;
            let mut reader = readers();
            while !stop.load(Ordering::Relaxed) {
                let Some(epoch) = now_of(&anchor).filter(|m| !m.paused).map(|m| m.epoch) else {
                    std::thread::sleep(pace.every);
                    continue;
                };
                let bound = seat.lock().ok().and_then(|seat| seat.clone());
                let at = Instant::now();
                let look = reader.read(&Ask {
                    until: at + call_names::MOST_READ,
                    stop: &stop,
                    seat: bound.as_ref(),
                });
                if let (None, Look::Call { seat: found, .. }) = (&bound, &look) {
                    if let Ok(mut seat) = seat.lock() {
                        seat.get_or_insert_with(|| found.clone());
                    }
                }
                if tx.send((epoch, at, look)).is_err() {
                    break;
                }
                let took = at.elapsed();
                rest(&stop, pace.every.max(took * 2).saturating_sub(took));
            }
        })
        .ok()
}

struct Sidecar {
    path: PathBuf,
    file: Option<std::fs::File>,
    waiting: String,
    failed: bool,
}

impl Sidecar {
    fn new(dir: &Path) -> Self {
        Self {
            path: dir.join(call_names::FILE),
            file: None,
            waiting: String::new(),
            failed: false,
        }
    }

    fn open(&self) -> std::io::Result<std::fs::File> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut file = leo_core::paths::open_append(&self.path)?;
        if file.metadata()?.len() > 0 {
            let mut last = [0u8];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
            }
        }
        Ok(file)
    }

    fn write(&mut self, dir: &Path, marks: impl IntoIterator<Item = Mark>) {
        use std::io::Write;
        for mark in marks {
            if let Ok(line) = serde_json::to_string(&mark) {
                self.waiting.push_str(&line);
                self.waiting.push('\n');
            }
        }
        if self.waiting.is_empty() {
            return;
        }
        let written = match self.file.take() {
            Some(file) => Ok(file),
            None => self.open(),
        }
        .and_then(|mut file| file.write_all(self.waiting.as_bytes()).map(|()| file));
        match written {
            Ok(file) => {
                self.file = Some(file);
                self.waiting.clear();
                self.failed = false;
            }
            Err(e) if !std::mem::replace(&mut self.failed, true) => {
                super::trace_to(
                    dir,
                    "call-names",
                    &format!("could not keep the names from the call app: {e}"),
                );
            }
            Err(_) => {}
        }
    }
}

fn watch(names: Names, dir: &Path, anchor: &Anchor, stop: &Arc<AtomicBool>, pace: &Pace) {
    let readers = names.readers;
    let mut sidecar = Sidecar::new(dir);
    let lag = Duration::from_secs_f64(call_names::LAG_SECS);
    let seat: Seated = Arc::default();
    let (tx, rx) = mpsc::channel();
    let Some(slot) = Slot::wait(&pace.alive, stop, call_names::MOST_READ + pace.every) else {
        super::trace_to(dir, "call-names", NO_ROOM);
        return;
    };
    let mut reader = spawn_reader(slot, &readers, anchor, &seat, stop, &tx, pace);
    let mut timeline = Timeline::owned_by(names.own.as_deref());
    let mut heard_at = Instant::now();
    let mut restarts = 0;
    let mut stuck = false;
    let mut told: Vec<String> = Vec::new();
    let mut seen = None;
    while !stop.load(Ordering::Relaxed) {
        let heard = rx.recv_timeout(pace.every);
        let moment = now_of(anchor);
        if let Some(held) = moment.and_then(|m| paused_at(&mut seen, m)) {
            sidecar.write(dir, timeline.cut(held));
        }
        match heard {
            Ok((epoch, at, look)) => {
                heard_at = Instant::now();
                stuck = false;
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
                if let Some(secs) = moment.and_then(|m| recorded_at(m, at, epoch, lag)) {
                    let reading = call_names::read(&look);
                    sidecar.write(dir, timeline.heard(secs, reading.as_ref()));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let ended = reader.as_ref().is_none_or(JoinHandle::is_finished);
        if ended && restarts < call_names::MOST_RESTARTS && !stop.load(Ordering::Relaxed) {
            restarts += 1;
            let Some(slot) = Slot::take(&pace.alive) else {
                super::trace_to(dir, "call-names", NO_ROOM);
                restarts = call_names::MOST_RESTARTS;
                continue;
            };
            super::trace_to(
                dir,
                "call-names",
                "the call reader stopped; started it again",
            );
            reader = spawn_reader(slot, &readers, anchor, &seat, stop, &tx, pace);
            heard_at = Instant::now();
        } else if !ended && !stuck && heard_at.elapsed() > pace.quiet {
            stuck = true;
            super::trace_to(
                dir,
                "call-names",
                "the call app stopped answering; its lines stay unnamed until it does",
            );
        }
    }
    sidecar.write(dir, timeline.end());
}

#[cfg(test)]
mod tests {
    use super::*;
    use leo_core::call_names::{Node, Reader, Window};
    use std::sync::atomic::AtomicU32;

    #[derive(Clone, Copy, PartialEq)]
    enum Acts {
        Well,
        PanicsOnce,
        Sticks,
        Hangs,
    }

    #[derive(Default)]
    struct Counts {
        calls: AtomicU32,
        made: AtomicU32,
        alive: AtomicU32,
        most: AtomicU32,
        seats: Mutex<Vec<Option<Seat>>>,
        release: AtomicBool,
    }

    struct Fake {
        counts: Arc<Counts>,
        acts: Acts,
    }

    impl Reader for Fake {
        fn read(&mut self, ask: &Ask) -> Look {
            let n = self.counts.calls.fetch_add(1, Ordering::Relaxed);
            self.counts.seats.lock().unwrap().push(ask.seat.cloned());
            match self.acts {
                Acts::Sticks => {
                    while !ask.stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Acts::Hangs => {
                    while !self.counts.release.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Acts::PanicsOnce if n == 1 => panic!("the call app went away"),
                _ => {}
            }
            Look::Call {
                app: "us.zoom.xos".into(),
                seat: Seat {
                    pid: 41,
                    app: "us.zoom.xos".into(),
                    title: None,
                },
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

    impl Drop for Fake {
        fn drop(&mut self) {
            self.counts.alive.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn readers(counts: &Arc<Counts>, acts: Acts) -> Names {
        let counts = counts.clone();
        Names {
            readers: Arc::new(move || {
                let first = counts.made.fetch_add(1, Ordering::Relaxed) == 0;
                let alive = counts.alive.fetch_add(1, Ordering::Relaxed) + 1;
                counts.most.fetch_max(alive, Ordering::Relaxed);
                Box::new(Fake {
                    counts: counts.clone(),
                    acts: if first { acts } else { Acts::Well },
                }) as Box<dyn Reader>
            }),
            own: None,
        }
    }

    fn watching(counts: &Arc<Counts>, acts: Acts, dir: &Path, quiet: Duration) -> Watch {
        let pace = Pace {
            every: Duration::from_millis(20),
            quiet,
            alive: Alive::default(),
        };
        Watch::start_with(readers(counts, acts), dir.to_path_buf(), pace)
    }

    fn named(dir: &Path) -> Timeline {
        Timeline::parse(&std::fs::read_to_string(dir.join(call_names::FILE)).unwrap_or_default())
    }

    #[test]
    fn readings_land_in_recording_time_and_are_kept_in_the_session() {
        let temp = tempfile::tempdir().unwrap();
        let counts = Arc::default();
        let watch = watching(&counts, Acts::Well, temp.path(), Duration::from_secs(5));
        watch.tick(100.0, false);
        let deadline = Instant::now() + Duration::from_secs(20);
        while named(temp.path()).name(100.05, 103.0).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(watch);
        let timeline = named(temp.path());
        assert_eq!(
            timeline.name(100.05, 103.0).as_deref(),
            Some("Rowan Ellery")
        );
        assert_eq!(timeline.name(99.0, 99.9), None);
        assert_eq!(timeline.name(10.0, 20.0), None);
    }

    #[test]
    fn nothing_is_kept_while_paused() {
        let temp = tempfile::tempdir().unwrap();
        let counts: Arc<Counts> = Arc::default();
        let watch = watching(&counts, Acts::Well, temp.path(), Duration::from_secs(5));
        watch.tick(100.0, true);
        std::thread::sleep(Duration::from_millis(200));
        drop(watch);
        assert_eq!(counts.calls.load(Ordering::Relaxed), 0);
        assert!(!temp.path().join(call_names::FILE).exists());
    }

    #[test]
    fn a_reading_is_placed_by_when_it_was_said_and_never_across_a_pause() {
        let lag = Duration::from_millis(750);
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let running = moved(None, at(0), 0.0, false);
        let later = moved(Some(running), at(10_000), 10.0, false);
        assert_eq!(later.epoch, running.epoch);
        let mapped = recorded_at(later, at(9_000), later.epoch, lag).unwrap();
        assert!((mapped - 8.25).abs() < 1e-6);
        assert_eq!(recorded_at(running, at(500), running.epoch, lag), None);
        let paused = moved(Some(later), at(11_000), 11.0, true);
        assert_eq!(recorded_at(paused, at(10_900), later.epoch, lag), None);
        let resumed = moved(Some(paused), at(60_000), 11.0, false);
        assert_eq!(recorded_at(resumed, at(10_900), later.epoch, lag), None);
        assert_eq!(recorded_at(resumed, at(60_400), resumed.epoch, lag), None);
        let after = recorded_at(resumed, at(61_000), resumed.epoch, lag).unwrap();
        assert!((after - 11.25).abs() < 1e-6);
    }

    #[test]
    fn a_pause_ends_the_name_and_the_resumed_audio_waits_for_a_fresh_reading() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let rowan = call_names::Reading {
            speaking: vec!["Rowan Ellery".into()],
            ..call_names::Reading::default()
        };
        let mut timeline = Timeline::default();
        let mut seen = None;
        let running = moved(None, at(0), 10.0, false);
        assert_eq!(paused_at(&mut seen, running), None);
        timeline.heard(10.0, Some(&rowan));
        timeline.heard(10.4, Some(&rowan));
        let paused = moved(Some(running), at(800), 10.8, true);
        let resumed = moved(Some(paused), at(60_000), 10.8, false);
        let later = moved(Some(resumed), at(60_500), 11.3, false);
        assert_eq!(paused_at(&mut seen, later), Some(10.8));
        assert_eq!(paused_at(&mut seen, later), None);
        timeline.cut(10.8);
        timeline.heard(11.5, Some(&rowan));
        timeline.heard(11.9, Some(&rowan));
        timeline.end();
        assert_eq!(timeline.name(10.0, 10.8).as_deref(), Some("Rowan Ellery"));
        assert_eq!(timeline.name(10.85, 11.45), None);
        assert_eq!(timeline.name(11.5, 11.9).as_deref(), Some("Rowan Ellery"));
    }

    #[cfg(unix)]
    #[test]
    fn the_names_file_is_never_written_through_a_link_or_a_pipe() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, "kept").unwrap();
        let dir = temp.path().join("linked");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join(call_names::FILE)).unwrap();
        let piped = temp.path().join("piped");
        std::fs::create_dir(&piped).unwrap();
        let fifo = std::ffi::CString::new(
            piped
                .join(call_names::FILE)
                .into_os_string()
                .into_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        for dir in [&dir, &piped] {
            let counts = Arc::default();
            let watch = watching(&counts, Acts::Well, dir, Duration::from_secs(5));
            watch.tick(100.0, false);
            std::thread::sleep(Duration::from_millis(1_300));
            let stopping = Instant::now();
            drop(watch);
            assert!(stopping.elapsed() < STOP_WAIT);
            let trace = std::fs::read_to_string(dir.join("trace.jsonl")).unwrap();
            assert!(trace.contains("could not keep the names"));
        }
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "kept");
    }

    #[test]
    fn rows_after_a_torn_line_and_an_open_run_survive_a_crash() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(call_names::FILE), "{\"from\":1.0,\"to").unwrap();
        let mut sidecar = Sidecar::new(temp.path());
        let mut timeline = Timeline::default();
        let reading = call_names::Reading {
            speaking: vec!["Rowan Ellery".into()],
            ..call_names::Reading::default()
        };
        for at in [10.0, 10.4, 10.8, 11.2] {
            sidecar.write(temp.path(), timeline.heard(at, Some(&reading)));
        }
        drop(sidecar);
        let text = std::fs::read_to_string(temp.path().join(call_names::FILE)).unwrap();
        assert!(text
            .lines()
            .skip(1)
            .all(|line| line.starts_with('{') && line.ends_with('}')));
        assert_eq!(
            named(temp.path()).name(10.0, 11.0).as_deref(),
            Some("Rowan Ellery")
        );
    }

    #[test]
    fn a_reader_that_ended_is_started_again_and_keeps_the_call_it_chose() {
        let temp = tempfile::tempdir().unwrap();
        let counts: Arc<Counts> = Arc::default();
        let watch = watching(
            &counts,
            Acts::PanicsOnce,
            temp.path(),
            Duration::from_millis(150),
        );
        watch.tick(50.0, false);
        std::thread::sleep(Duration::from_millis(1_500));
        drop(watch);
        assert_eq!(counts.made.load(Ordering::Relaxed), 2);
        assert_eq!(counts.most.load(Ordering::Relaxed), 1);
        let seats = counts.seats.lock().unwrap().clone();
        assert_eq!(seats[0], None);
        assert!(seats[1..]
            .iter()
            .all(|s| s.as_ref().is_some_and(|s| s.pid == 41)));
        let trace = std::fs::read_to_string(temp.path().join("trace.jsonl")).unwrap();
        assert!(trace.contains("started it again"));
        assert!(named(temp.path()).name(50.1, 50.3).is_some());
    }

    #[test]
    fn a_stuck_reader_is_never_joined_by_a_second_one() {
        let temp = tempfile::tempdir().unwrap();
        let counts: Arc<Counts> = Arc::default();
        let watch = watching(
            &counts,
            Acts::Sticks,
            temp.path(),
            Duration::from_millis(100),
        );
        watch.tick(50.0, false);
        std::thread::sleep(Duration::from_millis(500));
        drop(watch);
        assert_eq!(counts.made.load(Ordering::Relaxed), 1);
        let trace = std::fs::read_to_string(temp.path().join("trace.jsonl")).unwrap();
        assert!(trace.contains("stopped answering"));
        let waited = Instant::now();
        while counts.alive.load(Ordering::Relaxed) > 0 && waited.elapsed() < STOP_WAIT {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(counts.alive.load(Ordering::Relaxed), 0);
    }

    struct Freed(Vec<Arc<Counts>>);

    impl Drop for Freed {
        fn drop(&mut self) {
            for counts in &self.0 {
                counts.release.store(true, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn one_hung_reader_leaves_room_for_a_fresh_one_and_a_second_turns_names_off() {
        let alive = Alive::default();
        let pace = || Pace {
            every: Duration::from_millis(20),
            quiet: Duration::from_secs(5),
            alive: alive.clone(),
        };
        let hung: [Arc<Counts>; 2] = Default::default();
        let freed = Freed(hung.to_vec());
        let fresh: [Arc<Counts>; 2] = Default::default();
        let dirs: Vec<tempfile::TempDir> = (0..4).map(|_| tempfile::tempdir().unwrap()).collect();
        let run = |counts: &Arc<Counts>, acts: Acts, dir: &Path| {
            let watch = Watch::start_with(readers(counts, acts), dir.to_path_buf(), pace());
            watch.tick(10.0, false);
            let waited = Instant::now();
            while counts.calls.load(Ordering::Relaxed) == 0 && waited.elapsed() < STOP_WAIT {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(100));
            drop(watch);
        };
        run(&hung[0], Acts::Hangs, dirs[0].path());
        run(&fresh[0], Acts::Well, dirs[1].path());
        assert_eq!(fresh[0].made.load(Ordering::Relaxed), 1);
        assert!(fresh[0].calls.load(Ordering::Relaxed) > 0);
        run(&hung[1], Acts::Hangs, dirs[2].path());
        assert_eq!(alive.load(Ordering::SeqCst), 2);
        run(&fresh[1], Acts::Well, dirs[3].path());
        assert_eq!(fresh[1].made.load(Ordering::Relaxed), 0);
        let trace = std::fs::read_to_string(dirs[3].path().join("trace.jsonl")).unwrap();
        assert!(trace.contains("names stay off"), "{trace}");
        drop(freed);
        let waited = Instant::now();
        while alive.load(Ordering::SeqCst) > 0 && waited.elapsed() < STOP_WAIT {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(alive.load(Ordering::SeqCst), 0);
    }
}
