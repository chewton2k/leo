use super::protocol::{
    encode_message, read_message, DeviceInfo, DeviceSelector, LoadedInfo, Request, Response, Turn,
};
use super::worker::LOG_LINE_PREFIX;
use super::{CPU_ONLY_FLAG, LOG_LEVEL_ENV, WORKER_FLAG};
use log::{debug, error, info, warn, Level};
use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use transcribe_cpp::{Backend, RunOptions, StreamOptions, StreamText, StreamUpdate, Transcript};

const HELLO_TIMEOUT: Duration = Duration::from_secs(60);
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
const CALL_FLOOR: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
const STDERR_TAIL_LINES: usize = 64;
const STDERR_LINE_BYTES: usize = 4096;
const QUEUED_COMMANDS: usize = 16;
const SAMPLE_RATE: f64 = 16_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Deadlines {
    call_floor: Duration,
    run_floor: Duration,
    audio_factor: f64,
}

const GPU_DEADLINES: Deadlines = Deadlines {
    call_floor: Duration::from_secs(30),
    run_floor: Duration::from_secs(60),
    audio_factor: 10.0,
};

const CPU_DEADLINES: Deadlines = Deadlines {
    call_floor: Duration::from_secs(120),
    run_floor: Duration::from_secs(300),
    audio_factor: 20.0,
};

impl Deadlines {
    fn for_device(on_gpu: bool) -> Self {
        if on_gpu {
            GPU_DEADLINES
        } else {
            CPU_DEADLINES
        }
    }

    fn audio(&self, samples: usize) -> Duration {
        self.call_floor.max(Duration::from_secs_f64(
            samples as f64 / SAMPLE_RATE * self.audio_factor,
        ))
    }

    fn run(&self, samples: usize) -> Duration {
        self.run_floor.max(self.audio(samples))
    }

    fn diarize(&self, samples: usize) -> Duration {
        self.run_floor
            + Duration::from_secs_f64(samples as f64 / SAMPLE_RATE * self.audio_factor / 40.0)
    }
}

fn ms_to_samples(ms: i64) -> usize {
    (ms.max(0) as usize) * (SAMPLE_RATE as usize / 1000)
}

#[derive(Debug, Clone)]
pub struct LoadSpec {
    pub path: PathBuf,
    pub backend: Backend,
    pub device: DeviceSelector,
}

impl LoadSpec {
    fn pinned(&self) -> bool {
        matches!(self.device, DeviceSelector::Index(_))
    }

    fn cpu_only(&self) -> bool {
        self.backend == Backend::Cpu
    }

    fn cpu(&self) -> Self {
        Self {
            path: self.path.clone(),
            backend: Backend::Cpu,
            device: DeviceSelector::Auto,
        }
    }
}

#[derive(Debug)]
pub enum EngineError {
    Cancelled,
    Failed(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Cancelled => f.write_str("transcription was cancelled"),
            EngineError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Clone, Copy)]
pub struct CancelMark(u64);

pub struct StreamProgress {
    pub update: StreamUpdate,
    pub text: Option<StreamText>,
    pub elapsed: Duration,
}

pub struct Finalized {
    pub update: StreamUpdate,
    pub text: StreamText,
    pub language: Option<String>,
    pub elapsed: Duration,
}

type Reply<T> = mpsc::Sender<T>;
type OnProgress = Box<dyn FnMut(StreamProgress) + Send>;

enum Command {
    Load(LoadSpec, Reply<Result<LoadedInfo, EngineError>>),
    Unload(Reply<()>),
    Transcribe {
        pcm: Vec<f32>,
        run: RunOptions,
        epoch: u64,
        reply: Reply<Result<Transcript, EngineError>>,
    },
    StreamBegin {
        active: ActiveStream,
        run: RunOptions,
        stream: StreamOptions,
        reply: Reply<Result<(), EngineError>>,
    },
    Feed {
        id: u64,
        pcm: Vec<f32>,
    },
    Finalize {
        id: u64,
        want_language: bool,
        epoch: u64,
        reply: Reply<Result<Option<Finalized>, EngineError>>,
    },
    StreamReset {
        id: u64,
    },
    Diarize {
        request: Request,
        samples: usize,
        epoch: u64,
        reply: Reply<Result<Vec<Turn>, EngineError>>,
    },
}

#[derive(Default)]
struct Shared {
    gpu_allowed: bool,
    gpu_unavailable: AtomicBool,
    loaded: Mutex<Option<LoadedInfo>>,
    devices: Mutex<Option<Vec<DeviceInfo>>>,
    probing: Mutex<()>,
    cancel_epoch: AtomicU64,
    in_flight: Mutex<Option<InFlight>>,
    unloads_requested: AtomicU64,
    next_id: AtomicU64,
}

impl Shared {
    fn probe_devices(&self) {
        let cpu_only = !self.gpu_allowed;
        match self.hello(cpu_only, true) {
            Ok(worker) => drop(worker),
            Err(failure) => {
                error!(
                    "Could not start a transcription worker to list compute devices: {}",
                    failure
                );
                if failure.worker_lost() && !cpu_only {
                    self.mark_gpu_unavailable(format!(
                        "a worker with GPU backends {failure} while listing compute devices"
                    ));
                }
            }
        }
    }

    fn hello(&self, cpu_only: bool, list_devices: bool) -> Result<Worker, Failure> {
        let mut worker = Worker::spawn(cpu_only).map_err(Failure::Spawn)?;
        match worker.call(&Request::Hello { list_devices }, None, HELLO_TIMEOUT)? {
            Response::Hello { devices } => {
                if let Some(devices) = devices {
                    self.set_devices(devices);
                }
                Ok(worker)
            }
            other => Err(unexpected(&other)),
        }
    }

    fn set_devices(&self, new: Vec<DeviceInfo>) {
        let mut current = lock(&self.devices);
        if let Some(old) = &*current {
            let identity = |d: &DeviceInfo| (d.key.clone(), d.index);
            if !old.iter().map(identity).eq(new.iter().map(identity)) {
                info!(
                    "transcribe-cpp compute devices changed: [{}]",
                    new.iter()
                        .map(|d| format!("{} ({})", d.name, d.kind))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        *current = Some(new);
    }

    fn gpu_unavailable(&self) -> bool {
        self.gpu_unavailable.load(Ordering::Acquire)
    }

    fn mark_gpu_unavailable(&self, reason: impl fmt::Display) {
        if !self.gpu_unavailable.swap(true, Ordering::AcqRel) {
            warn!("Transcription GPU marked unavailable: {}", reason);
        }
    }

    fn epoch(&self) -> u64 {
        self.cancel_epoch.load(Ordering::SeqCst)
    }

    fn cancelled_since(&self, epoch: u64) -> bool {
        self.epoch() != epoch
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }
}

struct InFlight {
    what: &'static str,
    control: Arc<Control>,
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
pub struct EngineSupervisor {
    commands: mpsc::SyncSender<Command>,
    shared: Arc<Shared>,
}

impl EngineSupervisor {
    pub fn new(gpu_allowed: bool) -> Self {
        #[cfg(all(unix, not(test)))]
        record_launched_exe();
        let (commands, queue) = mpsc::sync_channel(QUEUED_COMMANDS);
        let shared = Arc::new(Shared {
            gpu_allowed,
            ..Shared::default()
        });
        let owner = Owner {
            shared: Arc::clone(&shared),
            worker: None,
            spec: None,
            stream: None,
            unloads_done: 0,
            gpu_backend_error: false,
        };
        thread::Builder::new()
            .name("transcribe-engine".into())
            .spawn(move || owner.run(queue))
            .expect("failed to start the transcription engine thread");
        Self { commands, shared }
    }

    fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Option<T> {
        let (reply, answer) = mpsc::channel();
        self.commands.try_send(command(reply)).ok()?;
        answer.recv().ok()
    }

    pub fn load(&self, spec: LoadSpec) -> Result<LoadedInfo, EngineError> {
        self.ask(|reply| Command::Load(spec, reply))
            .unwrap_or_else(|| Err(engine_stopped()))
    }

    pub fn unload(&self) -> Unloading {
        let (reply, exited) = mpsc::channel();
        {
            let mut loaded = lock(&self.shared.loaded);
            self.shared.unloads_requested.fetch_add(1, Ordering::SeqCst);
            *loaded = None;
            if self.commands.try_send(Command::Unload(reply)).is_err() {
                return Unloading(None);
            }
        }
        Unloading(Some(exited))
    }

    pub fn loaded(&self) -> Option<LoadedInfo> {
        lock(&self.shared.loaded).clone()
    }

    pub fn transcribe(&self, pcm: Vec<f32>, run: RunOptions) -> Result<Transcript, EngineError> {
        self.transcribe_since(self.cancel_mark(), pcm, run)
    }

    pub fn cancel_mark(&self) -> CancelMark {
        CancelMark(self.shared.epoch())
    }

    pub fn transcribe_since(
        &self,
        mark: CancelMark,
        pcm: Vec<f32>,
        run: RunOptions,
    ) -> Result<Transcript, EngineError> {
        let epoch = mark.0;
        self.ask(|reply| Command::Transcribe {
            pcm,
            run,
            epoch,
            reply,
        })
        .unwrap_or_else(|| Err(engine_stopped()))
    }

    pub fn diarize(
        &self,
        model: PathBuf,
        parts: Vec<(PathBuf, u64)>,
        samples: usize,
    ) -> Result<Vec<Turn>, EngineError> {
        let epoch = self.cancel_mark().0;
        self.ask(|reply| Command::Diarize {
            request: Request::Diarize { model, parts },
            samples,
            epoch,
            reply,
        })
        .unwrap_or_else(|| Err(engine_stopped()))
    }

    pub fn start_stream(
        &self,
        run: RunOptions,
        stream: StreamOptions,
        on_progress: impl FnMut(StreamProgress) + Send + 'static,
    ) -> Result<StreamHandle, EngineError> {
        let id = self.shared.next_id();
        let epoch = self.shared.epoch();
        let closed = Arc::new(AtomicBool::new(false));
        self.ask(|reply| Command::StreamBegin {
            active: ActiveStream {
                id,
                epoch,
                closed: Arc::clone(&closed),
                on_progress: Box::new(on_progress),
                pending_ms: 0,
            },
            run,
            stream,
            reply,
        })
        .unwrap_or_else(|| Err(engine_stopped()))?;
        Ok(StreamHandle {
            id,
            epoch,
            closed,
            commands: self.commands.clone(),
            finished: false,
        })
    }

    pub fn cancel(&self) {
        self.shared.cancel_epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(call) = lock(&self.shared.in_flight).take() {
            info!(
                "Cancelling the {} by stopping its worker (pid {})",
                call.what, call.control.pid
            );
            call.control.kill();
        }
    }

    pub fn devices(&self) -> Option<Vec<DeviceInfo>> {
        let _probing = lock(&self.shared.probing);
        if lock(&self.shared.devices).is_none() && !self.shared.gpu_unavailable() {
            self.shared.probe_devices();
        }
        lock(&self.shared.devices).clone()
    }
}

pub struct Unloading(Option<mpsc::Receiver<()>>);

impl Unloading {
    pub fn wait(self) {
        if let Some(exited) = self.0 {
            let _ = exited.recv();
        }
    }
}

pub struct StreamHandle {
    id: u64,
    epoch: u64,
    closed: Arc<AtomicBool>,
    commands: mpsc::SyncSender<Command>,
    finished: bool,
}

pub enum Fed {
    Sent,
    Full(Vec<f32>),
    Lost,
}

impl StreamHandle {
    pub fn feed(&self, pcm: Vec<f32>) -> Fed {
        match self.commands.try_send(Command::Feed { id: self.id, pcm }) {
            Ok(()) => Fed::Sent,
            Err(mpsc::TrySendError::Full(Command::Feed { pcm, .. })) => Fed::Full(pcm),
            Err(_) => Fed::Lost,
        }
    }

    pub fn finalize(mut self, want_language: bool) -> Result<Option<Finalized>, EngineError> {
        self.finished = true;
        let (reply, answer) = mpsc::channel();
        let command = Command::Finalize {
            id: self.id,
            want_language,
            epoch: self.epoch,
            reply,
        };
        if self.commands.send(command).is_err() {
            return Ok(None);
        }
        answer.recv().unwrap_or(Ok(None))
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        if !self.finished {
            self.closed.store(true, Ordering::Release);
            let _ = self.commands.try_send(Command::StreamReset { id: self.id });
        }
    }
}

fn engine_stopped() -> EngineError {
    EngineError::Failed("the transcription engine has stopped".to_string())
}

fn not_loaded() -> EngineError {
    EngineError::Failed("no transcription model is loaded".to_string())
}

#[derive(Debug)]
enum Failure {
    Spawn(io::Error),
    Remote(String),
    Crashed(String),
    Hung(Duration),
    Backend(String),
    Cancelled,
}

impl Failure {
    fn worker_lost(&self) -> bool {
        matches!(
            self,
            Failure::Crashed(_) | Failure::Hung(_) | Failure::Backend(_)
        )
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Spawn(e) => write!(f, "failed to start transcription worker: {e}"),
            Failure::Remote(message) => f.write_str(message),
            Failure::Crashed(detail) => write!(f, "transcription worker crashed ({detail})"),
            Failure::Hung(t) => write!(f, "transcription worker did not respond within {t:?}"),
            Failure::Backend(message) => write!(f, "transcription backend failed ({message})"),
            Failure::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl From<Failure> for EngineError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Cancelled => EngineError::Cancelled,
            other => EngineError::Failed(other.to_string()),
        }
    }
}

fn unexpected(response: &Response) -> Failure {
    Failure::Remote(format!(
        "unexpected transcription worker response: {response:?}"
    ))
}

struct ActiveStream {
    id: u64,
    epoch: u64,
    closed: Arc<AtomicBool>,
    on_progress: OnProgress,
    pending_ms: i64,
}

struct Owner {
    shared: Arc<Shared>,
    worker: Option<Worker>,
    spec: Option<LoadSpec>,
    stream: Option<ActiveStream>,
    unloads_done: u64,
    gpu_backend_error: bool,
}

impl Owner {
    fn run(mut self, queue: mpsc::Receiver<Command>) {
        while let Ok(command) = queue.recv() {
            match command {
                Command::Load(spec, reply) => {
                    let _ = reply.send(self.load(spec));
                }
                Command::Unload(reply) => {
                    self.unload();
                    self.unloads_done += 1;
                    let _ = reply.send(());
                }
                Command::Transcribe {
                    pcm,
                    run,
                    epoch,
                    reply,
                } => {
                    let _ = reply.send(self.transcribe(&pcm, &run, epoch));
                }
                Command::StreamBegin {
                    active,
                    run,
                    stream,
                    reply,
                } => {
                    let _ = reply.send(self.stream_begin(active, &run, &stream));
                }
                Command::Feed { id, pcm } => self.feed(id, &pcm),
                Command::Finalize {
                    id,
                    want_language,
                    epoch,
                    reply,
                } => {
                    let _ = reply.send(self.finalize(id, want_language, epoch));
                }
                Command::Diarize {
                    request,
                    samples,
                    epoch,
                    reply,
                } => {
                    let _ = reply.send(self.diarize(&request, samples, epoch));
                }
                Command::StreamReset { id } => {
                    if self.stream.as_ref().is_some_and(|s| s.id == id) {
                        self.end_stream();
                    }
                }
            }
        }
        self.unload();
    }

    fn load(&mut self, spec: LoadSpec) -> Result<LoadedInfo, EngineError> {
        self.unload();
        self.spec = Some(spec);
        let result = self.start();
        if result.is_err() {
            self.spec = None;
        }
        result
    }

    fn unload(&mut self) {
        self.stream = None;
        if let Some(worker) = self.worker.take() {
            let started = Instant::now();
            let pid = worker.control.pid;
            drop(worker);
            debug!(
                "Transcription worker (pid {}) stopped in {}ms",
                pid,
                started.elapsed().as_millis()
            );
        }
        self.spec = None;
        *lock(&self.shared.loaded) = None;
    }

    fn effective_spec(&self, spec: &LoadSpec) -> LoadSpec {
        if spec.pinned() || spec.cpu_only() || !self.shared.gpu_unavailable() {
            return spec.clone();
        }
        info!("Loading the transcription model on CPU: the GPU failed earlier");
        spec.cpu()
    }

    fn cpu_only_worker(&self, spec: &LoadSpec) -> bool {
        spec.cpu_only() || !self.shared.gpu_allowed
    }

    fn start_worker(&self, spec: &LoadSpec) -> Result<(Worker, LoadedInfo), Failure> {
        let cpu_only = self.cpu_only_worker(spec);
        let mut worker = self.shared.hello(cpu_only, !cpu_only)?;
        let info = worker.load(spec)?;
        Ok((worker, info))
    }

    fn start(&mut self) -> Result<LoadedInfo, EngineError> {
        let spec = self.spec.clone().ok_or_else(not_loaded)?;
        let started = Instant::now();
        let wanted = self.effective_spec(&spec);
        let (worker, info) = match self.start_worker(&wanted) {
            Ok(started) => started,
            Err(failure)
                if failure.worker_lost() && !self.cpu_only_worker(&wanted) && !wanted.pinned() =>
            {
                self.shared.mark_gpu_unavailable(format!(
                    "a worker for '{}' with GPU backends {}",
                    wanted.path.display(),
                    failure
                ));
                warn!("Retrying in a CPU-only worker");
                self.start_worker(&wanted.cpu())?
            }
            Err(failure) => return Err(failure.into()),
        };
        debug!(
            "Worker (pid {}) spawned and loaded the model in {}ms",
            worker.control.pid,
            started.elapsed().as_millis()
        );
        self.publish_loaded(&info);
        self.worker = Some(worker);
        Ok(info)
    }

    fn publish_loaded(&self, info: &LoadedInfo) {
        let mut loaded = lock(&self.shared.loaded);
        if self.shared.unloads_requested.load(Ordering::SeqCst) == self.unloads_done {
            *loaded = Some(info.clone());
        }
    }

    fn ensure_worker(&mut self) -> Result<(), EngineError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if self.spec.is_none() {
            return Err(not_loaded());
        }
        info!("Starting a fresh transcription worker for the loaded model");
        if let Err(e) = self.start() {
            self.spec = None;
            *lock(&self.shared.loaded) = None;
            return Err(e);
        }
        Ok(())
    }

    fn worker_lost(&mut self, during: &str, failure: &Failure) -> bool {
        let info = self.worker.take().and_then(|worker| worker.info.clone());
        *lock(&self.shared.loaded) = None;
        let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
        match info {
            Some(info) if info.on_gpu => {
                if matches!(failure, Failure::Backend(_)) && !self.gpu_backend_error {
                    self.gpu_backend_error = true;
                    warn!(
                        "{failure} during {during} on GPU '{}' ({}); restarting the worker",
                        info.device, info.backend
                    );
                } else if !pinned {
                    self.shared.mark_gpu_unavailable(format!(
                        "{failure} during {during} on GPU '{}' ({})",
                        info.device, info.backend
                    ));
                }
                true
            }
            _ => false,
        }
    }

    fn deadlines(&self) -> Deadlines {
        Deadlines::for_device(self.worker_on_gpu())
    }

    fn worker_on_gpu(&self) -> bool {
        self.worker
            .as_ref()
            .and_then(|worker| worker.info.as_ref())
            .is_some_and(|info| info.on_gpu)
    }

    fn call_work(
        &mut self,
        request: &Request,
        pcm: Option<&[f32]>,
        deadline: Duration,
        epoch: u64,
    ) -> Result<Response, Failure> {
        let worker = self.worker.as_mut().expect("caller ensured a worker");
        let frame = encode_request(request, pcm)?;
        let sent = {
            let mut in_flight = lock(&self.shared.in_flight);
            if self.shared.cancelled_since(epoch) {
                return Err(Failure::Cancelled);
            }
            let sent = worker.control.send(frame);
            *in_flight = Some(InFlight {
                what: request_name(request),
                control: Arc::clone(&worker.control),
            });
            sent
        };
        let result = if sent {
            worker.wait(request, deadline)
        } else {
            Err(worker.died())
        };
        let killed = lock(&self.shared.in_flight).take().is_none();
        if killed || self.shared.cancelled_since(epoch) {
            if killed || result.as_ref().is_err_and(Failure::worker_lost) {
                self.worker = None;
                *lock(&self.shared.loaded) = None;
            }
            return Err(Failure::Cancelled);
        }
        result
    }

    fn run_succeeded(&mut self) {
        if self.worker_on_gpu() {
            self.gpu_backend_error = false;
        }
    }

    fn transcribe(
        &mut self,
        pcm: &[f32],
        run: &RunOptions,
        epoch: u64,
    ) -> Result<Transcript, EngineError> {
        self.end_stream();
        let request = Request::Run {
            options: run.clone(),
        };
        let mut retried = false;
        loop {
            if self.shared.cancelled_since(epoch) {
                return Err(EngineError::Cancelled);
            }
            self.ensure_worker()?;
            let deadline = self.deadlines().run(pcm.len());
            match self.call_work(&request, Some(pcm), deadline, epoch) {
                Ok(Response::Transcript(transcript)) => {
                    self.run_succeeded();
                    return Ok(transcript);
                }
                Ok(other) => return Err(unexpected(&other).into()),
                Err(failure) if failure.worker_lost() => {
                    let on_gpu = self.worker_lost("transcription", &failure);
                    let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
                    let failed_on_cpu =
                        !on_gpu && matches!(failure, Failure::Hung(_) | Failure::Backend(_));
                    let fresh_gpu = on_gpu && !self.shared.gpu_unavailable();
                    if pinned || failed_on_cpu || (retried && !fresh_gpu) {
                        return Err(failure.into());
                    }
                    if fresh_gpu {
                        warn!("Retrying the transcription in a fresh GPU worker");
                    } else {
                        warn!("Retrying the transcription in a fresh worker");
                        retried = true;
                    }
                }
                Err(failure) => return Err(failure.into()),
            }
        }
    }

    fn diarize(
        &mut self,
        request: &Request,
        samples: usize,
        epoch: u64,
    ) -> Result<Vec<Turn>, EngineError> {
        self.end_stream();
        self.ensure_worker()?;
        let deadline = self.deadlines().diarize(samples);
        match self.call_work(request, None, deadline, epoch) {
            Ok(Response::Turns(turns)) => Ok(turns),
            Ok(other) => Err(unexpected(&other).into()),
            Err(failure) => {
                if failure.worker_lost() {
                    self.worker = None;
                    *lock(&self.shared.loaded) = None;
                }
                Err(failure.into())
            }
        }
    }

    fn stream_begin(
        &mut self,
        active: ActiveStream,
        run: &RunOptions,
        stream: &StreamOptions,
    ) -> Result<(), EngineError> {
        self.end_stream();
        if self.shared.cancelled_since(active.epoch) {
            return Err(EngineError::Cancelled);
        }
        self.ensure_worker()?;
        let worker = self.worker.as_mut().expect("worker was just ensured");
        let request = Request::StreamBegin {
            run: run.clone(),
            stream: stream.clone(),
        };
        match worker.call(&request, None, CALL_FLOOR) {
            Ok(Response::Ok) => {
                self.stream = Some(active);
                Ok(())
            }
            Ok(other) => Err(unexpected(&other).into()),
            Err(failure) => {
                if failure.worker_lost() {
                    self.worker_lost("stream start", &failure);
                }
                Err(failure.into())
            }
        }
    }

    fn feed(&mut self, id: u64, pcm: &[f32]) {
        let Some(stream) = self.stream.as_ref().filter(|s| s.id == id) else {
            return;
        };
        if stream.closed.load(Ordering::Acquire)
            || self.shared.cancelled_since(stream.epoch)
            || self.worker.is_none()
        {
            return;
        }
        let epoch = stream.epoch;
        let deadline = self
            .deadlines()
            .audio(pcm.len() + ms_to_samples(stream.pending_ms));
        let started = Instant::now();
        match self.call_work(&Request::Feed, Some(pcm), deadline, epoch) {
            Ok(Response::Fed { update, text }) => {
                let Some(stream) = self.stream.as_mut() else {
                    return;
                };
                stream.pending_ms = pending_ms(&update);
                let progress = StreamProgress {
                    update,
                    text,
                    elapsed: started.elapsed(),
                };
                if catch_unwind(AssertUnwindSafe(|| (stream.on_progress)(progress))).is_err() {
                    error!("Live transcription callback panicked");
                }
            }
            Ok(other) => {
                warn!("Stream feed failed: {}", unexpected(&other));
                self.end_stream();
            }
            Err(Failure::Remote(message)) => {
                warn!("Stream feed failed: {}", message);
                self.end_stream();
            }
            Err(Failure::Cancelled) => {}
            Err(failure) => {
                self.stream = None;
                self.worker_lost("stream feed", &failure);
            }
        }
    }

    fn finalize(
        &mut self,
        id: u64,
        want_language: bool,
        epoch: u64,
    ) -> Result<Option<Finalized>, EngineError> {
        if self.shared.cancelled_since(epoch) {
            if self.stream.as_ref().is_some_and(|s| s.id == id) {
                self.end_stream();
            }
            return Err(EngineError::Cancelled);
        }
        let Some(stream) = self.stream.take_if(|s| s.id == id) else {
            return Ok(None);
        };
        if self.worker.is_none() {
            return Ok(None);
        }
        let deadline = self.deadlines().audio(ms_to_samples(stream.pending_ms));
        let started = Instant::now();
        match self.call_work(&Request::Finalize { want_language }, None, deadline, epoch) {
            Ok(Response::Finalized {
                update,
                text,
                language,
            }) => {
                self.run_succeeded();
                Ok(Some(Finalized {
                    update,
                    text,
                    language,
                    elapsed: started.elapsed(),
                }))
            }
            Ok(other) => {
                error!("Stream finalize failed: {}", unexpected(&other));
                Ok(None)
            }
            Err(Failure::Cancelled) => {
                self.reset_worker_stream();
                Err(EngineError::Cancelled)
            }
            Err(Failure::Remote(message)) => {
                error!("Stream finalize failed: {}", message);
                Ok(None)
            }
            Err(failure) => {
                self.worker_lost("stream finalize", &failure);
                Ok(None)
            }
        }
    }

    fn end_stream(&mut self) {
        if self.stream.take().is_some() {
            self.reset_worker_stream();
        }
    }

    fn reset_worker_stream(&mut self) {
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        match worker.call(&Request::StreamReset, None, CALL_FLOOR) {
            Ok(_) => {}
            Err(Failure::Remote(message)) => debug!("Stream reset: {}", message),
            Err(failure) => {
                self.worker_lost("stream reset", &failure);
            }
        }
    }
}

fn pending_ms(update: &StreamUpdate) -> i64 {
    update
        .buffered_ms
        .max(update.input_received_ms - update.audio_committed_ms)
        .max(0)
}

struct Control {
    pid: u32,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<mpsc::SyncSender<Vec<u8>>>>,
}

impl Control {
    fn send(&self, frame: Vec<u8>) -> bool {
        lock(&self.stdin)
            .as_ref()
            .is_some_and(|stdin| stdin.try_send(frame).is_ok())
    }

    fn close_stdin(&self) {
        lock(&self.stdin).take();
    }

    fn kill(&self) {
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

struct Worker {
    control: Arc<Control>,
    responses: mpsc::Receiver<Response>,
    stderr_done: mpsc::Receiver<()>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    info: Option<LoadedInfo>,
}

impl Worker {
    fn spawn(cpu_only: bool) -> io::Result<Self> {
        #[cfg(test)]
        let exe = PathBuf::from(
            std::env::var_os("LEO_TRANSCRIBE_WORKER_EXE").expect("set LEO_TRANSCRIBE_WORKER_EXE"),
        );
        #[cfg(not(test))]
        let exe = worker_exe()?;
        let mut command = leo_core::children::quiet_command(exe);
        #[cfg(target_os = "linux")]
        if let Some(arg0) = std::env::args_os().next() {
            use std::os::unix::process::CommandExt;
            command.arg0(arg0);
        }
        command.arg(WORKER_FLAG);
        if cpu_only {
            command.arg(CPU_ONLY_FLAG);
        }
        command
            .env(LOG_LEVEL_ENV, log::max_level().as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let pid = child.id();
        leo_core::children::mark(pid, "Speech engine");
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

        let (stdin_tx, frames) = mpsc::sync_channel(2);
        let (response_tx, responses) = mpsc::sync_channel(2);
        let (stderr_done_tx, stderr_done) = mpsc::channel();
        #[cfg(test)]
        LIVE_WORKERS.fetch_add(1, Ordering::SeqCst);
        let worker = Worker {
            control: Arc::new(Control {
                pid,
                child: Mutex::new(Some(child)),
                stdin: Mutex::new(Some(stdin_tx)),
            }),
            responses,
            stderr_done,
            stderr_tail: Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES))),
            info: None,
        };

        thread::Builder::new()
            .name("transcribe-worker-in".into())
            .spawn(move || write_frames(stdin, frames))?;
        thread::Builder::new()
            .name("transcribe-worker-out".into())
            .spawn(move || read_responses(stdout, response_tx))?;
        let tail = Arc::clone(&worker.stderr_tail);
        thread::Builder::new()
            .name("transcribe-worker-err".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                while let Ok(Some(line)) = read_log_line(&mut reader) {
                    forward_worker_log(&line);
                    let mut tail = lock(&tail);
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
                let _ = stderr_done_tx.send(());
            })?;

        debug!(
            "Started {}transcription worker (pid {})",
            if cpu_only { "CPU-only " } else { "" },
            pid
        );
        Ok(worker)
    }

    fn load(&mut self, spec: &LoadSpec) -> Result<LoadedInfo, Failure> {
        let request = Request::Load {
            path: spec.path.clone(),
            backend: spec.backend,
            device: spec.device.clone(),
        };
        match self.call(&request, None, LOAD_TIMEOUT)? {
            Response::Loaded(info) => {
                self.info = Some(info.clone());
                Ok(info)
            }
            other => Err(unexpected(&other)),
        }
    }

    fn call(
        &mut self,
        request: &Request,
        pcm: Option<&[f32]>,
        deadline: Duration,
    ) -> Result<Response, Failure> {
        if !self.control.send(encode_request(request, pcm)?) {
            return Err(self.died());
        }
        self.wait(request, deadline)
    }

    fn wait(&mut self, request: &Request, deadline: Duration) -> Result<Response, Failure> {
        match self.responses.recv_timeout(deadline) {
            Ok(Response::Error(message)) => Err(Failure::Remote(message)),
            Ok(Response::BackendError(message)) => Err(Failure::Backend(message)),
            Ok(response) => Ok(response),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.control.kill();
                let tail = self.native_tail();
                error!(
                    "Transcription worker (pid {}) did not answer {} within {:?}; killed it{}",
                    self.control.pid,
                    request_name(request),
                    deadline,
                    if tail.is_empty() {
                        String::new()
                    } else {
                        format!(". Native output before the hang:\n{}", tail.join("\n"))
                    }
                );
                Err(Failure::Hung(deadline))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.died()),
        }
    }

    fn died(&mut self) -> Failure {
        let _ = self.stderr_done.recv_timeout(Duration::from_secs(1));
        let status = {
            let mut child = lock(&self.control.child)
                .take()
                .expect("the worker owns its child");
            let status = match wait_exit(&mut child, Duration::from_secs(1)) {
                Some(status) => status.to_string(),
                None => {
                    let _ = child.kill();
                    "unresponsive after closing its output; killed".to_string()
                }
            };
            *lock(&self.control.child) = Some(child);
            status
        };
        let tail = self.native_tail();
        if tail.is_empty() {
            error!(
                "Transcription worker (pid {}) died: {}",
                self.control.pid, status
            );
        } else {
            error!(
                "Transcription worker (pid {}) died: {}. Native output before exit:\n{}",
                self.control.pid,
                status,
                tail.join("\n")
            );
        }
        Failure::Crashed(match tail.last() {
            Some(last) => format!("{status}: {last}"),
            None => status,
        })
    }

    fn native_tail(&self) -> Vec<String> {
        lock(&self.stderr_tail)
            .iter()
            .filter(|line| !line.starts_with(LOG_LINE_PREFIX))
            .cloned()
            .collect()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.control.close_stdin();
        let Some(mut child) = lock(&self.control.child).take() else {
            return;
        };
        if wait_exit(&mut child, SHUTDOWN_GRACE).is_none() {
            warn!(
                "Transcription worker (pid {}) did not exit within {:?}; killing it",
                self.control.pid, SHUTDOWN_GRACE
            );
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(test)]
        LIVE_WORKERS.fetch_sub(1, Ordering::SeqCst);
    }
}

fn wait_exit(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

#[cfg(test)]
static LIVE_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(all(unix, not(test)))]
static LAUNCHED_EXE: std::sync::OnceLock<Option<(PathBuf, (u64, u64))>> =
    std::sync::OnceLock::new();

#[cfg(all(unix, not(test)))]
fn file_id(path: &std::path::Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(all(unix, not(test)))]
fn record_launched_exe() {
    LAUNCHED_EXE.get_or_init(|| {
        std::env::current_exe()
            .and_then(|path| {
                let id = file_id(&path)?;
                Ok((path, id))
            })
            .inspect_err(|e| warn!("Could not identify Leo's executable: {}", e))
            .ok()
    });
}

#[cfg(not(test))]
fn worker_exe() -> io::Result<PathBuf> {
    #[cfg(unix)]
    if let Some((path, launched)) = LAUNCHED_EXE.get().and_then(Option::as_ref) {
        if file_id(path).ok().as_ref() != Some(launched) {
            return Err(io::Error::other(
                "Leo was updated or moved while it was running; restart Leo",
            ));
        }
    }
    #[cfg(target_os = "linux")]
    return Ok(PathBuf::from("/proc/self/exe"));
    #[cfg(not(target_os = "linux"))]
    std::env::current_exe()
}

fn encode_request(request: &Request, pcm: Option<&[f32]>) -> Result<Vec<u8>, Failure> {
    encode_message(request, pcm)
        .map_err(|e| Failure::Remote(format!("failed to encode request: {e}")))
}

fn write_frames(mut stdin: ChildStdin, frames: mpsc::Receiver<Vec<u8>>) {
    for frame in frames {
        if stdin
            .write_all(&frame)
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
}

fn read_responses(stdout: ChildStdout, responses: mpsc::SyncSender<Response>) {
    let mut stdout = BufReader::new(stdout);
    loop {
        match read_message::<Response>(&mut stdout) {
            Ok(Some((response, _))) => {
                if responses.send(response).is_err() {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                error!("Transcription worker protocol error: {}", e);
                break;
            }
        }
    }
}

fn request_name(request: &Request) -> &'static str {
    match request {
        Request::Hello { .. } => "hello",
        Request::Load { .. } => "model load",
        Request::Run { .. } => "transcription",
        Request::StreamBegin { .. } => "stream begin",
        Request::Feed => "stream feed",
        Request::Finalize { .. } => "stream finalize",
        Request::StreamReset => "stream reset",
        Request::Diarize { .. } => "speaker detection",
    }
}

fn read_log_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let read = reader
        .take(STDERR_LINE_BYTES as u64)
        .read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    ))
}

fn forward_worker_log(line: &str) {
    const TARGET: &str = "transcribe_worker";
    if let Some(rest) = line.strip_prefix(LOG_LINE_PREFIX) {
        let mut parts = rest.splitn(3, '\t');
        if let (Some(level), Some(target), Some(message)) =
            (parts.next(), parts.next(), parts.next())
        {
            let level = level.parse::<Level>().unwrap_or(Level::Info);
            log::log!(target: TARGET, level, "[{}] {}", target, message);
            return;
        }
    }
    log::info!(target: TARGET, "{}", line);
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn deadlines_scale_with_the_audio_above_a_floor() {
        let gpu = Deadlines::for_device(true);
        assert_eq!(gpu, GPU_DEADLINES);
        assert_eq!(gpu.audio(0), gpu.call_floor);
        assert_eq!(gpu.audio(16_000 / 2), gpu.call_floor);
        assert_eq!(gpu.audio(16_000 * 5), Duration::from_secs(50));
        assert_eq!(gpu.run(16_000), gpu.run_floor);
        assert_eq!(gpu.run(16_000 * 10), Duration::from_secs(100));
        let two_hours = 16_000 * 60 * 60 * 2;
        assert_eq!(gpu.diarize(two_hours), Duration::from_secs(60 + 1_800));
        assert_eq!(
            Deadlines::for_device(false).diarize(two_hours),
            Duration::from_secs(300 + 3_600)
        );

        let cpu = Deadlines::for_device(false);
        assert_eq!(cpu, CPU_DEADLINES);
        assert_eq!(cpu.audio(16_000), cpu.call_floor);
        assert_eq!(cpu.audio(16_000 * 10), Duration::from_secs(200));
        assert_eq!(cpu.run(16_000 * 5), cpu.run_floor);
        assert_eq!(cpu.run(16_000 * 60), Duration::from_secs(1200));
        for secs in [0, 1, 5, 30, 120, 600] {
            assert!(cpu.audio(secs * 16_000) >= gpu.audio(secs * 16_000));
            assert!(cpu.run(secs * 16_000) >= gpu.run(secs * 16_000));
        }

        assert_eq!(ms_to_samples(1500), 24_000);
        assert_eq!(ms_to_samples(-5), 0);
    }
}

#[cfg(all(test, unix))]
pub(super) mod fake_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    pub(crate) static ENV: Mutex<()> = Mutex::new(());

    pub(crate) fn fake() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("worker");
        std::fs::write(
            &script,
            format!(
                "#!/usr/bin/env python3\n{}",
                include_str!("fixtures/worker.py")
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var("LEO_TRANSCRIBE_WORKER_EXE", script);
        dir
    }

    #[test]
    fn crashed_and_malformed_workers_retry_on_cpu_and_are_reaped() {
        let _guard = ENV.lock().unwrap();
        for (mode, workers) in [("crashed", 2), ("garbage", 2), ("backend", 3)] {
            let dir = fake();
            let engine = EngineSupervisor::new(true);
            engine
                .load(LoadSpec {
                    path: dir.path().join(mode),
                    backend: Backend::Auto,
                    device: DeviceSelector::Auto,
                })
                .unwrap();
            let answer = engine
                .transcribe(vec![0.2; 1600], RunOptions::default())
                .unwrap();
            assert_eq!(answer.text, "local words");
            assert!(!engine.loaded().unwrap().on_gpu);
            engine.unload().wait();
            let pids = std::fs::read_to_string(dir.path().join("pids")).unwrap();
            assert_eq!(pids.lines().count(), workers);
            for pid in pids.lines().map(|p| p.parse::<i32>().unwrap()) {
                assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
            }
        }
        std::env::remove_var("LEO_TRANSCRIBE_WORKER_EXE");
    }

    #[test]
    fn cancellation_kills_and_reaps_a_hung_worker() {
        let _guard = ENV.lock().unwrap();
        let dir = fake();
        let engine = EngineSupervisor::new(false);
        engine
            .load(LoadSpec {
                path: dir.path().join("hung"),
                backend: Backend::Cpu,
                device: DeviceSelector::Auto,
            })
            .unwrap();
        let other = engine.clone();
        let work =
            std::thread::spawn(move || other.transcribe(vec![0.2; 1600], RunOptions::default()));
        let started = Instant::now();
        while !dir.path().join("running").exists() {
            assert!(started.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(5));
        }
        let stopped = Instant::now();
        engine.cancel();
        assert!(matches!(work.join().unwrap(), Err(EngineError::Cancelled)));
        assert!(engine.loaded().is_none());
        engine.unload().wait();
        assert!(stopped.elapsed() < Duration::from_secs(3));
        for pid in std::fs::read_to_string(dir.path().join("pids"))
            .unwrap()
            .lines()
            .map(|p| p.parse::<i32>().unwrap())
        {
            assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
        }
        std::env::remove_var("LEO_TRANSCRIBE_WORKER_EXE");
    }

    #[test]
    fn a_rejected_stream_feed_leaves_the_whole_audio_to_batch_transcription() {
        let _guard = ENV.lock().unwrap();
        let dir = fake();
        let engine = EngineSupervisor::new(false);
        for (mode, finalized) in [("streaming", true), ("feedfail", false)] {
            engine
                .load(LoadSpec {
                    path: dir.path().join(mode),
                    backend: Backend::Cpu,
                    device: DeviceSelector::Auto,
                })
                .unwrap();
            let stream = engine
                .start_stream(RunOptions::default(), StreamOptions::default(), |_| {})
                .unwrap();
            assert!(matches!(stream.feed(vec![0.2; 1600]), Fed::Sent));
            assert!(matches!(stream.feed(vec![0.2; 1600]), Fed::Sent));
            assert_eq!(stream.finalize(false).unwrap().is_some(), finalized);
            engine.unload().wait();
        }
        std::env::remove_var("LEO_TRANSCRIBE_WORKER_EXE");
    }

    #[test]
    fn stderr_without_a_newline_is_read_in_bounded_pieces() {
        let bytes = vec![b'x'; 1_000_000];
        let mut reader = std::io::Cursor::new(bytes);
        let mut total = 0;
        while let Some(line) = read_log_line(&mut reader).unwrap() {
            assert!(line.len() <= STDERR_LINE_BYTES);
            total += line.len();
        }
        assert_eq!(total, 1_000_000);
    }
}
