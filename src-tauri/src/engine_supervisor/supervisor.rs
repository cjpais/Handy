//! Parent side: [`EngineSupervisor`], the single owner of the transcribe.cpp
//! worker process.
//!
//! One owner thread works through a command queue and holds the only worker
//! slot, the loaded model's spec and any active stream, so "at most one
//! model worker alive" holds by construction. Callers' commands wait their
//! turn; waiting in the queue (e.g. behind a model load) is never a timeout.
//! Only [`EngineSupervisor::cancel`] and device probes bypass the queue: a
//! probe runs its own short-lived, model-less worker on the caller's thread,
//! so a hung GPU driver can't hold up dictation behind it.
//!
//! A worker that will only use the CPU is started CPU-only, so it never
//! registers a GPU backend. One rule decides: the spec it loads asks for
//! [`Backend::Cpu`] (the user's choice, or a fallback after a GPU failure),
//! or the host must not use the GPU at all.

use super::protocol::{
    encode_message, read_message, DeviceInfo, DeviceSelector, LoadedInfo, Request, Response,
};
use super::worker::LOG_LINE_PREFIX;
use super::{CPU_ONLY_FLAG, LOG_LEVEL_ENV, WORKER_FLAG};
use crate::degraded_state::Degraded;
use log::{debug, error, info, warn, Level};
use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use transcribe_cpp::{Backend, RunOptions, StreamOptions, StreamText, StreamUpdate, Transcript};

/// Spawn, backend init and device listing. Listing first opens the GPU,
/// which on macOS compiles ggml's Metal library when the system shader cache
/// does not hold it yet (~15 s after an install or update).
const HELLO_TIMEOUT: Duration = Duration::from_secs(60);
/// Model loads stay generous: a slow load is slow, not hung (#1841).
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Floor for every other call, so sub-second clips and first-use shader
/// compiles are not mistaken for hangs.
const CALL_FLOOR: Duration = Duration::from_secs(10);
/// Floor for a batch run. A run has a fixed cost however short the clip:
/// Whisper always encodes a full 30 s window, which on CPU takes seconds
/// even for a 1 s clip.
const RUN_FLOOR: Duration = Duration::from_secs(60);
/// A live stream this far behind real time can't keep up: it is dropped and
/// the dictation is transcribed in one batch run instead.
const MAX_STREAM_BACKLOG: Duration = Duration::from_secs(5);
/// Work on N seconds of audio may take up to this many times N. Past that,
/// something is wrong anyway.
const AUDIO_DEADLINE_FACTOR: f64 = 10.0;
/// How long a worker gets to exit after its stdin closes before it's killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// How long a polite cancel gets before the worker is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(2);
/// Native stderr lines kept for crash reports (a `GGML_ASSERT` message lands
/// here right before an abort).
const STDERR_TAIL_LINES: usize = 64;
const SAMPLE_RATE: f64 = 16_000.0;

/// Deadline for work on `samples` of 16 kHz audio.
fn audio_deadline(samples: usize) -> Duration {
    CALL_FLOOR.max(Duration::from_secs_f64(
        samples as f64 / SAMPLE_RATE * AUDIO_DEADLINE_FACTOR,
    ))
}

/// Deadline for a batch run on `samples` of 16 kHz audio.
fn run_deadline(samples: usize) -> Duration {
    RUN_FLOOR.max(audio_deadline(samples))
}

fn ms_to_samples(ms: i64) -> usize {
    (ms.max(0) as usize) * (SAMPLE_RATE as usize / 1000)
}

/// What to load. The supervisor keeps it so a dead worker can be replaced
/// transparently.
#[derive(Debug, Clone)]
pub struct LoadSpec {
    pub path: PathBuf,
    pub backend: Backend,
    pub device: DeviceSelector,
}

impl LoadSpec {
    /// An explicitly chosen device (`--device-index`) never falls back to
    /// CPU: its failures are reported, not hidden.
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

/// The compute devices transcribe.cpp registers, as last reported by a
/// worker.
#[derive(Debug, Clone, Default)]
pub enum DeviceList {
    /// No worker has listed devices yet (or none could be started).
    #[default]
    Unknown,
    /// A worker crashed, hung or failed while initializing backends or
    /// listing. Models load on CPU, and devices are not listed again, until
    /// [`EngineSupervisor::retry_gpu`].
    Failed(String),
    Known(Vec<DeviceInfo>),
}

#[derive(Debug)]
pub enum EngineError {
    /// [`EngineSupervisor::cancel`] stopped the work.
    Cancelled,
    /// No model, a transcribe.cpp error, or a worker failure that could not
    /// be recovered.
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

/// The result of one stream feed, handed to the stream's callback.
pub struct StreamProgress {
    pub update: StreamUpdate,
    /// Present only when the committed or tentative text changed.
    pub text: Option<StreamText>,
    /// Time from sending the frame to the worker until its answer.
    pub elapsed: Duration,
}

pub struct Finalized {
    pub update: StreamUpdate,
    pub text: StreamText,
    /// The model's detected language, when asked for.
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
        id: u64,
        epoch: u64,
        run: RunOptions,
        stream: StreamOptions,
        queue: Arc<StreamQueue>,
        on_progress: OnProgress,
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
}

/// State readable without going through the queue.
#[derive(Default)]
struct Shared {
    /// Whether this host may use a GPU at all. When not, every worker is
    /// CPU-only, device probes included.
    gpu_allowed: bool,
    /// The loaded model (logically: it stays loaded while its worker is
    /// replaced after a crash or cancel).
    loaded: Mutex<Option<LoadedInfo>>,
    devices: Mutex<DeviceList>,
    /// Held for the length of a device probe, so concurrent callers wait for
    /// the one probe instead of starting their own.
    probing: Mutex<()>,
    /// Bumped by every [`EngineSupervisor::cancel`]. Work queued before the
    /// bump counts as cancelled.
    cancel_epoch: AtomicU64,
    /// The cancellable call the owner is waiting on, if any.
    in_flight: Mutex<Option<InFlight>>,
    /// Bumped by every [`EngineSupervisor::unload`] as it is called, so the
    /// model reads as unloaded at once, even while the unload waits its turn.
    unloads_requested: AtomicU64,
    /// Bumped by every [`EngineSupervisor::retry_gpu`]; GPU failures from
    /// before the bump no longer count.
    gpu_retries: AtomicU64,
    next_id: AtomicU64,
}

impl Shared {
    /// Start a model-less worker just to list devices.
    fn probe_devices(&self) {
        match self.hello(!self.gpu_allowed, true) {
            Ok(worker) => drop(worker),
            Err(failure) => error!(
                "Could not start a transcription worker to list compute devices: {}",
                failure
            ),
        }
    }

    /// Start a worker and say hello, publishing its device list if it lists
    /// one. If listing was asked for, a worker that fails backend init or
    /// listing (crash, hang or init error) marks the list failed. A worker
    /// that could not be launched at all may be a passing problem, so that
    /// is not recorded.
    fn hello(&self, cpu_only: bool, list_devices: bool) -> Result<Worker, Failure> {
        let mut worker = Worker::spawn(cpu_only).map_err(Failure::Spawn)?;
        match worker.call(&Request::Hello { list_devices }, None, HELLO_TIMEOUT) {
            Ok(Response::Hello { devices }) => {
                if let Some(devices) = devices {
                    self.set_devices(DeviceList::Known(devices));
                }
                Ok(worker)
            }
            Ok(other) => Err(unexpected(&other)),
            Err(failure) => {
                if list_devices && !matches!(failure, Failure::Spawn(_)) {
                    self.set_devices(DeviceList::Failed(failure.to_string()));
                }
                Err(failure)
            }
        }
    }

    fn set_devices(&self, devices: DeviceList) {
        let mut current = lock(&self.devices);
        if let (DeviceList::Known(old), DeviceList::Known(new)) = (&*current, &devices) {
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
        *current = devices;
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

/// Shared between a stream's handle and the owner.
#[derive(Default)]
struct StreamQueue {
    /// Set when the caller drops its handle.
    closed: AtomicBool,
    /// Samples fed but not yet taken up by the owner: how far the stream is
    /// behind real time.
    backlog: AtomicUsize,
}

struct InFlight {
    id: u64,
    what: &'static str,
    control: Arc<Control>,
    cancellable: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Owns the one transcribe.cpp worker slot. Cheap to clone.
#[derive(Clone)]
pub struct EngineSupervisor {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}

impl EngineSupervisor {
    /// Start the owner thread. No worker runs until something needs one.
    /// Without `gpu_allowed`, every worker is CPU-only, so no GPU driver is
    /// ever loaded.
    pub fn new(gpu_allowed: bool) -> Self {
        let (commands, queue) = mpsc::channel();
        let shared = Arc::new(Shared {
            gpu_allowed,
            ..Shared::default()
        });
        let owner = Owner {
            shared: Arc::clone(&shared),
            worker: None,
            spec: None,
            stream: None,
            gpu_suspect: None,
            unloads_done: 0,
        };
        thread::Builder::new()
            .name("transcribe-engine".into())
            .spawn(move || owner.run(queue))
            .expect("failed to start the transcription engine thread");
        Self { commands, shared }
    }

    fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Option<T> {
        let (reply, answer) = mpsc::channel();
        self.commands.send(command(reply)).ok()?;
        answer.recv().ok()
    }

    /// Load a model, replacing the current one. The old worker has fully
    /// exited before the new one starts, so two models are never held at
    /// once.
    pub fn load(&self, spec: LoadSpec) -> Result<LoadedInfo, EngineError> {
        self.ask(|reply| Command::Load(spec, reply))
            .unwrap_or_else(|| Err(engine_stopped()))
    }

    /// Unload the model, without waiting. It reads as unloaded at once and
    /// the unload is queued ahead of anything requested later, so a new
    /// dictation queues a fresh load behind it and is never affected by it.
    /// [`Unloading::wait`] returns once the worker has exited.
    pub fn unload(&self) -> Unloading {
        let (reply, exited) = mpsc::channel();
        {
            let mut loaded = lock(&self.shared.loaded);
            self.shared.unloads_requested.fetch_add(1, Ordering::SeqCst);
            *loaded = None;
            if self.commands.send(Command::Unload(reply)).is_err() {
                return Unloading(None);
            }
        }
        Unloading(Some(exited))
    }

    /// An explicit transcribe.cpp accelerator or GPU device change: forget
    /// every GPU failure so far, so the next load may use (and list) the GPU
    /// again. A new failure raises its flag again.
    pub fn retry_gpu(&self, reason: &str) {
        self.shared.gpu_retries.fetch_add(1, Ordering::SeqCst);
        Degraded::GpuUnavailable.clear(reason);
        let mut devices = lock(&self.shared.devices);
        if matches!(*devices, DeviceList::Failed(_)) {
            *devices = DeviceList::Unknown;
        }
    }

    /// The loaded model. A snapshot; never waits on the queue.
    pub fn loaded(&self) -> Option<LoadedInfo> {
        lock(&self.shared.loaded).clone()
    }

    /// Transcribe 16 kHz mono PCM with the loaded model. A worker crash or
    /// hang is retried once in a fresh worker (on CPU if the GPU may be at
    /// fault).
    pub fn transcribe(&self, pcm: Vec<f32>, run: RunOptions) -> Result<Transcript, EngineError> {
        let epoch = self.shared.epoch();
        self.ask(|reply| Command::Transcribe {
            pcm,
            run,
            epoch,
            reply,
        })
        .unwrap_or_else(|| Err(engine_stopped()))
    }

    /// Begin a live stream on the loaded model. `on_progress` runs on the
    /// owner thread after every feed, so it must not block or call back into
    /// the supervisor.
    pub fn start_stream(
        &self,
        run: RunOptions,
        stream: StreamOptions,
        on_progress: impl FnMut(StreamProgress) + Send + 'static,
    ) -> Result<StreamHandle, EngineError> {
        let id = self.shared.next_id();
        // The stream is one piece of cancellable work: a cancel from here on
        // stops its feeds and its finalize.
        let epoch = self.shared.epoch();
        let queue = Arc::new(StreamQueue::default());
        self.ask(|reply| Command::StreamBegin {
            id,
            epoch,
            run,
            stream,
            queue: Arc::clone(&queue),
            on_progress: Box::new(on_progress),
            reply,
        })
        .unwrap_or_else(|| Err(engine_stopped()))?;
        Ok(StreamHandle {
            id,
            epoch,
            queue,
            commands: self.commands.clone(),
            finished: false,
        })
    }

    /// Stop the transcription or stream in progress, and any queued behind
    /// it. Bypasses the queue. Families with `Feature::Cancellation`
    /// get a polite cancel and keep their model loaded; if that gets no
    /// answer within [`CANCEL_GRACE`], or the family can't cancel, the
    /// worker is killed and the next use starts a fresh one. Model loads are
    /// never cancelled.
    pub fn cancel(&self) {
        self.shared.cancel_epoch.fetch_add(1, Ordering::SeqCst);
        let in_flight = lock(&self.shared.in_flight);
        let Some(call) = in_flight.as_ref() else {
            return;
        };
        let polite = call.cancellable
            && encode_message(&Request::Cancel, None).is_ok_and(|frame| call.control.send(frame));
        if !polite {
            info!(
                "Cancelling the {} by stopping its worker (pid {})",
                call.what, call.control.pid
            );
            call.control.kill();
            return;
        }
        info!("Cancelling the {}", call.what);
        let shared = Arc::clone(&self.shared);
        let id = call.id;
        let _ = thread::Builder::new()
            .name("transcribe-cancel".into())
            .spawn(move || {
                thread::sleep(CANCEL_GRACE);
                // Same lock the owner clears the call under, so this can
                // only kill the call it was started for.
                let in_flight = lock(&shared.in_flight);
                if let Some(call) = in_flight.as_ref().filter(|call| call.id == id) {
                    warn!(
                        "The {} did not stop within {:?} of cancelling; killing its worker (pid {})",
                        call.what, CANCEL_GRACE, call.control.pid
                    );
                    call.control.kill();
                }
            });
    }

    /// The compute devices, as listed by the latest worker. If no worker has
    /// listed them yet, starts a short-lived worker on this thread to list
    /// them. It runs beside any model worker, which it leaves alone.
    pub fn devices(&self) -> DeviceList {
        let _probing = lock(&self.shared.probing);
        if needs_probe(&lock(&self.shared.devices)) {
            self.shared.probe_devices();
        }
        lock(&self.shared.devices).clone()
    }
}

/// Only an unknown list is probed, and not while the GPU is known to fail:
/// that probe would crash or hang the same way. A failed list stays failed
/// until [`EngineSupervisor::retry_gpu`].
fn needs_probe(devices: &DeviceList) -> bool {
    matches!(devices, DeviceList::Unknown) && !Degraded::GpuUnavailable.is_set()
}

/// An unload in progress; see [`EngineSupervisor::unload`].
pub struct Unloading(Option<mpsc::Receiver<()>>);

impl Unloading {
    /// Wait until the worker has exited (e.g. before deleting the model file).
    pub fn wait(self) {
        if let Some(exited) = self.0 {
            let _ = exited.recv();
        }
    }
}

/// A live stream in the worker. Feeds are queued without waiting, in order
/// with finalize. Dropping the handle without finalizing resets the stream.
pub struct StreamHandle {
    id: u64,
    /// Cancel epoch when the stream started.
    epoch: u64,
    queue: Arc<StreamQueue>,
    commands: mpsc::Sender<Command>,
    finished: bool,
}

impl StreamHandle {
    pub fn feed(&self, pcm: Vec<f32>) {
        self.queue.backlog.fetch_add(pcm.len(), Ordering::AcqRel);
        let _ = self.commands.send(Command::Feed { id: self.id, pcm });
    }

    /// Flush the stream and return its final text. `Ok(None)` if the stream
    /// was lost or failed; fall back to batch transcription then.
    /// `Err(Cancelled)` if [`EngineSupervisor::cancel`] stopped it.
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
            // Queued feeds for this stream are skipped from here on.
            self.queue.closed.store(true, Ordering::Release);
            let _ = self.commands.send(Command::StreamReset { id: self.id });
        }
    }
}

fn engine_stopped() -> EngineError {
    EngineError::Failed("the transcription engine has stopped".to_string())
}

fn not_loaded() -> EngineError {
    EngineError::Failed("no transcription model is loaded".to_string())
}

/// How a call to the worker failed.
#[derive(Debug)]
enum Failure {
    /// The worker process could not be started.
    Spawn(io::Error),
    /// transcribe.cpp returned an error; the worker is fine.
    Remote(String),
    /// The worker exited or crashed. Carries exit status and its last
    /// native output.
    Crashed(String),
    /// The worker did not answer within the deadline and was killed.
    Hung(Duration),
    Cancelled,
}

impl Failure {
    /// Whether the worker is gone.
    fn worker_lost(&self) -> bool {
        matches!(self, Failure::Crashed(_) | Failure::Hung(_))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Spawn(e) => write!(f, "failed to start transcription worker: {e}"),
            Failure::Remote(message) => f.write_str(message),
            Failure::Crashed(detail) => write!(f, "transcription worker crashed ({detail})"),
            Failure::Hung(t) => write!(f, "transcription worker did not respond within {t:?}"),
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

// --- Owner thread ---------------------------------------------------------

struct ActiveStream {
    id: u64,
    /// Cancel epoch when the stream started; a later cancel stops it.
    epoch: u64,
    queue: Arc<StreamQueue>,
    on_progress: OnProgress,
    /// Audio received but not yet committed, which finalize must still
    /// decode; sizes the feed and finalize deadlines.
    pending_ms: i64,
}

struct Owner {
    shared: Arc<Shared>,
    /// The single worker slot.
    worker: Option<Worker>,
    /// The model the app asked for. Outlives its worker: after a crash,
    /// hang or cancel the next use starts a fresh worker for it.
    spec: Option<LoadSpec>,
    stream: Option<ActiveStream>,
    /// A worker failed on a GPU during dictation work. Workers use CPU while
    /// this is pending; once the same kind of work succeeds there,
    /// [`Degraded::GpuUnavailable`] is set. If it fails on CPU too, the GPU
    /// was not at fault and this is dropped.
    gpu_suspect: Option<GpuSuspect>,
    /// Unload commands processed, against [`Shared::unloads_requested`].
    unloads_done: u64,
}

struct GpuSuspect {
    reason: String,
    /// [`Shared::gpu_retries`] when the failure happened.
    gpu_retries: u64,
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
                    id,
                    epoch,
                    run,
                    stream,
                    queue,
                    on_progress,
                    reply,
                } => {
                    let _ = reply.send(self.stream_begin(
                        ActiveStream {
                            id,
                            epoch,
                            queue,
                            on_progress,
                            pending_ms: 0,
                        },
                        &run,
                        &stream,
                    ));
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
                Command::StreamReset { id } => {
                    if self.stream.as_ref().is_some_and(|s| s.id == id) {
                        self.end_stream();
                    }
                }
            }
        }
        // Every handle is gone.
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
        // The worker's exit ends any stream; no need to reset it first.
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

    /// The spec a fresh worker should load: CPU while the GPU is unusable or
    /// suspect, unless the device was chosen explicitly.
    fn effective_spec(&self, spec: &LoadSpec) -> LoadSpec {
        if spec.pinned() || spec.cpu_only() {
            return spec.clone();
        }
        let reason = if Degraded::GpuUnavailable.is_set() {
            "a worker with GPU backends crashed or hung earlier"
        } else if matches!(*lock(&self.shared.devices), DeviceList::Failed(_)) {
            // A worker with GPU backends would fail the same way first.
            "listing compute devices failed"
        } else if self.pending_gpu_suspect().is_some() {
            "the last attempt failed on the GPU"
        } else {
            return spec.clone();
        };
        info!("Loading the transcription model on CPU: {}", reason);
        spec.cpu()
    }

    /// Whether a worker for `spec` registers only the CPU backends.
    fn cpu_only_worker(&self, spec: &LoadSpec) -> bool {
        spec.cpu_only() || !self.shared.gpu_allowed
    }

    /// Start a worker for `spec` and load it there. A worker that may use the
    /// GPU lists the devices in its hello, which keeps the list current (a
    /// GPU that comes back is just noticed). A CPU-only worker would only
    /// see the CPU, so it doesn't list.
    fn start_worker(&self, spec: &LoadSpec) -> Result<(Worker, LoadedInfo), Failure> {
        let cpu_only = self.cpu_only_worker(spec);
        let mut worker = self.shared.hello(cpu_only, !cpu_only)?;
        let info = worker.load(spec)?;
        Ok((worker, info))
    }

    /// Start a worker and load the current spec in it. If a worker with GPU
    /// backends is lost on the way (backend init, device listing or the
    /// load), retry once in a CPU-only worker. Only if that succeeds is the
    /// GPU marked unavailable: if it fails too, the GPU was not at fault.
    fn start(&mut self) -> Result<LoadedInfo, EngineError> {
        let spec = self.spec.clone().ok_or_else(not_loaded)?;
        let started = Instant::now();
        let mut wanted = self.effective_spec(&spec);
        if !self.cpu_only_worker(&wanted) {
            // A device probe in flight is starting the same GPU driver: wait
            // for its verdict rather than crash or hang beside it. CPU-only
            // starts never wait on a probe.
            drop(lock(&self.shared.probing));
            wanted = self.effective_spec(&spec);
        }
        let (worker, info) = match self.start_worker(&wanted) {
            Ok(started) => started,
            Err(failure)
                if failure.worker_lost() && !self.cpu_only_worker(&wanted) && !wanted.pinned() =>
            {
                warn!(
                    "Starting a worker for '{}' failed with GPU backends ({}); retrying in a CPU-only worker",
                    wanted.path.display(),
                    failure
                );
                match self.start_worker(&wanted.cpu()) {
                    Ok(started) => {
                        Degraded::GpuUnavailable.set(format!(
                            "a worker for '{}' with GPU backends {} but a CPU-only worker loaded it",
                            wanted.path.display(),
                            failure
                        ));
                        started
                    }
                    Err(cpu_failure) => {
                        warn!(
                            "The CPU-only worker failed too ({}), so this is not treated as a GPU failure",
                            cpu_failure
                        );
                        return Err(cpu_failure.into());
                    }
                }
            }
            Err(failure) => {
                if failure.worker_lost() && wanted.cpu_only() {
                    self.resolve_gpu_suspect(false);
                }
                return Err(failure.into());
            }
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

    /// Report the model loaded, unless an unload was requested since and is
    /// still waiting in the queue.
    fn publish_loaded(&self, info: &LoadedInfo) {
        let mut loaded = lock(&self.shared.loaded);
        if self.shared.unloads_requested.load(Ordering::SeqCst) == self.unloads_done {
            *loaded = Some(info.clone());
        }
    }

    /// Make sure the slot holds a worker with the model loaded, starting a
    /// fresh one if the last one died or was cancelled.
    fn ensure_worker(&mut self) -> Result<(), EngineError> {
        if self.worker.is_some() {
            return Ok(());
        }
        if self.spec.is_none() {
            return Err(not_loaded());
        }
        info!("Starting a fresh transcription worker for the loaded model");
        if let Err(e) = self.start() {
            // The model can't be brought back; report it unloaded so the
            // next use loads it from scratch (and reports why it can't).
            self.spec = None;
            *lock(&self.shared.loaded) = None;
            return Err(e);
        }
        Ok(())
    }

    /// Drop a worker that crashed or hung (its handle already logged the
    /// details and native output). Returns whether it was on a GPU.
    fn worker_lost(&mut self, during: &str, failure: &Failure) -> bool {
        // Dropping the handle reaps the process.
        let info = self.worker.take().and_then(|worker| worker.info.clone());
        let on_gpu = info.as_ref().is_some_and(|info| info.on_gpu);
        let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
        match info {
            Some(info) if on_gpu && !pinned => {
                warn!(
                    "Transcription worker failed during {} on GPU '{}' ({}); using CPU",
                    during, info.device, info.backend
                );
                self.gpu_suspect = Some(GpuSuspect {
                    reason: format!(
                        "{failure} during {during} on GPU '{}' ({})",
                        info.device, info.backend
                    ),
                    gpu_retries: self.shared.gpu_retries.load(Ordering::SeqCst),
                });
            }
            _ if !on_gpu => self.resolve_gpu_suspect(false),
            _ => {}
        }
        on_gpu
    }

    /// The pending GPU failure, unless an explicit accelerator change has
    /// cleared GPU failures since.
    fn pending_gpu_suspect(&self) -> Option<&str> {
        self.gpu_suspect
            .as_ref()
            .filter(|s| s.gpu_retries == self.shared.gpu_retries.load(Ordering::SeqCst))
            .map(|s| s.reason.as_str())
    }

    /// Settle a pending GPU suspicion once work has run on CPU.
    fn resolve_gpu_suspect(&mut self, cpu_succeeded: bool) {
        let Some(reason) = self.pending_gpu_suspect().map(str::to_string) else {
            self.gpu_suspect = None;
            return;
        };
        self.gpu_suspect = None;
        if cpu_succeeded {
            Degraded::GpuUnavailable.set(format!("{reason}; it succeeded on CPU"));
        } else {
            info!(
                "Transcription failed on CPU too, so the earlier GPU failure ({}) is not treated as a GPU problem",
                reason
            );
        }
    }

    fn work_succeeded(&mut self) {
        let on_cpu = self
            .worker
            .as_ref()
            .and_then(|worker| worker.info.as_ref())
            .is_some_and(|info| !info.on_gpu);
        if on_cpu {
            self.resolve_gpu_suspect(true);
        }
    }

    /// A call that [`EngineSupervisor::cancel`] can stop. Returns
    /// `Cancelled` if a cancel came after `epoch`, whatever the worker said.
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
            // Check, send and publish under the lock cancel() takes after
            // bumping the epoch: either this sees the cancel and sends
            // nothing, or the cancel sees the call and its Cancel frame
            // follows the request.
            let mut in_flight = lock(&self.shared.in_flight);
            if self.shared.cancelled_since(epoch) {
                return Err(Failure::Cancelled);
            }
            let sent = worker.control.send(frame);
            *in_flight = Some(InFlight {
                id: self.shared.next_id(),
                what: request_name(request),
                control: Arc::clone(&worker.control),
                cancellable: worker
                    .info
                    .as_ref()
                    .is_some_and(|info| info.supports_cancellation),
            });
            sent
        };
        let result = if sent {
            worker.wait(request, deadline)
        } else {
            Err(worker.died())
        };
        lock(&self.shared.in_flight).take();
        if self.shared.cancelled_since(epoch) {
            if result.as_ref().is_err_and(Failure::worker_lost) {
                // Killed by the cancel; the next use starts a fresh worker.
                self.worker = None;
            }
            return Err(Failure::Cancelled);
        }
        result
    }

    /// Retry once after a crash or hang: on CPU if the GPU may be at fault,
    /// never on the same device after a hang, and never for an explicitly
    /// chosen device.
    fn transcribe(
        &mut self,
        pcm: &[f32],
        run: &RunOptions,
        epoch: u64,
    ) -> Result<Transcript, EngineError> {
        // A stream left open would make the worker refuse the run.
        self.end_stream();
        let request = Request::Run {
            options: run.clone(),
        };
        let mut retried = false;
        loop {
            if self.shared.cancelled_since(epoch) {
                return Err(EngineError::Cancelled);
            }
            // A respawn here runs under the load deadline; the run's own
            // deadline starts only once the model is loaded.
            self.ensure_worker()?;
            match self.call_work(&request, Some(pcm), run_deadline(pcm.len()), epoch) {
                Ok(Response::Transcript(transcript)) => {
                    self.work_succeeded();
                    return Ok(transcript);
                }
                Ok(other) => return Err(unexpected(&other).into()),
                Err(failure) if failure.worker_lost() => {
                    let on_gpu = self.worker_lost("transcription", &failure);
                    let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
                    let hung_on_cpu = matches!(failure, Failure::Hung(_)) && !on_gpu;
                    if retried || pinned || hung_on_cpu {
                        return Err(failure.into());
                    }
                    warn!("Retrying the transcription in a fresh worker");
                    retried = true;
                }
                Err(failure) => return Err(failure.into()),
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
        if worker.streams_too_slowly {
            return Err(EngineError::Failed(
                "live transcription can't keep up on this device; using batch transcription"
                    .to_string(),
            ));
        }
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

    /// Feeds never respawn the worker (the stream state would be lost); a
    /// lost stream just drops the rest of its frames and finalizes to
    /// `None`, and the caller falls back to batch. Feeds are cancellable like
    /// any other dictation work.
    fn feed(&mut self, id: u64, pcm: &[f32]) {
        let Some(stream) = self.stream.as_ref().filter(|s| s.id == id) else {
            return;
        };
        // Includes this frame and every frame queued behind it.
        let backlog = stream.queue.backlog.fetch_sub(pcm.len(), Ordering::AcqRel);
        // Skipped once the caller dropped its handle or cancelled.
        if stream.queue.closed.load(Ordering::Acquire)
            || self.shared.cancelled_since(stream.epoch)
            || self.worker.is_none()
        {
            return;
        }
        if backlog > ms_to_samples(MAX_STREAM_BACKLOG.as_millis() as i64) {
            // Streaming slower than real time only falls further behind, and
            // stopping would wait for the whole backlog. Drop the stream (its
            // finalize then reports no result, so the dictation runs as one
            // batch) and don't stream on this worker again; a new worker,
            // e.g. back on the GPU, may try again.
            warn!(
                "Live transcription fell {:.1}s behind real time; using batch transcription instead",
                backlog as f64 / SAMPLE_RATE
            );
            if let Some(worker) = self.worker.as_mut() {
                worker.streams_too_slowly = true;
            }
            self.end_stream();
            return;
        }
        let epoch = stream.epoch;
        let deadline = audio_deadline(pcm.len() + ms_to_samples(stream.pending_ms));
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
            Ok(other) => warn!("Stream feed failed: {}", unexpected(&other)),
            Err(Failure::Remote(message)) => warn!("Stream feed failed: {}", message),
            // Later feeds are skipped, and finalize reports the cancel.
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
        // Checked first: a stream lost to a cancel must report the cancel,
        // not "no result", which would start a batch run.
        if self.shared.cancelled_since(epoch) {
            if self.stream.as_ref().is_some_and(|s| s.id == id) {
                self.end_stream();
            }
            return Err(EngineError::Cancelled);
        }
        let Some(stream) = self.stream.take_if(|s| s.id == id) else {
            // Lost earlier (or replaced): fall back to batch.
            return Ok(None);
        };
        if self.worker.is_none() {
            return Ok(None);
        }
        let deadline = audio_deadline(ms_to_samples(stream.pending_ms));
        let started = Instant::now();
        match self.call_work(&Request::Finalize { want_language }, None, deadline, epoch) {
            Ok(Response::Finalized {
                update,
                text,
                language,
            }) => {
                self.work_succeeded();
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
                // The finalize may never have reached the worker.
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
            // Expected when the stream already ended in the worker.
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

// --- Worker process handle ------------------------------------------------

/// The parts of a worker that other threads touch: its stdin queue and the
/// process (to kill it). Every write goes through the one writer thread, so
/// a cancel can never interleave with a large frame.
struct Control {
    pid: u32,
    child: Mutex<Child>,
    stdin: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
}

impl Control {
    fn send(&self, frame: Vec<u8>) -> bool {
        lock(&self.stdin)
            .as_ref()
            .is_some_and(|stdin| stdin.send(frame).is_ok())
    }

    /// Closing stdin tells the worker to exit (it ends on EOF).
    fn close_stdin(&self) {
        lock(&self.stdin).take();
    }

    fn kill(&self) {
        let _ = lock(&self.child).kill();
    }
}

/// One worker process, owned by the supervisor's thread. Strict
/// request/response. Dropping it closes the worker's stdin, gives it
/// [`SHUTDOWN_GRACE`] to exit, kills it otherwise, and reaps it: once
/// dropped, the worker is gone.
struct Worker {
    control: Arc<Control>,
    responses: mpsc::Receiver<Response>,
    stderr_done: mpsc::Receiver<()>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// Set once a model is loaded.
    info: Option<LoadedInfo>,
    /// A stream on this worker fell too far behind real time.
    streams_too_slowly: bool,
}

impl Worker {
    /// Start a worker process. A `cpu_only` worker never registers a GPU
    /// backend.
    fn spawn(cpu_only: bool) -> io::Result<Self> {
        // Under `cargo test` the current exe is the test harness; tests point
        // this at the built app instead.
        #[cfg(test)]
        let exe = PathBuf::from(
            std::env::var_os("HANDY_TRANSCRIBE_WORKER_EXE")
                .expect("set HANDY_TRANSCRIBE_WORKER_EXE"),
        );
        #[cfg(not(test))]
        let exe = std::env::current_exe()?;
        let mut command = ProcessCommand::new(exe);
        command.arg(WORKER_FLAG);
        if cpu_only {
            command.arg(CPU_ONLY_FLAG);
        }
        command
            .env(LOG_LEVEL_ENV, log::max_level().as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let pid = child.id();
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

        let (stdin_tx, frames) = mpsc::channel();
        let (response_tx, responses) = mpsc::channel();
        let (stderr_done_tx, stderr_done) = mpsc::channel();
        // Built before the threads so a failure below still reaps the child.
        let worker = Worker {
            control: Arc::new(Control {
                pid,
                child: Mutex::new(child),
                stdin: Mutex::new(Some(stdin_tx)),
            }),
            responses,
            stderr_done,
            stderr_tail: Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES))),
            info: None,
            streams_too_slowly: false,
        };

        thread::Builder::new()
            .name("transcribe-worker-in".into())
            .spawn(move || write_frames(stdin, frames))?;
        thread::Builder::new()
            .name("transcribe-worker-out".into())
            .spawn(move || read_responses(stdout, response_tx))?;
        // Always drain stderr: a full pipe would block the worker.
        let tail = Arc::clone(&worker.stderr_tail);
        thread::Builder::new()
            .name("transcribe-worker-err".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
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

    /// Send one request and wait up to `deadline` for its response. Hitting
    /// the deadline kills the worker; after any `Crashed`/`Hung` the handle
    /// is dead and should be dropped.
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

    /// Wait for the response to `request`, already sent.
    fn wait(&mut self, request: &Request, deadline: Duration) -> Result<Response, Failure> {
        match self.responses.recv_timeout(deadline) {
            Ok(Response::Error(message)) => Err(Failure::Remote(message)),
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

    /// The worker's stdout closed: collect its exit status and last output.
    fn died(&mut self) -> Failure {
        // Let the stderr thread catch the final lines (the abort message).
        let _ = self.stderr_done.recv_timeout(Duration::from_secs(1));
        let status = {
            let mut child = lock(&self.control.child);
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.to_string(),
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    _ => {
                        let _ = child.kill();
                        break "unresponsive after closing its output; killed".to_string();
                    }
                }
            }
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

    /// Recent raw native output (worker log lines are excluded; they were
    /// already re-logged).
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
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        let mut child = lock(&self.control.child);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                _ => break,
            }
        }
        warn!(
            "Transcription worker (pid {}) did not exit within {:?}; killing it",
            self.control.pid, SHUTDOWN_GRACE
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn encode_request(request: &Request, pcm: Option<&[f32]>) -> Result<Vec<u8>, Failure> {
    encode_message(request, pcm)
        .map_err(|e| Failure::Remote(format!("failed to encode request: {e}")))
}

fn write_frames(mut stdin: ChildStdin, frames: mpsc::Receiver<Vec<u8>>) {
    // Ends when the handle closes stdin (dropping it here sends EOF) or the
    // worker is gone.
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

fn read_responses(stdout: ChildStdout, responses: mpsc::Sender<Response>) {
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
        Request::Cancel => "cancel",
    }
}

/// Re-log a worker stderr line in the parent. Structured lines keep their
/// level; anything else is raw native output.
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
    fn audio_deadline_is_ten_times_the_audio_with_a_floor() {
        assert_eq!(audio_deadline(0), CALL_FLOOR);
        assert_eq!(audio_deadline(16_000 / 2), CALL_FLOOR);
        assert_eq!(audio_deadline(16_000 * 5), Duration::from_secs(50));
        assert_eq!(run_deadline(16_000), RUN_FLOOR);
        assert_eq!(run_deadline(16_000 * 10), Duration::from_secs(100));
        assert_eq!(ms_to_samples(1500), 24_000);
        assert_eq!(ms_to_samples(-5), 0);
    }
}

/// End-to-end checks against a real worker and model. Crash and hang tests
/// need a debug worker exe (fault injection is compiled out of release), and
/// the stream tests a model that can stream.
/// Run with:
/// `HANDY_TRANSCRIBE_WORKER_EXE=target/debug/handy HANDY_TEST_MODEL=<gguf>
///  HANDY_TEST_WAV=<16 kHz mono wav> cargo test --lib engine_supervisor --
///  --ignored --nocapture --test-threads=1`
#[cfg(test)]
mod tests {
    use super::*;

    fn env_path(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
    }

    fn test_pcm() -> Vec<f32> {
        crate::audio_toolkit::read_wav_samples(env_path("HANDY_TEST_WAV")).expect("read wav")
    }

    fn spec() -> LoadSpec {
        LoadSpec {
            path: env_path("HANDY_TEST_MODEL"),
            backend: Backend::Auto,
            device: DeviceSelector::Auto,
        }
    }

    fn percentile(sorted: &[Duration], p: f64) -> Duration {
        sorted[((sorted.len() - 1) as f64 * p).round() as usize]
    }

    fn summarize(label: &str, mut samples: Vec<Duration>) {
        samples.sort();
        let total: Duration = samples.iter().sum();
        println!(
            "{label}: n={} mean={:?} p50={:?} p95={:?} max={:?}",
            samples.len(),
            total / samples.len() as u32,
            percentile(&samples, 0.5),
            percentile(&samples, 0.95),
            samples.last().unwrap()
        );
    }

    /// Faults are read by each worker at spawn, so set them before loading.
    /// Also resets the (process-global) degraded state.
    fn set_fault(fault: Option<&str>, gpu_only: bool) {
        Degraded::GpuUnavailable.clear("test reset");
        let marker = std::env::temp_dir().join("handy-worker-fault-once");
        let _ = std::fs::remove_file(&marker);
        std::env::remove_var("HANDY_WORKER_FAULT_ONCE");
        std::env::remove_var("HANDY_WORKER_FAULT_GPU_ONLY");
        match fault {
            Some(fault) => {
                std::env::set_var("HANDY_WORKER_FAULT", fault);
                if gpu_only {
                    std::env::set_var("HANDY_WORKER_FAULT_GPU_ONLY", "1");
                } else {
                    std::env::set_var("HANDY_WORKER_FAULT_ONCE", marker);
                }
            }
            None => std::env::remove_var("HANDY_WORKER_FAULT"),
        }
    }

    /// Child processes of this test process (the workers alive right now).
    fn worker_count() -> usize {
        let out = std::process::Command::new("pgrep")
            .args(["-P", &std::process::id().to_string()])
            .output()
            .expect("run pgrep");
        String::from_utf8_lossy(&out.stdout).lines().count()
    }

    /// Feed 30 ms frames a little faster than a microphone would, but not so
    /// fast that the stream looks like it fell behind real time.
    fn feed_paced(stream: &StreamHandle, pcm: &[f32]) {
        for frame in pcm.chunks(480) {
            stream.feed(frame.to_vec());
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn stream_all(
        engine: &EngineSupervisor,
        pcm: &[f32],
    ) -> Result<Option<Finalized>, EngineError> {
        let stream =
            engine.start_stream(RunOptions::default(), StreamOptions::default(), |_| {})?;
        feed_paced(&stream, pcm);
        stream.finalize(false)
    }

    /// Block until the owner is waiting on `what` (a [`request_name`]) in
    /// the worker, so a cancel lands on that work rather than before or
    /// after it, however fast the model is.
    fn wait_in_flight(engine: &EngineSupervisor, what: &str) {
        let started = Instant::now();
        while !lock(&engine.shared.in_flight)
            .as_ref()
            .is_some_and(|call| call.what == what)
        {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "{what} never started"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// The test clip repeated to at least `secs` seconds.
    fn long_pcm(secs: usize) -> Vec<f32> {
        let clip = test_pcm();
        clip.repeat((secs * 16_000).div_ceil(clip.len()))
    }

    /// Pure IPC cost: requests the worker rejects immediately, carrying a
    /// 30 ms recorder frame and a 60 s batch buffer.
    #[test]
    #[ignore]
    fn ipc_round_trip_overhead() {
        set_fault(None, false);
        let started = Instant::now();
        let mut worker = Worker::spawn(false).unwrap();
        worker
            .call(&Request::Hello { list_devices: true }, None, HELLO_TIMEOUT)
            .unwrap();
        println!(
            "spawn + backend init + list devices: {:?}",
            started.elapsed()
        );

        for (label, samples, n) in [("30ms frame", 480, 2000), ("60s buffer", 960_000, 50)] {
            let pcm = vec![0.25f32; samples];
            let times = (0..n)
                .map(|_| {
                    let t = Instant::now();
                    let r = worker.call(
                        &Request::Run {
                            options: RunOptions::default(),
                        },
                        Some(&pcm),
                        LOAD_TIMEOUT,
                    );
                    assert!(matches!(r, Err(Failure::Remote(_))));
                    t.elapsed()
                })
                .collect();
            summarize(&format!("round trip, {label}"), times);
        }
    }

    /// Streams the test WAV in 30 ms frames through the worker and, for
    /// comparison, through transcribe-cpp in this process.
    #[test]
    #[ignore]
    fn stream_feed_latency_vs_in_process() {
        set_fault(None, false);
        let pcm = test_pcm();
        let run = RunOptions::default();
        let stream_options = StreamOptions::default();

        let engine = EngineSupervisor::new(true);
        let started = Instant::now();
        let info = engine.load(spec()).unwrap();
        println!(
            "isolated: spawn + load {:?} on '{}'",
            started.elapsed(),
            info.backend
        );
        let isolated = Arc::new(Mutex::new(Vec::new()));
        let feeds = Arc::clone(&isolated);
        let stream = engine
            .start_stream(run.clone(), stream_options.clone(), move |progress| {
                lock(&feeds).push(progress.elapsed);
            })
            .unwrap();
        feed_paced(&stream, &pcm);
        let finalized = stream.finalize(false).unwrap().unwrap();
        println!(
            "isolated finalize {:?}: {}",
            finalized.elapsed, finalized.text.full
        );
        engine.unload().wait();

        transcribe_cpp::init_backends_default().unwrap();
        let started = Instant::now();
        let model = transcribe_cpp::Model::load(env_path("HANDY_TEST_MODEL")).unwrap();
        let mut session = model.session().unwrap();
        println!("in-process: load {:?}", started.elapsed());
        let mut stream = session.stream(&run, &stream_options).unwrap();
        let mut in_process = Vec::new();
        for frame in pcm.chunks(480) {
            let t = Instant::now();
            let update = stream.feed(frame).unwrap();
            if update.committed_changed || update.tentative_changed {
                let _ = stream.text();
            }
            in_process.push(t.elapsed());
        }
        let t = Instant::now();
        stream.finalize().unwrap();
        println!(
            "in-process finalize {:?}: {}",
            t.elapsed(),
            stream.text().full
        );

        summarize("feed, isolated", lock(&isolated).clone());
        summarize("feed, in-process", in_process);
    }

    /// Cost of (re)loading: a fresh worker per load (what idle unload or a
    /// crash now costs) vs reloading inside an already-warm process.
    #[test]
    #[ignore]
    fn reload_cost_worker_vs_in_process() {
        set_fault(None, false);
        let pcm = test_pcm();
        let options = RunOptions::default();
        let engine = EngineSupervisor::new(true);
        for i in 0..3 {
            let t = Instant::now();
            engine.load(spec()).unwrap();
            let loaded = t.elapsed();
            engine.transcribe(pcm.clone(), options.clone()).unwrap();
            println!(
                "worker #{i}: spawn+load {:?}, +first run {:?}",
                loaded,
                t.elapsed()
            );
            engine.unload().wait();
        }
        transcribe_cpp::init_backends_default().unwrap();
        for i in 0..3 {
            let t = Instant::now();
            let model = transcribe_cpp::Model::load(env_path("HANDY_TEST_MODEL")).unwrap();
            let mut session = model.session().unwrap();
            let loaded = t.elapsed();
            session.run(&pcm, &options).unwrap();
            println!(
                "in-process #{i}: load {:?}, +first run {:?}",
                loaded,
                t.elapsed()
            );
        }
    }

    /// Never two workers: a reload or unload has fully stopped the old
    /// worker before anything else happens.
    #[test]
    #[ignore]
    fn one_worker_at_a_time() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        for _ in 0..3 {
            engine.load(spec()).unwrap();
            assert_eq!(worker_count(), 1);
        }
        engine.unload().wait();
        assert_eq!(worker_count(), 0);
        assert!(engine.loaded().is_none());
        // A device probe with nothing loaded leaves no worker behind.
        assert!(matches!(engine.devices(), DeviceList::Known(_)));
        assert_eq!(worker_count(), 0);
    }

    /// A worker crash mid-stream loses the stream; the batch fallback then
    /// runs in a fresh worker.
    #[test]
    #[ignore]
    fn crash_mid_stream_falls_back_to_batch() {
        let pcm = test_pcm();
        set_fault(Some("abort@feed"), false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        assert!(stream_all(&engine, &pcm).unwrap().is_none());
        let t = Instant::now();
        let transcript = engine.transcribe(pcm, RunOptions::default()).unwrap();
        println!(
            "batch fallback after crash {:?} on '{}': {}",
            t.elapsed(),
            engine.loaded().unwrap().backend,
            transcript.text
        );
        assert!(engine.loaded().is_some());
        set_fault(None, false);
    }

    /// A hung finalize is killed at its deadline (sized by the buffered
    /// audio, at least CALL_FLOOR) and the batch fallback runs in a fresh
    /// worker.
    #[test]
    #[ignore]
    fn hung_finalize_is_killed_and_recovers() {
        let pcm = test_pcm();
        set_fault(Some("hang@finalize"), false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let t = Instant::now();
        assert!(stream_all(&engine, &pcm).unwrap().is_none());
        println!("stream + hung finalize gave up after {:?}", t.elapsed());
        let t = Instant::now();
        let transcript = engine.transcribe(pcm, RunOptions::default()).unwrap();
        println!(
            "batch fallback after hang {:?}: {}",
            t.elapsed(),
            transcript.text
        );
        set_fault(None, false);
    }

    /// Cancelling a stream finalize reports `Cancelled`, never "no result"
    /// (which would start a batch run), and the next use works.
    #[test]
    #[ignore]
    fn cancel_during_finalize_is_cancelled() {
        let pcm = test_pcm();
        set_fault(Some("hang@finalize"), false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let stream = engine
            .start_stream(RunOptions::default(), StreamOptions::default(), |_| {})
            .unwrap();
        feed_paced(&stream, &pcm);
        let finalize = thread::spawn(move || stream.finalize(false));
        wait_in_flight(&engine, "stream finalize");
        let t = Instant::now();
        engine.cancel();
        let result = finalize.join().unwrap();
        println!("finalize cancelled {:?} after cancel()", t.elapsed());
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < CANCEL_GRACE + Duration::from_secs(2));
        assert!(!Degraded::GpuUnavailable.is_set());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// Cancelling during a hung feed stops it within the cancel grace (not
    /// the feed deadline), skips the queued frames, and the finalize reports
    /// `Cancelled` even though the stream's worker is gone.
    #[test]
    #[ignore]
    fn cancel_during_hung_feed_is_cancelled() {
        let pcm = test_pcm();
        set_fault(Some("hang@feed"), false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let stream = engine
            .start_stream(RunOptions::default(), StreamOptions::default(), |_| {})
            .unwrap();
        feed_paced(&stream, &pcm);
        wait_in_flight(&engine, "stream feed");
        let t = Instant::now();
        engine.cancel();
        let result = stream.finalize(false);
        println!("hung feed cancelled {:?} after cancel()", t.elapsed());
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < CANCEL_GRACE + Duration::from_secs(2));
        assert!(!Degraded::GpuUnavailable.is_set());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// A stream slower than real time is dropped once it falls 5 s behind:
    /// finalize reports no result (so the dictation runs as one batch), and
    /// this worker doesn't stream again.
    #[test]
    #[ignore]
    fn slow_stream_falls_back_to_batch() {
        let pcm = test_pcm();
        set_fault(Some("slow@feed"), false);
        std::env::remove_var("HANDY_WORKER_FAULT_ONCE");
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let t = Instant::now();
        assert!(stream_all(&engine, &pcm).unwrap().is_none());
        println!("slow stream gave up after {:?}", t.elapsed());
        assert!(t.elapsed() < Duration::from_secs(10));
        assert!(engine
            .start_stream(RunOptions::default(), StreamOptions::default(), |_| {})
            .is_err());
        engine
            .transcribe(pcm.clone(), RunOptions::default())
            .unwrap();
        // A new worker may stream again.
        set_fault(None, false);
        engine.load(spec()).unwrap();
        assert!(stream_all(&engine, &pcm).unwrap().is_some());
    }

    /// After a listing crash, "try the GPU again" refreshes the device list
    /// right away with a probe beside the CPU-only worker holding the model,
    /// which keeps running.
    #[test]
    #[ignore]
    fn retry_gpu_refreshes_devices_with_a_worker_running() {
        let pcm = test_pcm();
        set_fault(Some("abort@list"), false);
        let engine = EngineSupervisor::new(true);
        assert!(!engine.load(spec()).unwrap().on_gpu);
        assert!(matches!(engine.devices(), DeviceList::Failed(_)));
        engine.retry_gpu("test");
        assert!(matches!(engine.devices(), DeviceList::Known(_)));
        assert!(engine.loaded().is_some());
        assert_eq!(worker_count(), 1, "the probe left the model worker alone");
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// An explicit accelerator change forgets a GPU failure that was still
    /// pending (a stream lost on the GPU, no CPU work since): the next load
    /// goes back to the GPU.
    #[test]
    #[ignore]
    fn retry_gpu_forgets_a_pending_gpu_failure() {
        let pcm = test_pcm();
        set_fault(Some("abort@feed"), true);
        let engine = EngineSupervisor::new(true);
        assert!(engine.load(spec()).unwrap().on_gpu, "needs a GPU");
        assert!(stream_all(&engine, &pcm).unwrap().is_none());
        set_fault(None, false);
        engine.retry_gpu("test");
        assert!(engine.load(spec()).unwrap().on_gpu);
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        assert!(!Degraded::GpuUnavailable.is_set());
    }

    /// An unload reads as unloaded at once, even while it waits behind a
    /// run, so a new dictation queues a fresh load instead of assuming the
    /// model is there.
    #[test]
    #[ignore]
    fn unload_reads_unloaded_at_once() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let run = {
            let engine = engine.clone();
            thread::spawn(move || engine.transcribe(long_pcm(60), RunOptions::default()))
        };
        wait_in_flight(&engine, "transcription");
        let unload = {
            let engine = engine.clone();
            thread::spawn(move || engine.unload().wait())
        };
        let t = Instant::now();
        while engine.loaded().is_some() {
            assert!(t.elapsed() < Duration::from_secs(1), "still reads loaded");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(lock(&engine.shared.in_flight).is_some(), "run still going");
        run.join().unwrap().unwrap();
        unload.join().unwrap();
        assert!(engine.loaded().is_none());
        assert_eq!(worker_count(), 0);
    }

    /// An unload that is still waiting its turn can't affect a load
    /// requested after it (a new recording right after an "Immediately"
    /// unload): the newer model ends up loaded and usable.
    #[test]
    #[ignore]
    fn queued_unload_never_affects_a_later_load() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let run = {
            let engine = engine.clone();
            thread::spawn(move || engine.transcribe(long_pcm(60), RunOptions::default()))
        };
        wait_in_flight(&engine, "transcription");
        drop(engine.unload());
        assert!(
            engine.loaded().is_none(),
            "a new recording must see no model"
        );
        engine.load(spec()).unwrap();
        run.join().unwrap().unwrap();
        assert!(engine.loaded().is_some());
        assert_eq!(worker_count(), 1);
        engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap();
    }

    /// A crash that only happens on the GPU: the dictation succeeds on CPU,
    /// GPU_UNAVAILABLE is set, and later loads use CPU.
    #[test]
    #[ignore]
    fn gpu_only_crash_recovers_on_cpu() {
        let pcm = test_pcm();
        set_fault(Some("abort@run"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        assert!(info.on_gpu, "needs a GPU to test GPU-only faults");
        let transcript = engine
            .transcribe(pcm.clone(), RunOptions::default())
            .unwrap();
        println!("recovered on CPU: {}", transcript.text);
        assert!(Degraded::GpuUnavailable.is_set());
        assert!(!engine.loaded().unwrap().on_gpu);
        let info = engine.load(spec()).unwrap();
        assert!(
            !info.on_gpu,
            "loads stay on CPU while GPU_UNAVAILABLE is set"
        );
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// A GPU hang on a short clip is killed at the run deadline floor and
    /// retried on CPU, never on the same GPU.
    #[test]
    #[ignore]
    fn gpu_hang_retries_on_cpu() {
        let pcm = test_pcm()[..16_000].to_vec();
        set_fault(Some("hang@run"), true);
        let engine = EngineSupervisor::new(true);
        assert!(engine.load(spec()).unwrap().on_gpu);
        let t = Instant::now();
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        let elapsed = t.elapsed();
        println!("hung on GPU, recovered on CPU after {elapsed:?}");
        assert!(elapsed >= RUN_FLOOR && elapsed < RUN_FLOOR + CALL_FLOOR);
        assert!(Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
    }

    /// A crash on CPU too means the GPU was not at fault: GPU_UNAVAILABLE
    /// stays clear and the error is reported.
    #[test]
    #[ignore]
    fn crash_everywhere_is_not_a_gpu_failure() {
        let pcm = test_pcm();
        set_fault(Some("abort@run"), false);
        std::env::remove_var("HANDY_WORKER_FAULT_ONCE");
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let err = engine.transcribe(pcm, RunOptions::default()).unwrap_err();
        println!("failed as expected: {err}");
        assert!(!Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
    }

    /// Device listing crashes: the model loads in a CPU-only worker,
    /// GPU_UNAVAILABLE is set, and the failed list is not probed again.
    #[test]
    #[ignore]
    fn device_listing_crash_uses_cpu() {
        let pcm = test_pcm();
        set_fault(Some("abort@list"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        assert!(!info.on_gpu, "loaded on '{}'", info.backend);
        assert!(Degraded::GpuUnavailable.is_set());
        assert!(matches!(engine.devices(), DeviceList::Failed(_)));
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// A GPU driver that crashes while its backend registers, before the
    /// hello: only a CPU-only worker survives it, so that is where the model
    /// loads. GPU_UNAVAILABLE is set and later loads stay CPU-only.
    #[test]
    #[ignore]
    fn gpu_backend_init_crash_uses_cpu_only_worker() {
        let pcm = test_pcm();
        set_fault(Some("abort@init"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        assert!(!info.on_gpu, "loaded on '{}'", info.backend);
        assert!(Degraded::GpuUnavailable.is_set());
        assert!(matches!(engine.devices(), DeviceList::Failed(_)));
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        // The fault still fires in any worker with GPU backends.
        assert!(!engine.load(spec()).unwrap().on_gpu);
        set_fault(None, false);
    }

    /// An explicit device never moves to a CPU-only worker, where its
    /// registry index would mean something else: an init crash is reported.
    #[test]
    #[ignore]
    fn pinned_device_init_crash_fails_loudly() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let DeviceList::Known(devices) = engine.devices() else {
            panic!("devices not listed");
        };
        let gpu = devices.iter().find(|d| d.is_gpu()).expect("needs a GPU");
        set_fault(Some("abort@init"), true);
        assert!(engine
            .load(LoadSpec {
                device: DeviceSelector::Index(gpu.index.unwrap()),
                ..spec()
            })
            .is_err());
        assert!(!Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
    }

    /// With the CPU accelerator, the model worker is CPU-only (a GPU-only
    /// init fault never fires), and a device probe runs beside it rather
    /// than stopping it.
    #[test]
    #[ignore]
    fn cpu_setting_uses_cpu_only_worker_and_probe_leaves_it_running() {
        let pcm = test_pcm();
        set_fault(Some("abort@init"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine
            .load(LoadSpec {
                backend: Backend::Cpu,
                ..spec()
            })
            .unwrap();
        assert!(!info.on_gpu);
        assert!(!Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
        assert!(matches!(engine.devices(), DeviceList::Known(_)));
        assert_eq!(worker_count(), 1);
        assert!(engine.loaded().is_some());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
    }

    /// A host that must not use the GPU: even an `Auto` load and the device
    /// probe run CPU-only, so a GPU-only init fault never fires.
    #[test]
    #[ignore]
    fn gpu_disallowed_host_never_registers_a_gpu_backend() {
        set_fault(Some("abort@init"), true);
        let engine = EngineSupervisor::new(false);
        let DeviceList::Known(devices) = engine.devices() else {
            panic!("devices not listed");
        };
        assert!(devices.iter().all(|d| !d.is_gpu()), "{devices:?}");
        assert!(!engine.load(spec()).unwrap().on_gpu);
        assert!(!Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
    }

    /// The mask itself: a CPU-only worker registers no GPU device, including
    /// compiled-in ones such as Metal.
    #[test]
    #[ignore]
    fn cpu_only_worker_lists_no_gpu() {
        set_fault(None, false);
        let mut worker = Worker::spawn(true).unwrap();
        let Ok(Response::Hello {
            devices: Some(devices),
        }) = worker.call(&Request::Hello { list_devices: true }, None, HELLO_TIMEOUT)
        else {
            panic!("no device list");
        };
        println!("CPU-only worker devices: {devices:?}");
        assert!(!devices.is_empty());
        assert!(devices.iter().all(|d| !d.is_gpu()));
    }

    /// An explicit device never falls back to CPU.
    #[test]
    #[ignore]
    fn pinned_device_fails_loudly() {
        let pcm = test_pcm();
        set_fault(Some("abort@run"), true);
        let engine = EngineSupervisor::new(true);
        let DeviceList::Known(devices) = engine.devices() else {
            panic!("devices not listed");
        };
        let gpu = devices.iter().find(|d| d.is_gpu()).expect("needs a GPU");
        let info = engine
            .load(LoadSpec {
                device: DeviceSelector::Index(gpu.index.unwrap()),
                ..spec()
            })
            .unwrap();
        assert!(info.on_gpu);
        assert!(engine.transcribe(pcm, RunOptions::default()).is_err());
        assert!(!Degraded::GpuUnavailable.is_set());
        set_fault(None, false);
    }

    /// Cancel mid-run: the run stops promptly, without a retry, and the next
    /// run works (on the same worker if the family can cancel politely).
    #[test]
    #[ignore]
    fn cancel_stops_a_run() {
        let pcm = long_pcm(60);
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        let run = {
            let engine = engine.clone();
            let pcm = pcm.clone();
            thread::spawn(move || engine.transcribe(pcm, RunOptions::default()))
        };
        wait_in_flight(&engine, "transcription");
        let t = Instant::now();
        engine.cancel();
        let result = run.join().unwrap();
        println!(
            "cancelled {:?} after cancel() (polite: {})",
            t.elapsed(),
            info.supports_cancellation
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < CANCEL_GRACE * 2);
        assert!(engine.loaded().is_some());
        assert!(!Degraded::GpuUnavailable.is_set());
        let t = Instant::now();
        engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap();
        println!("next run took {:?}", t.elapsed());
    }

    /// Cancel a hung run: killed within the cancel grace, no retry, no GPU
    /// flag, and the next run starts a fresh worker.
    #[test]
    #[ignore]
    fn cancel_kills_a_hung_run() {
        let pcm = test_pcm();
        set_fault(Some("hang@run"), false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let run = {
            let engine = engine.clone();
            let pcm = pcm.clone();
            thread::spawn(move || engine.transcribe(pcm, RunOptions::default()))
        };
        wait_in_flight(&engine, "transcription");
        let t = Instant::now();
        engine.cancel();
        let result = run.join().unwrap();
        println!("hung run cancelled {:?} after cancel()", t.elapsed());
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < CANCEL_GRACE + Duration::from_secs(2));
        assert!(!Degraded::GpuUnavailable.is_set());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// A worker hung in native code still exits when its parent goes away
    /// (stdin closes), without being killed.
    #[test]
    #[ignore]
    fn hung_worker_exits_when_stdin_closes() {
        set_fault(Some("hang@run"), false);
        let mut worker = Worker::spawn(false).unwrap();
        worker
            .call(
                &Request::Hello {
                    list_devices: false,
                },
                None,
                HELLO_TIMEOUT,
            )
            .unwrap();
        let spec = spec();
        let load = Request::Load {
            path: spec.path,
            backend: spec.backend,
            device: spec.device,
        };
        worker.call(&load, None, LOAD_TIMEOUT).unwrap();
        let run = Request::Run {
            options: RunOptions::default(),
        };
        assert!(worker
            .control
            .send(encode_message(&run, Some(&test_pcm())).unwrap()));
        thread::sleep(Duration::from_secs(1));
        worker.control.close_stdin();
        let t = Instant::now();
        let status = loop {
            if let Some(status) = lock(&worker.control.child).try_wait().unwrap() {
                break status;
            }
            assert!(t.elapsed() < SHUTDOWN_GRACE, "worker outlived its stdin");
            thread::sleep(Duration::from_millis(10));
        };
        println!(
            "hung worker exited {:?} after stdin closed: {status}",
            t.elapsed()
        );
        assert!(status.success());
        set_fault(None, false);
    }
}
