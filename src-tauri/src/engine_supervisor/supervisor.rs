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
use log::{debug, error, info, warn, Level};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// Floor for stream calls that do no decoding (begin, reset).
const CALL_FLOOR: Duration = Duration::from_secs(30);
/// How long a worker gets to exit after its stdin closes before it's killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Native stderr lines kept for crash reports (a `GGML_ASSERT` message lands
/// here right before an abort).
const STDERR_TAIL_LINES: usize = 64;
const SAMPLE_RATE: f64 = 16_000.0;
/// Bound retries even if devices repeatedly appear during recovery.
const MAX_GPU_RECOVERY_ATTEMPTS: usize = 8;
/// Routine discovery is independent of per-device failure cooldowns.
const AUTO_DEVICE_RECHECK_INTERVAL: Duration = Duration::from_secs(60);
const GPU_RETRY_INITIAL: Duration = Duration::from_secs(30);
const GPU_RETRY_MAX: Duration = Duration::from_secs(300);

/// Per stable device identity, retained until successful inference. A model
/// load only starts a trial; it does not establish that the driver recovered.
struct GpuFailure {
    name: String,
    delay: Duration,
    retry_at: Instant,
    quarantined: bool,
}

impl From<&str> for GpuFailure {
    fn from(name: &str) -> Self {
        Self {
            name: name.to_string(),
            delay: GPU_RETRY_INITIAL,
            retry_at: Instant::now() + GPU_RETRY_INITIAL,
            quarantined: true,
        }
    }
}

impl GpuFailure {
    fn eligible(&self, now: Instant) -> bool {
        !self.quarantined || now >= self.retry_at
    }

    fn failed_again(&mut self, now: Instant) {
        // Duplicate failure reports must not restart or increase a cooldown.
        if !self.quarantined {
            self.delay = (self.delay * 2).min(GPU_RETRY_MAX);
            self.retry_at = now + self.delay;
            self.quarantined = true;
        }
    }
}

/// How long decoding work may take before its worker counts as hung, by the
/// device the model runs on. On a GPU a hang is recovered from (retried on
/// CPU), so it should be caught soon. On CPU a hang is never retried, so the
/// deadline only decides when to give up: a slow machine must not be
/// mistaken for a hung one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Deadlines {
    /// Floor for a stream feed or finalize, so sub-second chunks and
    /// first-use shader compiles are not mistaken for hangs.
    call_floor: Duration,
    /// Floor for a batch run. A run has a fixed cost however short the
    /// clip: Whisper always encodes a full 30 s window, which on CPU takes
    /// seconds even for a 1 s clip.
    run_floor: Duration,
    /// Work on N seconds of audio may take up to this many times N.
    audio_factor: f64,
}

const GPU_DEADLINES: Deadlines = Deadlines {
    call_floor: Duration::from_secs(30),
    run_floor: Duration::from_secs(60),
    audio_factor: 10.0,
};

/// A single CPU feed or finalize can take well over 10 s, and a large model
/// on an older CPU runs slower than real time.
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

    /// Deadline for stream work on `samples` of 16 kHz audio.
    fn audio(&self, samples: usize) -> Duration {
        self.call_floor.max(Duration::from_secs_f64(
            samples as f64 / SAMPLE_RATE * self.audio_factor,
        ))
    }

    /// Deadline for a batch run on `samples` of 16 kHz audio.
    fn run(&self, samples: usize) -> Duration {
        self.run_floor.max(self.audio(samples))
    }
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

/// See [`EngineSupervisor::cancel_mark`].
#[derive(Debug, Clone, Copy)]
pub struct CancelMark(u64);

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
type OnDeviceChanged = Box<dyn Fn(&LoadedInfo) + Send + Sync>;

enum Command {
    Load(LoadSpec, Reply<Result<LoadedInfo, EngineError>>),
    Unload(Reply<()>),
    RecheckGpu(Reply<Result<Option<LoadedInfo>, EngineError>>),
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
}

/// State readable without going through the queue.
#[derive(Default)]
struct Shared {
    /// Whether this host may use a GPU at all. When not, every worker is
    /// CPU-only, device probes included.
    gpu_allowed: bool,
    /// A GPU-backend failure could not be attributed to one device (e.g.
    /// backend initialization crashed). Models then load in CPU-only workers
    /// until an explicit accelerator change or restart; known devices
    /// fail independently.
    gpu_unavailable: AtomicBool,
    /// Quarantine individual GPU identities while other GPUs remain usable.
    failed_gpus: Mutex<HashMap<String, GpuFailure>>,
    gpu_recheck: Mutex<Option<Instant>>,
    fallback_active: AtomicBool,
    on_device_changed: Option<OnDeviceChanged>,
    /// The loaded model (logically: it stays loaded while its worker is
    /// replaced after a crash or cancel).
    loaded: Mutex<Option<LoadedInfo>>,
    /// The compute devices as last listed by a worker; `None` until one has.
    devices: Mutex<Option<Vec<DeviceInfo>>>,
    /// Held for the length of a device probe, so concurrent callers wait for
    /// the one probe instead of starting their own.
    probing: Mutex<()>,
    /// Bumped by every [`EngineSupervisor::cancel`]. Work queued before the
    /// bump counts as cancelled.
    cancel_epoch: AtomicU64,
    /// The call the owner is waiting on, if any. A cancel takes it and kills
    /// its worker.
    in_flight: Mutex<Option<InFlight>>,
    /// Bumped by every [`EngineSupervisor::unload`] as it is called, so the
    /// model reads as unloaded at once, even while the unload waits its turn.
    unloads_requested: AtomicU64,
    next_id: AtomicU64,
}

impl Shared {
    /// Start a model-less worker just to list devices. If it may use the GPU
    /// and crashes or hangs doing so, the GPU is marked unavailable.
    fn probe_devices(&self) {
        let cpu_only = !self.gpu_allowed;
        match self.hello(cpu_only, true) {
            Ok(worker) => drop(worker),
            Err(failure) => {
                error!(
                    "Could not start a transcription worker to list compute devices: {}",
                    failure
                );
                if failure.requires_restart() && !cpu_only {
                    self.mark_gpu_unavailable(format!(
                        "a worker with GPU backends {failure} while listing compute devices"
                    ));
                }
            }
        }
    }

    /// Start a worker and say hello, publishing its device list if it lists
    /// one.
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
        // The existing registry snapshot already records absence. Reuse it
        // to permit an early trial on return without separate per-device flags.
        if let Some(old) = current.as_deref() {
            for (key, failure) in lock(&self.failed_gpus).iter_mut() {
                if device_missing(key, Some(old)) && !device_missing(key, Some(&new)) {
                    failure.retry_at = Instant::now();
                }
            }
        }
        *current = Some(new);
    }

    fn gpu_unavailable(&self) -> bool {
        self.gpu_unavailable.load(Ordering::Acquire)
    }

    /// `reason` is logged when this changes the state.
    fn mark_gpu_unavailable(&self, reason: impl fmt::Display) {
        if !self.gpu_unavailable.swap(true, Ordering::AcqRel) {
            warn!("Transcription GPU marked unavailable: {}", reason);
        }
    }

    fn mark_gpu_failed(&self, key: &str, name: &str, reason: impl fmt::Display) -> bool {
        let mut failed = lock(&self.failed_gpus);
        let fresh = failed.get(key).is_none_or(|failure| !failure.quarantined);
        let failure = failed.entry(key.to_string()).or_insert_with(|| name.into());
        failure.failed_again(Instant::now());
        warn!(
            "Transcription GPU '{}' quarantined for {:?}: {}",
            name, failure.delay, reason
        );
        fresh
    }

    /// Reserve routine discovery without postponing another device's future
    /// retry. Already-expired, absent devices must not cause a scan on every
    /// dictation; they can be discovered again at the routine interval.
    fn next_gpu_check(&self, now: Instant) -> Instant {
        lock(&self.failed_gpus)
            .values()
            .filter(|failure| failure.quarantined && failure.retry_at > now)
            .map(|failure| failure.retry_at)
            .min()
            .unwrap_or(now + AUTO_DEVICE_RECHECK_INTERVAL)
            .min(now + AUTO_DEVICE_RECHECK_INTERVAL)
    }

    /// Admission is provisional until this exact device has loaded. Spawn or
    /// other pre-load errors may not be attributable to the GPU, and a fresh
    /// registry may select a fallback instead. Neither should let the unused
    /// candidate bypass quarantine on a later ordinary load.
    fn finish_gpu_trial_load(&self, key: &str, loaded_key: Option<&str>) {
        if loaded_key == Some(key) {
            return;
        }
        if let Some(failure) = lock(&self.failed_gpus).get_mut(key) {
            if !failure.quarantined {
                failure.quarantined = true;
                failure.retry_at = Instant::now() + failure.delay;
            }
        }
    }

    fn recovery_spec(&self, spec: &LoadSpec) -> LoadSpec {
        // Match set_devices' lock order: registry before failure state.
        let devices = lock(&self.devices);
        let failed = lock(&self.failed_gpus);
        // The first GPU-capable hello must discover the registry before a
        // selected device can be declared absent and replaced with strict CPU.
        if devices.is_none() && failed.is_empty() && !self.gpu_unavailable() {
            return spec.clone();
        }
        select_recovery_spec(
            spec,
            self.gpu_unavailable(),
            &failed,
            devices.as_deref().unwrap_or_default(),
        )
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
    #[cfg(test)]
    pub fn new(gpu_allowed: bool) -> Self {
        Self::with_device_changed_handler(gpu_allowed, |_| {})
    }

    /// Report each published device binding immediately. The handler runs on
    /// the owner thread and must not block or call back into the supervisor.
    pub fn with_device_changed_handler(
        gpu_allowed: bool,
        on_device_changed: impl Fn(&LoadedInfo) + Send + Sync + 'static,
    ) -> Self {
        #[cfg(all(unix, not(test)))]
        record_launched_exe();
        let (commands, queue) = mpsc::channel();
        let shared = Arc::new(Shared {
            gpu_allowed,
            on_device_changed: Some(Box::new(on_device_changed)),
            ..Shared::default()
        });
        let owner = Owner {
            shared: Arc::clone(&shared),
            worker: None,
            spec: None,
            stream: None,
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
    /// the GPU failure, if any, so the next load may use (and list) the GPU
    /// again. A new failure sets the flag again.
    pub fn retry_gpu(&self, reason: &str) {
        let had_device_failures = !lock(&self.shared.failed_gpus).is_empty();
        lock(&self.shared.failed_gpus).clear();
        *lock(&self.shared.gpu_recheck) = None;
        if self.shared.gpu_unavailable.swap(false, Ordering::AcqRel) || had_device_failures {
            info!("Transcription GPU no longer marked unavailable: {}", reason);
        }
    }

    /// The loaded model. A snapshot; never waits on the queue.
    pub fn loaded(&self) -> Option<LoadedInfo> {
        lock(&self.shared.loaded).clone()
    }

    pub fn is_in_fallback(&self) -> bool {
        self.shared.fallback_active.load(Ordering::Acquire)
    }

    /// Cheap time check: never enumerates hardware on the caller's thread.
    pub fn gpu_recheck_due(&self) -> bool {
        lock(&self.shared.gpu_recheck).is_some_and(|next| Instant::now() >= next)
    }

    /// Run between dictations on the engine owner, preserving the preference.
    pub fn recheck_gpu(&self) -> Result<Option<LoadedInfo>, EngineError> {
        self.ask(Command::RecheckGpu)
            .unwrap_or_else(|| Err(engine_stopped()))
    }

    pub fn failed_gpu_devices(&self) -> Vec<(String, String)> {
        let mut devices: Vec<_> = lock(&self.shared.failed_gpus)
            .iter()
            .filter(|(_, failure)| failure.quarantined)
            .map(|(key, failure)| (key.clone(), failure.name.clone()))
            .collect();
        devices.sort_by(|a, b| a.0.cmp(&b.0));
        devices
    }

    /// Transcribe 16 kHz mono PCM with the loaded model. A worker crash or
    /// hang replays the audio on another healthy GPU or CPU, with bounded retries.
    pub fn transcribe(&self, pcm: Vec<f32>, run: RunOptions) -> Result<Transcript, EngineError> {
        self.transcribe_since(self.cancel_mark(), pcm, run)
    }

    /// A point to cancel from: every run given it is cancelled by any
    /// [`cancel`](Self::cancel) after it was taken, so the runs that make up
    /// one transcription (its chunks) stop together, even between runs.
    pub fn cancel_mark(&self) -> CancelMark {
        CancelMark(self.shared.epoch())
    }

    /// [`transcribe`](Self::transcribe), cancelled by any cancel since `mark`.
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

    /// Begin a live stream on the loaded model. `on_progress` runs on the
    /// owner thread after every feed, so it must not block or call back into
    /// the supervisor.
    #[cfg(test)]
    pub fn start_stream(
        &self,
        run: RunOptions,
        stream: StreamOptions,
        on_progress: impl FnMut(StreamProgress) + Send + 'static,
    ) -> Result<StreamHandle, EngineError> {
        self.start_stream_with_failure_handler(run, stream, on_progress, || {})
    }

    /// Report a lost live stream once, so the UI can explain why preview
    /// stopped while recording continues. Callbacks must not block or reenter.
    pub fn start_stream_with_failure_handler(
        &self,
        run: RunOptions,
        stream: StreamOptions,
        on_progress: impl FnMut(StreamProgress) + Send + 'static,
        on_lost: impl FnMut() + Send + 'static,
    ) -> Result<StreamHandle, EngineError> {
        let id = self.shared.next_id();
        // The stream is one piece of cancellable work: a cancel from here on
        // stops its feeds and its finalize.
        let epoch = self.shared.epoch();
        let closed = Arc::new(AtomicBool::new(false));
        self.ask(|reply| Command::StreamBegin {
            active: ActiveStream {
                id,
                epoch,
                closed: Arc::clone(&closed),
                on_progress: Box::new(on_progress),
                on_lost: Box::new(on_lost),
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

    /// Stop the transcription or stream in progress, and any queued behind
    /// it. Bypasses the queue. The worker doing the work is killed, so the
    /// next use starts a fresh one and reloads the model. Model loads are
    /// never cancelled.
    pub fn cancel(&self) {
        self.shared.cancel_epoch.fetch_add(1, Ordering::SeqCst);
        // Taking the call tells the owner its worker was killed.
        if let Some(call) = lock(&self.shared.in_flight).take() {
            info!(
                "Cancelling the {} by stopping its worker (pid {})",
                call.what, call.control.pid
            );
            call.control.kill();
        }
    }

    /// The compute devices, as listed by the latest worker. If no worker has
    /// listed them yet, starts a short-lived worker on this thread to list
    /// them. It runs beside any model worker, which it leaves alone. Not
    /// while the GPU is marked unavailable: that probe would crash or hang
    /// the same way.
    pub fn devices(&self) -> Option<Vec<DeviceInfo>> {
        let _probing = lock(&self.shared.probing);
        if lock(&self.shared.devices).is_none() && !self.shared.gpu_unavailable() {
            self.shared.probe_devices();
        }
        lock(&self.shared.devices).clone()
    }
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
    /// Set when the handle is dropped; shared with the owner.
    closed: Arc<AtomicBool>,
    commands: mpsc::Sender<Command>,
    finished: bool,
}

impl StreamHandle {
    pub fn feed(&self, pcm: Vec<f32>) {
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
            self.closed.store(true, Ordering::Release);
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
    /// A request, model, or allocation error; the worker is still usable.
    Remote(String),
    /// The native context failed even though the worker could still reply.
    Backend(String),
    /// Not a device failure unless a fresh probe confirms device removal.
    Allocation(String),
    /// The worker exited or crashed. Carries exit status and its last
    /// native output.
    Crashed(String),
    /// The worker did not answer within the deadline and was killed.
    Hung(Duration),
    Cancelled,
}

impl Failure {
    /// A crashed, hung, or failed native context must be discarded.
    fn requires_restart(&self) -> bool {
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
            Failure::Remote(message) | Failure::Backend(message) | Failure::Allocation(message) => {
                f.write_str(message)
            }
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
    /// Set when the caller drops its handle.
    closed: Arc<AtomicBool>,
    on_progress: OnProgress,
    on_lost: Box<dyn FnMut() + Send>,
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
    /// Unload commands processed, against [`Shared::unloads_requested`].
    unloads_done: u64,
}

impl Owner {
    fn run(mut self, queue: mpsc::Receiver<Command>) {
        while let Ok(command) = queue.recv() {
            match command {
                Command::Load(spec, reply) => {
                    let _ = reply.send(self.load(spec));
                }
                Command::RecheckGpu(reply) => {
                    let _ = reply.send(self.recheck_gpu());
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
        // Idle unloading must not prevent a due retry on the next dictation.
        let result = match self.recheck_gpu() {
            Ok(Some(info)) => Ok(info),
            Ok(None) => self.start(),
            Err(error) => Err(error),
        };
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

    /// Keep the preference intact while choosing a healthy recovery device.
    fn effective_spec(&self, spec: &LoadSpec) -> LoadSpec {
        self.shared.recovery_spec(spec)
    }

    fn cpu_only_worker(&self, spec: &LoadSpec) -> bool {
        spec.cpu_only() || !self.shared.gpu_allowed
    }

    fn start_worker(
        &self,
        spec: &LoadSpec,
        reselect: bool,
    ) -> Result<(Worker, LoadedInfo), StartFailure> {
        let cpu_only = self.cpu_only_worker(spec);
        let mut worker =
            self.shared
                .hello(cpu_only, !cpu_only)
                .map_err(|failure| StartFailure {
                    failure,
                    gpu_worker: !cpu_only,
                    device: None,
                })?;
        // Re-select after hello: this registry belongs to the new process and
        // notices removed devices and new registry indices.
        let wanted = if cpu_only || spec.pinned() || !reselect {
            spec.clone()
        } else {
            self.effective_spec(spec)
        };
        if wanted.cpu_only() && !cpu_only {
            drop(worker);
            return self.start_worker(&wanted, reselect);
        }
        let device = match &wanted.device {
            DeviceSelector::Key(key) | DeviceSelector::RecoveryKey(key) => {
                lock(&self.shared.devices).as_ref().and_then(|devices| {
                    devices
                        .iter()
                        .find(|device| device.is_gpu() && &device.key == key)
                        .map(|device| (device.key.clone(), device.label().to_string()))
                })
            }
            _ => None,
        };
        let info = worker.load(&wanted).map_err(|failure| StartFailure {
            failure,
            gpu_worker: !cpu_only,
            device,
        })?;
        Ok((worker, info))
    }

    /// Each failed known GPU is excluded. A fresh worker tries another GPU,
    /// then a strictly CPU-only worker when no healthy GPU remains. A fault
    /// during backend initialization cannot be attributed to one GPU, so it
    /// retains the upstream CPU-only fallback.
    fn start(&mut self) -> Result<LoadedInfo, EngineError> {
        self.start_on(None)
    }

    fn start_on(&mut self, first_gpu: Option<&str>) -> Result<LoadedInfo, EngineError> {
        let spec = self.spec.clone().ok_or_else(not_loaded)?;
        let started = Instant::now();
        let mut gpu_attempts = 0;
        let (worker, info) = loop {
            let wanted = match first_gpu.filter(|_| gpu_attempts == 0) {
                Some(key) => LoadSpec {
                    path: spec.path.clone(),
                    backend: Backend::Auto,
                    device: DeviceSelector::RecoveryKey(key.to_string()),
                },
                None => self.effective_spec(&spec),
            };
            match self.start_worker(&wanted, true) {
                Ok(started) => break started,
                Err(StartFailure {
                    failure,
                    gpu_worker,
                    device,
                }) if gpu_worker
                    && !spec.pinned()
                    && (failure.requires_restart()
                        || (matches!(spec.device, DeviceSelector::Key(_))
                            && matches!(failure, Failure::Allocation(_)))) =>
                {
                    let isolated = device.as_ref().is_some_and(|device| {
                        gpu_attempts < MAX_GPU_RECOVERY_ATTEMPTS
                            && self.shared.mark_gpu_failed(&device.0, &device.1, &failure)
                    });
                    if !isolated {
                        self.shared.mark_gpu_unavailable(&failure);
                    }
                    gpu_attempts += 1;
                    warn!(
                        "Retrying model load on the next healthy device: {}",
                        failure
                    );
                }
                Err(error) => {
                    // An upgrade may have advanced beyond its original candidate.
                    // Preserve the identity of the GPU that actually failed,
                    // including ordinary errors such as exhausted VRAM.
                    if first_gpu.is_some() && error.gpu_worker {
                        if let Some((key, name)) = &error.device {
                            self.shared.mark_gpu_failed(key, name, &error.failure);
                        }
                    }
                    return Err(error.failure.into());
                }
            }
        };
        debug!(
            "Worker (pid {}) spawned and loaded the model in {}ms",
            worker.control.pid,
            started.elapsed().as_millis()
        );
        self.worker = Some(worker);
        if let Some(key) = first_gpu {
            // Finalize provisional admission before scheduling the next check.
            self.shared
                .finish_gpu_trial_load(key, info.device_key.as_deref());
        }
        self.update_gpu_recheck(&spec, &info);
        self.publish_loaded(&info);
        info!(
            "Transcription device bound to '{}' ({})",
            info.device, info.backend
        );
        Ok(info)
    }

    /// Compare the bound device with the original request, preserving the
    /// saved preference while a temporary replacement is used. Auto and
    /// fallbacks discover devices and schedule independent failure retries.
    fn update_gpu_recheck(&self, spec: &LoadSpec, info: &LoadedInfo) {
        let fallback = self.shared.gpu_allowed
            && !spec.cpu_only()
            && !spec.pinned()
            && match &spec.device {
                DeviceSelector::Key(key) | DeviceSelector::RecoveryKey(key) => {
                    !info.on_gpu || info.device_key.as_ref() != Some(key)
                }
                // Auto accepts any healthy GPU. A different device's blacklist
                // must not keep this binding in fallback or suppress upgrades.
                _ => !info.on_gpu,
            };
        self.shared
            .fallback_active
            .store(fallback, Ordering::Release);
        let mut recheck = lock(&self.shared.gpu_recheck);
        if self.shared.gpu_allowed
            && !spec.cpu_only()
            && !spec.pinned()
            && !self.shared.gpu_unavailable()
            && (fallback || matches!(spec.device, DeviceSelector::Auto))
        {
            let next = self.shared.next_gpu_check(Instant::now());
            *recheck = Some((*recheck).map_or(next, |previous| previous.min(next)));
        } else {
            *recheck = None;
        }
    }

    /// Only successful, nonempty inference resets this device's backoff.
    /// Success on a fallback must not forgive a different device's failures.
    fn confirm_device_success(&self) {
        if let Some(key) = self
            .worker
            .as_ref()
            .and_then(|worker| worker.info.as_ref())
            .filter(|info| info.on_gpu)
            .and_then(|info| info.device_key.as_ref())
        {
            lock(&self.shared.failed_gpus).remove(key);
        }
    }

    /// An optional upgrade must never take away a working fallback. Restore
    /// its exact identity first, then attempt a strictly CPU-only worker.
    /// This also covers allocation failures on an alternate GPU: retrying the
    /// normal candidate ranking could select that failing GPU again and lose
    /// the previously working CPU path.
    fn restore_fallback(
        &mut self,
        previous: Option<&LoadedInfo>,
    ) -> Result<LoadedInfo, EngineError> {
        let spec = self.spec.clone().ok_or_else(not_loaded)?;
        let previous_spec = previous
            .filter(|info| info.on_gpu)
            .and_then(|info| info.device_key.as_ref())
            .map(|key| LoadSpec {
                path: spec.path.clone(),
                backend: Backend::Auto,
                device: DeviceSelector::RecoveryKey(key.clone()),
            })
            .unwrap_or_else(|| spec.cpu());
        let restored = self.start_worker(&previous_spec, false).or_else(|error| {
            if previous_spec.cpu_only() {
                return Err(error);
            }
            if let Some((key, name)) = &error.device {
                self.shared.mark_gpu_failed(key, name, &error.failure);
            } else if error.failure.requires_restart() {
                self.shared.mark_gpu_unavailable(&error.failure);
            }
            warn!(
                "Previous GPU could not be restored; trying strict CPU: {}",
                error.failure
            );
            self.start_worker(&spec.cpu(), false)
        });
        match restored {
            Ok((worker, info)) => {
                self.worker = Some(worker);
                self.update_gpu_recheck(&spec, &info);
                self.publish_loaded(&info);
                Ok(info)
            }
            Err(error) => {
                self.spec = None;
                *lock(&self.shared.loaded) = None;
                Err(error.failure.into())
            }
        }
    }

    /// Probe at a safe boundary when discovery or a device retry is due.
    /// Cooldown expiry allows a trial even if removal was never observed.
    /// Fresh enumeration resolves stable device
    /// identities again because driver restarts can change registry indices.
    /// Backend initialization can be slow or hang, so the model-less probe is
    /// isolated with the usual timeout and cancellation protections. The existing
    /// model stays alive while probing; only a changed candidate replaces it,
    /// and at most one model is held during the actual switch.
    fn recheck_gpu(&mut self) -> Result<Option<LoadedInfo>, EngineError> {
        if self.stream.is_some()
            || !lock(&self.shared.gpu_recheck).is_some_and(|next| Instant::now() >= next)
        {
            return Ok(None);
        }
        let Some(spec) = self.spec.clone() else {
            return Ok(None);
        };
        if !self.shared.gpu_allowed
            || self.shared.gpu_unavailable()
            || spec.cpu_only()
            || spec.pinned()
        {
            *lock(&self.shared.gpu_recheck) = None;
            return Ok(None);
        }
        // Reserve discovery before touching the driver, even if probing fails.
        let next = self.shared.next_gpu_check(Instant::now());
        *lock(&self.shared.gpu_recheck) = Some(next);
        let epoch = self.shared.epoch();
        let probe = (|| {
            let _probing = lock(&self.shared.probing);
            let mut worker = Worker::spawn(false).map_err(Failure::Spawn)?;
            worker.call_cancellable(
                &self.shared,
                &Request::Hello { list_devices: true },
                None,
                HELLO_TIMEOUT,
                epoch,
            )
        })();
        let devices = match probe {
            Ok(Response::Hello {
                devices: Some(devices),
            }) => devices,
            Err(Failure::Cancelled) => return Err(EngineError::Cancelled),
            other => {
                warn!(
                    "Automatic GPU recheck failed; keeping current device: {:?}",
                    other
                );
                return Ok(None);
            }
        };
        self.shared.set_devices(devices.clone());
        let current = self.worker.as_ref().and_then(|worker| worker.info.as_ref());
        let eligible: Vec<_> = {
            let failed = lock(&self.shared.failed_gpus);
            devices
                .iter()
                .filter(|device| {
                    failed
                        .get(&device.key)
                        .is_none_or(|failure| failure.eligible(Instant::now()))
                })
                .cloned()
                .collect()
        };
        let Some(candidate) = upgrade_candidate(&spec, current, &eligible) else {
            return Ok(None);
        };
        let previous = current.cloned();
        let key = candidate.key.clone();
        let name = candidate.label().to_string();
        info!(
            "Automatically selecting available transcription GPU '{}'",
            name
        );
        // Admit only the selected device, not all expired quarantines. Normal
        // recovery still excludes other failed GPUs, even if their timers expired.
        if let Some(failure) = lock(&self.shared.failed_gpus).get_mut(&key) {
            failure.quarantined = false;
        }
        self.worker = None;
        match self.start_on(Some(&key)) {
            Ok(info) => {
                info!(
                    "Automatic GPU recheck bound '{}' ({})",
                    info.device, info.backend
                );
                Ok(Some(info))
            }
            Err(error) => {
                warn!(
                    "GPU upgrade failed; restoring the previous device: {}",
                    error
                );
                self.shared.finish_gpu_trial_load(&key, None);
                self.restore_fallback(previous.as_ref()).map(Some)
            }
        }
    }

    /// Report the model loaded, unless an unload was requested since and is
    /// still waiting in the queue.
    fn publish_loaded(&self, info: &LoadedInfo) -> bool {
        let mut loaded = lock(&self.shared.loaded);
        if self.shared.unloads_requested.load(Ordering::SeqCst) != self.unloads_done {
            return false;
        }
        *loaded = Some(info.clone());
        // Release the snapshot lock before invoking the UI status handler.
        drop(loaded);
        if let Some(handler) = &self.shared.on_device_changed {
            if catch_unwind(AssertUnwindSafe(|| handler(info))).is_err() {
                error!("Transcription device status callback panicked");
            }
        }
        true
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

    /// Quarantine the bound GPU without disabling other healthy GPUs.
    fn worker_lost(&mut self, during: &str, failure: &Failure) -> bool {
        let info = self.worker.take().and_then(|worker| worker.info.clone());
        let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
        match info {
            Some(info) if info.on_gpu => {
                if !pinned {
                    let reason = format!(
                        "{failure} during {during} on '{}' ({})",
                        info.device, info.backend
                    );
                    if let Some(key) = &info.device_key {
                        self.shared.mark_gpu_failed(key, &info.device, &reason);
                    } else {
                        self.shared.mark_gpu_unavailable(&reason);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Deadlines for the device the current worker's model runs on.
    fn deadlines(&self) -> Deadlines {
        let on_gpu = self
            .worker
            .as_ref()
            .and_then(|worker| worker.info.as_ref())
            .is_some_and(|info| info.on_gpu);
        Deadlines::for_device(on_gpu)
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
        let result = worker.call_cancellable(&self.shared, request, pcm, deadline, epoch);
        if matches!(result, Err(Failure::Cancelled)) {
            // A cancel may have killed the worker just after its reply.
            self.worker = None;
            return result;
        }
        match result {
            Err(Failure::Allocation(message)) if self.bound_gpu_removed(epoch)? => {
                Err(Failure::Backend(message))
            }
            other => other,
        }
    }

    /// A dead GPU can report OOM instead of a backend failure. Never blame
    /// ordinary memory pressure: only a successful fresh enumeration that
    /// no longer contains the bound device confirms removal. The probe is
    /// isolated and cancellable just like inference; it never loads a model.
    fn bound_gpu_removed(&self, epoch: u64) -> Result<bool, Failure> {
        if self.spec.as_ref().is_some_and(LoadSpec::pinned) {
            return Ok(false);
        }
        let Some(info) = self.worker.as_ref().and_then(|worker| worker.info.as_ref()) else {
            return Ok(false);
        };
        let Some(key) = info.device_key.as_deref().filter(|_| info.on_gpu) else {
            return Ok(false);
        };
        let probe = (|| {
            let mut worker = Worker::spawn(false).map_err(Failure::Spawn)?;
            worker.call_cancellable(
                &self.shared,
                &Request::Hello { list_devices: true },
                None,
                HELLO_TIMEOUT,
                epoch,
            )
        })();
        if self.shared.cancelled_since(epoch) {
            return Err(Failure::Cancelled);
        }
        match probe {
            Ok(Response::Hello {
                devices: Some(devices),
            }) => {
                let missing = device_missing(key, Some(&devices));
                self.shared.set_devices(devices);
                Ok(missing)
            }
            Err(Failure::Cancelled) => Err(Failure::Cancelled),
            other => {
                warn!(
                    "Could not confirm GPU removal after allocation failure: {:?}",
                    other
                );
                Ok(false)
            }
        }
    }

    /// Replay captured audio on the remaining GPUs, then CPU. Bound GPU
    /// attempts even if new devices keep appearing, and retry a CPU crash
    /// only once. Never retry a CPU hang or an explicitly pinned device.
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
        let mut gpu_failures = 0;
        let mut retried_cpu = false;
        loop {
            if self.shared.cancelled_since(epoch) {
                return Err(EngineError::Cancelled);
            }
            // A respawn here runs under the load deadline; the run's own
            // deadline starts only once the model is loaded.
            self.ensure_worker()?;
            let deadline = self.deadlines().run(pcm.len());
            match self.call_work(&request, Some(pcm), deadline, epoch) {
                Ok(Response::Transcript(transcript)) => {
                    if !transcript.text.trim().is_empty() {
                        self.confirm_device_success();
                    }
                    return Ok(transcript);
                }
                Ok(other) => return Err(unexpected(&other).into()),
                Err(failure) if failure.requires_restart() => {
                    let on_gpu = self.worker_lost("transcription", &failure);
                    let pinned = self.spec.as_ref().is_some_and(LoadSpec::pinned);
                    let hung_on_cpu = matches!(failure, Failure::Hung(_)) && !on_gpu;
                    if pinned || hung_on_cpu {
                        return Err(failure.into());
                    }
                    if on_gpu {
                        gpu_failures += 1;
                        if gpu_failures > MAX_GPU_RECOVERY_ATTEMPTS {
                            return Err(failure.into());
                        }
                        if gpu_failures == MAX_GPU_RECOVERY_ATTEMPTS {
                            self.shared
                                .mark_gpu_unavailable("GPU transcription retry limit reached");
                        }
                    } else {
                        if retried_cpu {
                            return Err(failure.into());
                        }
                        retried_cpu = true;
                    }
                    warn!("Retrying the transcription in a fresh worker");
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
                if failure.requires_restart() {
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
        // Skipped once the caller dropped its handle or cancelled.
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
            Ok(other) => warn!("Stream feed failed: {}", unexpected(&other)),
            Err(Failure::Remote(message) | Failure::Allocation(message)) => {
                warn!("Stream feed failed: {}", message)
            }
            // Later feeds are skipped, and finalize reports the cancel.
            Err(Failure::Cancelled) => {}
            Err(failure) => {
                let lost = self.stream.take();
                self.worker_lost("stream feed", &failure);
                if let Some(mut stream) = lost {
                    // Cancelled/closed recordings must not receive stale UI events.
                    if !stream.closed.load(Ordering::Acquire)
                        && !self.shared.cancelled_since(stream.epoch)
                        && catch_unwind(AssertUnwindSafe(|| (stream.on_lost)())).is_err()
                    {
                        error!("Live transcription failure callback panicked");
                    }
                }
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
        let deadline = self.deadlines().audio(ms_to_samples(stream.pending_ms));
        let started = Instant::now();
        match self.call_work(&Request::Finalize { want_language }, None, deadline, epoch) {
            Ok(Response::Finalized {
                update,
                text,
                language,
            }) => {
                if !text.full.trim().is_empty() {
                    self.confirm_device_success();
                }
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
            Err(Failure::Remote(message) | Failure::Allocation(message)) => {
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

#[derive(Debug)]
struct StartFailure {
    failure: Failure,
    gpu_worker: bool,
    device: Option<(String, String)>,
}

/// Shared system RAM is not comparable to dedicated VRAM. Match upstream's
/// preference for discrete GPUs before using memory to order peers. Use the
/// backend's GPU/IGPU classification rather than vendor names. This is a
/// selection heuristic, not a performance benchmark; stable keys break ties
/// in the callers so re-enumeration order does not choose the winner.
fn gpu_rank(device: &DeviceInfo) -> (bool, u64) {
    (
        device.device_type == transcribe_cpp::DeviceType::Gpu,
        device.memory_total,
    )
}

/// A named selection allows only that GPU. Auto orders by device class,
/// memory, then stable identity (never transient registry order).
fn best_gpu<'a>(
    spec: &LoadSpec,
    devices: impl Iterator<Item = &'a DeviceInfo>,
) -> Option<&'a DeviceInfo> {
    let preferred = match &spec.device {
        DeviceSelector::Key(key) | DeviceSelector::RecoveryKey(key) => Some(key),
        _ => None,
    };
    devices
        .filter(|device| device.is_gpu() && preferred.is_none_or(|key| key == &device.key))
        .max_by(|a, b| {
            gpu_rank(a)
                .cmp(&gpu_rank(b))
                .then_with(|| a.key.cmp(&b.key))
        })
}

/// Keep the requested preference intact while selecting a temporary worker.
/// Exclude each failed GPU independently, so a failed integrated GPU can fall
/// back to a healthy dedicated GPU (or vice versa) before strict CPU is needed.
fn select_recovery_spec(
    spec: &LoadSpec,
    gpu_unavailable: bool,
    failed: &HashMap<String, GpuFailure>,
    devices: &[DeviceInfo],
) -> LoadSpec {
    if spec.pinned() || spec.cpu_only() {
        return spec.clone();
    }
    if gpu_unavailable {
        return spec.cpu();
    }
    if failed.is_empty() && matches!(spec.device, DeviceSelector::Auto) {
        return spec.clone();
    }
    let selected = best_gpu(
        spec,
        devices.iter().filter(|device| {
            failed
                .get(&device.key)
                .is_none_or(|failure| !failure.quarantined)
        }),
    );
    match selected {
        Some(device) => LoadSpec {
            path: spec.path.clone(),
            backend: Backend::Auto,
            device: DeviceSelector::RecoveryKey(device.key.clone()),
        },
        None => spec.cpu(),
    }
}

/// Restore only the named GPU, or upgrade Auto to a stronger GPU. Never
/// reload an already active device just because another scan was due.
fn upgrade_candidate<'a>(
    spec: &LoadSpec,
    current: Option<&LoadedInfo>,
    devices: &'a [DeviceInfo],
) -> Option<&'a DeviceInfo> {
    let candidate = best_gpu(spec, devices.iter())?;
    if let Some(current) = current.filter(|info| info.on_gpu) {
        if current.device_key.as_ref() == Some(&candidate.key) {
            return None;
        }
        // Do not downgrade a working fallback GPU based on reindexing or a
        // driver temporarily reporting a smaller set of devices.
        if matches!(spec.device, DeviceSelector::Auto) {
            let active = devices
                .iter()
                .find(|device| Some(&device.key) == current.device_key.as_ref());
            if active.is_some_and(|active| gpu_rank(active) >= gpu_rank(candidate)) {
                return None;
            }
        }
    }
    Some(candidate)
}

/// A failed or skipped enumeration cannot prove a device was removed.
fn device_missing(key: &str, devices: Option<&[DeviceInfo]>) -> bool {
    devices.is_some_and(|devices| !devices.iter().any(|device| device.key == key))
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
}

impl Worker {
    /// Send and publish the call under the same lock cancellation takes, so
    /// cancellation cannot miss a just-started model call or device probe.
    fn call_cancellable(
        &mut self,
        shared: &Shared,
        request: &Request,
        pcm: Option<&[f32]>,
        deadline: Duration,
        epoch: u64,
    ) -> Result<Response, Failure> {
        let frame = encode_request(request, pcm)?;
        let sent = {
            // Check, send and publish under the lock cancel() takes after
            // bumping the epoch: either this sees the cancel and sends
            // nothing, or the cancel sees the call and kills its worker.
            let mut in_flight = lock(&shared.in_flight);
            if shared.cancelled_since(epoch) {
                return Err(Failure::Cancelled);
            }
            let sent = self.control.send(frame);
            *in_flight = Some(InFlight {
                what: request_name(request),
                control: Arc::clone(&self.control),
            });
            sent
        };
        let result = if sent {
            self.wait(request, deadline)
        } else {
            Err(self.died())
        };
        // Gone if cancel() took the call, which means it killed the worker
        // (possibly just after it answered).
        let killed = lock(&shared.in_flight).take().is_none();
        if killed || shared.cancelled_since(epoch) {
            return Err(Failure::Cancelled);
        }
        result
    }

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
        let exe = worker_exe()?;
        let mut command = ProcessCommand::new(exe);
        #[cfg(target_os = "linux")]
        if let Some(arg0) = std::env::args_os().next() {
            // Keep `ps` showing Handy's own path rather than /proc/self/exe.
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
        #[cfg(test)]
        LIVE_WORKERS.fetch_add(1, Ordering::SeqCst);
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
                // Split on raw bytes: native code may write non-UTF-8 (e.g. a
                // path in the Windows ANSI code page), and `lines()` would end
                // the loop there, losing every later line, abort message included.
                for line in BufReader::new(stderr).split(b'\n') {
                    let Ok(line) = line else { break };
                    let line = String::from_utf8_lossy(&line);
                    let line = line.strip_suffix('\r').unwrap_or(&line).to_string();
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
            Ok(Response::BackendError(message)) => Err(Failure::Backend(message)),
            Ok(Response::AllocationError(message)) => Err(Failure::Allocation(message)),
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
            match wait_exit(&mut child, Duration::from_secs(1)) {
                Some(status) => status.to_string(),
                None => {
                    let _ = child.kill();
                    "unresponsive after closing its output; killed".to_string()
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
        let mut child = lock(&self.control.child);
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

/// Wait up to `timeout` for `child` to exit. `None` if it is still running
/// (or its status can't be read).
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

/// Worker processes started and not yet reaped. Every worker is reaped when
/// its handle drops, so this is exactly the worker processes still alive.
#[cfg(test)]
static LIVE_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The file Handy was started from, with its identity (device and inode),
/// recorded at startup; see [`worker_exe`]. `None` if either couldn't be
/// read, which skips the check.
#[cfg(all(unix, not(test)))]
static LAUNCHED_EXE: std::sync::OnceLock<Option<(PathBuf, (u64, u64))>> =
    std::sync::OnceLock::new();

#[cfg(all(unix, not(test)))]
fn file_id(path: &std::path::Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

/// Must run before an upgrade can replace the file, so at startup.
#[cfg(all(unix, not(test)))]
fn record_launched_exe() {
    LAUNCHED_EXE.get_or_init(|| {
        std::env::current_exe()
            .and_then(|path| {
                let id = file_id(&path)?;
                Ok((path, id))
            })
            .inspect_err(|e| warn!("Could not identify Handy's executable: {}", e))
            .ok()
    });
}

/// The executable a worker runs: always this very binary, so the two agree
/// on the protocol and on transcribe.cpp's backend libraries. Upgrading or
/// moving Handy while it runs replaces or removes the file it started from,
/// and a worker started from there would be the new version (macOS), or this
/// binary loading the new version's backend libraries (Linux). Only a
/// restart fixes that, so refuse to start one and say so. Windows locks a
/// running executable against replacement. On Linux the worker runs from
/// `/proc/self/exe`, which is this binary even once its file is replaced.
#[cfg(not(test))]
fn worker_exe() -> io::Result<PathBuf> {
    #[cfg(unix)]
    if let Some((path, launched)) = LAUNCHED_EXE.get().and_then(Option::as_ref) {
        if file_id(path).ok().as_ref() != Some(launched) {
            return Err(io::Error::other(
                "Handy was updated or moved while it was running; restart Handy",
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

    fn loaded(key: Option<&str>) -> LoadedInfo {
        LoadedInfo {
            arch: "test".into(),
            variant: "test".into(),
            backend: "test".into(),
            device: key.unwrap_or("CPU").into(),
            device_key: key.map(str::to_string),
            on_gpu: key.is_some(),
            supports_initial_prompt: false,
            supports_long_form: false,
            supports_vocabulary: false,
            capabilities: transcribe_cpp::Capabilities {
                native_sample_rate: 16000,
                languages: vec![],
                translate_target_languages: vec![],
                max_timestamp_kind: transcribe_cpp::TimestampKind::None,
                supports_language_detect: false,
                supports_translate: false,
                supports_streaming: false,
                supports_spec_decode: false,
                max_audio_ms: 0,
            },
        }
    }

    fn policy_owner() -> Owner {
        Owner {
            shared: Arc::new(Shared {
                gpu_allowed: true,
                ..Shared::default()
            }),
            worker: None,
            spec: None,
            stream: None,
            unloads_done: 0,
        }
    }

    #[test]
    fn discovery_preserves_other_devices_retry_deadlines() {
        let shared = Shared::default();
        shared.mark_gpu_failed("absent", "Absent GPU", "lost");
        shared.mark_gpu_failed("returning", "Returning GPU", "lost");
        let now = Instant::now();
        let due = now + Duration::from_secs(20);
        {
            let mut failed = lock(&shared.failed_gpus);
            failed.get_mut("absent").unwrap().retry_at = now;
            failed.get_mut("returning").unwrap().retry_at = due;
        }
        assert_eq!(
            shared.next_gpu_check(now),
            due,
            "an expired/missing device must not postpone another GPU by 60s"
        );
        lock(&shared.failed_gpus)
            .get_mut("returning")
            .unwrap()
            .quarantined = false;
        assert_eq!(
            shared.next_gpu_check(now),
            now + AUTO_DEVICE_RECHECK_INTERVAL,
            "expired or admitted devices must not cause continuous scans"
        );
    }

    #[test]
    fn an_unloaded_trial_candidate_cannot_bypass_quarantine() {
        let shared = Shared::default();
        shared.set_devices(vec![gpu("candidate", 12), gpu("fallback", 6)]);
        shared.mark_gpu_failed("candidate", "Candidate GPU", "lost");
        for loaded_key in [None, Some("fallback")] {
            lock(&shared.failed_gpus)
                .get_mut("candidate")
                .unwrap()
                .quarantined = false;
            shared.finish_gpu_trial_load("candidate", loaded_key);
            assert_eq!(
                shared.recovery_spec(&auto_spec()).device,
                DeviceSelector::RecoveryKey("fallback".into())
            );
            assert_eq!(
                lock(&shared.failed_gpus)["candidate"].delay,
                GPU_RETRY_INITIAL,
                "an unattempted load is not another attributed GPU failure"
            );
        }
        lock(&shared.failed_gpus)
            .get_mut("candidate")
            .unwrap()
            .quarantined = false;
        shared.finish_gpu_trial_load("candidate", Some("candidate"));
        assert!(!lock(&shared.failed_gpus)["candidate"].quarantined);
        assert!(
            lock(&shared.failed_gpus).contains_key("candidate"),
            "loading still must not reset the inference failure history"
        );
        shared.mark_gpu_failed("candidate", "Candidate GPU", "trial failed");
        let retry_at = lock(&shared.failed_gpus)["candidate"].retry_at;
        shared.finish_gpu_trial_load("candidate", None);
        assert_eq!(
            lock(&shared.failed_gpus)["candidate"].retry_at,
            retry_at,
            "cleanup must not postpone or double an already recorded failure"
        );
    }

    #[test]
    fn missed_disconnect_still_allows_a_trial_after_cooldown() {
        let shared = Shared::default();
        let devices = vec![gpu("preferred", 12), gpu("fallback", 6)];
        shared.set_devices(devices.clone());
        shared.mark_gpu_failed("preferred", "Preferred GPU", "device lost");
        let retry_at = lock(&shared.failed_gpus)["preferred"].retry_at;
        // The registry always lists both devices: no absent sample is required.
        shared.set_devices(devices);
        let failed = lock(&shared.failed_gpus);
        assert!(!failed["preferred"].eligible(retry_at - Duration::from_nanos(1)));
        assert!(failed["preferred"].eligible(retry_at));
        drop(failed);
        // Expiry permits a deliberate upgrade, not accidental fallback retries.
        assert_eq!(
            shared.recovery_spec(&auto_spec()).device,
            DeviceSelector::RecoveryKey("fallback".into())
        );
    }

    #[test]
    fn device_backoffs_are_independent_and_capped() {
        let shared = Shared::default();
        shared.set_devices(vec![gpu("unstable", 12), gpu("other", 6)]);
        assert!(shared.mark_gpu_failed("unstable", "Unstable GPU", "first failure"));
        let first = lock(&shared.failed_gpus)["unstable"].retry_at;
        assert!(!shared.mark_gpu_failed("unstable", "Unstable GPU", "duplicate report"));
        assert_eq!(lock(&shared.failed_gpus)["unstable"].retry_at, first);
        for seconds in [60, 120, 240, 300, 300] {
            lock(&shared.failed_gpus)
                .get_mut("unstable")
                .unwrap()
                .quarantined = false;
            let before = Instant::now();
            assert!(shared.mark_gpu_failed("unstable", "Unstable GPU", "trial failed"));
            let failed = lock(&shared.failed_gpus);
            assert_eq!(failed["unstable"].delay, Duration::from_secs(seconds));
            assert!(failed["unstable"].retry_at >= before + Duration::from_secs(seconds));
        }
        shared.mark_gpu_failed("other", "Other GPU", "independent failure");
        let failed = lock(&shared.failed_gpus);
        assert_eq!(failed["other"].delay, GPU_RETRY_INITIAL);
        assert_eq!(failed["unstable"].delay, GPU_RETRY_MAX);
    }

    #[test]
    fn retry_admits_only_one_device_and_survives_idle_unload() {
        let mut owner = policy_owner();
        owner
            .shared
            .set_devices(vec![gpu("preferred", 12), gpu("other", 6)]);
        owner
            .shared
            .mark_gpu_failed("preferred", "Preferred GPU", "lost");
        owner.shared.mark_gpu_failed("other", "Other GPU", "lost");
        {
            let mut failed = lock(&owner.shared.failed_gpus);
            let selected = failed.get_mut("preferred").unwrap();
            selected.retry_at = Instant::now();
            selected.quarantined = false;
        }
        owner.unload();
        assert_eq!(
            owner.shared.recovery_spec(&auto_spec()).device,
            DeviceSelector::RecoveryKey("preferred".into())
        );
        assert_eq!(
            lock(&owner.shared.failed_gpus).len(),
            2,
            "loading/unloading is not inference success"
        );
        assert!(lock(&owner.shared.failed_gpus)["other"].quarantined);
    }

    #[test]
    fn observed_return_preserves_a_longer_device_backoff() {
        let shared = Shared::default();
        let devices = vec![gpu("unstable", 12), gpu("healthy", 6)];
        shared.set_devices(devices.clone());
        shared.mark_gpu_failed("unstable", "Unstable GPU", "lost");
        lock(&shared.failed_gpus)
            .get_mut("unstable")
            .unwrap()
            .quarantined = false;
        shared.mark_gpu_failed("unstable", "Unstable GPU", "trial failed");
        shared.set_devices(vec![devices[1].clone()]);
        shared.set_devices(devices);
        let failed = lock(&shared.failed_gpus);
        assert!(failed["unstable"].eligible(Instant::now()));
        assert_eq!(failed["unstable"].delay, Duration::from_secs(60));
    }

    #[test]
    fn discovery_is_throttled_without_postponing_the_next_scan() {
        let owner = policy_owner();
        let spec = auto_spec();
        let info = loaded(Some("healthy"));
        let before = Instant::now();
        owner.update_gpu_recheck(&spec, &info);
        let first = lock(&owner.shared.gpu_recheck).unwrap();
        assert!(first >= before + AUTO_DEVICE_RECHECK_INTERVAL);
        owner.update_gpu_recheck(&spec, &info);
        assert_eq!(*lock(&owner.shared.gpu_recheck), Some(first));
    }

    #[test]
    fn repeated_enumeration_does_not_clear_a_device_cooldown() {
        let shared = Shared::default();
        let devices = vec![gpu("failed", 12), gpu("healthy", 6)];
        shared.set_devices(devices.clone());
        shared.mark_gpu_failed("failed", "Failed GPU", "device lost");
        for _ in 0..5 {
            shared.set_devices(devices.clone());
            assert_eq!(
                shared.recovery_spec(&auto_spec()).device,
                DeviceSelector::RecoveryKey("healthy".into())
            );
        }
    }

    #[test]
    fn observed_removal_and_return_allows_an_early_trial() {
        let shared = Shared::default();
        let devices = vec![gpu("returning", 12), gpu("unstable", 6)];
        shared.set_devices(devices.clone());
        shared.mark_gpu_failed("returning", "Returning GPU", "removed");
        shared.mark_gpu_failed("unstable", "Unstable GPU", "driver fault");
        shared.set_devices(vec![devices[1].clone()]);
        assert!(lock(&shared.failed_gpus).contains_key("returning"));
        shared.set_devices(devices);
        assert!(lock(&shared.failed_gpus)["returning"].eligible(Instant::now()));
        assert!(lock(&shared.failed_gpus).contains_key("unstable"));
        // Becoming eligible does not admit it to ordinary fallback selection.
        assert!(shared.recovery_spec(&auto_spec()).cpu_only());
    }

    #[test]
    fn removal_confirmed_before_failure_attribution_still_allows_return() {
        let shared = Shared::default();
        shared.set_devices(vec![gpu("healthy", 6)]);
        shared.mark_gpu_failed("removed", "Removed GPU", "allocation on a removed GPU");
        shared.set_devices(vec![gpu("removed", 12), gpu("healthy", 6)]);
        assert!(lock(&shared.failed_gpus)["removed"].eligible(Instant::now()));
    }

    #[test]
    fn named_gpu_absence_stays_on_cpu_despite_other_available_gpus() {
        let selected = LoadSpec {
            device: DeviceSelector::Key("selected".into()),
            ..auto_spec()
        };
        let devices = vec![gpu("other", 24)];
        assert!(select_recovery_spec(&selected, false, &HashMap::new(), &devices).cpu_only());
        assert!(upgrade_candidate(&selected, Some(&loaded(None)), &devices).is_none());
        assert_eq!(
            upgrade_candidate(&auto_spec(), Some(&loaded(None)), &devices)
                .unwrap()
                .key,
            "other"
        );
        assert_eq!(selected.device, DeviceSelector::Key("selected".into()));
        let shared = Shared::default();
        assert_eq!(
            shared.recovery_spec(&selected).device,
            selected.device,
            "an unknown registry must first be discovered"
        );
    }

    #[test]
    fn recovery_scales_to_multiple_devices_without_vendor_assumptions() {
        let preferred = LoadSpec {
            device: DeviceSelector::Key("chosen".into()),
            ..auto_spec()
        };
        let devices = vec![
            gpu("chosen", 4),
            gpu("integrated", 2),
            gpu("external", 24),
            gpu("discrete", 12),
        ];
        let mut failed = HashMap::from([("chosen".into(), "Chosen GPU".into())]);
        for key in ["external", "discrete", "integrated"] {
            let next = select_recovery_spec(&auto_spec(), false, &failed, &devices);
            assert_eq!(next.device, DeviceSelector::RecoveryKey(key.into()));
            failed.insert(key.into(), key.into());
        }
        assert!(select_recovery_spec(&auto_spec(), false, &failed, &devices).cpu_only());
        // Recovery upgrades to the saved choice even when another device is larger.
        assert_eq!(
            upgrade_candidate(&preferred, Some(&loaded(None)), &devices)
                .unwrap()
                .key,
            "chosen"
        );
        let missing = LoadSpec {
            device: DeviceSelector::Key("absent".into()),
            ..auto_spec()
        };
        assert!(upgrade_candidate(&missing, Some(&loaded(None)), &devices).is_none());
        assert_eq!(preferred.device, DeviceSelector::Key("chosen".into()));
    }

    #[test]
    fn automatic_upgrade_prefers_saved_gpu_and_never_reloads_the_active_gpu() {
        let preferred = LoadSpec {
            device: DeviceSelector::Key("preferred_gpu".into()),
            ..auto_spec()
        };
        let devices = vec![gpu("alternate_gpu", 12), gpu("preferred_gpu", 6)];
        assert_eq!(
            upgrade_candidate(&preferred, Some(&loaded(Some("alternate_gpu"))), &devices)
                .unwrap()
                .key,
            "preferred_gpu"
        );
        assert!(
            upgrade_candidate(&preferred, Some(&loaded(Some("preferred_gpu"))), &devices).is_none()
        );
        assert!(upgrade_candidate(
            &preferred,
            Some(&loaded(Some("alternate_gpu"))),
            &devices[..1]
        )
        .is_none());
        assert!(
            upgrade_candidate(&auto_spec(), Some(&loaded(Some("alternate_gpu"))), &devices)
                .is_none(),
            "Auto must not downgrade a healthy GPU"
        );
        // Shared system memory must not outrank dedicated VRAM. The explicit
        // preference still wins when the user deliberately chooses the iGPU.
        let mut integrated = gpu("integrated", 32);
        integrated.device_type = transcribe_cpp::DeviceType::Igpu;
        let devices = vec![integrated, gpu("dedicated", 6)];
        let failed = HashMap::from([("unavailable".into(), "Unavailable GPU".into())]);
        assert_eq!(
            upgrade_candidate(&auto_spec(), Some(&loaded(Some("integrated"))), &devices)
                .unwrap()
                .key,
            "dedicated"
        );
        assert_eq!(
            select_recovery_spec(&auto_spec(), false, &failed, &devices).device,
            DeviceSelector::RecoveryKey("dedicated".into())
        );
        let explicit = LoadSpec {
            device: DeviceSelector::Key("integrated".into()),
            ..auto_spec()
        };
        assert_eq!(
            select_recovery_spec(&explicit, false, &failed, &devices).device,
            DeviceSelector::RecoveryKey("integrated".into())
        );
    }

    #[test]
    fn fallback_status_and_discovery_clear_when_preferred_device_returns() {
        let owner = policy_owner();
        let preferred = LoadSpec {
            device: DeviceSelector::Key("preferred_gpu".into()),
            ..auto_spec()
        };
        owner.update_gpu_recheck(&preferred, &loaded(Some("alternate_gpu")));
        assert!(owner.shared.fallback_active.load(Ordering::Acquire));
        assert!(lock(&owner.shared.gpu_recheck).is_some());
        owner
            .shared
            .mark_gpu_failed("alternate_gpu", "Alternate GPU", "old failure");
        owner.update_gpu_recheck(&preferred, &loaded(Some("preferred_gpu")));
        assert!(!owner.shared.fallback_active.load(Ordering::Acquire));
        assert!(
            lock(&owner.shared.gpu_recheck).is_none(),
            "unrelated failure history must not keep polling"
        );
    }

    #[test]
    fn backend_initialization_failure_requires_an_explicit_retry() {
        let owner = policy_owner();
        owner
            .shared
            .mark_gpu_unavailable("backend initialization crashed");
        owner.update_gpu_recheck(&auto_spec(), &loaded(None));
        assert!(lock(&owner.shared.gpu_recheck).is_none());
        assert!(owner.shared.recovery_spec(&auto_spec()).cpu_only());
    }

    #[test]
    fn healthy_auto_gpu_clears_fallback_without_releasing_failed_devices() {
        let owner = policy_owner();
        owner
            .shared
            .mark_gpu_failed("unstable", "Unstable GPU", "driver fault");
        owner.update_gpu_recheck(&auto_spec(), &loaded(None));
        assert!(owner.shared.fallback_active.load(Ordering::Acquire));
        owner.update_gpu_recheck(&auto_spec(), &loaded(Some("healthy")));
        assert!(!owner.shared.fallback_active.load(Ordering::Acquire));
        assert!(lock(&owner.shared.failed_gpus).contains_key("unstable"));
        assert!(lock(&owner.shared.gpu_recheck).is_some());
    }

    #[test]
    fn explicit_cpu_and_gpu_disabled_hosts_never_schedule_gpu_rechecks() {
        let owner = policy_owner();
        owner.update_gpu_recheck(&auto_spec().cpu(), &loaded(None));
        assert!(!owner.shared.fallback_active.load(Ordering::Acquire));
        assert!(lock(&owner.shared.gpu_recheck).is_none());
        let owner = Owner {
            shared: Arc::new(Shared::default()),
            ..owner
        };
        owner.update_gpu_recheck(&auto_spec(), &loaded(None));
        assert!(lock(&owner.shared.gpu_recheck).is_none());
    }

    #[test]
    fn removal_requires_a_fresh_list_without_the_bound_identity() {
        let mut gpu = DeviceInfo {
            name: "GPU".into(),
            description: "GPU".into(),
            kind: "Vulkan".into(),
            device_type: transcribe_cpp::DeviceType::Gpu,
            device_id: Some("pci:01:00.0".into()),
            memory_total: 0,
            index: Some(0),
            key: "bound-gpu-key".into(),
        };
        assert!(!device_missing("bound-gpu-key", None));
        assert!(!device_missing(
            "bound-gpu-key",
            Some(std::slice::from_ref(&gpu))
        ));
        gpu.index = Some(1);
        assert!(!device_missing(
            "bound-gpu-key",
            Some(std::slice::from_ref(&gpu))
        ));
        // A different GPU with the same label must not hide device removal.
        gpu.key = "different-gpu-key".into();
        assert!(device_missing("bound-gpu-key", Some(&[gpu])));
        assert!(device_missing("bound-gpu-key", Some(&[])));
    }

    fn gpu(key: &str, memory: u64) -> DeviceInfo {
        DeviceInfo {
            name: key.into(),
            description: key.into(),
            kind: "vulkan".into(),
            device_type: transcribe_cpp::DeviceType::Gpu,
            device_id: Some(key.into()),
            memory_total: memory,
            index: Some(0),
            key: key.into(),
        }
    }

    #[test]
    fn failed_named_gpu_uses_only_cpu_and_keeps_its_selection() {
        let selected = LoadSpec {
            device: DeviceSelector::Key("selected".into()),
            ..auto_spec()
        };
        let devices = vec![gpu("selected", 6), gpu("other", 24)];
        let failed = HashMap::from([("selected".into(), "Selected GPU".into())]);
        assert!(select_recovery_spec(&selected, false, &failed, &devices).cpu_only());
        assert_eq!(selected.device, DeviceSelector::Key("selected".into()));
        assert_eq!(
            select_recovery_spec(&auto_spec(), false, &failed, &devices).device,
            DeviceSelector::RecoveryKey("other".into())
        );
    }

    #[test]
    fn cpu_fallback_is_used_only_when_no_healthy_gpu_remains() {
        let devices = vec![gpu("preferred_gpu", 6), gpu("alternate_gpu", 2)];
        let mut failed = HashMap::from([("preferred_gpu".into(), "Preferred GPU".into())]);
        assert_eq!(
            select_recovery_spec(&auto_spec(), false, &failed, &devices).device,
            DeviceSelector::RecoveryKey("alternate_gpu".into())
        );
        failed.insert("alternate_gpu".into(), "Alternate GPU".into());
        let recovery = select_recovery_spec(&auto_spec(), false, &failed, &devices);
        assert_eq!(recovery.backend, Backend::Cpu);
        assert_eq!(recovery.device, DeviceSelector::Auto);
    }

    #[test]
    fn healthy_saved_gpu_wins_over_another_larger_gpu_during_recovery() {
        let preferred = LoadSpec {
            device: DeviceSelector::Key("preferred_gpu".into()),
            ..auto_spec()
        };
        let failed = HashMap::from([("alternate_gpu".into(), "Alternate GPU".into())]);
        assert_eq!(
            select_recovery_spec(
                &preferred,
                false,
                &failed,
                &[gpu("preferred_gpu", 2), gpu("other", 12)]
            )
            .device,
            DeviceSelector::RecoveryKey("preferred_gpu".into())
        );
    }

    #[test]
    fn explicit_cpu_and_pinned_devices_are_preserved_and_init_faults_use_cpu() {
        let failed = HashMap::from([("preferred_gpu".into(), "Preferred GPU".into())]);
        let devices = vec![gpu("alternate_gpu", 2)];
        let cpu = auto_spec().cpu();
        assert_eq!(
            select_recovery_spec(&cpu, false, &failed, &devices).backend,
            Backend::Cpu
        );
        let pinned = LoadSpec {
            device: DeviceSelector::Index(2),
            ..auto_spec()
        };
        assert_eq!(
            select_recovery_spec(&pinned, true, &failed, &devices).device,
            pinned.device
        );
        assert_eq!(
            select_recovery_spec(&auto_spec(), true, &failed, &devices).backend,
            Backend::Cpu
        );
    }

    #[test]
    fn explicit_retry_clears_only_the_runtime_quarantine() {
        let engine = EngineSupervisor::new(true);
        assert!(engine
            .shared
            .mark_gpu_failed("preferred_gpu", "Preferred GPU", "test"));
        assert!(!engine.shared.gpu_unavailable());
        assert!(!engine.failed_gpu_devices().is_empty());
        // Failure history alone is not an active fallback with no model.
        assert!(!engine.is_in_fallback());
        assert_eq!(
            engine.failed_gpu_devices(),
            vec![("preferred_gpu".into(), "Preferred GPU".into())]
        );
        engine.retry_gpu("user explicitly selected a device");
        assert!(!engine.is_in_fallback());
        assert!(engine.failed_gpu_devices().is_empty());
        assert_eq!(
            engine.shared.recovery_spec(&auto_spec()).device,
            DeviceSelector::Auto
        );
    }

    #[test]
    fn only_context_failures_require_worker_replacement() {
        assert!(Failure::Backend("device lost".into()).requires_restart());
        assert!(Failure::Crashed("abort".into()).requires_restart());
        assert!(Failure::Hung(Duration::from_secs(1)).requires_restart());
        assert!(!Failure::Remote("backend error: device lost".into()).requires_restart());
        assert!(!Failure::Cancelled.requires_restart());
        assert!(!Failure::Allocation("device lost".into()).requires_restart());
        assert!(!Failure::Spawn(io::Error::other("could not spawn")).requires_restart());
    }

    fn auto_spec() -> LoadSpec {
        LoadSpec {
            path: "model.gguf".into(),
            backend: Backend::Auto,
            device: DeviceSelector::Auto,
        }
    }

    #[test]
    fn deadlines_scale_with_the_audio_above_a_floor() {
        let gpu = Deadlines::for_device(true);
        assert_eq!(gpu, GPU_DEADLINES);
        assert_eq!(gpu.audio(0), gpu.call_floor);
        assert_eq!(gpu.audio(16_000 / 2), gpu.call_floor);
        assert_eq!(gpu.audio(16_000 * 5), Duration::from_secs(50));
        assert_eq!(gpu.run(16_000), gpu.run_floor);
        assert_eq!(gpu.run(16_000 * 10), Duration::from_secs(100));

        let cpu = Deadlines::for_device(false);
        assert_eq!(cpu, CPU_DEADLINES);
        assert_eq!(cpu.audio(16_000), cpu.call_floor);
        assert_eq!(cpu.audio(16_000 * 10), Duration::from_secs(200));
        assert_eq!(cpu.run(16_000 * 5), cpu.run_floor);
        assert_eq!(cpu.run(16_000 * 60), Duration::from_secs(1200));
        // CPU is never stricter than GPU.
        for secs in [0, 1, 5, 30, 120, 600] {
            assert!(cpu.audio(secs * 16_000) >= gpu.audio(secs * 16_000));
            assert!(cpu.run(secs * 16_000) >= gpu.run(secs * 16_000));
        }

        assert_eq!(ms_to_samples(1500), 24_000);
        assert_eq!(ms_to_samples(-5), 0);
    }

    #[test]
    fn device_status_publishes_before_inference_and_ignores_pending_unloads() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let observed = changes.clone();
        let mut owner = policy_owner();
        owner.shared = Arc::new(Shared {
            gpu_allowed: true,
            on_device_changed: Some(Box::new(move |info| {
                lock(&observed).push(info.device.clone());
            })),
            ..Shared::default()
        });
        assert!(owner.publish_loaded(&loaded(None)));
        assert!(owner.publish_loaded(&loaded(Some("alternate_gpu"))));
        assert_eq!(*lock(&changes), vec!["CPU", "alternate_gpu"]);
        assert_eq!(
            lock(&owner.shared.loaded).as_ref().unwrap().device,
            "alternate_gpu"
        );
        // An unload requested before a load finishes must not publish stale status.
        owner.shared.unloads_requested.store(1, Ordering::SeqCst);
        assert!(!owner.publish_loaded(&loaded(Some("preferred_gpu"))));
        assert_eq!(*lock(&changes), vec!["CPU", "alternate_gpu"]);
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

    /// Faults are read by each worker at spawn, so set them before loading.
    fn set_fault(fault: Option<&str>, gpu_only: bool) {
        let marker = std::env::temp_dir().join("handy-worker-fault-once");
        let _ = std::fs::remove_file(&marker);
        std::env::remove_var("HANDY_WORKER_LOAD_ERRORS");
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

    /// The workers alive right now.
    fn worker_count() -> usize {
        LIVE_WORKERS.load(Ordering::SeqCst)
    }

    /// Feed 30 ms frames a little faster than a microphone would.
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
        assert!(engine.devices().is_some());
        assert_eq!(worker_count(), 0);
    }

    /// A cancel between two runs from one mark (two chunks of one
    /// transcription) stops the second; a fresh mark runs again.
    #[test]
    #[ignore]
    fn cancel_between_runs_stops_the_rest_of_the_mark() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        engine.load(spec()).unwrap();
        let mark = engine.cancel_mark();
        engine
            .transcribe_since(mark, test_pcm(), RunOptions::default())
            .unwrap();
        engine.cancel();
        assert!(matches!(
            engine.transcribe_since(mark, test_pcm(), RunOptions::default()),
            Err(EngineError::Cancelled)
        ));
        engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap();
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
    /// audio, at least the device's call floor) and the batch fallback runs in a fresh
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
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(!engine.shared.gpu_unavailable());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// Cancelling during a hung feed stops it at once (not at the feed
    /// deadline), skips the queued frames, and the finalize reports
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
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(!engine.shared.gpu_unavailable());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
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

    fn test_gpus(engine: &EngineSupervisor) -> Vec<DeviceInfo> {
        let mut devices: Vec<_> = engine
            .devices()
            .unwrap()
            .into_iter()
            .filter(DeviceInfo::is_gpu)
            .collect();
        devices.sort_by(|a, b| {
            gpu_rank(b)
                .cmp(&gpu_rank(a))
                .then_with(|| a.key.cmp(&b.key))
        });
        assert!(!devices.is_empty(), "test requires a GPU");
        devices
    }

    fn two_test_gpus(engine: &EngineSupervisor) -> (DeviceInfo, DeviceInfo) {
        let devices = test_gpus(engine);
        assert!(devices.len() >= 2, "test requires two available GPUs");
        (devices[0].clone(), devices[1].clone())
    }

    fn expire_device_discovery(engine: &EngineSupervisor) {
        *lock(&engine.shared.gpu_recheck) = Some(Instant::now());
    }

    /// Before its cooldown expires, a failed GPU stays excluded across discovery scans.
    /// A real worker proves both fallback dictation and explicit retry still work.
    #[test]
    #[ignore]
    fn present_failed_gpu_stays_on_fallback_during_cooldown() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let devices = test_gpus(&engine);
        let preferred_gpu = devices[0].clone();
        // Isolate GPU-to-CPU quarantine from unrelated failures on another
        // installed GPU. The restoration test below covers a working GPU fallback.
        for device in &devices[1..] {
            engine.shared.mark_gpu_failed(
                &device.key,
                device.label(),
                "unavailable for this scenario",
            );
        }
        let preferred = LoadSpec {
            device: DeviceSelector::Key(preferred_gpu.key.clone()),
            ..spec()
        };
        set_fault(Some("abort@run"), false);
        engine.load(preferred.clone()).unwrap();
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        for failure in lock(&engine.shared.failed_gpus).values_mut() {
            failure.retry_at = Instant::now() + Duration::from_secs(3600);
        }
        let fallback = engine.loaded().unwrap();
        assert_ne!(fallback.device_key.as_ref(), Some(&preferred_gpu.key));
        set_fault(None, false);
        for _ in 0..3 {
            expire_device_discovery(&engine);
            assert!(engine.recheck_gpu().unwrap().is_none());
            assert_eq!(engine.loaded().unwrap().device_key, fallback.device_key);
            assert!(!engine
                .transcribe(test_pcm(), RunOptions::default())
                .unwrap()
                .text
                .trim()
                .is_empty());
        }
        // Idle unloading must not release the failed device either.
        engine.unload().wait();
        assert_ne!(
            engine.load(preferred.clone()).unwrap().device_key.as_ref(),
            Some(&preferred_gpu.key)
        );
        engine.retry_gpu("user explicitly retried the accelerator");
        assert_eq!(
            engine.load(preferred).unwrap().device_key.as_ref(),
            Some(&preferred_gpu.key)
        );
        engine.unload().wait();
    }

    #[test]
    #[ignore]
    fn named_gpu_repeated_failures_return_to_cpu_without_trying_other_gpus() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let (selected, other) = two_test_gpus(&engine);
        for _ in 0..3 {
            // Fault settings are inherited only at worker spawn, not by an
            // already-loaded worker. Configure before each model load.
            set_fault(Some("abort@run"), false);
            engine
                .load(LoadSpec {
                    device: DeviceSelector::Key(selected.key.clone()),
                    ..spec()
                })
                .unwrap();
            assert!(!engine
                .transcribe(test_pcm(), RunOptions::default())
                .unwrap()
                .text
                .trim()
                .is_empty());
            assert!(!engine.loaded().unwrap().on_gpu);
            assert!(!lock(&engine.shared.failed_gpus).contains_key(&other.key));
            set_fault(None, false);
            lock(&engine.shared.failed_gpus)
                .get_mut(&selected.key)
                .unwrap()
                .retry_at = Instant::now();
            expire_device_discovery(&engine);
            assert_eq!(
                engine.recheck_gpu().unwrap().unwrap().device_key.as_ref(),
                Some(&selected.key)
            );
        }
        engine.unload().wait();
    }

    #[test]
    #[ignore]
    fn named_gpu_load_failure_uses_cpu_without_touching_another_gpu() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let (selected, other) = two_test_gpus(&engine);
        for kind in ["backend", "allocation"] {
            engine.retry_gpu("explicit test retry");
            std::env::set_var(
                "HANDY_WORKER_LOAD_ERRORS",
                serde_json::to_string(&HashMap::from([(selected.key.clone(), kind)])).unwrap(),
            );
            let info = engine
                .load(LoadSpec {
                    device: DeviceSelector::Key(selected.key.clone()),
                    ..spec()
                })
                .unwrap();
            assert!(!info.on_gpu, "selected GPU load failure must use CPU");
            assert!(!lock(&engine.shared.failed_gpus).contains_key(&other.key));
            assert!(!engine
                .transcribe(test_pcm(), RunOptions::default())
                .unwrap()
                .text
                .trim()
                .is_empty());
        }
        set_fault(None, false);
        engine.unload().wait();
    }

    #[test]
    #[ignore]
    fn missing_named_device_worker_never_uses_another_gpu() {
        set_fault(None, false);
        let shared = Shared::default();
        let mut worker = shared.hello(false, true).unwrap();
        let info = worker
            .load(&LoadSpec {
                device: DeviceSelector::Key("missing-test-device".into()),
                ..spec()
            })
            .unwrap();
        assert!(!info.on_gpu);
    }

    /// No absent registry sample: expiry alone permits a recovery trial. Real
    /// workers exercise a failed load, restoration, batch success and streaming.
    #[test]
    #[ignore]
    fn missed_transition_retries_with_backoff_and_confirms_inference() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let (preferred_gpu, other_gpu) = two_test_gpus(&engine);
        let preferred = LoadSpec {
            device: DeviceSelector::Key(preferred_gpu.key.clone()),
            ..spec()
        };
        engine
            .shared
            .mark_gpu_failed(&preferred_gpu.key, preferred_gpu.label(), "device lost");
        assert!(!engine.load(preferred).unwrap().on_gpu);
        // Both devices stay visible throughout. Before expiry, no retry.
        expire_device_discovery(&engine);
        assert!(engine.recheck_gpu().unwrap().is_none());
        std::env::set_var(
            "HANDY_WORKER_LOAD_ERRORS",
            serde_json::to_string(&HashMap::from([(preferred_gpu.key.clone(), "backend")]))
                .unwrap(),
        );
        lock(&engine.shared.failed_gpus)
            .get_mut(&preferred_gpu.key)
            .unwrap()
            .retry_at = Instant::now();
        expire_device_discovery(&engine);
        assert!(!engine.recheck_gpu().unwrap().unwrap().on_gpu);
        assert!(
            !lock(&engine.shared.failed_gpus).contains_key(&other_gpu.key),
            "the unselected GPU must never be attempted"
        );
        assert!(
            !engine.shared.gpu_unavailable(),
            "a repeated attributed failure is not global failure"
        );
        assert_eq!(
            lock(&engine.shared.failed_gpus)[&preferred_gpu.key].delay,
            Duration::from_secs(60)
        );
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        assert!(
            lock(&engine.shared.failed_gpus).contains_key(&preferred_gpu.key),
            "fallback success preserves the preferred GPU's cooldown"
        );
        set_fault(None, false);
        lock(&engine.shared.failed_gpus)
            .get_mut(&preferred_gpu.key)
            .unwrap()
            .retry_at = Instant::now();
        expire_device_discovery(&engine);
        assert_eq!(
            engine.recheck_gpu().unwrap().unwrap().device_key.as_ref(),
            Some(&preferred_gpu.key)
        );
        assert_eq!(
            lock(&engine.shared.failed_gpus)[&preferred_gpu.key].delay,
            Duration::from_secs(60),
            "loading alone must not reset backoff"
        );
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        assert!(!lock(&engine.shared.failed_gpus).contains_key(&preferred_gpu.key));
        // Recreate a pending trial on the active GPU and verify streaming also
        // confirms successful inference, rather than merely a successful load.
        engine.shared.mark_gpu_failed(
            &preferred_gpu.key,
            preferred_gpu.label(),
            "test stream trial",
        );
        lock(&engine.shared.failed_gpus)
            .get_mut(&preferred_gpu.key)
            .unwrap()
            .quarantined = false;
        assert!(stream_all(&engine, &test_pcm()).unwrap().is_some());
        assert!(!lock(&engine.shared.failed_gpus).contains_key(&preferred_gpu.key));
        engine.unload().wait();
    }

    #[test]
    #[ignore]
    fn optional_upgrade_load_errors_preserve_working_cpu() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        let (preferred_gpu, alternate_gpu) = two_test_gpus(&engine);
        let preferred = spec();
        engine
            .shared
            .mark_gpu_unavailable("test previous backend failure");
        assert!(!engine.load(preferred).unwrap().on_gpu);
        engine.retry_gpu("user retry");
        std::env::set_var(
            "HANDY_WORKER_LOAD_ERRORS",
            serde_json::to_string(&HashMap::from([
                (preferred_gpu.key.clone(), "backend"),
                (alternate_gpu.key.clone(), "allocation"),
            ]))
            .unwrap(),
        );
        expire_device_discovery(&engine);
        assert!(!engine.recheck_gpu().unwrap().unwrap().on_gpu);
        assert!(lock(&engine.shared.failed_gpus).contains_key(&alternate_gpu.key));
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        set_fault(None, false);
        engine.unload().wait();
    }

    #[test]
    #[ignore]
    fn returning_gpu_oom_restores_previous_gpu_then_cpu_if_needed() {
        set_fault(None, false);
        let engine = EngineSupervisor::new(true);
        // Keep the strongest GPU as the working fallback; load errors on the
        // returning alternate are injected before its model is allocated.
        let (preferred_gpu, alternate_gpu) = two_test_gpus(&engine);
        let preferred = spec();
        engine.shared.mark_gpu_failed(
            &preferred_gpu.key,
            preferred_gpu.label(),
            "test previous failure",
        );
        assert_eq!(
            engine.load(preferred.clone()).unwrap().device_key.as_ref(),
            Some(&alternate_gpu.key)
        );
        std::env::set_var(
            "HANDY_WORKER_LOAD_ERRORS",
            serde_json::to_string(&HashMap::from([(preferred_gpu.key.clone(), "allocation")]))
                .unwrap(),
        );
        // Simulate a confirmed off/on transition before each fresh attempt.
        engine.shared.set_devices(vec![alternate_gpu.clone()]);
        expire_device_discovery(&engine);
        assert_eq!(
            engine.recheck_gpu().unwrap().unwrap().device_key.as_ref(),
            Some(&alternate_gpu.key)
        );
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        std::env::set_var(
            "HANDY_WORKER_LOAD_ERRORS",
            serde_json::to_string(&HashMap::from([
                (preferred_gpu.key.clone(), "allocation"),
                (alternate_gpu.key.clone(), "allocation"),
            ]))
            .unwrap(),
        );
        // Simulate a confirmed off/on transition before each fresh attempt.
        engine.shared.set_devices(vec![alternate_gpu.clone()]);
        expire_device_discovery(&engine);
        assert!(
            !engine.recheck_gpu().unwrap().unwrap().on_gpu,
            "failed GPU restoration must still try CPU"
        );
        assert!(!engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap()
            .text
            .trim()
            .is_empty());
        set_fault(None, false);
        engine.unload().wait();
    }

    /// A GPU-only crash retries the dictation on another healthy device.
    /// Failed GPUs stay excluded during cooldown; explicit retry clears their history.
    #[test]
    #[ignore]
    fn gpu_only_crash_recovers_on_another_device() {
        let pcm = test_pcm();
        set_fault(Some("abort@run"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        assert!(info.on_gpu, "needs a GPU to test GPU-only faults");
        let transcript = engine
            .transcribe(pcm.clone(), RunOptions::default())
            .unwrap();
        println!(
            "recovered on {}: {}",
            engine.loaded().unwrap().device,
            transcript.text
        );
        assert!(engine.shared.gpu_unavailable() || !engine.failed_gpu_devices().is_empty());
        assert_ne!(engine.loaded().unwrap().device_key, info.device_key);
        let info = engine.load(spec()).unwrap();
        assert!(
            !info.on_gpu
                || !lock(&engine.shared.failed_gpus)
                    .contains_key(info.device_key.as_deref().unwrap_or_default()),
            "later loads must not reuse a failed GPU"
        );
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        set_fault(None, false);
    }

    /// A GPU driver that crashes while its backend registers, before the
    /// hello: only a CPU-only worker survives it, so that is where the model
    /// loads. The GPU is marked unavailable, later loads stay CPU-only and devices
    /// aren't probed, until "try the GPU again" (a settings change).
    #[test]
    #[ignore]
    fn gpu_backend_init_crash_uses_cpu_only_worker() {
        let pcm = test_pcm();
        set_fault(Some("abort@init"), true);
        let engine = EngineSupervisor::new(true);
        let info = engine.load(spec()).unwrap();
        assert!(!info.on_gpu, "loaded on '{}'", info.backend);
        assert!(engine.shared.gpu_unavailable());
        assert!(engine.devices().is_none());
        engine.transcribe(pcm, RunOptions::default()).unwrap();
        // The fault still fires in any worker with GPU backends.
        assert!(!engine.load(spec()).unwrap().on_gpu);
        set_fault(None, false);
        engine.retry_gpu("test");
        assert!(engine.load(spec()).unwrap().on_gpu);
        assert!(engine.devices().is_some());
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
        assert!(!engine.shared.gpu_unavailable());
        set_fault(None, false);
        assert!(engine.devices().is_some());
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
        let devices = engine.devices().expect("devices not listed");
        assert!(devices.iter().all(|d| !d.is_gpu()), "{devices:?}");
        assert!(!engine.load(spec()).unwrap().on_gpu);
        assert!(!engine.shared.gpu_unavailable());
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
        let devices = engine.devices().expect("devices not listed");
        let gpu = devices.iter().find(|d| d.is_gpu()).expect("needs a GPU");
        let info = engine
            .load(LoadSpec {
                device: DeviceSelector::Index(gpu.index.unwrap()),
                ..spec()
            })
            .unwrap();
        assert!(info.on_gpu);
        assert!(engine.transcribe(pcm, RunOptions::default()).is_err());
        assert!(!engine.shared.gpu_unavailable());
        set_fault(None, false);
    }

    /// Cancel mid-run: the run stops promptly, without a retry, the model
    /// still reads as loaded, and the next run works in a fresh worker.
    #[test]
    #[ignore]
    fn cancel_stops_a_run() {
        let pcm = long_pcm(60);
        set_fault(None, false);
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
        println!("cancelled {:?} after cancel()", t.elapsed());
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(engine.loaded().is_some());
        assert!(!engine.shared.gpu_unavailable());
        let t = Instant::now();
        engine
            .transcribe(test_pcm(), RunOptions::default())
            .unwrap();
        println!("next run took {:?}", t.elapsed());
    }

    /// Cancel a hung run: killed at once, no retry, no GPU
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
        // Cancel only once the worker is actually hung, not on its way there.
        let marker = std::env::temp_dir().join("handy-worker-fault-once");
        while !marker.exists() {
            thread::sleep(Duration::from_millis(1));
        }
        let t = Instant::now();
        engine.cancel();
        let result = run.join().unwrap();
        println!("hung run cancelled {:?} after cancel()", t.elapsed());
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(!engine.shared.gpu_unavailable());
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
