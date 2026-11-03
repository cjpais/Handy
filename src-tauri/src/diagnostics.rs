//! Opt-in diagnostics tracing.
//!
//! Records one Chrome Trace Event file (Perfetto-compatible) per capture,
//! holding only timing evidence: fixed-vocabulary event names, allowlisted
//! outcome codes, counters, and booleans. By construction no user audio,
//! text, clipboard data, local path, or free-form error string can enter a
//! trace; [`Event`] accepts only enums, numbers, and bools.
//!
//! The free functions target the process-global recorder. Tests build
//! their own [`Recorder`] with tiny caps instead.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Bounded queue between emitters and the writer thread.
const QUEUE_CAP: usize = 1024;
/// Hard ceiling on trace bytes per capture (32 MiB).
const BYTE_CAP: u64 = 32 * 1024 * 1024;
/// How long `stop` waits for the writer before giving up on it.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// Value stamped into `otherData.schema`.
const SCHEMA: &str = "handy-diagnostics/1";
const TRACE_HEADER: &[u8] = b"{\"traceEvents\":[\n";

/// Identifies one dictation run across threads and capture start/stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(NonZeroU64);

impl RunId {
    pub fn as_u64(self) -> u64 {
        self.0.get()
    }
}

/// Chrome trace phase of an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Async span begin (`ph: "b"`).
    Begin,
    /// Async span end (`ph: "e"`).
    End,
    /// Thread-scoped instant (`ph: "i"`, `s: "t"`).
    Instant,
}

/// Fixed event vocabulary. Extend only with reviewer sign-off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Name {
    ShortcutEvent,
    InputReceived,
    InputDequeued,
    Effect,
    Run,
    Recording,
    MicOpen,
    FirstAudioDelivered,
    FirstVadFrame,
    FirstSpeech,
    ReadyObserved,
    CaptureStop,
    ModelWait,
    LiveFinalize,
    BatchDecode,
    PostProcess,
    HistorySave,
    Output,
}

impl Name {
    pub const fn as_str(self) -> &'static str {
        match self {
            Name::ShortcutEvent => "shortcut_event",
            Name::InputReceived => "input_received",
            Name::InputDequeued => "input_dequeued",
            Name::Effect => "effect",
            Name::Run => "run",
            Name::Recording => "recording",
            Name::MicOpen => "mic_open",
            Name::FirstAudioDelivered => "first_audio_delivered",
            Name::FirstVadFrame => "first_vad_frame",
            Name::FirstSpeech => "first_speech",
            Name::ReadyObserved => "ready_observed",
            Name::CaptureStop => "capture_stop",
            Name::ModelWait => "model_wait",
            Name::LiveFinalize => "live_finalize",
            Name::BatchDecode => "batch_decode",
            Name::PostProcess => "post_process",
            Name::HistorySave => "history_save",
            Name::Output => "output",
        }
    }
}

/// Allowlisted outcome and error codes. Mapped from internal enums at emit
/// sites, never from an error `Display` output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    Start,
    Stop,
    Typed,
    Empty,
    Silent,
    Failed,
    Cancelled,
    NoModel,
    MicrophonePermissionDenied,
    NoInputDevice,
    Unknown,
    RecorderFailed,
    SinkPanicked,
    TranscriptionFailed,
    PasteFailed,
}

impl Code {
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Start => "start",
            Code::Stop => "stop",
            Code::Typed => "typed",
            Code::Empty => "empty",
            Code::Silent => "silent",
            Code::Failed => "failed",
            Code::Cancelled => "cancelled",
            Code::NoModel => "no_model",
            Code::MicrophonePermissionDenied => "microphone_permission_denied",
            Code::NoInputDevice => "no_input_device",
            Code::Unknown => "unknown",
            Code::RecorderFailed => "recorder_failed",
            Code::SinkPanicked => "sink_panicked",
            Code::TranscriptionFailed => "transcription_failed",
            Code::PasteFailed => "paste_failed",
        }
    }
}

/// Numeric and boolean payload; the only payload an [`Event`] accepts.
/// `Copy` rules out owned strings by construction.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Args {
    pub code: Option<Code>,
    pub count: Option<u64>,
    pub flag: Option<bool>,
}

impl Args {
    /// Args carrying an optional outcome code.
    pub fn code(code: impl Into<Option<Code>>) -> Self {
        Args {
            code: code.into(),
            ..Default::default()
        }
    }
}

/// Emitting thread identity: a small stable id plus an allowlisted
/// code-defined thread name. Resolved once per thread, but only while a
/// capture is active (the public emit methods return before this runs
/// when the recorder is disabled).
#[derive(Clone, Copy)]
struct ThreadTag {
    tid: u64,
    name: Option<&'static str>,
}

static THREAD_IDS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static THREAD_TAG: ThreadTag = ThreadTag::capture();
}

impl ThreadTag {
    fn capture() -> Self {
        let tid = THREAD_IDS.fetch_add(1, Ordering::Relaxed) + 1;
        let name = thread::current().name().and_then(allowed_thread_name);
        ThreadTag { tid, name }
    }
}

fn allowed_thread_name(name: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "main",
        "tokio-runtime-worker",
        "tokio-rt-worker",
        "paste",
        "transcription-jobs",
        "transcribe-engine",
        "transcribe-worker-in",
        "transcribe-worker-out",
        "transcribe-worker-err",
        "diagnostics-writer",
        "handy-recorder-device",
        "handy-recorder-delivery",
        "handy-recorder-notify",
        "handy-recorder-pulse-watch",
    ];
    NAMES.iter().copied().find(|known| *known == name)
}

fn thread_tag() -> ThreadTag {
    THREAD_TAG.with(|t| *t)
}

/// One trace record.
#[derive(Clone, Copy)]
struct Event {
    at: Instant,
    phase: Phase,
    name: Name,
    run: Option<RunId>,
    thread: ThreadTag,
    args: Args,
}

/// Configuration snapshot embedded in `otherData` at capture start. Strings
/// are allowlisted at construction (catalog ids, `"custom"`, variant names)
/// and never enter an [`Event`].
#[derive(Clone, Debug, Serialize)]
pub struct ConfigSnapshot {
    pub model: String,
    /// Decode mode the start path would choose: "stream", "chunked", or
    /// "none" — not just the model capability, so chunked live decode is
    /// not recorded as batch.
    pub live_decode: String,
    pub paste_method: String,
    pub push_to_talk: bool,
    pub post_process_enabled: bool,
    pub vad: bool,
}

/// Live capture status, polled by the UI while a capture runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub active: bool,
    pub started_at_ms: Option<u64>,
    pub events: u64,
    pub dropped: u64,
}

/// Errors from `start`/`stop`.
#[derive(Debug)]
pub enum DiagError {
    Io(std::io::Error),
    /// `stop` was called while no capture was active.
    NotActive,
    /// The writer did not finish within the stop timeout.
    StopTimedOut,
    /// The writer ended without reporting success.
    WriterFailed,
}

impl std::fmt::Display for DiagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagError::Io(e) => write!(f, "diagnostics io error: {e}"),
            DiagError::NotActive => write!(f, "no diagnostics capture is active"),
            DiagError::StopTimedOut => write!(f, "diagnostics writer did not stop in time"),
            DiagError::WriterFailed => write!(f, "diagnostics writer exited unexpectedly"),
        }
    }
}

impl std::error::Error for DiagError {}

/// State present only while a capture is active. Emitters read it with
/// `try_read`; `stop` takes the write side to drop the sender, which is the
/// writer's drain-and-finish signal.
struct Capture {
    sender: SyncSender<Event>,
    epoch: Instant,
    started_at_ms: u64,
    /// Per-capture counters: every capture starts from zero so the file
    /// and status describe this capture's losses, not the session's.
    admitted: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

/// The recorder. The process-global instance is a [`LazyLock`]; tests build
/// their own with tiny caps.
pub struct Recorder {
    active: AtomicBool,
    capture: RwLock<Option<Capture>>,
    /// Serializes `start`/`stop` control flow.
    control: Mutex<()>,
    /// Takes the writer's final report (the finished file path).
    done_rx: Mutex<Option<mpsc::Receiver<PathBuf>>>,
    next_id: AtomicU64,
}

/// An operation span tied to its recorder, run, and event name.
///
/// Call [`Span::end`] to complete it. Dropping it emits no end event, so an
/// interrupted operation remains incomplete until the capture stops.
#[must_use = "end completed spans explicitly; dropping leaves them incomplete"]
pub struct Span<'a> {
    recorder: &'a Recorder,
    run: Option<RunId>,
    name: Name,
}

impl Span<'_> {
    /// Ends this operation with the supplied outcome and counters.
    pub fn end(self, args: Args) {
        if let Some(run) = self.run {
            self.recorder.end(run, self.name, args);
        }
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub fn new() -> Self {
        Recorder {
            active: AtomicBool::new(false),
            capture: RwLock::new(None),
            control: Mutex::new(()),
            done_rx: Mutex::new(None),
            next_id: AtomicU64::new(0),
        }
    }

    /// Allocates a run id. Always succeeds, even while inactive, so ids stay
    /// stable across capture start/stop.
    pub fn next_run(&self) -> RunId {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        RunId(NonZeroU64::new(n).expect("counter starts at 1"))
    }

    /// Begins an operation span. An absent run suppresses both span events.
    pub fn span(&self, run: impl Into<Option<RunId>>, name: Name) -> Span<'_> {
        let run = run.into();
        if let Some(run) = run {
            self.begin(run, name);
        }
        Span {
            recorder: self,
            run,
            name,
        }
    }

    pub fn begin(&self, run: RunId, name: Name) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        self.emit(Event {
            at: Instant::now(),
            phase: Phase::Begin,
            name,
            run: Some(run),
            thread: thread_tag(),
            args: Args::default(),
        });
    }

    pub fn end(&self, run: RunId, name: Name, args: Args) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        self.emit(Event {
            at: Instant::now(),
            phase: Phase::End,
            name,
            run: Some(run),
            thread: thread_tag(),
            args,
        });
    }

    /// Records an instant event; an absent run leaves the event unbound.
    pub fn instant(&self, run: impl Into<Option<RunId>>, name: Name, args: Args) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        self.emit(Event {
            at: Instant::now(),
            phase: Phase::Instant,
            name,
            run: run.into(),
            thread: thread_tag(),
            args,
        });
    }

    pub fn instant_at(&self, at: Instant, run: impl Into<Option<RunId>>, name: Name, args: Args) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        self.emit(Event {
            at,
            phase: Phase::Instant,
            name,
            run: run.into(),
            thread: thread_tag(),
            args,
        });
    }

    /// Slow path, only reached while active. Never blocks on disk, queue
    /// capacity, or locks. A full queue or a pre-epoch timestamp counts
    /// the event as dropped on the live capture's counter; drops in the
    /// stop race (lock contended or sender already gone) go uncounted,
    /// since that capture is finalizing anyway.
    fn emit(&self, ev: Event) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let guard = match self.capture.try_read() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(cap) = guard.as_ref() else {
            return;
        };
        if ev.at < cap.epoch {
            // Timestamps from before the capture epoch are unusable;
            // reject rather than clamp them to zero.
            cap.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match cap.sender.try_send(ev) {
            Ok(()) => {
                cap.admitted.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                cap.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> Status {
        let guard = self
            .capture
            .read()
            .expect("diagnostics capture lock poisoned");
        let live = guard.as_ref().map(|cap| {
            (
                cap.started_at_ms,
                cap.admitted.load(Ordering::Relaxed),
                cap.dropped.load(Ordering::Relaxed),
            )
        });
        Status {
            active: self.is_active(),
            started_at_ms: live.map(|l| l.0),
            events: live.map_or(0, |l| l.1),
            dropped: live.map_or(0, |l| l.2),
        }
    }

    pub fn start(&self, dir: &Path, config: ConfigSnapshot) -> Result<Status, DiagError> {
        self.start_inner(dir, config, QUEUE_CAP, BYTE_CAP)
    }

    fn start_inner(
        &self,
        dir: &Path,
        config: ConfigSnapshot,
        queue_cap: usize,
        byte_cap: u64,
    ) -> Result<Status, DiagError> {
        let _control = self
            .control
            .lock()
            .expect("diagnostics control lock poisoned");
        if self.active.load(Ordering::Relaxed) {
            return Ok(self.status());
        }
        minimum_trace_bytes(&config, byte_cap).map_err(DiagError::Io)?;
        fs::create_dir_all(dir).map_err(DiagError::Io)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
        let mut n: u32 = 0;
        let (partial, final_path) = loop {
            let tag = if n == 0 {
                stamp.clone()
            } else {
                format!("{stamp}-{n}")
            };
            let partial = dir.join(format!("trace-{tag}.json.partial"));
            if !partial.exists() {
                break (partial, dir.join(format!("trace-{tag}.json")));
            }
            n += 1;
        };
        let file = File::create(&partial).map_err(DiagError::Io)?;
        let orphan = partial.clone();
        let (tx, rx) = sync_channel(queue_cap);
        let (done_tx, done_rx) = mpsc::channel();
        let epoch = Instant::now();
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let admitted = Arc::new(AtomicU64::new(0));
        let dropped = Arc::new(AtomicU64::new(0));

        *self.done_rx.lock().expect("diagnostics done lock poisoned") = Some(done_rx);
        *self
            .capture
            .write()
            .expect("diagnostics capture lock poisoned") = Some(Capture {
            sender: tx,
            epoch,
            started_at_ms,
            admitted: Arc::clone(&admitted),
            dropped: Arc::clone(&dropped),
        });
        self.active.store(true, Ordering::Release);

        let writer = Writer {
            rx,
            done_tx,
            out: BufWriter::new(file),
            partial,
            final_path,
            epoch,
            byte_cap,
            dropped,
            config,
        };
        let spawn = thread::Builder::new()
            .name("diagnostics-writer".to_string())
            .spawn(move || writer.run());
        if let Err(e) = spawn {
            // Roll back to idle so a later start can retry cleanly.
            self.active.store(false, Ordering::Release);
            *self
                .capture
                .write()
                .expect("diagnostics capture lock poisoned") = None;
            *self.done_rx.lock().expect("diagnostics done lock poisoned") = None;
            let _ = fs::remove_file(orphan);
            return Err(DiagError::Io(e));
        }
        Ok(self.status())
    }

    /// Stops the capture. Drains every event already admitted, marks spans
    /// still open as incomplete, finalizes the file, and returns its path.
    /// Never waits on transcription or paste; those spans simply end as
    /// incomplete.
    pub fn stop(&self) -> Result<PathBuf, DiagError> {
        let _control = self
            .control
            .lock()
            .expect("diagnostics control lock poisoned");
        let rx = self
            .done_rx
            .lock()
            .expect("diagnostics done lock poisoned")
            .take();
        let Some(rx) = rx else {
            return Err(DiagError::NotActive);
        };
        self.active.store(false, Ordering::Release);
        *self
            .capture
            .write()
            .expect("diagnostics capture lock poisoned") = None;
        match rx.recv_timeout(STOP_TIMEOUT) {
            Ok(path) => Ok(path),
            Err(RecvTimeoutError::Timeout) => Err(DiagError::StopTimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(DiagError::WriterFailed),
        }
    }
}

/// Owns the trace file between `start` and `stop`.
struct Writer {
    rx: Receiver<Event>,
    done_tx: mpsc::Sender<PathBuf>,
    out: BufWriter<File>,
    partial: PathBuf,
    final_path: PathBuf,
    epoch: Instant,
    byte_cap: u64,
    dropped: Arc<AtomicU64>,
    config: ConfigSnapshot,
}

struct OpenSpan {
    tid: u64,
    reserved_bytes: u64,
}

impl Writer {
    fn run(mut self) {
        let mut bytes = TRACE_HEADER.len() as u64;
        let mut reserved_bytes = match minimum_trace_bytes(&self.config, self.byte_cap) {
            Ok(minimum) => minimum - bytes,
            Err(error) => {
                log::warn!("diagnostics writer: capture metadata exceeds byte cap: {error}");
                drop(self.out);
                drop(self.done_tx);
                let _ = fs::remove_file(&self.partial);
                return;
            }
        };
        let mut truncated: u64 = 0;
        // Cleared on any write error. A file that failed mid-stream cannot
        // be trusted to parse, so it must not reach the caller as success.
        let mut io_ok = true;
        // Open async spans keyed by (run, name), holding the thread the span
        // began on so the incomplete end lands on an honest track.
        let mut open: BTreeMap<(RunId, Name), OpenSpan> = BTreeMap::new();
        let mut threads: BTreeMap<u64, Option<&'static str>> = BTreeMap::new();
        let mut record_buffer = Vec::with_capacity(128);

        io_ok &= self.out.write_all(TRACE_HEADER).is_ok();
        let mut first = true;
        for ev in self.rx {
            let thread_bytes = if threads.contains_key(&ev.thread.tid) {
                0
            } else {
                let label = ev
                    .thread
                    .name
                    .map_or_else(|| format!("thread-{}", ev.thread.tid), str::to_string);
                match metadata_line(Some(ev.thread.tid), &label, &mut record_buffer) {
                    Ok(line) => line.len() as u64 + 2,
                    Err(error) => {
                        io_ok = false;
                        log::warn!("diagnostics writer: metadata serialization failed: {error}");
                        break;
                    }
                }
            };
            let old_end_bytes = match ev.phase {
                Phase::Begin | Phase::End => ev
                    .run
                    .and_then(|run| open.get(&(run, ev.name)))
                    .map_or(0, |span| span.reserved_bytes),
                Phase::Instant => 0,
            };
            let new_end_bytes = if ev.phase == Phase::Begin && ev.run.is_some() {
                let end = Event {
                    phase: Phase::End,
                    args: Args::default(),
                    ..ev
                };
                match event_line_at_timestamp(&end, u64::MAX, true, &mut record_buffer) {
                    Ok(line) => line.len() as u64 + 2,
                    Err(error) => {
                        io_ok = false;
                        log::warn!("diagnostics writer: event serialization failed: {error}");
                        break;
                    }
                }
            } else {
                0
            };
            let line = match event_line(&ev, self.epoch, false, &mut record_buffer) {
                Ok(Some(line)) => line,
                Ok(None) => {
                    truncated = truncated.saturating_add(1);
                    continue;
                }
                Err(error) => {
                    io_ok = false;
                    log::warn!("diagnostics writer: event serialization failed: {error}");
                    break;
                }
            };
            let sep_len = u64::from(!first);
            let next_reserved =
                reserved_bytes as u128 + thread_bytes as u128 + new_end_bytes as u128
                    - old_end_bytes as u128;
            let next_bytes = bytes as u128 + line.len() as u128 + sep_len as u128 + 1;
            if next_bytes + next_reserved > self.byte_cap as u128 {
                truncated = truncated.saturating_add(1);
                continue;
            }
            let ok = (first || self.out.write_all(b",").is_ok())
                && self.out.write_all(line).is_ok()
                && self.out.write_all(b"\n").is_ok();
            if !ok {
                io_ok = false;
                log::warn!("diagnostics writer: file write failed; stopping capture early");
                break;
            }
            first = false;
            bytes = next_bytes as u64;
            reserved_bytes = next_reserved as u64;
            threads.entry(ev.thread.tid).or_insert(ev.thread.name);
            match ev.phase {
                Phase::Begin => {
                    if let Some(run) = ev.run {
                        open.insert(
                            (run, ev.name),
                            OpenSpan {
                                tid: ev.thread.tid,
                                reserved_bytes: new_end_bytes,
                            },
                        );
                    }
                }
                Phase::End => {
                    if let Some(run) = ev.run {
                        open.remove(&(run, ev.name));
                    }
                }
                Phase::Instant => {}
            }
        }

        // The sender is gone: every admitted event has been handled. End
        // still-open spans at the drain moment rather than losing them.
        let stop_at = Instant::now();
        let mut incomplete: u64 = 0;
        for ((run, name), span) in open {
            let tid = span.tid;
            let ev = Event {
                at: stop_at,
                phase: Phase::End,
                name,
                run: Some(run),
                thread: ThreadTag {
                    tid,
                    name: threads.get(&tid).copied().flatten(),
                },
                args: Args::default(),
            };
            match event_line(&ev, self.epoch, true, &mut record_buffer) {
                Ok(Some(line)) => {
                    io_ok &= (first || self.out.write_all(b",").is_ok())
                        && self.out.write_all(line).is_ok()
                        && self.out.write_all(b"\n").is_ok();
                    first = false;
                    incomplete = incomplete.saturating_add(1);
                }
                Ok(None) => {}
                Err(error) => {
                    io_ok = false;
                    log::warn!("diagnostics writer: event serialization failed: {error}");
                    break;
                }
            }
        }

        // Metadata: process and thread names.
        match metadata_line(None, "Handy", &mut record_buffer) {
            Ok(line) => {
                io_ok &= (first || self.out.write_all(b",").is_ok())
                    && self.out.write_all(line).is_ok()
                    && self.out.write_all(b"\n").is_ok();
            }
            Err(error) => {
                io_ok = false;
                log::warn!("diagnostics writer: metadata serialization failed: {error}");
            }
        }
        for (tid, name) in &threads {
            let label = name.map_or_else(|| format!("thread-{tid}"), str::to_string);
            match metadata_line(Some(*tid), &label, &mut record_buffer) {
                Ok(line) => {
                    io_ok &= self.out.write_all(b",").is_ok()
                        && self.out.write_all(line).is_ok()
                        && self.out.write_all(b"\n").is_ok();
                }
                Err(error) => {
                    io_ok = false;
                    log::warn!("diagnostics writer: metadata serialization failed: {error}");
                    break;
                }
            }
        }

        io_ok &= write_footer(
            &mut self.out,
            &self.config,
            self.dropped.load(Ordering::Relaxed),
            truncated,
            incomplete,
        )
        .is_ok();
        if let Err(e) = self.out.flush() {
            io_ok = false;
            log::warn!("diagnostics writer: flush failed: {e}");
        }
        if let Err(e) = self.out.get_ref().sync_all() {
            io_ok = false;
            log::warn!("diagnostics writer: fsync failed: {e}");
        }

        if !io_ok {
            // Half-written JSON must not reach the caller as a success.
            // Dropping done_tx without sending makes stop report
            // WriterFailed; the partial file is not worth keeping.
            drop(self.done_tx);
            let _ = fs::remove_file(&self.partial);
            return;
        }

        let mut done_path = self.final_path.clone();
        if let Err(e) = fs::rename(&self.partial, &self.final_path) {
            log::warn!(
                "diagnostics writer: rename to {} failed: {e}",
                self.final_path.display()
            );
            done_path = self.partial.clone();
        }
        let _ = self.done_tx.send(done_path);
    }
}

fn write_footer(
    out: &mut impl Write,
    config: &ConfigSnapshot,
    dropped_events: u64,
    truncated_events: u64,
    incomplete_spans: u64,
) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct OtherData<'a> {
        app_version: &'static str,
        arch: &'static str,
        capture_coverage: &'static str,
        config: &'a ConfigSnapshot,
        dropped_events: u64,
        first_audio_semantics: &'static str,
        incomplete_spans: u64,
        os: &'static str,
        output_semantics: &'static str,
        schema: &'static str,
        truncated_events: u64,
    }

    let other = OtherData {
        app_version: env!("CARGO_PKG_VERSION"),
        arch: std::env::consts::ARCH,
        capture_coverage: "unknown",
        config,
        dropped_events,
        first_audio_semantics: "delivery_latency",
        incomplete_spans,
        os: std::env::consts::OS,
        output_semantics: "handy_call_returned",
        schema: SCHEMA,
        truncated_events,
    };
    out.write_all(b"],\"otherData\":")?;
    serde_json::to_writer(&mut *out, &other).map_err(|error| {
        std::io::Error::new(
            error.io_error_kind().unwrap_or(std::io::ErrorKind::Other),
            error,
        )
    })?;
    out.write_all(b"}\n")
}

fn minimum_trace_bytes(config: &ConfigSnapshot, byte_cap: u64) -> std::io::Result<u64> {
    struct BoundedCount {
        bytes: u64,
        limit: u64,
    }

    impl Write for BoundedCount {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            let next = self.bytes as u128 + buffer.len() as u128;
            if next > self.limit as u128 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "diagnostics byte cap cannot fit capture metadata",
                ));
            }
            self.bytes = next as u64;
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut count = BoundedCount {
        bytes: 0,
        limit: byte_cap,
    };
    count.write_all(TRACE_HEADER)?;
    let mut buffer = Vec::with_capacity(128);
    let process = metadata_line(None, "Handy", &mut buffer).map_err(std::io::Error::other)?;
    count.write_all(process)?;
    count.write_all(b",\n")?;
    write_footer(&mut count, config, u64::MAX, u64::MAX, u64::MAX)?;
    Ok(count.bytes)
}

fn metadata_line<'a>(
    tid: Option<u64>,
    label: &str,
    buffer: &'a mut Vec<u8>,
) -> serde_json::Result<&'a [u8]> {
    #[derive(Serialize)]
    struct Metadata<'a> {
        ph: &'static str,
        name: &'static str,
        pid: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        tid: Option<u64>,
        args: Label<'a>,
    }

    #[derive(Serialize)]
    struct Label<'a> {
        name: &'a str,
    }

    let record = Metadata {
        ph: "M",
        name: if tid.is_some() {
            "thread_name"
        } else {
            "process_name"
        },
        pid: 1,
        tid,
        args: Label { name: label },
    };
    buffer.clear();
    serde_json::to_writer(&mut *buffer, &record)?;
    Ok(buffer.as_slice())
}

fn event_line<'a>(
    ev: &Event,
    epoch: Instant,
    incomplete: bool,
    buffer: &'a mut Vec<u8>,
) -> serde_json::Result<Option<&'a [u8]>> {
    let Some(ts) = ev.at.checked_duration_since(epoch) else {
        return Ok(None);
    };
    event_line_at_timestamp(ev, ts.as_micros() as u64, incomplete, buffer).map(Some)
}

fn event_line_at_timestamp<'a>(
    ev: &Event,
    timestamp: u64,
    incomplete: bool,
    buffer: &'a mut Vec<u8>,
) -> serde_json::Result<&'a [u8]> {
    #[derive(Serialize)]
    struct EventRecord {
        ph: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        cat: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        s: Option<&'static str>,
        name: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        ts: u64,
        pid: u8,
        tid: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<RecordArgs>,
    }

    #[derive(Serialize)]
    struct RecordArgs {
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        count: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        flag: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        incomplete: Option<bool>,
    }

    let (ph, cat, scope) = match ev.phase {
        Phase::Begin => ("b", Some("run"), None),
        Phase::End => ("e", Some("run"), None),
        Phase::Instant => ("i", None, Some("t")),
    };
    let args =
        (ev.args.code.is_some() || ev.args.count.is_some() || ev.args.flag.is_some() || incomplete)
            .then_some(RecordArgs {
                code: ev.args.code.map(Code::as_str),
                count: ev.args.count,
                flag: ev.args.flag,
                incomplete: incomplete.then_some(true),
            });
    let record = EventRecord {
        ph,
        cat,
        s: scope,
        name: ev.name.as_str(),
        id: ev.run.map(RunId::as_u64),
        ts: timestamp,
        pid: 1,
        tid: ev.thread.tid,
        args,
    };
    buffer.clear();
    serde_json::to_writer(&mut *buffer, &record)?;
    Ok(buffer.as_slice())
}

static RECORDER: LazyLock<Recorder> = LazyLock::new(Recorder::new);

pub fn next_run() -> RunId {
    RECORDER.next_run()
}

/// Begins an operation span on the global recorder; an absent run is a no-op.
pub fn span(run: impl Into<Option<RunId>>, name: Name) -> Span<'static> {
    RECORDER.span(run, name)
}

pub fn begin(run: RunId, name: Name) {
    RECORDER.begin(run, name);
}

pub fn end(run: RunId, name: Name, args: Args) {
    RECORDER.end(run, name, args);
}

/// Records an event on the global recorder; an absent run leaves it unbound.
pub fn instant(run: impl Into<Option<RunId>>, name: Name, args: Args) {
    RECORDER.instant(run, name, args);
}

pub fn instant_at(at: Instant, run: impl Into<Option<RunId>>, name: Name, args: Args) {
    RECORDER.instant_at(at, run, name, args);
}

pub fn start(dir: &Path, config: ConfigSnapshot) -> Result<Status, DiagError> {
    RECORDER.start(dir, config)
}

pub fn stop() -> Result<PathBuf, DiagError> {
    RECORDER.stop()
}

pub fn status() -> Status {
    RECORDER.status()
}

// Single-source variant lists for the privacy test. The size assertions in
// `vocabulary_lists_are_complete` fail when a variant is added without
// extending these lists.
#[cfg(test)]
const NAME_LIST: &[Name] = &[
    Name::ShortcutEvent,
    Name::InputReceived,
    Name::InputDequeued,
    Name::Effect,
    Name::Run,
    Name::Recording,
    Name::MicOpen,
    Name::FirstAudioDelivered,
    Name::FirstVadFrame,
    Name::FirstSpeech,
    Name::ReadyObserved,
    Name::CaptureStop,
    Name::ModelWait,
    Name::LiveFinalize,
    Name::BatchDecode,
    Name::PostProcess,
    Name::HistorySave,
    Name::Output,
];

#[cfg(test)]
const CODE_LIST: &[Code] = &[
    Code::Start,
    Code::Stop,
    Code::Typed,
    Code::Empty,
    Code::Silent,
    Code::Failed,
    Code::Cancelled,
    Code::NoModel,
    Code::MicrophonePermissionDenied,
    Code::NoInputDevice,
    Code::Unknown,
    Code::RecorderFailed,
    Code::SinkPanicked,
    Code::TranscriptionFailed,
    Code::PasteFailed,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg() -> ConfigSnapshot {
        ConfigSnapshot {
            model: "whisper-small".to_string(),
            live_decode: "none".to_string(),
            paste_method: "None".to_string(),
            push_to_talk: false,
            post_process_enabled: false,
            vad: true,
        }
    }

    fn start(rec: &Recorder, dir: &Path) {
        rec.start_inner(dir, cfg(), 1024, BYTE_CAP).unwrap();
    }

    fn parse(path: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn non_meta_events(v: &serde_json::Value) -> Vec<&serde_json::Value> {
        v["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["ph"].as_str() != Some("M"))
            .collect()
    }

    #[test]
    fn start_stop_roundtrip() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        rec.begin(run, Name::Run);
        rec.instant(Some(run), Name::FirstAudioDelivered, Args::default());
        rec.end(
            run,
            Name::Run,
            Args {
                code: Some(Code::Typed),
                ..Default::default()
            },
        );
        let path = rec.stop().unwrap();
        let v = parse(&path);

        let evs = non_meta_events(&v);
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0]["ph"], "b");
        assert_eq!(evs[0]["name"], "run");
        assert_eq!(evs[0]["id"].as_u64(), Some(run.as_u64()));
        assert_eq!(evs[1]["ph"], "i");
        assert_eq!(evs[1]["name"], "first_audio_delivered");
        assert_eq!(evs[2]["ph"], "e");
        assert_eq!(evs[2]["args"]["code"], "typed");

        // ts is non-decreasing per thread.
        let mut last_by_tid: HashMap<u64, u64> = HashMap::new();
        for e in &evs {
            let tid = e["tid"].as_u64().unwrap();
            let ts = e["ts"].as_u64().unwrap();
            if let Some(prev) = last_by_tid.get(&tid) {
                assert!(ts >= *prev, "ts went backwards on thread {tid}");
            }
            last_by_tid.insert(tid, ts);
        }

        assert_eq!(
            v["otherData"]["schema"].as_str().unwrap(),
            "handy-diagnostics/1"
        );
    }

    #[test]
    fn trace_preserves_optional_fields() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        rec.begin(run, Name::Run);
        rec.instant(
            None,
            Name::Effect,
            Args {
                code: Some(Code::Start),
                count: Some(0),
                flag: Some(false),
            },
        );
        rec.instant(
            Some(run),
            Name::FirstAudioDelivered,
            Args {
                count: Some(u64::MAX),
                flag: Some(true),
                ..Default::default()
            },
        );
        rec.end(run, Name::Run, Args::code(Code::Stop));
        let path = rec.stop().unwrap();
        let trace = parse(&path);
        let mut events: Vec<_> = non_meta_events(&trace).into_iter().cloned().collect();
        for event in &mut events {
            let fields = event.as_object_mut().unwrap();
            assert!(fields.remove("ts").unwrap().is_u64());
            assert!(fields.remove("tid").unwrap().as_u64().unwrap() > 0);
        }
        assert_eq!(
            events,
            serde_json::json!([
                {"ph": "b", "cat": "run", "name": "run", "id": run.as_u64(), "pid": 1},
                {"ph": "i", "s": "t", "name": "effect", "pid": 1,
                    "args": {"code": "start", "count": 0, "flag": false}},
                {"ph": "i", "s": "t", "name": "first_audio_delivered", "id": run.as_u64(),
                    "pid": 1, "args": {"count": u64::MAX, "flag": true}},
                {"ph": "e", "cat": "run", "name": "run", "id": run.as_u64(), "pid": 1,
                    "args": {"code": "stop"}}
            ])
            .as_array()
            .unwrap()
            .clone()
        );
        let process = trace["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["name"] == "process_name")
            .unwrap();
        assert_eq!(
            process,
            &serde_json::json!({"ph": "M", "name": "process_name", "pid": 1,
                "args": {"name": "Handy"}})
        );
    }

    #[test]
    fn thread_names_are_allowlisted() {
        let rec = Arc::new(Recorder::new());
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let names = [
            "handy-recorder-delivery",
            "tokio-runtime-worker",
            "private:/home/speaker/recording",
        ];
        for (index, name) in names.iter().enumerate() {
            let rec = Arc::clone(&rec);
            thread::Builder::new()
                .name((*name).to_owned())
                .spawn(move || {
                    rec.instant(
                        None,
                        Name::Effect,
                        Args {
                            count: Some(index as u64),
                            ..Default::default()
                        },
                    );
                })
                .unwrap()
                .join()
                .unwrap();
        }
        let path = rec.stop().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(names[2]));
        let trace = parse(&path);
        let events = trace["traceEvents"].as_array().unwrap();
        for (index, name) in names.iter().enumerate() {
            let event = events
                .iter()
                .find(|event| event["args"]["count"] == index as u64)
                .unwrap();
            let metadata = events
                .iter()
                .find(|metadata| {
                    metadata["name"] == "thread_name" && metadata["tid"] == event["tid"]
                })
                .unwrap();
            let expected = if index == 2 {
                format!("thread-{}", event["tid"].as_u64().unwrap())
            } else {
                (*name).to_owned()
            };
            assert_eq!(metadata["args"]["name"], expected);
        }
    }

    #[test]
    fn optional_operation_spans_preserve_outcomes() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        for run in [None, Some(run)] {
            let span = rec.span(run, Name::CaptureStop);
            span.end(Args {
                code: Some(Code::RecorderFailed),
                count: Some(0),
                flag: Some(false),
            });
        }
        rec.instant(None, Name::InputDequeued, Args::default());
        let trace = parse(&rec.stop().unwrap());
        let events = non_meta_events(&trace);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["ph"], "b");
        assert_eq!(events[1]["ph"], "e");
        for event in &events[..2] {
            assert_eq!(event["id"], run.as_u64());
            assert_eq!(event["name"], "capture_stop");
        }
        assert_eq!(
            events[1]["args"],
            serde_json::json!({"code": "recorder_failed", "count": 0, "flag": false})
        );
        assert_eq!(events[2]["name"], "input_dequeued");
        assert!(events[2].get("id").is_none());
    }

    #[test]
    fn operation_can_end_on_another_thread() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        let span = rec.span(run, Name::Output);
        thread::scope(|scope| {
            scope
                .spawn(move || span.end(Args::code(Code::Typed)))
                .join()
                .unwrap();
        });
        let trace = parse(&rec.stop().unwrap());
        let events = non_meta_events(&trace);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["name"], events[1]["name"]);
        assert_eq!(events[0]["id"], events[1]["id"]);
        assert_ne!(events[0]["tid"], events[1]["tid"]);
        assert_eq!(events[1]["args"]["code"], "typed");
        assert_eq!(trace["otherData"]["incomplete_spans"], 0);
    }

    #[test]
    fn abandoned_and_unwound_operations_remain_incomplete() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        {
            let _span = rec.span(run, Name::HistorySave);
        }
        let unwind = std::panic::catch_unwind(|| {
            let _span = rec.span(run, Name::Output);
            panic!("operation interrupted");
        });
        assert!(unwind.is_err());
        assert_eq!(rec.status().events, 2);
        let trace = parse(&rec.stop().unwrap());
        let events = non_meta_events(&trace);
        assert_eq!(events.len(), 4);
        assert_eq!(trace["otherData"]["incomplete_spans"], 2);
        for event in events.iter().filter(|event| event["ph"] == "e") {
            assert_eq!(event["args"], serde_json::json!({"incomplete": true}));
        }
    }

    #[test]
    fn operation_end_observes_current_capture() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        let run = rec.next_run();
        let inactive = rec.span(run, Name::Output);
        start(&rec, dir.path());
        inactive.end(Args::code(Code::Typed));
        let trace = parse(&rec.stop().unwrap());
        let events = non_meta_events(&trace);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["ph"], "e");
        assert_eq!(events[0]["args"]["code"], "typed");

        start(&rec, dir.path());
        let interrupted = rec.span(run, Name::Output);
        let trace = parse(&rec.stop().unwrap());
        assert_eq!(trace["otherData"]["incomplete_spans"], 1);
        start(&rec, dir.path());
        interrupted.end(Args::code(None));
        let trace = parse(&rec.stop().unwrap());
        let events = non_meta_events(&trace);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["ph"], "e");
        assert!(events[0].get("args").is_none());
    }

    #[test]
    fn inactive_records_nothing() {
        let rec = Recorder::new();
        for _ in 0..1000 {
            rec.instant(None, Name::Effect, Args::default());
        }
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let path = rec.stop().unwrap();
        let v = parse(&path);
        assert!(non_meta_events(&v).is_empty());
    }

    #[test]
    fn saturation_truncates_and_still_parses() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        rec.start_inner(dir.path(), cfg(), 1024, 2048).unwrap();
        for _ in 0..50 {
            rec.begin(rec.next_run(), Name::MicOpen);
        }
        let path = rec.stop().unwrap();
        let v = parse(&path);
        let other = &v["otherData"];
        assert!(
            other["truncated_events"].as_u64().unwrap() > 0
                || other["dropped_events"].as_u64().unwrap() > 0
        );
        assert!(v["traceEvents"].as_array().unwrap().len() < 50);
    }

    #[test]
    fn counters_reset_between_captures() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        rec.start_inner(dir.path(), cfg(), 1024, 2048).unwrap();
        // One deterministic drop (pre-epoch) on top of byte-cap truncation,
        // so the reset assertions below exercise both counters.
        rec.instant_at(
            Instant::now() - Duration::from_secs(5),
            None,
            Name::Effect,
            Args::default(),
        );
        for _ in 0..50 {
            rec.begin(rec.next_run(), Name::MicOpen);
        }
        let path1 = rec.stop().unwrap();
        assert_eq!(
            parse(&path1)["otherData"]["dropped_events"].as_u64(),
            Some(1)
        );

        start(&rec, dir.path());
        assert_eq!(rec.status().dropped, 0);
        let run = rec.next_run();
        rec.begin(run, Name::Run);
        rec.instant(Some(run), Name::Effect, Args::default());
        rec.end(run, Name::Run, Args::default());
        let path2 = rec.stop().unwrap();
        let v = parse(&path2);
        let other = &v["otherData"];
        assert_eq!(other["dropped_events"].as_u64(), Some(0));
        assert_eq!(other["truncated_events"].as_u64(), Some(0));
        assert_eq!(non_meta_events(&v).len(), 3);
    }

    #[test]
    fn open_span_marked_incomplete() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        rec.begin(run, Name::Run);
        rec.begin(run, Name::Recording);
        let path = rec.stop().unwrap();
        let v = parse(&path);
        let evs = non_meta_events(&v);
        let ends: Vec<_> = evs
            .iter()
            .filter(|e| e["ph"] == "e" && e["args"]["incomplete"] == true)
            .collect();
        assert_eq!(ends.len(), 2);
        assert_eq!(v["otherData"]["incomplete_spans"].as_u64(), Some(2));
    }

    #[test]
    fn double_start_double_stop() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let second = rec.start_inner(dir.path(), cfg(), 1024, BYTE_CAP).unwrap();
        assert!(second.active);
        let path = rec.stop().unwrap();
        assert!(matches!(rec.stop(), Err(DiagError::NotActive)));
        let mut files: Vec<PathBuf> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], path);
    }

    #[test]
    fn vocabulary_lists_are_complete() {
        assert_eq!(NAME_LIST.len(), 18);
        assert_eq!(CODE_LIST.len(), 15);
    }

    #[test]
    fn fixed_vocabulary_is_private() {
        let mut blob = String::new();
        for n in NAME_LIST {
            blob.push_str(n.as_str());
        }
        for c in CODE_LIST {
            blob.push_str(c.as_str());
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(!blob.contains('/'));
        assert!(!blob.contains('\\'));
        assert!(!blob.contains(dir.path().to_str().unwrap()));

        // Compile-level shape: payload and event cannot own heap strings
        // (String and PathBuf are not Copy). A borrowed `&'static str`
        // would still satisfy Copy; the vocabulary assertions above are
        // what pin down content.
        fn is_copy<T: Copy>() {}
        is_copy::<Args>();
        is_copy::<Event>();
    }

    #[test]
    fn pre_epoch_event_rejected() {
        let rec = Recorder::new();
        let dir = tempfile::tempdir().unwrap();
        start(&rec, dir.path());
        let run = rec.next_run();
        let past = Instant::now() - Duration::from_secs(5);
        rec.instant_at(past, Some(run), Name::InputReceived, Args::default());
        rec.instant(Some(run), Name::InputDequeued, Args::default());
        let path = rec.stop().unwrap();
        let v = parse(&path);
        let evs = non_meta_events(&v);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["name"], "input_dequeued");
        assert_eq!(v["otherData"]["dropped_events"].as_u64(), Some(1));
    }
}
