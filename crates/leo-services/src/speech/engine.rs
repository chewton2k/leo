use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use transcribe_cpp::{Backend, RunOptions};

use super::library::Selected;
use super::protocol::DeviceSelector;
use super::supervisor::{lock, EngineError, EngineSupervisor, Fed, LoadSpec};
use super::vad::{detector, CaptureVad, VAD_OFFLINE_HANGOVER_MS};

const QUEUED: usize = 8;
const SHADERS_MARKER: &str = "speech-shaders-ready";
const IDLE: Duration = Duration::from_secs(5 * 60);
const POLL: Duration = Duration::from_millis(20);
const CANCEL_POLL: Duration = Duration::from_millis(50);
const CONTROL_LIMIT: Duration = Duration::from_secs(5);
const ANSWER_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);
const MOST_FEEDS: usize = 300 * 16_000 / 512 + 2;
const FRAME: usize = 256;
const MOST_MARKER_BYTES: u64 = 64;
const SPEECH_LEVEL: f32 = 0.1;
const MOST_GAIN: f32 = 50.0;
const SHORTEST_CLIP: usize = 16_000;
const SHORT_CLIP_PADDED: usize = 20_000;
const _: () = assert!(MOST_FEEDS * 512 >= 300 * 16_000);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Dictation,
    Segment,
    Live,
}

struct Job {
    _admission: Pin,
    selected: Selected,
    gain: f32,
    chunks: VecDeque<Vec<f32>>,
    text: Vec<String>,
    priority: Priority,
    words: Vec<String>,
    cancels: u64,
    reply: mpsc::SyncSender<Result<String>>,
}

enum Message {
    Run(Job),
    Warm(Selected),
    Stream {
        selected: Selected,
        words: Vec<String>,
        cancels: u64,
        audio: mpsc::Receiver<Option<Vec<f32>>>,
        text: Box<dyn FnMut(String, String) + Send>,
        reply: mpsc::SyncSender<Result<String>>,
    },
    Stop(mpsc::SyncSender<()>),
    Unload(mpsc::SyncSender<()>),
    Edit {
        path: std::path::PathBuf,
        current: bool,
        work: Box<dyn FnOnce() -> Result<serde_json::Value> + Send>,
        reply: mpsc::SyncSender<Result<serde_json::Value>>,
    },
}

pub struct Engine {
    requests: mpsc::SyncSender<Message>,
    supervisor: EngineSupervisor,
    pins: Arc<AtomicUsize>,
    dictating: Arc<AtomicUsize>,
    stopped: AtomicBool,
    admitted: Arc<AtomicUsize>,
    dictation_cancels: Arc<AtomicU64>,
    serving_dictation: Arc<Mutex<bool>>,
}

pub struct Pin(Arc<AtomicUsize>);
impl Drop for Pin {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(Engine::new)
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::with(heard_speech)
    }

    fn with(hears: fn(&[f32], f32) -> Result<bool>) -> Self {
        let (requests, queue) = mpsc::sync_channel(QUEUED);
        let supervisor =
            EngineSupervisor::new(!cfg!(all(target_os = "macos", target_arch = "x86_64")));
        let pins = Arc::new(AtomicUsize::new(0));
        let dictating = Arc::new(AtomicUsize::new(0));
        let dictation_cancels = Arc::new(AtomicU64::new(0));
        let serving_dictation = Arc::new(Mutex::new(false));
        let (worker, held, first) = (supervisor.clone(), pins.clone(), dictating.clone());
        let (cancels, serving) = (dictation_cancels.clone(), serving_dictation.clone());
        std::thread::spawn(move || run(queue, worker, held, first, cancels, serving, hears));
        Self {
            requests,
            supervisor,
            pins,
            dictating,
            stopped: AtomicBool::new(false),
            admitted: Arc::new(AtomicUsize::new(0)),
            dictation_cancels,
            serving_dictation,
        }
    }

    pub fn pin(&self) -> Pin {
        self.pins.fetch_add(1, Ordering::AcqRel);
        Pin(self.pins.clone())
    }
    pub fn dictation(&self) -> Pin {
        self.dictating.fetch_add(1, Ordering::AcqRel);
        Pin(self.dictating.clone())
    }
    pub fn recording(&self) -> bool {
        self.pins.load(Ordering::Acquire) > 0
    }
    pub fn dictating(&self) -> bool {
        self.dictating.load(Ordering::Acquire) > 0
    }
    pub fn loaded(&self) -> bool {
        self.supervisor.loaded().is_some()
    }
    pub fn cancel(&self) {
        let serving = lock(&self.serving_dictation);
        self.dictation_cancels.fetch_add(1, Ordering::AcqRel);
        if *serving {
            self.supervisor.cancel();
        }
    }

    pub fn transcribe(
        &self,
        selected: Selected,
        pcm: Vec<f32>,
        priority: Priority,
        words: Vec<String>,
    ) -> Result<String> {
        let cancels = self.dictation_cancels.load(Ordering::Acquire);
        ensure!(
            !self.stopped.load(Ordering::Acquire),
            "The speech engine is stopping. Reopen Leo to transcribe again."
        );
        #[allow(deprecated)]
        self.admitted
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < QUEUED).then_some(n + 1)
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again in a moment."))?;
        let admission = Pin(self.admitted.clone());
        let gain = if priority == Priority::Dictation {
            1.0
        } else {
            speech_gain(&pcm)
        };
        let chunks = speech_chunks(pcm, gain)?.into();
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests.try_send(Message::Run(Job { _admission: admission, selected, gain, chunks, text: Vec::new(), priority, words, cancels, reply }))
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Your recorded audio is kept; try again in a moment."))?;
        wait_for(&answer, || {
            priority == Priority::Dictation
                && self.dictation_cancels.load(Ordering::Acquire) != cancels
        })
        .context("Transcription took too long. Your audio is kept; try a faster speech model.")?
    }

    pub fn warm(&self, selected: Selected) {
        if !self.stopped.load(Ordering::Acquire) {
            let _ = self.requests.try_send(Message::Warm(selected));
        }
    }

    pub fn prepare_after_update(&self, config: &Path, version: &str) {
        let marker = config.join(SHADERS_MARKER);
        if cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && !self.stopped.load(Ordering::Acquire)
            && !shaders_ready(&marker, version)
            && self.supervisor.devices().is_some()
        {
            if let Err(error) = std::fs::write(&marker, version) {
                log::warn!("Could not remember the prepared speech shaders: {error}");
            }
        }
    }

    pub fn stream(
        &self,
        selected: Selected,
        words: Vec<String>,
        text: impl FnMut(String, String) + Send + 'static,
    ) -> Result<DictationStream> {
        let cancels = self.dictation_cancels.load(Ordering::Acquire);
        ensure!(
            !self.stopped.load(Ordering::Acquire),
            "The speech engine is stopping. Reopen Leo."
        );
        let (feed, audio) = mpsc::sync_channel(MOST_FEEDS);
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Message::Stream {
                selected,
                words,
                cancels,
                audio,
                text: Box::new(text),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again."))?;
        Ok(DictationStream {
            feed,
            answer,
            cancels,
            dictation_cancels: self.dictation_cancels.clone(),
        })
    }

    pub fn edit_model(
        &self,
        path: std::path::PathBuf,
        current: bool,
        work: impl FnOnce() -> Result<serde_json::Value> + Send + 'static,
    ) -> Result<serde_json::Value> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Message::Edit {
                path,
                current,
                work: Box::new(work),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again in a moment."))?;
        answer
            .recv_timeout(ANSWER_LIMIT)
            .context("The model is still in use. Stop recording or dictation and try again.")?
    }

    pub fn unload_worker(&self) -> Result<()> {
        let deadline = Instant::now() + CONTROL_LIMIT;
        self.supervisor.cancel();
        let (done, finished) = mpsc::sync_channel(1);
        send_control_before(
            &self.requests,
            Message::Unload(done),
            deadline,
            std::thread::sleep,
        )
        .context(
            "The speech engine is busy. Finish recording or dictation, then try updating again.",
        )?;
        finished
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("The speech worker did not stop. Try updating again in a moment.")
    }

    pub fn stop(&self) -> Result<()> {
        let deadline = Instant::now() + CONTROL_LIMIT;
        self.stopped.store(true, Ordering::Release);
        self.supervisor.cancel();
        let (done, stopped) = mpsc::sync_channel(1);
        send_control_before(
            &self.requests,
            Message::Stop(done),
            deadline,
            std::thread::sleep,
        )
        .context("The speech engine is still busy. Try again in a moment.")?;
        stopped
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("The speech engine has not stopped yet. Try again in a moment.")
    }
}

fn send_control_before<T>(
    requests: &mpsc::SyncSender<T>,
    mut message: T,
    deadline: Instant,
    mut pause: impl FnMut(Duration),
) -> Result<()> {
    loop {
        ensure!(
            Instant::now() < deadline,
            "The speech engine is still busy. Try again in a moment."
        );
        match requests.try_send(message) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(waiting)) => {
                message = waiting;
                pause(POLL.min(deadline.saturating_duration_since(Instant::now())));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                anyhow::bail!("The speech engine has stopped. Reopen Leo.")
            }
        }
    }
}

fn wait_for<T>(
    answer: &mpsc::Receiver<Result<T>>,
    cancelled: impl Fn() -> bool,
) -> Result<Result<T>, mpsc::RecvTimeoutError> {
    let deadline = Instant::now() + ANSWER_LIMIT;
    loop {
        match answer
            .recv_timeout(CANCEL_POLL.min(deadline.saturating_duration_since(Instant::now())))
        {
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {
                if cancelled() {
                    return Ok(Err(EngineError::Cancelled.into()));
                }
            }
            other => return other,
        }
    }
}

fn cancelled_dictation(job: &Job, dictation_cancels: &AtomicU64) -> bool {
    job.priority == Priority::Dictation && job.cancels != dictation_cancels.load(Ordering::Acquire)
}

fn run_options(selected: &Selected, words: Vec<String>, supports_vocabulary: bool) -> RunOptions {
    RunOptions {
        language: (selected.language != "auto").then(|| selected.language.clone()),
        vocabulary: words
            .into_iter()
            .filter(|word| supports_vocabulary && !word.contains('\0'))
            .collect(),
        ..RunOptions::default()
    }
}

fn without_echo(text: String, vocabulary: &[String]) -> String {
    if vocabulary.is_empty() {
        text
    } else {
        crate::dictation::text::strip_vocabulary_echo(&text, vocabulary)
    }
}

pub struct DictationStream {
    feed: mpsc::SyncSender<Option<Vec<f32>>>,
    answer: mpsc::Receiver<Result<String>>,
    cancels: u64,
    dictation_cancels: Arc<AtomicU64>,
}
impl DictationStream {
    pub fn feed(&self, pcm: Vec<f32>) -> bool {
        self.feed.try_send(Some(pcm)).is_ok()
    }
    pub fn finish(self) -> Result<String> {
        self.feed.try_send(None).map_err(|_| {
            anyhow::anyhow!("Live transcription fell behind. Finishing from the recorded audio.")
        })?;
        wait_for(&self.answer, || {
            self.dictation_cancels.load(Ordering::Acquire) != self.cancels
        })
        .context("Live transcription did not finish. Try a smaller model.")?
    }
}

#[derive(Default)]
struct Heard {
    queued: Vec<f32>,
    samples: usize,
}
impl Heard {
    fn take(&mut self, pcm: Vec<f32>) -> Result<()> {
        self.samples = self.samples.saturating_add(pcm.len());
        ensure!(
            self.samples <= 300 * 16_000
                && pcm.len() <= 16_000
                && pcm.iter().all(|n| n.is_finite()),
            "Dictation audio is too long. Start another dictation."
        );
        self.queued.extend_from_slice(&pcm);
        Ok(())
    }
    fn send(&mut self, mut feed: impl FnMut(Vec<f32>) -> Fed) -> Result<()> {
        while !self.queued.is_empty() {
            let part: Vec<f32> = self.queued.drain(..self.queued.len().min(16_000)).collect();
            match feed(part) {
                Fed::Sent => {}
                Fed::Full(part) => {
                    self.queued.splice(0..0, part);
                    return Ok(());
                }
                Fed::Lost => {
                    anyhow::bail!("Live transcription stopped. Finishing from recorded audio.")
                }
            }
        }
        Ok(())
    }
}

fn speech_gain(pcm: &[f32]) -> f32 {
    let mut levels: Vec<f32> = pcm
        .chunks(FRAME)
        .map(frame_level)
        .filter(|level| *level > 0.0)
        .collect();
    levels.sort_by(f32::total_cmp);
    match levels.get(levels.len() / 2) {
        Some(level) => (SPEECH_LEVEL / level).clamp(1.0, MOST_GAIN),
        None => 1.0,
    }
}

fn raised(pcm: &[f32], gain: f32) -> Vec<f32> {
    pcm.iter().map(|n| n * gain).collect()
}

fn shaders_ready(marker: &Path, version: &str) -> bool {
    let mut seen = String::new();
    std::fs::File::open(marker)
        .and_then(|file| file.take(MOST_MARKER_BYTES).read_to_string(&mut seen))
        .is_ok()
        && seen == version
}

fn frame_level(frame: &[f32]) -> f32 {
    (frame.iter().map(|n| n * n).sum::<f32>() / frame.len() as f32).sqrt()
}

fn speech_chunks(pcm: Vec<f32>, gain: f32) -> Result<Vec<Vec<f32>>> {
    let mut chunks = super::prepare(raised(&pcm, gain))?;
    for chunk in &mut chunks {
        chunk.iter_mut().for_each(|n| *n /= gain);
    }
    Ok(chunks)
}

fn heard_speech(pcm: &[f32], gain: f32) -> Result<bool> {
    let mut vad = CaptureVad::new(detector()?, VAD_OFFLINE_HANGOVER_MS);
    for part in pcm.chunks(16_000) {
        if !vad.push(&raised(part, gain)).is_empty() {
            return Ok(true);
        }
    }
    Ok(!vad.finish().is_empty())
}

fn padded(mut pcm: Vec<f32>) -> Vec<f32> {
    if !pcm.is_empty() && pcm.len() < SHORTEST_CLIP {
        pcm.resize(SHORT_CLIP_PADDED, 0.0);
    }
    pcm
}

fn run(
    queue: mpsc::Receiver<Message>,
    supervisor: EngineSupervisor,
    pins: Arc<AtomicUsize>,
    dictating: Arc<AtomicUsize>,
    dictation_cancels: Arc<AtomicU64>,
    serving: Arc<Mutex<bool>>,
    hears: fn(&[f32], f32) -> Result<bool>,
) {
    let mut pending: Vec<Job> = Vec::new();
    let mut last_work = Instant::now();
    let mut model = None;
    loop {
        let message = queue.recv_timeout(if pending.is_empty() { IDLE } else { POLL });
        if matches!(message, Err(mpsc::RecvTimeoutError::Disconnected)) {
            supervisor.unload().wait();
            return;
        }
        let mut incoming = message.ok().into_iter().chain(queue.try_iter());
        for message in &mut incoming {
            match message {
                Message::Run(job) => pending.push(job),
                Message::Warm(selected) => {
                    let pinned_elsewhere = pins.load(Ordering::Acquire) > 0
                        && model.as_ref().is_some_and(|path| path != &selected.path);
                    if !pinned_elsewhere
                        && (model.as_ref() != Some(&selected.path) || supervisor.loaded().is_none())
                    {
                        match supervisor.load(LoadSpec {
                            path: selected.path.clone(),
                            backend: Backend::Auto,
                            device: DeviceSelector::Auto,
                        }) {
                            Ok(_) => model = Some(selected.path),
                            Err(error) => log::warn!("Speech model preload failed: {error}"),
                        }
                    }
                    last_work = Instant::now();
                }
                Message::Stream {
                    selected,
                    words,
                    cancels,
                    audio,
                    mut text,
                    reply,
                } => {
                    let result = (|| -> Result<String> {
                        if model.as_ref() != Some(&selected.path) || supervisor.loaded().is_none() {
                            supervisor.load(LoadSpec {
                                path: selected.path.clone(),
                                backend: Backend::Auto,
                                device: DeviceSelector::Auto,
                            })?;
                            model = Some(selected.path.clone());
                        }
                        {
                            let mut serving = lock(&serving);
                            ensure!(
                                dictation_cancels.load(Ordering::Acquire) == cancels,
                                EngineError::Cancelled
                            );
                            *serving = true;
                        }
                        let vocabulary = supervisor
                            .loaded()
                            .is_some_and(|info| info.supports_vocabulary);
                        let run = run_options(&selected, words, vocabulary);
                        let echo = run.vocabulary.clone();
                        let stream =
                            supervisor.start_stream(run, Default::default(), move |progress| {
                                if let Some(snapshot) = progress.text {
                                    text(snapshot.committed, snapshot.tentative);
                                }
                            })?;
                        let started = Instant::now();
                        let mut heard = Heard::default();
                        let mut open = true;
                        while open || !heard.queued.is_empty() {
                            ensure!(
                                dictation_cancels.load(Ordering::Acquire) == cancels,
                                EngineError::Cancelled
                            );
                            ensure!(
                                started.elapsed() < Duration::from_secs(310),
                                "Dictation reached five minutes. Start another dictation."
                            );
                            let wait = if heard.queued.is_empty() {
                                Duration::from_millis(100)
                            } else {
                                POLL
                            };
                            if open {
                                match audio.recv_timeout(wait) {
                                    Ok(Some(pcm)) => heard.take(pcm)?,
                                    Ok(None) => open = false,
                                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                                    Err(_) => anyhow::bail!("Dictation cancelled."),
                                }
                            } else {
                                std::thread::sleep(wait);
                            }
                            while open {
                                match audio.try_recv() {
                                    Ok(Some(pcm)) => heard.take(pcm)?,
                                    Ok(None) => open = false,
                                    Err(mpsc::TryRecvError::Empty) => break,
                                    Err(_) => anyhow::bail!("Dictation cancelled."),
                                }
                            }
                            heard.send(|part| stream.feed(part))?;
                        }
                        Ok(without_echo(
                            stream
                                .finalize(false)?
                                .map(|f| f.text.full)
                                .unwrap_or_default(),
                            &echo,
                        ))
                    })();
                    *lock(&serving) = false;
                    let _ = reply.send(result);
                    last_work = Instant::now();
                }
                Message::Edit {
                    path,
                    current,
                    work,
                    reply,
                } => {
                    if (current || model.as_ref() == Some(&path))
                        && (pins.load(Ordering::Acquire) > 0
                            || dictating.load(Ordering::Acquire) > 0)
                    {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Stop recording or dictation before deleting the model in use."
                        )));
                        continue;
                    }
                    if model.as_ref() == Some(&path) {
                        supervisor.unload().wait();
                        model = None;
                    }
                    pending.retain(|job| {
                        if job.selected.path != path { true } else {
                            let _ = job.reply.send(Err(anyhow::anyhow!("Choose a model in Settings → Speech models. Your recorded audio is kept.")));
                            false
                        }
                    });
                    let _ = reply.send(work());
                }
                Message::Unload(done) => {
                    supervisor.unload().wait();
                    model = None;
                    let _ = done.send(());
                }
                Message::Stop(done) => {
                    supervisor.unload().wait();
                    for job in pending {
                        let _ = job.reply.send(Err(anyhow::anyhow!(
                            "Transcription stopped. Your recorded audio is kept."
                        )));
                    }
                    let _ = done.send(());
                    return;
                }
            }
        }
        if pending.is_empty() {
            if pins.load(Ordering::Acquire) == 0
                && dictating.load(Ordering::Acquire) == 0
                && last_work.elapsed() >= IDLE
            {
                supervisor.unload().wait();
                model = None;
            }
            continue;
        }
        pending.sort_by_key(|job| job.priority);
        if dictating.load(Ordering::Acquire) > 0 && pending[0].priority != Priority::Dictation {
            continue;
        }
        let mut job = pending.remove(0);
        if cancelled_dictation(&job, &dictation_cancels) {
            let _ = job.reply.send(Err(EngineError::Cancelled.into()));
            continue;
        }
        let result = (|| -> Result<()> {
            if model.as_ref() != Some(&job.selected.path) || supervisor.loaded().is_none() {
                supervisor.load(LoadSpec {
                    path: job.selected.path.clone(),
                    backend: Backend::Auto,
                    device: DeviceSelector::Auto,
                })?;
                model = Some(job.selected.path.clone());
            }
            if let Some(chunk) = job.chunks.pop_front() {
                let pcm = if job.priority == Priority::Dictation {
                    padded(chunk)
                } else {
                    if hears(&chunk, job.gain)? {
                        chunk
                    } else {
                        Vec::new()
                    }
                };
                if !pcm.is_empty() {
                    let mark = {
                        let mut serving = lock(&serving);
                        ensure!(
                            !cancelled_dictation(&job, &dictation_cancels),
                            EngineError::Cancelled
                        );
                        *serving = job.priority == Priority::Dictation;
                        supervisor.cancel_mark()
                    };
                    let vocabulary = supervisor
                        .loaded()
                        .is_some_and(|info| info.supports_vocabulary);
                    let run = run_options(&job.selected, job.words.clone(), vocabulary);
                    let echo = run.vocabulary.clone();
                    let heard = supervisor.transcribe_since(mark, pcm, run);
                    *lock(&serving) = false;
                    job.text.push(without_echo(heard?.text, &echo));
                }
            }
            Ok(())
        })();
        last_work = Instant::now();
        match result {
            Err(error) => {
                let _ = job.reply.send(Err(error.context("The speech model could not finish. Your recorded audio is kept; try again or choose a smaller model in Settings.")));
            }
            Ok(()) if job.chunks.is_empty() => {
                let refs = job.text.iter().map(String::as_str).collect::<Vec<_>>();
                let _ = job.reply.send(Ok(super::chunking::join_transcripts(&refs)));
            }
            Ok(()) => pending.push(job),
        }
    }
}

#[cfg(test)]
mod audio_level_tests {
    use super::*;

    #[test]
    fn a_short_dictation_is_padded_and_an_empty_one_stays_empty() {
        let short = padded(vec![0.5; 4_000]);
        assert_eq!(short.len(), SHORT_CLIP_PADDED);
        assert_eq!(short[3_999], 0.5);
        assert_eq!(short[4_000], 0.0);
        assert!(padded(Vec::new()).is_empty());
        assert_eq!(padded(vec![0.5; SHORTEST_CLIP]).len(), SHORTEST_CLIP);
    }

    #[test]
    fn a_quiet_voice_is_raised_and_a_loud_one_left_alone() {
        assert_eq!(speech_gain(&[0.0; 1_024]), 1.0);
        assert_eq!(speech_gain(&[0.5; 1_024]), 1.0);
        assert!((speech_gain(&[0.01; 1_024]) - 10.0).abs() < 0.01);
        assert_eq!(speech_gain(&[0.000_01; 1_024]), MOST_GAIN);
    }

    #[test]
    fn a_silent_lead_in_does_not_hide_a_quiet_voice() {
        let mut clip = vec![0.0; 256 * 99];
        clip.extend(vec![0.01; 256]);
        assert!((speech_gain(&clip) - 10.0).abs() < 0.01);
    }

    #[test]
    fn the_model_gets_a_recording_at_its_original_level() {
        let mut clip = vec![0.01; 25_600];
        clip[0] = 0.5;
        let gain = speech_gain(&clip);
        let chunks = speech_chunks(clip, gain).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!((chunks[0][0] - 0.5).abs() < 0.000_1);
        assert!((chunks[0][25_000] - 0.01).abs() < 0.000_1);
    }

    #[test]
    fn a_loud_voice_near_the_microphone_does_not_hide_the_lecturer() {
        let mut clip = vec![0.002; 7_000];
        clip.extend(vec![0.5; 3_000]);
        assert!((speech_gain(&clip) - 50.0).abs() < 0.01);
    }

    #[test]
    fn digital_silence_has_no_speech_in_it() {
        assert!(!heard_speech(&[0.0; 32_000], 1.0).unwrap());
    }

    #[test]
    fn shaders_are_prepared_once_per_version() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(SHADERS_MARKER);
        assert!(!shaders_ready(&marker, "1.0.0"));
        std::fs::write(&marker, "1.0.0").unwrap();
        assert!(shaders_ready(&marker, "1.0.0"));
        assert!(!shaders_ready(&marker, "1.0.1"));
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;

    #[test]
    fn stop_and_unload_wait_for_capacity_without_losing_queued_work() {
        for capacity in [1, QUEUED] {
            for command in ["stop", "unload"] {
                let (requests, queue) = mpsc::sync_channel(capacity);
                for _ in 0..capacity {
                    requests.try_send("queued").unwrap();
                }
                let mut pauses = 0;
                send_control_before(
                    &requests,
                    command,
                    Instant::now() + CONTROL_LIMIT,
                    |delay| {
                        assert!(delay <= POLL);
                        pauses += 1;
                        assert_eq!(queue.try_recv().unwrap(), "queued");
                    },
                )
                .unwrap();
                assert_eq!(pauses, 1);
                for _ in 1..capacity {
                    assert_eq!(queue.try_recv().unwrap(), "queued");
                }
                assert_eq!(queue.try_recv().unwrap(), command);
            }
        }
    }

    #[test]
    fn an_expired_control_deadline_never_enqueues_a_command() {
        let (requests, queue) = mpsc::sync_channel(1);
        let result = send_control_before(&requests, "stop", Instant::now(), |_| {
            panic!("An expired control must not wait.")
        });
        assert!(result.is_err());
        assert_eq!(queue.try_recv(), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn a_control_that_expires_while_waiting_is_not_sent_later() {
        for delay in [Duration::ZERO, Duration::from_millis(10)] {
            let (requests, queue) = mpsc::sync_channel(1);
            requests.try_send("queued").unwrap();
            let mut paused = false;
            let deadline = Instant::now() + Duration::from_millis(5);
            std::thread::sleep(delay);
            let result = send_control_before(&requests, "stop", deadline, |_| {
                paused = true;
                std::thread::sleep(Duration::from_millis(10));
                assert_eq!(queue.try_recv().unwrap(), "queued");
            });
            assert!(result.is_err());
            assert!(Instant::now() >= deadline);
            if !paused {
                assert_eq!(queue.try_recv().unwrap(), "queued");
            }
            assert_eq!(queue.try_recv(), Err(mpsc::TryRecvError::Empty));
        }
    }

    #[test]
    fn a_disconnected_engine_does_not_wait_for_control_capacity() {
        let (requests, queue) = mpsc::sync_channel(1);
        drop(queue);
        let result = send_control_before(&requests, "stop", Instant::now() + CONTROL_LIMIT, |_| {
            panic!("A disconnected engine must not wait.")
        });
        assert!(result.unwrap_err().to_string().contains("Reopen Leo"));
    }

    #[test]
    fn available_control_capacity_needs_no_wait() {
        let (requests, queue) = mpsc::sync_channel(1);
        send_control_before(&requests, "unload", Instant::now() + CONTROL_LIMIT, |_| {
            panic!("Available control capacity must not wait.")
        })
        .unwrap();
        assert_eq!(queue.try_recv().unwrap(), "unload");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_jobs_are_bounded_even_while_dictation_reserves_the_engine() {
        let engine = Arc::new(Engine::new());
        let _dictating = engine.dictation();
        let (send, answers) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..QUEUED * 2 {
            let engine = engine.clone();
            let send = send.clone();
            workers.push(std::thread::spawn(move || {
                let selected = Selected {
                    id: "test".into(),
                    language: "en".into(),
                    path: "unused".into(),
                    streaming: false,
                    live: true,
                };
                let result = engine.transcribe(selected, Vec::new(), Priority::Segment, Vec::new());
                let _ = send.send(result);
            }));
        }
        drop(send);
        for _ in 0..QUEUED {
            assert!(answers
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .is_err());
        }
        assert_eq!(engine.admitted.load(Ordering::Acquire), QUEUED);
        engine.stop().unwrap();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(engine.admitted.load(Ordering::Acquire), 0);
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    #[test]
    fn a_cold_model_cannot_be_deleted_during_capture() {
        let engine = Engine::new();
        let pin = engine.pin();
        let changed = Arc::new(AtomicBool::new(false));
        let observed = changed.clone();
        assert!(engine
            .edit_model("cold-model".into(), true, move || {
                observed.store(true, Ordering::Release);
                Ok(serde_json::json!({}))
            })
            .is_err());
        assert!(!changed.load(Ordering::Acquire));
        drop(pin);
        engine.stop().unwrap();
    }
}

#[cfg(test)]
mod update_tests {
    #[test]
    fn unloading_for_an_update_keeps_the_engine_usable_if_installation_fails() {
        let engine = super::Engine::new();
        engine.unload_worker().unwrap();
        assert!(!engine.loaded());
        assert_eq!(
            engine
                .edit_model("unused".into(), false, || Ok(
                    serde_json::json!({"alive":true})
                ))
                .unwrap()["alive"],
            true
        );
        engine.stop().unwrap();
    }
}

#[cfg(test)]
mod heard_tests {
    use super::*;

    #[test]
    fn queued_audio_is_fed_together_in_order() {
        let mut heard = Heard::default();
        heard.take(vec![1.0; 300]).unwrap();
        heard.take(vec![2.0; 200]).unwrap();
        let mut fed = Vec::new();
        heard
            .send(|part| {
                fed.push(part);
                Fed::Sent
            })
            .unwrap();
        assert_eq!(fed.len(), 1);
        assert_eq!(fed[0].len(), 500);
        assert_eq!(fed[0][299], 1.0);
        assert_eq!(fed[0][300], 2.0);
        assert!(heard.queued.is_empty());
    }

    #[test]
    fn a_full_engine_keeps_the_audio_for_the_next_feed() {
        let mut heard = Heard::default();
        heard.take(vec![1.0; 10_000]).unwrap();
        heard.take(vec![2.0; 10_000]).unwrap();
        let mut calls = 0;
        heard
            .send(|part| {
                calls += 1;
                if calls == 1 {
                    assert_eq!(part.len(), 16_000);
                    Fed::Sent
                } else {
                    Fed::Full(part)
                }
            })
            .unwrap();
        assert_eq!(heard.queued, vec![2.0; 4_000]);
        heard.take(vec![3.0; 100]).unwrap();
        let mut fed = Vec::new();
        heard
            .send(|part| {
                fed.extend(part);
                Fed::Sent
            })
            .unwrap();
        assert_eq!(fed.len(), 4_100);
        assert_eq!(fed[3_999], 2.0);
        assert_eq!(fed[4_000], 3.0);
    }

    #[test]
    fn a_lost_stream_and_oversized_audio_are_refused() {
        let mut heard = Heard::default();
        heard.take(vec![0.0; 10]).unwrap();
        assert!(heard.send(|_| Fed::Lost).is_err());
        assert!(Heard::default().take(vec![0.0; 16_001]).is_err());
        assert!(Heard::default().take(vec![f32::NAN]).is_err());
        let mut long = Heard::default();
        for _ in 0..300 {
            long.take(vec![0.0; 16_000]).unwrap();
        }
        assert!(long.take(vec![0.0; 1]).is_err());
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;

    #[test]
    fn custom_words_reach_only_models_that_take_vocabulary() {
        let selected = Selected {
            id: "test".into(),
            language: "auto".into(),
            path: "unused".into(),
            streaming: false,
            live: false,
        };
        let words = vec!["Leo".to_string()];
        let damaged = vec!["Leo".to_string(), "a\0b".to_string()];
        assert_eq!(run_options(&selected, damaged, true).vocabulary, words);
        assert_eq!(
            run_options(&selected, words.clone(), true).vocabulary,
            words
        );
        let run = run_options(&selected, words, false);
        assert!(run.vocabulary.is_empty());
        assert_eq!(run.language, None);
    }

    #[test]
    fn an_echo_is_stripped_only_from_runs_that_were_given_vocabulary() {
        let list = ["OpenWhispr", "Parakeet", "Alcahest"].map(String::from);
        let echo = "OpenWhispr, Parakeet, Alcahest".to_string();
        assert_eq!(without_echo(echo.clone(), &list), "");
        assert_eq!(without_echo(echo.clone(), &[]), echo);
        let names = ["Leo", "Felix", "Claude", "Codex"].map(String::from);
        assert_eq!(
            without_echo("Leo Felix Claude".into(), &names),
            "Leo Felix Claude"
        );
    }
}

#[cfg(all(test, unix))]
mod cancel_tests {
    use super::super::supervisor::fake_tests::{fake, ENV};
    use super::*;

    struct Worker;
    impl Drop for Worker {
        fn drop(&mut self) {
            std::env::remove_var("LEO_TRANSCRIBE_WORKER_EXE");
        }
    }

    fn appears(path: &Path) {
        let started = Instant::now();
        while !path.exists() {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn selected(dir: &Path, mode: &str) -> Selected {
        Selected {
            id: "test".into(),
            language: "en".into(),
            path: dir.join(mode),
            streaming: false,
            live: false,
        }
    }

    fn start(
        engine: &Arc<Engine>,
        selected: Selected,
        priority: Priority,
    ) -> std::thread::JoinHandle<Result<String>> {
        let engine = engine.clone();
        std::thread::spawn(move || {
            engine.transcribe(selected, vec![0.2; 1_600], priority, Vec::new())
        })
    }

    fn cancelled(answer: Result<String>) -> bool {
        matches!(
            answer.unwrap_err().downcast_ref::<EngineError>(),
            Some(EngineError::Cancelled)
        )
    }

    #[test]
    fn a_dictation_cancel_leaves_a_recording_call_running() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let _worker = Worker;
        let dir = fake();
        let engine = Arc::new(Engine::with(|_, _| Ok(true)));
        let segment = start(&engine, selected(dir.path(), "waits"), Priority::Segment);
        appears(&dir.path().join("running"));
        engine.cancel();
        std::fs::write(dir.path().join("go"), "").unwrap();
        assert_eq!(segment.join().unwrap().unwrap(), "local words");
        engine.stop().unwrap();
    }

    #[test]
    fn a_dictation_waiting_behind_a_recording_call_cancels_at_once() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let _worker = Worker;
        let dir = fake();
        let engine = Arc::new(Engine::with(|_, _| Ok(true)));
        let segment = start(&engine, selected(dir.path(), "waits"), Priority::Segment);
        appears(&dir.path().join("running"));
        let dictation = start(&engine, selected(dir.path(), "waits"), Priority::Dictation);
        let admitting = Instant::now();
        while engine.admitted.load(Ordering::Acquire) < 2 {
            assert!(admitting.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(1));
        }
        let asked = Instant::now();
        engine.cancel();
        assert!(cancelled(dictation.join().unwrap()));
        assert!(asked.elapsed() < Duration::from_millis(200));
        std::fs::write(dir.path().join("go"), "").unwrap();
        assert_eq!(segment.join().unwrap().unwrap(), "local words");
        engine.stop().unwrap();
    }

    #[test]
    fn a_dictation_cancelled_while_its_model_loads_is_never_transcribed() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let _worker = Worker;
        let dir = fake();
        let engine = Arc::new(Engine::with(|_, _| Ok(true)));
        let dictation = start(
            &engine,
            selected(dir.path(), "loadwaits"),
            Priority::Dictation,
        );
        appears(&dir.path().join("loading"));
        engine.cancel();
        assert!(cancelled(dictation.join().unwrap()));
        std::fs::write(dir.path().join("go"), "").unwrap();
        engine
            .edit_model("unused".into(), false, || Ok(serde_json::json!({})))
            .unwrap();
        assert!(!dir.path().join("running").exists());
        engine.stop().unwrap();
    }
}
