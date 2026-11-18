use anyhow::{anyhow, Context, Result};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

pub const PIECE_AFTER: usize = 240 * 16_000;
pub const PIECE_AT_MOST: usize = 300 * 16_000;
pub const PAUSE: usize = 1_600;

#[derive(Default)]
pub struct Pieces {
    since: usize,
    spoke: bool,
    quiet: usize,
}

impl Pieces {
    pub fn cut(&mut self, samples: usize, heard_speech: bool) -> bool {
        self.quiet = if heard_speech {
            0
        } else {
            self.quiet + samples
        };
        let cut = self.since + samples > PIECE_AT_MOST
            || (self.since >= PIECE_AFTER && self.spoke && self.quiet >= PAUSE);
        if cut {
            self.since = 0;
            self.spoke = false;
        }
        self.since += samples;
        self.spoke |= heard_speech;
        cut
    }
}

pub type Live = Box<dyn FnOnce() -> Option<String> + Send>;

pub struct Piece {
    pub index: usize,
    pub live: Option<Live>,
    pub kept: Vec<f32>,
    pub raw: Vec<f32>,
}

enum Waiting {
    Memory(usize, Vec<f32>),
    Disk(usize, PathBuf),
}

#[derive(Default)]
struct Line {
    waiting: VecDeque<Waiting>,
    busy: bool,
    sorted: bool,
    texts: Vec<(usize, String)>,
    failed: Option<anyhow::Error>,
}

struct Shared {
    line: Mutex<Line>,
    ready: Condvar,
    held: AtomicUsize,
    stop: AtomicBool,
}

impl Shared {
    fn line(&self) -> std::sync::MutexGuard<'_, Line> {
        self.line.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Pipeline {
    spool: PathBuf,
    shared: Arc<Shared>,
    send: Option<SyncSender<Piece>>,
    sorter: Option<JoinHandle<()>>,
    transcriber: Option<JoinHandle<()>>,
}

fn to_samples(audio: &[f32]) -> Vec<i16> {
    audio
        .iter()
        .map(|n| (n * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

impl Pipeline {
    pub fn new(
        spool: PathBuf,
        mut transcribe: impl FnMut(Vec<f32>) -> Result<String> + Send + 'static,
    ) -> Result<Self> {
        std::fs::create_dir_all(&spool).context("leo could not make a folder for dictation")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o700))?;
        }
        let shared = Arc::new(Shared {
            line: Mutex::new(Line::default()),
            ready: Condvar::new(),
            held: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
        });
        let (send, pieces) = sync_channel::<Piece>(1);
        let sorter = {
            let shared = shared.clone();
            let spool = spool.clone();
            std::thread::Builder::new()
                .name("leo-dictation-pieces".into())
                .spawn(move || {
                    for piece in pieces {
                        if shared.stop.load(Ordering::Acquire) {
                            break;
                        }
                        if let Some(text) = piece
                            .live
                            .and_then(|live| live())
                            .filter(|text| !text.trim().is_empty())
                        {
                            shared.line().texts.push((piece.index, text));
                            shared.held.fetch_sub(1, Ordering::AcqRel);
                            continue;
                        }
                        let audio = if piece.kept.is_empty() {
                            piece.raw
                        } else {
                            piece.kept
                        };
                        let audio = {
                            let mut line = shared.line();
                            if !line.busy && line.waiting.is_empty() {
                                line.waiting.push_back(Waiting::Memory(piece.index, audio));
                                shared.ready.notify_all();
                                None
                            } else {
                                Some(audio)
                            }
                        };
                        let Some(audio) = audio else {
                            continue;
                        };
                        let path = spool.join(format!("{:06}.wav", piece.index));
                        let waiting = match crate::session::wav::write(&path, &to_samples(&audio)) {
                            Ok(()) => {
                                drop(audio);
                                shared.held.fetch_sub(1, Ordering::AcqRel);
                                Waiting::Disk(piece.index, path)
                            }
                            Err(error) => {
                                log::warn!("A dictation piece is kept in memory: {error:#}");
                                Waiting::Memory(piece.index, audio)
                            }
                        };
                        shared.line().waiting.push_back(waiting);
                        shared.ready.notify_all();
                    }
                    shared.line().sorted = true;
                    shared.ready.notify_all();
                })?
        };
        let transcriber = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("leo-dictation-transcriber".into())
                .spawn(move || loop {
                    let next = {
                        let mut line = shared.line();
                        loop {
                            if shared.stop.load(Ordering::Acquire) || line.failed.is_some() {
                                break None;
                            }
                            if let Some(next) = line.waiting.pop_front() {
                                line.busy = true;
                                break Some(next);
                            }
                            if line.sorted {
                                break None;
                            }
                            line = shared.ready.wait(line).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    let Some(next) = next else {
                        break;
                    };
                    let (index, audio, file) = match next {
                        Waiting::Memory(index, audio) => (index, Ok(audio), None),
                        Waiting::Disk(index, path) => {
                            shared.held.fetch_add(1, Ordering::AcqRel);
                            let audio = crate::session::wav::read(&path).map(|samples| {
                                samples.iter().map(|n| *n as f32 / 32768.0).collect()
                            });
                            (index, audio, Some(path))
                        }
                    };
                    let text = audio.and_then(&mut transcribe);
                    shared.held.fetch_sub(1, Ordering::AcqRel);
                    let mut line = shared.line();
                    line.busy = false;
                    match text {
                        Ok(text) => {
                            if let Some(file) = file {
                                let _ = std::fs::remove_file(file);
                            }
                            line.texts.push((index, text));
                        }
                        Err(error) => line.failed = Some(error),
                    }
                })?
        };
        Ok(Self {
            spool,
            shared,
            send: Some(send),
            sorter: Some(sorter),
            transcriber: Some(transcriber),
        })
    }

    pub fn push(&self, piece: Piece) -> Result<()> {
        let send = self.send.as_ref().context("This dictation has finished.")?;
        self.shared.held.fetch_add(1, Ordering::AcqRel);
        send.send(piece).map_err(|_| {
            self.shared.held.fetch_sub(1, Ordering::AcqRel);
            anyhow!("Dictation could not keep its audio. Try again.")
        })
    }

    pub fn held(&self) -> usize {
        self.shared.held.load(Ordering::Acquire)
    }

    pub fn finish(mut self) -> Result<Vec<String>> {
        drop(self.send.take());
        let stopped = |_| anyhow!("Dictation stopped unexpectedly. Try again.");
        if let Some(sorter) = self.sorter.take() {
            sorter.join().map_err(stopped)?;
        }
        if let Some(transcriber) = self.transcriber.take() {
            transcriber.join().map_err(stopped)?;
        }
        let mut line = self.shared.line();
        if let Some(error) = line.failed.take() {
            return Err(error);
        }
        let mut texts = std::mem::take(&mut line.texts);
        texts.sort_by_key(|(index, _)| *index);
        Ok(texts.into_iter().map(|(_, text)| text).collect())
    }

    pub fn cancel(self) {}
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        drop(self.send.take());
        self.shared.ready.notify_all();
        let _ = std::fs::remove_dir_all(&self.spool);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::{Duration, Instant};

    const CHUNK: usize = 512;

    fn second(n: usize) -> usize {
        n * 16_000
    }

    #[test]
    fn a_short_dictation_is_never_cut() {
        let mut pieces = Pieces::default();
        for _ in 0..239 {
            assert!(!pieces.cut(second(1), true));
        }
        for _ in 0..10 {
            assert!(!pieces.cut(second(1) / 10, false));
        }
    }

    #[test]
    fn a_piece_is_cut_at_the_first_pause_after_four_minutes() {
        let mut pieces = Pieces::default();
        for _ in 0..250 {
            assert!(!pieces.cut(second(1), true));
        }
        assert!(pieces.cut(second(1), false));
        for _ in 0..239 {
            assert!(!pieces.cut(second(1), true));
        }
        assert!(pieces.cut(second(1), false));
    }

    #[test]
    fn a_pause_is_heard_only_after_a_tenth_of_a_second_of_quiet() {
        let mut pieces = Pieces::default();
        for _ in 0..(PIECE_AFTER / 160) {
            assert!(!pieces.cut(160, true));
        }
        for _ in 0..9 {
            assert!(!pieces.cut(160, false));
        }
        assert!(pieces.cut(160, false));
    }

    #[test]
    fn a_dictation_with_no_pause_is_cut_at_five_minutes() {
        let mut pieces = Pieces::default();
        let mut cuts = Vec::new();
        for n in 0..second(1) * 900 / CHUNK {
            if pieces.cut(CHUNK, true) {
                cuts.push(n * CHUNK);
            }
        }
        assert_eq!(cuts.len(), 2);
        for (n, at) in cuts.iter().enumerate() {
            assert!(at.abs_diff(PIECE_AT_MOST * (n + 1)) <= CHUNK * (n + 1));
        }
    }

    fn largest_piece(seconds: usize) -> (usize, usize) {
        let mut pieces = Pieces::default();
        let (mut largest, mut piece, mut cuts) = (0, 0, 0);
        for n in 0..second(seconds) / CHUNK {
            let at = n * CHUNK;
            let speech = at % second(20) >= second(1) / 2;
            if pieces.cut(CHUNK, speech) {
                largest = largest.max(piece);
                piece = 0;
                cuts += 1;
            }
            piece += CHUNK;
        }
        (largest.max(piece), cuts)
    }

    #[test]
    fn an_hour_of_dictation_never_holds_more_than_one_piece() {
        let (largest, cuts) = largest_piece(3_600);
        assert!(largest <= PIECE_AT_MOST);
        assert!(cuts >= 11);
    }

    #[test]
    #[ignore]
    fn eight_hours_of_dictation_hold_one_piece() {
        let (largest, cuts) = largest_piece(8 * 3_600);
        assert!(largest <= PIECE_AT_MOST);
        println!(
            "eight hours: {cuts} cuts, largest piece {:.1} s",
            largest as f64 / 16_000.0
        );
    }

    fn piece(index: usize) -> Piece {
        Piece {
            index,
            live: None,
            kept: vec![index as f32 / 32768.0; 16_000],
            raw: Vec::new(),
        }
    }

    fn blocked(gate: Receiver<()>) -> impl FnMut(Vec<f32>) -> Result<String> + Send + 'static {
        move |audio| {
            let _ = gate.recv();
            Ok(((audio[0] * 32768.0).round() as usize).to_string())
        }
    }

    fn spooled(spool: &std::path::Path) -> usize {
        std::fs::read_dir(spool).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
                .count()
        })
    }

    fn until(what: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !what() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_blocked_transcriber_keeps_memory_flat_and_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let spool = dir.path().join("spool");
        let (gate, wait) = channel();
        let pipeline = Pipeline::new(spool.clone(), blocked(wait)).unwrap();
        let mut most = 0;
        for index in 0..50 {
            pipeline.push(piece(index)).unwrap();
            most = most.max(pipeline.held());
        }
        assert!(most <= 3, "{most} pieces held");
        until(|| spooled(&spool) == 49 && pipeline.held() == 1);
        drop(gate);
        let texts = pipeline.finish().unwrap();
        let wanted: Vec<String> = (0..50).map(|n| n.to_string()).collect();
        assert_eq!(texts, wanted);
        assert_eq!(spooled(&spool), 0);
    }

    #[test]
    fn a_cancelled_pipeline_removes_its_spool() {
        let dir = tempfile::tempdir().unwrap();
        let spool = dir.path().join("spool");
        let (gate, wait) = channel();
        let pipeline = Pipeline::new(spool.clone(), blocked(wait)).unwrap();
        for index in 0..5 {
            pipeline.push(piece(index)).unwrap();
        }
        until(|| spooled(&spool) == 4);
        pipeline.cancel();
        assert!(!spool.exists());
        drop(gate);
    }

    #[test]
    fn a_live_piece_needs_no_spool() {
        let dir = tempfile::tempdir().unwrap();
        let spool = dir.path().join("spool");
        let (gate, wait) = channel();
        let pipeline = Pipeline::new(spool.clone(), blocked(wait)).unwrap();
        pipeline.push(piece(0)).unwrap();
        let mut live = piece(1);
        live.live = Some(Box::new(|| Some("one".to_string())));
        pipeline.push(live).unwrap();
        until(|| pipeline.held() == 1);
        assert_eq!(spooled(&spool), 0);
        drop(gate);
        assert_eq!(pipeline.finish().unwrap(), ["0", "one"]);
    }

    #[test]
    fn a_failed_piece_fails_the_dictation_with_its_own_error() {
        #[derive(Debug)]
        struct Cancelled;
        impl std::fmt::Display for Cancelled {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("cancelled")
            }
        }
        impl std::error::Error for Cancelled {}
        let dir = tempfile::tempdir().unwrap();
        let pipeline = Pipeline::new(dir.path().join("spool"), |_| {
            Err(anyhow::Error::new(Cancelled))
        })
        .unwrap();
        pipeline.push(piece(0)).unwrap();
        let error = pipeline.finish().unwrap_err();
        assert!(error.downcast_ref::<Cancelled>().is_some());
    }
}
