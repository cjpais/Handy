//! The worker process: the only place transcribe.cpp native code runs. Owns
//! the model, session and stream, and serves [`Request`]s until the parent
//! closes its stdin.
//!
//! A dedicated thread reads stdin so the parent's writes never block (R2),
//! [`Request::Cancel`] takes effect while the main thread is busy in native
//! code, and the worker exits the moment the parent goes away, even if hung
//! (R3).

use super::protocol::{
    read_message, write_message, DeviceInfo, DeviceSelector, LoadedInfo, Request, Response,
};
use super::{CPU_ONLY_FLAG, LOG_LEVEL_ENV};
use log::{debug, error, warn, LevelFilter, Log, Metadata, Record};
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use transcribe_cpp::{
    Backend, BackendMask, CancelToken, DeviceType, Feature, Model, ModelOptions, Session, Stream,
};

/// Prefix on worker log lines so the parent can tell them apart from raw
/// native output (e.g. a `GGML_ASSERT` message right before an abort).
pub(super) const LOG_LINE_PREFIX: &str = "\u{1}";

/// A request as handed from the stdin reader to the main loop. `seq` numbers
/// requests (from 1) so a [`Request::Cancel`] can name the one it targets.
struct Incoming {
    seq: u64,
    request: Request,
    pcm: Vec<f32>,
}

/// Shared between the stdin reader and the main loop. A cancel only aborts
/// the request it was sent for: one arriving after that request finished
/// must not abort the next.
#[derive(Default)]
struct Canceller {
    token: CancelToken,
    /// `(running, cancelled)`: the request in progress (0 when idle) and the
    /// latest request a cancel targeted.
    state: Mutex<(u64, u64)>,
}

impl Canceller {
    /// Called by the reader: cancel request `seq` now if it is running, or
    /// as soon as it starts.
    fn cancel(&self, seq: u64) {
        if seq == 0 {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.1 = seq;
        if state.0 == seq {
            self.token.cancel();
        }
    }

    fn begin(&self, seq: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.0 = seq;
        self.token.reset();
        if state.1 >= seq {
            self.token.cancel();
        }
    }

    fn end(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.0 = 0;
        self.token.reset();
    }
}

pub fn run() -> i32 {
    // Take the real stdout for the protocol before any native code runs, and
    // point fd 1 at stderr: ggml writes to stdout in places, and any stray
    // byte there would corrupt the protocol stream.
    let protocol_out = match take_stdout_for_protocol() {
        Ok(file) => file,
        Err(e) => {
            eprintln!("transcribe worker: failed to claim stdout: {e}");
            return 2;
        }
    };
    init_logger();

    let canceller = Arc::new(Canceller::default());
    let (requests_tx, requests) = mpsc::channel();
    {
        let canceller = Arc::clone(&canceller);
        if let Err(e) = thread::Builder::new()
            .name("transcribe-worker-in".into())
            .spawn(move || read_requests(requests_tx, &canceller))
        {
            error!("Failed to start the request reader: {}", e);
            return 2;
        }
    }

    transcribe_cpp::init_logging();
    // Registering a GPU backend runs its driver code, so this is where a
    // broken driver crashes or hangs, before the hello is answered.
    let cpu_only = std::env::args_os().any(|arg| arg == CPU_ONLY_FLAG);
    inject_fault("init", !cpu_only);
    let init = if cpu_only {
        transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
    } else {
        transcribe_cpp::init_backends_default()
    };
    // Init only fails when no compute device registered at all, which no
    // other worker could fix either: the hello reports it.
    let init_error = init
        .err()
        .map(|e| format!("Failed to initialize transcribe.cpp backends: {e}"));
    debug!(
        "transcribe.cpp allowed backends: {:#x}",
        transcribe_cpp::allowed_backends().bits()
    );

    let mut output = protocol_out;
    let mut session: Option<(Session, LoadedInfo)> = None;

    while let Ok(Incoming { seq, request, pcm }) = requests.recv() {
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
                // Free any previous model first to avoid holding two at once.
                session = None;
                match load(&path, backend, device, &canceller.token) {
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
                    canceller.begin(seq);
                    inject_fault("run", info.on_gpu);
                    let result = session.run(&pcm, &options);
                    canceller.end();
                    match result {
                        Ok(transcript) => Response::Transcript(transcript),
                        Err(e) => Response::Error(e.to_string()),
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
                        if let Err(e) =
                            serve_stream(stream, &requests, &mut output, &canceller, on_gpu)
                        {
                            error!("Protocol write failed during stream: {}", e);
                            return 1;
                        }
                        continue;
                    }
                    Err(e) => Response::Error(e.to_string()),
                },
                None => not_loaded(),
            },
            Request::Feed | Request::Finalize { .. } | Request::StreamReset => {
                Response::Error("no active stream".to_string())
            }
            // Handled by the reader; never forwarded.
            Request::Cancel => continue,
        };
        if let Err(e) = write_message(&mut output, &response, None) {
            error!("Protocol write failed: {}", e);
            return 1;
        }
    }
    // Unreachable in practice: the reader exits the process on EOF.
    0
}

/// The stdin reader. Forwards requests to the main loop, applies cancels
/// immediately, and ends the process when the parent closes stdin (unload,
/// quit, or the parent died). `_exit` skips C++ static destructors, so a
/// model still alive at that point can't trip ggml-metal's teardown asserts,
/// and the OS reclaims its CPU and GPU memory.
fn read_requests(requests: mpsc::Sender<Incoming>, canceller: &Canceller) -> ! {
    let mut input = BufReader::new(io::stdin().lock());
    let mut seq = 0;
    loop {
        match read_message::<Request>(&mut input) {
            Ok(Some((Request::Cancel, _))) => canceller.cancel(seq),
            Ok(Some((request, pcm))) => {
                seq += 1;
                if requests.send(Incoming { seq, request, pcm }).is_err() {
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
    // SAFETY: `_exit` only ends the process; nothing runs afterwards.
    unsafe { libc::_exit(code) }
}

/// Serve requests against an active stream until it is finalized or reset.
fn serve_stream(
    mut stream: Stream<'_>,
    requests: &mpsc::Receiver<Incoming>,
    output: &mut impl Write,
    canceller: &Canceller,
    on_gpu: bool,
) -> io::Result<()> {
    while let Ok(Incoming { seq, request, pcm }) = requests.recv() {
        let (response, done) = match request {
            Request::Feed => {
                canceller.begin(seq);
                inject_fault("feed", on_gpu);
                let result = stream.feed(&pcm);
                canceller.end();
                match result {
                    Ok(update) => {
                        let text = (update.committed_changed || update.tentative_changed)
                            .then(|| stream.text());
                        (Response::Fed { update, text }, false)
                    }
                    Err(e) => (Response::Error(e.to_string()), false),
                }
            }
            Request::Finalize { want_language } => {
                canceller.begin(seq);
                inject_fault("finalize", on_gpu);
                let result = stream.finalize();
                canceller.end();
                match result {
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
                    // Finalize ends the stream even when it fails.
                    Err(e) => (Response::Error(e.to_string()), true),
                }
            }
            Request::StreamReset => {
                stream.reset();
                (Response::Ok, true)
            }
            Request::Cancel => continue,
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

fn load(
    path: &Path,
    backend: Backend,
    selector: DeviceSelector,
    cancel: &CancelToken,
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
    let model = Model::load_with(path, &ModelOptions { backend, device })
        .map_err(|e| format!("Failed to load model: {e}"))?;
    let mut session = model
        .session()
        .map_err(|e| format!("Failed to create session: {e}"))?;
    let supports_cancellation = model.supports(Feature::Cancellation);
    if supports_cancellation {
        // Installed once, while nothing is in flight; the canceller arms and
        // resets the shared flag per request.
        session.set_cancel_token(cancel);
    }
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
        capabilities: model.capabilities(),
        supports_initial_prompt: model.supports(Feature::InitialPrompt),
        supports_cancellation,
    };
    Ok((session, info))
}

/// Debug-build fault injection for exercising crash/hang recovery:
/// `HANDY_WORKER_FAULT=<abort|segv|hang|slow>@<init|list|load|run|feed|finalize>`
/// (`slow` adds 100 ms to every call at that stage; `init` is backend
/// registration, before the hello). With `HANDY_WORKER_FAULT_ONCE=<marker
/// path>` it fires only in the first worker to reach that stage (the marker
/// file records that it fired). With `HANDY_WORKER_FAULT_GPU_ONLY=1` it fires
/// only on a GPU (backend init and device listing count as GPU work unless
/// the worker is CPU-only), so recovery on CPU can succeed.
#[cfg(debug_assertions)]
fn inject_fault(stage: &str, on_gpu: bool) {
    let Ok(spec) = std::env::var("HANDY_WORKER_FAULT") else {
        return;
    };
    let Some((kind, at)) = spec.split_once('@') else {
        return;
    };
    if at != stage {
        return;
    }
    if !on_gpu && std::env::var_os("HANDY_WORKER_FAULT_GPU_ONLY").is_some() {
        return;
    }
    if let Some(marker) = std::env::var_os("HANDY_WORKER_FAULT_ONCE") {
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
        // SAFETY: deliberately crash the worker with a real SIGSEGV. Rust's
        // stack-overflow handler would swallow a raised signal, so restore
        // the default disposition first.
        "segv" => unsafe {
            libc::signal(libc::SIGSEGV, libc::SIG_DFL);
            libc::raise(libc::SIGSEGV);
        },
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
        "slow" => std::thread::sleep(std::time::Duration::from_millis(100)),
        _ => {}
    }
}

#[cfg(not(debug_assertions))]
fn inject_fault(_stage: &str, _on_gpu: bool) {}

/// Logs go to stderr as `\x01LEVEL\ttarget\tmessage` lines; the parent
/// re-logs them at the same level.
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
fn take_stdout_for_protocol() -> io::Result<File> {
    use std::os::fd::FromRawFd;
    // SAFETY: plain fd duplication; the new fd is owned by the returned File.
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
    use std::os::windows::io::FromRawHandle;
    // Native code writes through the CRT's fd 1, so redirect that one. The
    // protocol uses the OS handle behind a CRT duplicate of the original.
    // SAFETY: CRT fd duplication; the duplicate's handle is owned by the File
    // for the rest of the process.
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
        Ok(File::from_raw_handle(handle as _))
    }
}
