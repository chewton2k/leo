use super::protocol::{
    read_message, write_message, DeviceInfo, DeviceSelector, LoadedInfo, Request, Response,
};
use super::{CPU_ONLY_FLAG, LOG_LEVEL_ENV};
use log::{debug, error, warn, LevelFilter, Log, Metadata, Record};
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use transcribe_cpp::{
    Backend, BackendMask, DeviceType, ErrorKind, Feature, Model, ModelOptions, Session, Stream,
};

pub(super) const LOG_LINE_PREFIX: &str = "\u{1}";

struct Incoming {
    request: Request,
    pcm: Vec<f32>,
}

pub fn run() -> i32 {
    #[cfg(unix)]
    ignore_app_signals();
    #[cfg(target_os = "linux")]
    set_process_name();
    let protocol_out = match take_stdout_for_protocol() {
        Ok(file) => file,
        Err(e) => {
            eprintln!("transcribe worker: failed to claim stdout: {e}");
            return 2;
        }
    };
    init_logger();

    let (requests_tx, requests) = mpsc::sync_channel(2);
    if let Err(e) = thread::Builder::new()
        .name("transcribe-worker-in".into())
        .spawn(move || read_requests(requests_tx))
    {
        error!("Failed to start the request reader: {}", e);
        return 2;
    }

    transcribe_cpp::init_logging();
    let cpu_only = std::env::args_os().any(|arg| arg == CPU_ONLY_FLAG);
    inject_fault("init", !cpu_only);
    let init = if cpu_only {
        transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
    } else {
        transcribe_cpp::init_backends_default()
    };
    let init_error = init
        .err()
        .map(|e| format!("Failed to initialize transcribe.cpp backends: {e}"));
    debug!(
        "transcribe.cpp allowed backends: {:#x}",
        transcribe_cpp::allowed_backends().bits()
    );

    let mut output = protocol_out;
    let mut session: Option<(Session, LoadedInfo)> = None;

    while let Ok(Incoming { request, pcm }) = requests.recv() {
        let finished_run = matches!(request, Request::Run { .. });
        let response = match request {
            Request::Hello { list_devices } => match &init_error {
                Some(message) => Response::Error(message.clone()),
                None => Response::Hello {
                    devices: list_devices.then(|| {
                        inject_fault("list", !cpu_only);
                        transcribe_cpp::devices()
                            .iter()
                            .map(DeviceInfo::from_device)
                            .collect()
                    }),
                },
            },
            Request::Load {
                path,
                backend,
                device,
            } => {
                session = None;
                match load(&path, backend, device) {
                    Ok(loaded) => {
                        let info = loaded.1.clone();
                        session = Some(loaded);
                        Response::Loaded(info)
                    }
                    Err(e) => Response::Error(e),
                }
            }
            Request::Run { options } => match session.as_mut() {
                Some((session, info)) => {
                    inject_fault("run", info.on_gpu);
                    match session.run(&pcm, &options) {
                        Ok(transcript) => Response::Transcript(transcript),
                        Err(e) => run_error(e),
                    }
                }
                None => not_loaded(),
            },
            Request::StreamBegin { run, stream } => match session.as_mut() {
                Some((session, info)) => match session.stream(&run, &stream) {
                    Ok(stream) => {
                        if write_message(&mut output, &Response::Ok, None).is_err() {
                            return 1;
                        }
                        let on_gpu = info.on_gpu;
                        if let Err(e) = serve_stream(stream, &requests, &mut output, on_gpu) {
                            error!("Protocol write failed during stream: {}", e);
                            return 1;
                        }
                        continue;
                    }
                    Err(e) => run_error(e),
                },
                None => not_loaded(),
            },
            Request::Feed | Request::Finalize { .. } | Request::StreamReset => {
                Response::Error("no active stream".to_string())
            }
        };
        if let Err(e) = write_message(&mut output, &response, None) {
            error!("Protocol write failed: {}", e);
            return 1;
        }
        if finished_run {
            drop(pcm);
        }
    }
    0
}

fn read_requests(requests: mpsc::SyncSender<Incoming>) -> ! {
    let mut input = BufReader::new(io::stdin().lock());
    loop {
        match read_message::<Request>(&mut input) {
            Ok(Some((request, pcm))) => {
                if requests.send(Incoming { request, pcm }).is_err() {
                    exit_now(1);
                }
            }
            Ok(None) => exit_now(0),
            Err(e) => {
                error!("Protocol read failed: {}", e);
                exit_now(1);
            }
        }
    }
}

fn exit_now(code: i32) -> ! {
    let _ = io::stderr().flush();
    unsafe { libc::_exit(code) }
}

fn serve_stream(
    mut stream: Stream<'_>,
    requests: &mpsc::Receiver<Incoming>,
    output: &mut impl Write,
    on_gpu: bool,
) -> io::Result<()> {
    while let Ok(Incoming { request, pcm }) = requests.recv() {
        let (response, done) = match request {
            Request::Feed => {
                inject_fault("feed", on_gpu);
                match stream.feed(&pcm) {
                    Ok(update) => {
                        let text = (update.committed_changed || update.tentative_changed)
                            .then(|| stream.text());
                        (Response::Fed { update, text }, false)
                    }
                    Err(e) => (run_error(e), false),
                }
            }
            Request::Finalize { want_language } => {
                inject_fault("finalize", on_gpu);
                match stream.finalize() {
                    Ok(update) => {
                        let language = if want_language {
                            stream.snapshot().language
                        } else {
                            None
                        };
                        let text = stream.text();
                        (
                            Response::Finalized {
                                update,
                                text,
                                language,
                            },
                            true,
                        )
                    }
                    Err(e) => (run_error(e), true),
                }
            }
            Request::StreamReset => {
                stream.reset();
                (Response::Ok, true)
            }
            _ => (
                Response::Error("a stream is active; finalize or reset it first".to_string()),
                false,
            ),
        };
        write_message(output, &response, None)?;
        if done {
            break;
        }
    }
    Ok(())
}

fn not_loaded() -> Response {
    Response::Error("no model loaded".to_string())
}

fn run_error(e: transcribe_cpp::Error) -> Response {
    if e.kind() == ErrorKind::Backend {
        Response::BackendError(e.to_string())
    } else {
        Response::Error(e.to_string())
    }
}

fn load(
    path: &Path,
    backend: Backend,
    selector: DeviceSelector,
) -> Result<(Session, LoadedInfo), String> {
    let device = match selector {
        DeviceSelector::Auto => None,
        DeviceSelector::Key(key) => {
            let found = transcribe_cpp::devices().into_iter().find(|d| {
                let info = DeviceInfo::from_device(d);
                info.is_gpu() && info.key == key
            });
            if found.is_none() {
                warn!(
                    "Stored transcribe GPU device '{}' is no longer available; using automatic device selection",
                    key
                );
            }
            found
        }
        DeviceSelector::Index(index) => {
            let device = transcribe_cpp::devices()
                .into_iter()
                .find(|d| d.index == Some(index))
                .ok_or_else(|| {
                    format!("No compute device with index {index} (see --list-devices)")
                })?;
            if matches!(device.device_type, DeviceType::Accel | DeviceType::Unknown) {
                return Err(format!(
                    "Device index {index} ({}) cannot host a model",
                    device.kind
                ));
            }
            Some(device)
        }
    };

    inject_fault("load", backend != Backend::Cpu);
    let model = Model::load_with(
        path,
        &ModelOptions {
            backend,
            device,
            no_mmap: false,
        },
    )
    .map_err(|e| format!("Failed to load model: {e}"))?;
    let session = model
        .session()
        .map_err(|e| format!("Failed to create session: {e}"))?;
    let bound = model.device().ok().map(|d| DeviceInfo::from_device(&d));
    let backend_name = model.backend();
    let info = LoadedInfo {
        arch: model.arch(),
        variant: model.variant(),
        on_gpu: match &bound {
            Some(device) => device.is_gpu(),
            None => !backend_name.is_empty() && !backend_name.eq_ignore_ascii_case("cpu"),
        },
        backend: backend_name,
        device: bound
            .as_ref()
            .map(|d| d.label().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        capabilities: model
            .capabilities()
            .map_err(|e| format!("Failed to read model capabilities: {e}"))?,
        supports_initial_prompt: model.supports(Feature::InitialPrompt),
        supports_long_form: model.supports(Feature::LongForm),
        supports_vocabulary: model.supports(Feature::Vocabulary),
    };
    Ok((session, info))
}

#[cfg(debug_assertions)]
fn inject_fault(stage: &str, on_gpu: bool) {
    let Ok(spec) = std::env::var("LEO_WORKER_FAULT") else {
        return;
    };
    let Some((kind, at)) = spec.split_once('@') else {
        return;
    };
    if at != stage {
        return;
    }
    if !on_gpu && std::env::var_os("LEO_WORKER_FAULT_GPU_ONLY").is_some() {
        return;
    }
    if let Some(marker) = std::env::var_os("LEO_WORKER_FAULT_ONCE") {
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .is_err()
        {
            return;
        }
    }
    eprintln!("injected fault: {kind} at {stage}");
    match kind {
        "abort" => std::process::abort(),
        "segv" => unsafe {
            libc::signal(libc::SIGSEGV, libc::SIG_DFL);
            libc::raise(libc::SIGSEGV);
            std::process::abort();
        },
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
        _ => {}
    }
}

#[cfg(not(debug_assertions))]
fn inject_fault(_stage: &str, _on_gpu: bool) {}

struct StderrLogger;

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut stderr = io::stderr().lock();
        for line in message.lines() {
            let _ = writeln!(
                stderr,
                "{LOG_LINE_PREFIX}{}\t{}\t{}",
                record.level(),
                record.target(),
                line
            );
        }
    }

    fn flush(&self) {
        let _ = io::stderr().flush();
    }
}

fn init_logger() {
    static LOGGER: StderrLogger = StderrLogger;
    let level = std::env::var(LOG_LEVEL_ENV)
        .ok()
        .and_then(|v| v.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::Info);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(unix)]
fn ignore_app_signals() {
    unsafe {
        libc::signal(libc::SIGUSR1, libc::SIG_IGN);
        libc::signal(libc::SIGUSR2, libc::SIG_IGN);
    }
}

#[cfg(target_os = "linux")]
fn set_process_name() {
    unsafe {
        libc::prctl(libc::PR_SET_NAME, c"Leo-worker".as_ptr());
    }
}

#[cfg(unix)]
fn take_stdout_for_protocol() -> io::Result<File> {
    use std::os::fd::FromRawFd;
    unsafe {
        let fd = libc::dup(libc::STDOUT_FILENO);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from_raw_fd(fd))
    }
}

#[cfg(windows)]
fn take_stdout_for_protocol() -> io::Result<File> {
    use std::os::windows::io::BorrowedHandle;
    unsafe {
        let fd = libc::dup(1);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::dup2(2, 1) < 0 {
            return Err(io::Error::last_os_error());
        }
        let handle = libc::get_osfhandle(fd);
        if handle == -1 {
            return Err(io::Error::last_os_error());
        }
        let owned = BorrowedHandle::borrow_raw(handle as _).try_clone_to_owned()?;
        libc::close(fd);
        Ok(File::from(owned))
    }
}
