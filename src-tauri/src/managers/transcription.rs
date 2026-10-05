use crate::audio_toolkit::{
    apply_custom_words, detect_output_language, normalize_transcription_output,
    remove_filler_words, OutputLanguageEvidence,
};
use crate::chinese_script::{convert_chinese_script, ChineseVariety};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::model::{EngineType, ModelManager};
use crate::settings::{
    get_settings, AppSettings, ChineseScript, ModelUnloadTimeout, OrtAcceleratorSetting,
    TranscribeAcceleratorSetting,
};
use crate::vulkan_probe;
use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tauri_specta::Event;
use transcribe_cpp::{
    Backend, Feature, Model, ModelOptions, RunExtension, RunOptions, Session, StreamOptions, Task,
    WhisperRunOptions,
};
use transcribe_rs::{
    onnx::{
        canary::CanaryModel,
        cohere::CohereModel,
        gigaam::GigaAMModel,
        moonshine::{MoonshineModel, MoonshineVariant, StreamingModel},
        parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
        sense_voice::{SenseVoiceModel, SenseVoiceParams},
        Quantization,
    },
    SpeechModel, TranscribeOptions,
};

const STREAM_PERF_LOG_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_FINALIZE_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Whether a transcribe-cpp error is a compute-backend failure (driver-level),
/// as opposed to a per-request error (invalid args, unsupported language, …).
/// On the run/stream paths `Error::Backend` means the compute backend is no
/// longer usable — e.g. ggml-vulkan reporting device loss after the GPU hung
/// and the driver was reset, or a laptop dGPU being powered off.
fn is_transcribe_cpp_backend_failure(error: &transcribe_cpp::Error) -> bool {
    matches!(error, transcribe_cpp::Error::Backend(_))
}

/// Whether an anyhow error (as produced by the batch run path) wraps a
/// transcribe-cpp backend failure.
fn is_gpu_backend_failure_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<RemovedGpuDevice>().is_some()
        || error
            .downcast_ref::<transcribe_cpp::Error>()
            .is_some_and(is_transcribe_cpp_backend_failure)
}

/// Recovery evidence attached without replacing the original typed OOM error.
#[derive(Debug)]
struct RemovedGpuDevice(String);

/// Retain the failed device identity after its native engine is disposed.
#[derive(Debug)]
struct FailedGpuDeviceKey(String);

impl std::fmt::Display for FailedGpuDeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compute failure on GPU '{}'", self.0)
    }
}

impl std::fmt::Display for RemovedGpuDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GPU device '{}' disappeared during transcription",
            self.0
        )
    }
}

fn allocation_failed_on_removed_gpu(
    error: &transcribe_cpp::Error,
    bound_gpu_label: Option<&str>,
    probed: Option<&[String]>,
) -> bool {
    matches!(error, transcribe_cpp::Error::OutOfMemory(_))
        && bound_gpu_label.is_some_and(|label| {
            probed.is_some_and(|names| !names.iter().any(|name| name == label))
        })
}

fn transcribe_cpp_failure_requires_recovery(
    error: &transcribe_cpp::Error,
    bound_gpu_label: Option<&str>,
) -> bool {
    if is_transcribe_cpp_backend_failure(error) {
        return true;
    }
    if !matches!(error, transcribe_cpp::Error::OutOfMemory(_)) || bound_gpu_label.is_none() {
        return false;
    }
    // 0.3.0 reports graph-allocation failure as OOM even when its GPU was
    // powered off. Only confirmed removal warrants recovery; ordinary OOM,
    // CPU allocation failure, and unavailable probes retain their behavior.
    let probed = vulkan_probe::probe_gpu_device_names();
    let removed = allocation_failed_on_removed_gpu(error, bound_gpu_label, probed.as_deref());
    if removed {
        warn!(
            "transcribe-cpp returned {:?}; fresh Vulkan probe confirms GPU '{}' was removed",
            error,
            bound_gpu_label.unwrap_or_default()
        );
    }
    removed
}

/// How long GPU loads are skipped after a GPU-bound load attempt failed, so a
/// device whose logical handle is dead (driver TDR/reset) doesn't add a failed
/// load to every dictation. This is a retry backoff, not a ban: after the
/// window GPU loads may be tried again. Devices with compute failures are
/// excluded separately until restart.
const GPU_RETRY_COOLDOWN: Duration = Duration::from_secs(5 * 60);

/// Selection rank for compute devices: discrete GPU over integrated GPU over
/// everything else. Mirrors what ggml's `Auto` prefers.
fn transcribe_device_rank(device: &transcribe_cpp::Device) -> u8 {
    match device.device_type {
        transcribe_cpp::DeviceType::Gpu => 2,
        transcribe_cpp::DeviceType::Igpu => 1,
        _ => 0,
    }
}

/// Whether a registered device is confirmed present by a fresh probe. The
/// registry label is the driver-reported device name (e.g. "AMD Radeon(TM)
/// Graphics"), which is what the probe reports too.
fn device_binding_removed(is_gpu: bool, label: &str, probed: &[String]) -> bool {
    is_gpu && !probed.iter().any(|name| name == label)
}

/// One GPU-capable registry device, reduced to the data selection needs. Kept
/// free of `transcribe_cpp::Device` so the choice logic is unit-testable
/// (`Device` can't be constructed outside the binding crate).
#[derive(Debug, Clone, PartialEq)]
struct GpuCandidate {
    /// Index into the registry device list.
    index: usize,
    key: String,
    label: String,
    rank: u8,
}

/// Ordered load plan for the transcribe-cpp backend, given what we know about
/// the machine right now. The first entry is the primary choice; later entries
/// are fallbacks tried in order when a load fails.
///
/// - CPU preference / host-disabled GPU → `[(Cpu, None)]`.
/// - No probe result (probe unavailable) → `[(Auto, None)]`: ggml decides —
///   exactly the pre-hot-plug behavior.
/// - Otherwise: GPU devices confirmed present by the fresh probe, best rank
///   first (the stored user device first when present), then a CPU terminal
///   fallback. An active GPU retry cooldown skips GPU attempts entirely.
fn build_load_plan(
    setting: TranscribeAcceleratorSetting,
    gpu_disabled: bool,
    probe: Option<&[String]>,
    gpu_candidates: &[GpuCandidate],
    stored_gpu_key: Option<&str>,
    gpu_cooldown_active: bool,
    failed_gpu_keys: &[String],
) -> Vec<(Backend, Option<usize>)> {
    if gpu_disabled || setting == TranscribeAcceleratorSetting::Cpu {
        return vec![(Backend::Cpu, None)];
    }

    let Some(probed) = probe else {
        if gpu_cooldown_active || !failed_gpu_keys.is_empty() {
            // Automatic selection could pick the failed device again.
            return vec![(Backend::Cpu, None)];
        }
        // No fresh device information: keep ggml's automatic selection.
        return vec![(Backend::Auto, None)];
    };

    let mut candidates: Vec<GpuCandidate> = gpu_candidates
        .iter()
        .filter(|candidate| probed.iter().any(|name| name == &candidate.label))
        .filter(|candidate| !failed_gpu_keys.contains(&candidate.key))
        .cloned()
        .collect();

    if gpu_cooldown_active {
        candidates.clear();
    }

    // Rank automatic choices first, then give the saved device priority.
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.rank));
    if let Some(stored_key) = stored_gpu_key {
        match candidates
            .iter()
            .position(|candidate| candidate.key == stored_key)
        {
            Some(position) if position > 0 => {
                let stored = candidates.remove(position);
                candidates.insert(0, stored);
            }
            Some(_) => {}
            None => {
                warn!(
                    "Stored transcribe GPU device is not currently present; \
                     using the best available device"
                );
            }
        }
    }

    if candidates.is_empty() {
        return vec![(Backend::Cpu, None)];
    }

    let mut plan: Vec<(Backend, Option<usize>)> = candidates
        .into_iter()
        .map(|candidate| (Backend::Auto, Some(candidate.index)))
        .collect();
    plan.push((Backend::Cpu, None));
    plan
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub device_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StaleDeviceBinding {
    Removed,
}

/// Live transcription snapshot emitted to the overlay during a streaming run.
/// `committed` is the append-only, flicker-free prefix; `tentative` is the
/// volatile suffix the model may still rewrite.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct StreamTextEvent {
    pub committed: String,
    pub tentative: String,
}

/// Phase of the streaming overlay card, emitted to drive its UI state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StreamPhase {
    /// Receiving audio / live text (or waiting for the stream to begin). Rust
    /// does not emit this today; the frontend starts in this phase and Rust only
    /// emits transitions away from it.
    Listening,
    /// Finalizing or post-processing — show a spinner.
    Working,
}

/// Semantic kind of "working" phase, used to localize the spinner label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StreamWorkKind {
    Transcribing,
    Polishing,
}

/// Emitted to switch the streaming overlay to a working spinner.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct StreamPhaseEvent {
    pub phase: StreamPhase,
    /// Present only when `phase` is `Working`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<StreamWorkKind>,
}

/// Commands sent to the streaming worker thread. Audio frames and the finalize
/// request travel the same channel so FIFO ordering guarantees every fed frame
/// is processed before finalize runs.
enum StreamFinalizeReply {
    Finished(Option<FinalizedStreamText>),
    NeedsRecovery {
        model_id: String,
        failed_gpu_key: Option<String>,
    },
}

enum StreamCmd {
    Feed(Vec<f32>),
    /// Flush the stream and reply with the final text, or `None` if no stream
    /// was ever active (caller should fall back to batch transcription).
    Finalize(mpsc::Sender<StreamFinalizeReply>),
    Cancel,
}

struct FinalizedStreamText {
    text: String,
    output_language: OutputLanguageEvidence,
    /// The streaming model's supported languages, for text-based detection.
    supported_languages: Vec<String>,
}

/// Routes real-time audio frames to the active streaming worker. Shared between
/// the [`TranscriptionManager`] (opens/closes the route) and the audio recorder's
/// per-frame callback (feeds frames). The recorder holds an `Arc<StreamRouter>`
/// directly, so a frame with no stream pending costs a single relaxed atomic
/// load — no Tauri state lookup, no mutex lock.
pub struct StreamRouter {
    /// Command channel to the active streaming worker, present from
    /// `start_stream` until `finalize_stream`/`cancel_stream`.
    tx: Mutex<Option<mpsc::Sender<StreamCmd>>>,
    /// True while a stream is pending or active (channel is open). The audio
    /// callback checks this first to avoid the mutex lock when no stream runs.
    open: Arc<AtomicBool>,
}

impl StreamRouter {
    fn new() -> Self {
        Self {
            tx: Mutex::new(None),
            open: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open a fresh command channel for a new streaming session, returning the
    /// receiver the worker should drain. Caller must ensure no prior channel is
    /// still open.
    fn open(&self) -> mpsc::Receiver<StreamCmd> {
        let (tx, rx) = mpsc::channel::<StreamCmd>();
        *self.tx.lock().unwrap() = Some(tx);
        self.open.store(true, Ordering::Relaxed);
        rx
    }

    /// Take the sender out (closing the channel to new feeds). Returns the
    /// sender so the caller can send the final `Finalize`/`Cancel` command.
    fn take(&self) -> Option<mpsc::Sender<StreamCmd>> {
        self.open.store(false, Ordering::Relaxed);
        self.tx.lock().unwrap().take()
    }

    /// Drop the channel and mark closed without sending a final command (used
    /// when the worker exits without a finalize/cancel handshake).
    fn clear(&self) {
        self.open.store(false, Ordering::Relaxed);
        *self.tx.lock().unwrap() = None;
    }

    /// Forward a 16 kHz frame to the active streaming worker. Cheap no-op (a
    /// single relaxed atomic load) when no stream is pending.
    pub fn feed(&self, frame: &[f32]) {
        if !self.open.load(Ordering::Relaxed) {
            return;
        }
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(StreamCmd::Feed(frame.to_vec()));
        }
    }

    /// Whether a stream is pending or active.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }
}

enum LoadedEngine {
    /// Whisper-family models (whisper, breeze-asr, custom .bin/.gguf) via
    /// transcribe-cpp. Holds the live `Session`, which keeps its `Model` alive
    /// internally, so repeated dictation reuses the session without reloading.
    TranscribeCpp(Session),
    Parakeet(ParakeetModel),
    Moonshine(MoonshineModel),
    MoonshineStreaming(StreamingModel),
    SenseVoice(SenseVoiceModel),
    GigaAM(GigaAMModel),
    Canary(CanaryModel),
    Cohere(CohereModel),
}

fn loaded_engine_gpu_key(engine: &LoadedEngine) -> Option<String> {
    let LoadedEngine::TranscribeCpp(session) = engine else {
        return None;
    };
    session
        .model()
        .device()
        .ok()
        .filter(is_transcribe_gpu_device)
        .map(|device| transcribe_device_key(&device))
}

/// RAII guard that clears the `is_loading` flag and notifies waiters on drop.
/// Ensures the loading flag is always reset, even on early returns or panics.
pub struct LoadingGuard {
    is_loading: Arc<Mutex<bool>>,
    loading_condvar: Arc<Condvar>,
}

impl Drop for LoadingGuard {
    fn drop(&mut self) {
        // Recover from a poisoned mutex instead of panicking —
        // a panic inside Drop calls abort().
        let mut is_loading = match self.is_loading.lock() {
            Ok(g) => g,
            Err(e) => {
                warn!("Recovered poisoned is_loading mutex during LoadingGuard drop — a panic occurred earlier this session");
                e.into_inner()
            }
        };
        *is_loading = false;
        self.loading_condvar.notify_all();
    }
}

/// RAII guard that clears the streaming worker/lease flags on any worker exit -
/// normal return, early return, or a panic in an engine call that unwinds the
/// detached worker thread. Tokens prevent an older worker from clearing a newer
/// worker's state if a start/finalize race ever slips through.
struct StreamWorkerGuard {
    worker_id: u64,
    active_stream_worker: Arc<AtomicU64>,
    active_engine_lease: Arc<AtomicU64>,
    stream_active: Arc<AtomicBool>,
}

impl Drop for StreamWorkerGuard {
    fn drop(&mut self) {
        if self.active_stream_worker.load(Ordering::Acquire) == self.worker_id {
            self.stream_active.store(false, Ordering::Release);
        }
        let _ = self.active_engine_lease.compare_exchange(
            self.worker_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        let _ = self.active_stream_worker.compare_exchange(
            self.worker_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[derive(Clone)]
pub struct TranscriptionManager {
    engine: Arc<Mutex<Option<LoadedEngine>>>,
    model_manager: Arc<ModelManager>,
    app_handle: AppHandle,
    current_model_id: Arc<Mutex<Option<String>>>,
    last_activity: Arc<AtomicU64>,
    shutdown_signal: Arc<AtomicBool>,
    watcher_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    is_loading: Arc<Mutex<bool>>,
    loading_condvar: Arc<Condvar>,
    reload_model_on_next_use: Arc<AtomicBool>,
    /// Routes real-time audio frames to the active streaming worker; see
    /// [`StreamRouter`]. Shared with the audio recorder so per-frame feeds skip
    /// Tauri state and the manager lock.
    router: Arc<StreamRouter>,
    /// True only while a transcribe-cpp `Stream` is actually in flight (set by
    /// the worker once `stream()` succeeds). Used for overlay/UI decisions.
    stream_active: Arc<AtomicBool>,
    /// Streaming uses four independent flags: router open = frames should route,
    /// worker active = no second worker may start, engine lease = engine is out
    /// of the mutex, stream active = UI should show a live session.
    ///
    /// Monotonic id source for stream workers; zero means "no worker".
    next_stream_worker_id: Arc<AtomicU64>,
    /// Nonzero while a stream worker exists, even if it has not leased the engine
    /// yet. This prevents a second worker from starting after finalize/cancel
    /// closes the router but before the first worker has fully exited.
    active_stream_worker: Arc<AtomicU64>,
    /// Nonzero while the streaming worker has taken the engine out of `engine`.
    /// `is_model_loaded()` consults this so the model still reports "loaded"
    /// while the worker holds it.
    active_engine_lease: Arc<AtomicU64>,
    /// When the last GPU-bound model load failed. Used to back off GPU attempts
    /// for [`GPU_RETRY_COOLDOWN`] so a dead device (driver TDR/reset, dGPU
    /// removed) doesn't add a failed load attempt to every dictation. Not a
    /// ban: GPU loads may be retried after the window. Compute failures are
    /// tracked separately until restart.
    last_gpu_load_failure: Arc<Mutex<Option<Instant>>>,
    /// Compute failures invalidate the native device handle for this process.
    /// Keep those devices out of subsequent loads until Handy restarts.
    failed_gpu_keys: Arc<Mutex<Vec<String>>>,
}

impl TranscriptionManager {
    pub fn new(app_handle: &AppHandle, model_manager: Arc<ModelManager>) -> Result<Self> {
        let manager = Self {
            engine: Arc::new(Mutex::new(None)),
            model_manager,
            app_handle: app_handle.clone(),
            current_model_id: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(AtomicU64::new(Self::now_ms())),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            watcher_handle: Arc::new(Mutex::new(None)),
            is_loading: Arc::new(Mutex::new(false)),
            loading_condvar: Arc::new(Condvar::new()),
            reload_model_on_next_use: Arc::new(AtomicBool::new(false)),
            router: Arc::new(StreamRouter::new()),
            stream_active: Arc::new(AtomicBool::new(false)),
            next_stream_worker_id: Arc::new(AtomicU64::new(1)),
            active_stream_worker: Arc::new(AtomicU64::new(0)),
            active_engine_lease: Arc::new(AtomicU64::new(0)),
            last_gpu_load_failure: Arc::new(Mutex::new(None)),
            failed_gpu_keys: Arc::new(Mutex::new(Vec::new())),
        };

        // Start the idle watcher
        {
            let app_handle_cloned = app_handle.clone();
            let manager_cloned = manager.clone();
            let shutdown_signal = manager.shutdown_signal.clone();
            let handle = thread::spawn(move || {
                debug!("Idle watcher thread started");
                while !shutdown_signal.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(10)); // Check every 10 seconds

                    // Check shutdown signal again after sleep
                    if shutdown_signal.load(Ordering::Relaxed) {
                        break;
                    }

                    let settings = get_settings(&app_handle_cloned);
                    let timeout = settings.model_unload_timeout;

                    // Skip Immediately — that variant is handled by
                    // maybe_unload_immediately() after each transcription.
                    // Treating it as 0s here would unload the model mid-recording.
                    if timeout == ModelUnloadTimeout::Immediately {
                        continue;
                    }

                    // While recording, keep the idle timer fresh so the
                    // model is never unloaded mid-session.
                    let is_recording = app_handle_cloned
                        .try_state::<Arc<AudioRecordingManager>>()
                        .is_some_and(|a| a.is_recording());
                    if is_recording {
                        manager_cloned.touch_activity();
                        continue;
                    }

                    if let Some(limit_seconds) = timeout.to_seconds() {
                        let last = manager_cloned.last_activity.load(Ordering::Relaxed);
                        let now_ms = TranscriptionManager::now_ms();
                        let idle_ms = now_ms.saturating_sub(last);
                        let limit_ms = limit_seconds * 1000;

                        if idle_ms > limit_ms {
                            // idle -> unload
                            if manager_cloned.is_model_loaded() {
                                let unload_start = std::time::Instant::now();
                                info!(
                                    "Model idle for {}s (limit: {}s), unloading",
                                    idle_ms / 1000,
                                    limit_seconds
                                );
                                match manager_cloned.unload_model() {
                                    Ok(()) => {
                                        let unload_duration = unload_start.elapsed();
                                        info!(
                                            "Model unloaded due to inactivity (took {}ms)",
                                            unload_duration.as_millis()
                                        );
                                    }
                                    Err(e) => {
                                        error!("Failed to unload idle model: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
                debug!("Idle watcher thread shutting down gracefully");
            });
            *manager.watcher_handle.lock().unwrap() = Some(handle);
        }

        Ok(manager)
    }

    /// Lock the engine mutex, recovering from poison if a previous transcription panicked.
    fn lock_engine(&self) -> MutexGuard<'_, Option<LoadedEngine>> {
        self.engine.lock().unwrap_or_else(|poisoned| {
            warn!("Engine mutex was poisoned by a previous panic, recovering");
            poisoned.into_inner()
        })
    }

    pub fn is_model_loaded(&self) -> bool {
        // The engine may be leased out to the streaming worker (taken out of
        // the mutex). It's still loaded, just in use, so report true.
        self.lock_engine().is_some() || self.active_engine_lease.load(Ordering::Acquire) != 0
    }

    /// Accelerator changes should not disturb the current transcription. Mark
    /// the cached engine stale; the next model-use path reloads it with the
    /// latest settings.
    pub fn reload_model_on_next_use(&self) {
        self.reload_model_on_next_use.store(true, Ordering::Release);
    }

    /// Atomically check whether a model load is in progress and, if not, mark
    /// one as starting. Returns a [`LoadingGuard`] whose [`Drop`] impl will
    /// clear the flag and wake waiters. Returns `None` if a load is already in
    /// progress.
    pub fn try_start_loading(&self) -> Option<LoadingGuard> {
        let mut is_loading = self.is_loading.lock().unwrap();
        if *is_loading {
            return None;
        }
        *is_loading = true;
        Some(LoadingGuard {
            is_loading: self.is_loading.clone(),
            loading_condvar: self.loading_condvar.clone(),
        })
    }

    pub fn unload_model(&self) -> Result<()> {
        let unload_start = std::time::Instant::now();
        debug!("Starting to unload model");

        {
            let mut engine = self.lock_engine();
            // Dropping the engine frees all resources
            *engine = None;
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = None;
        }

        // Emit unloaded event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "unloaded".to_string(),
                model_id: None,
                model_name: None,
                device_name: None,
                error: None,
            },
        );

        let unload_duration = unload_start.elapsed();
        debug!(
            "Model unloaded manually (took {}ms)",
            unload_duration.as_millis()
        );
        Ok(())
    }

    /// Record that a GPU-bound model load just failed, starting the retry
    /// cooldown window.
    fn note_gpu_load_failure(&self) {
        *self.last_gpu_load_failure.lock().unwrap() = Some(Instant::now());
    }

    /// Whether GPU load attempts are currently in the retry cooldown window.
    fn gpu_cooldown_active(&self) -> bool {
        self.last_gpu_load_failure
            .lock()
            .unwrap()
            .is_some_and(|at| at.elapsed() < GPU_RETRY_COOLDOWN)
    }

    fn note_failed_compute_device(&self, key: Option<&str>) {
        if let Some(key) = key {
            let mut failed = self.failed_gpu_keys.lock().unwrap();
            if !failed.iter().any(|failed_key| failed_key == key) {
                failed.push(key.to_string());
            }
        } else {
            // Without an identity, automatic selection could reuse the failure.
            self.note_gpu_load_failure();
        }
    }

    /// Handle a hard GPU compute failure (e.g. ggml-vulkan device loss after a
    /// Windows driver reset/TDR, or a laptop dGPU powering off mid-stream):
    /// drop the engine bound to the failed device, notify the UI, and (best
    /// effort) reload the model on the best device that
    /// currently exists, excluding devices that have already failed in this
    /// process, or the CPU when nothing else is available. Returns true
    /// when a freshly loaded engine is available.
    fn recover_backend_failure(&self, model_id: &str, failed_gpu_key: Option<String>) -> bool {
        error!(
            "GPU compute backend failed (device lost / driver reset); \
             recovering on the best available device"
        );

        // Drop the engine that hit the failure — its native context is bound
        // to the failed device and must not be reused.
        {
            let mut engine = self.lock_engine();
            let failed_gpu_key =
                failed_gpu_key.or_else(|| engine.as_ref().and_then(loaded_engine_gpu_key));
            self.note_failed_compute_device(failed_gpu_key.as_deref());
            *engine = None;
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = None;
        }

        match self.load_model(model_id) {
            Ok(()) => {
                self.emit_device_fallback_notification(model_id);
                true
            }
            Err(e) => {
                error!("Failed to reload model after GPU failure: {}", e);
                false
            }
        }
    }

    fn current_transcription_device_name(&self) -> Option<String> {
        let engine = self.lock_engine();
        let Some(LoadedEngine::TranscribeCpp(session)) = engine.as_ref() else {
            return None;
        };
        let model = session.model();
        let Ok(device) = model.device() else {
            return None;
        };
        match device.device_type {
            transcribe_cpp::DeviceType::Cpu => Some("cpu".to_string()),
            _ if !device.description.trim().is_empty() => Some(device.description),
            _ => Some(device.name),
        }
    }

    fn emit_device_fallback_notification(&self, model_id: &str) {
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "device_fallback".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: None,
                device_name: self.current_transcription_device_name(),
                error: None,
            },
        );
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// Reset the idle timer to now.
    fn touch_activity(&self) {
        self.last_activity.store(Self::now_ms(), Ordering::Relaxed);
    }

    /// Unloads the model immediately if the setting is enabled and the model is loaded
    pub fn maybe_unload_immediately(&self, context: &str) {
        let settings = get_settings(&self.app_handle);
        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately
            && self.is_model_loaded()
        {
            info!("Immediately unloading model after {}", context);
            if let Err(e) = self.unload_model() {
                warn!("Failed to immediately unload model: {}", e);
            }
        }
    }

    pub fn load_model(&self, model_id: &str) -> Result<()> {
        self.load_model_with_device(model_id, None)
    }

    /// Like [`load_model`](Self::load_model), but lets a caller hard-select the
    /// compute device for this one load by its `transcribe_cpp::devices()`
    /// registry index (the index shown by `--list-devices`). `None` keeps the
    /// persisted accelerator setting (which may be Auto). Only affects
    /// transcribe-cpp (whisper-family) models; the selection is not persisted.
    pub fn load_model_with_device(
        &self,
        model_id: &str,
        device_index: Option<usize>,
    ) -> Result<()> {
        apply_accelerator_settings(&self.app_handle);

        let load_start = std::time::Instant::now();
        debug!("Starting to load model: {}", model_id);

        // Emit loading started event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_started".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: None,
                device_name: None,
                error: None,
            },
        );

        let model_info = match self.model_manager.get_model_info(model_id) {
            Some(model_info) => model_info,
            None => {
                let error_msg = format!("Model not found: {}", model_id);
                let _ = self.app_handle.emit(
                    "model-state-changed",
                    ModelStateEvent {
                        event_type: "loading_failed".to_string(),
                        model_id: Some(model_id.to_string()),
                        model_name: None,
                        device_name: None,
                        error: Some(error_msg.clone()),
                    },
                );
                return Err(anyhow::anyhow!(error_msg));
            }
        };

        // Every failure after loading starts must emit a terminal event so the
        // frontend can never remain in its loading state.
        let emit_loading_failed = |error_msg: &str| {
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    model_id: Some(model_id.to_string()),
                    model_name: Some(model_info.name.clone()),
                    device_name: None,
                    error: Some(error_msg.to_string()),
                },
            );
        };

        if !model_info.is_downloaded {
            let error_msg = "Model not downloaded";
            emit_loading_failed(error_msg);
            return Err(anyhow::anyhow!(error_msg));
        }

        let model_path = self
            .model_manager
            .get_model_path(model_id)
            .inspect_err(|error| emit_loading_failed(&error.to_string()))?;

        // Drop the current engine BEFORE building the new one so transcribe-cpp
        // frees the previous native context first — avoids holding two models at
        // once (peak memory on large GGUFs). Clear the id too: if the new load
        // fails, status should read "no loaded model", not the dropped engine.
        {
            let mut engine = self.lock_engine();
            *engine = None;
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = None;
        }

        // Create appropriate engine based on model type

        let loaded_engine = match model_info.engine_type {
            EngineType::TranscribeCpp => {
                // The whisper backend is chosen at load time (transcribe-cpp has
                // no runtime global). With an explicit `device_index` (the
                // --device-index flag) hard-select that registered device;
                // otherwise resolve the persisted accelerator preference against
                // the devices that actually exist right now (fresh probe, so
                // hot-plugged/removed GPUs are seen), with ordered fallbacks.
                let load_plan: Vec<(Backend, Option<transcribe_cpp::Device>)> = match device_index {
                    Some(index) => vec![resolve_device_index(index).inspect_err(|e| {
                        emit_loading_failed(&e.to_string());
                    })?],
                    None => {
                        let settings = get_settings(&self.app_handle);
                        let gpu_registry = transcribe_compute_devices();
                        let gpu_candidates: Vec<GpuCandidate> = gpu_registry
                            .iter()
                            .enumerate()
                            .filter(|(_, device)| is_transcribe_gpu_device(device))
                            .map(|(index, device)| GpuCandidate {
                                index,
                                key: transcribe_device_key(device),
                                label: transcribe_device_label(device),
                                rank: transcribe_device_rank(device),
                            })
                            .collect();
                        let probe = vulkan_probe::probe_gpu_device_names();
                        if probe.is_none() {
                            debug!("GPU device probe unavailable; using ggml automatic selection");
                        }
                        build_load_plan(
                            settings.transcribe_accelerator,
                            transcribe_gpu_disabled_for_host(),
                            probe.as_deref(),
                            &gpu_candidates,
                            settings.transcribe_gpu_device.as_deref(),
                            self.gpu_cooldown_active(),
                            &self.failed_gpu_keys.lock().unwrap(),
                        )
                        .into_iter()
                        .map(|(backend, index)| (backend, index.map(|i| gpu_registry[i].clone())))
                        .collect()
                    }
                };

                // Try the plan in order. A failed GPU-bound attempt starts the
                // retry cooldown and falls through to the next device (finally
                // the CPU); the last error is surfaced if nothing loads.
                let mut loaded = None;
                let mut last_error: Option<anyhow::Error> = None;
                let mut loaded_with = (Backend::Auto, "automatic".to_string());
                for (backend, device) in load_plan {
                    let is_gpu_attempt = device.is_some();
                    let requested_device = device
                        .as_ref()
                        .map(transcribe_device_label)
                        .unwrap_or_else(|| "automatic".to_string());
                    match Model::load_with(&model_path, &ModelOptions { backend, device }) {
                        Ok(model) => {
                            loaded_with = (backend, requested_device);
                            loaded = Some(model);
                            break;
                        }
                        Err(e) => {
                            if is_gpu_attempt && last_error.is_none() {
                                self.note_gpu_load_failure();
                            }
                            warn!(
                                "Failed to load whisper model '{}' on '{}': {}",
                                model_id, requested_device, e
                            );
                            last_error = Some(anyhow::Error::new(e));
                        }
                    }
                }
                let model = match loaded {
                    Some(model) => model,
                    None => {
                        let error_msg = format!(
                            "Failed to load whisper model {}: {}",
                            model_id,
                            last_error
                                .unwrap_or_else(|| anyhow::anyhow!("no load attempt was made"))
                        );
                        emit_loading_failed(&error_msg);
                        return Err(anyhow::anyhow!(error_msg));
                    }
                };
                let (backend, requested_device) = loaded_with;
                // The bound backend may differ from the request (e.g. CPU
                // fallback under Auto); log what actually loaded.
                let bound_backend = model.backend();
                let session = model.session().map_err(|e| {
                    let error_msg = format!(
                        "Failed to create session for whisper model {}: {}",
                        model_id, e
                    );
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
                // Reconcile the registry's advertised capabilities with the
                // loaded model's real ones (GGUF metadata) so badges/gating
                // reflect runtime truth, not the pre-download probe. The
                // load-completed event below triggers the frontend refresh.
                let caps = session.model().capabilities();
                self.model_manager.set_runtime_capabilities(
                    model_id,
                    caps.supports_streaming,
                    caps.supports_translate,
                    caps.supports_language_detect,
                    caps.languages.clone(),
                );
                let bound_device = model
                    .device()
                    .map(|device| transcribe_device_label(&device))
                    .unwrap_or_else(|_| "unknown".to_string());
                info!(
                    "Loaded whisper model '{}' (requested {:?}, requested device '{}', \
                     bound backend '{}', bound device '{}', supports_streaming={}, \
                     supports_translate={}, supports_language_detect={})",
                    model_id,
                    backend,
                    requested_device,
                    bound_backend,
                    bound_device,
                    caps.supports_streaming,
                    caps.supports_translate,
                    caps.supports_language_detect
                );
                LoadedEngine::TranscribeCpp(session)
            }
            EngineType::Parakeet => {
                let engine =
                    ParakeetModel::load(&model_path, &Quantization::Int8).map_err(|e| {
                        let error_msg =
                            format!("Failed to load parakeet model {}: {}", model_id, e);
                        emit_loading_failed(&error_msg);
                        anyhow::anyhow!(error_msg)
                    })?;
                LoadedEngine::Parakeet(engine)
            }
            EngineType::Moonshine => {
                let engine = MoonshineModel::load(
                    &model_path,
                    MoonshineVariant::Base,
                    &Quantization::default(),
                )
                .map_err(|e| {
                    let error_msg = format!("Failed to load moonshine model {}: {}", model_id, e);
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
                LoadedEngine::Moonshine(engine)
            }
            EngineType::MoonshineStreaming => {
                let engine = StreamingModel::load(&model_path, 0, &Quantization::default())
                    .map_err(|e| {
                        let error_msg = format!(
                            "Failed to load moonshine streaming model {}: {}",
                            model_id, e
                        );
                        emit_loading_failed(&error_msg);
                        anyhow::anyhow!(error_msg)
                    })?;
                LoadedEngine::MoonshineStreaming(engine)
            }
            EngineType::SenseVoice => {
                let engine =
                    SenseVoiceModel::load(&model_path, &Quantization::Int8).map_err(|e| {
                        let error_msg =
                            format!("Failed to load SenseVoice model {}: {}", model_id, e);
                        emit_loading_failed(&error_msg);
                        anyhow::anyhow!(error_msg)
                    })?;
                LoadedEngine::SenseVoice(engine)
            }
            EngineType::GigaAM => {
                let engine = GigaAMModel::load(&model_path, &Quantization::Int8).map_err(|e| {
                    let error_msg = format!("Failed to load gigaam model {}: {}", model_id, e);
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
                LoadedEngine::GigaAM(engine)
            }
            EngineType::Canary => {
                let engine = CanaryModel::load(&model_path, &Quantization::Int8).map_err(|e| {
                    let error_msg = format!("Failed to load canary model {}: {}", model_id, e);
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
                LoadedEngine::Canary(engine)
            }
            EngineType::Cohere => {
                let engine = CohereModel::load(&model_path, &Quantization::Int8).map_err(|e| {
                    let error_msg = format!("Failed to load cohere model {}: {}", model_id, e);
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
                LoadedEngine::Cohere(engine)
            }
        };

        // Update the current engine and model ID
        {
            let mut engine = self.lock_engine();
            *engine = Some(loaded_engine);
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = Some(model_id.to_string());
        }

        // Reset idle timer so the watcher doesn't immediately unload a just-loaded model
        self.touch_activity();

        // Emit loading completed event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_completed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                device_name: None,
                error: None,
            },
        );

        let load_duration = load_start.elapsed();
        debug!(
            "Successfully loaded transcription model: {} (took {}ms)",
            model_id,
            load_duration.as_millis()
        );
        Ok(())
    }

    /// Kicks off the model loading in a background thread if it's not already loaded
    pub fn initiate_model_load(&self) {
        let mut is_loading = self.is_loading.lock().unwrap();
        if *is_loading {
            return;
        }

        let reload_pending = self.reload_model_on_next_use.load(Ordering::Acquire);
        let mut removed_device = false;
        if !reload_pending && self.is_model_loaded() {
            // Hot-plug check: if the loaded model is bound to a device that no
            // longer exists, reload onto current hardware instead of keeping
            // the stale binding.
            if let Some(reason) = self.device_binding_stale() {
                info!("Loaded model's device was removed; reloading onto current hardware");
                if let Err(e) = self.unload_model() {
                    warn!("Failed to unload stale-bound model: {}", e);
                    return;
                }
                removed_device = reason == StaleDeviceBinding::Removed;
                // Fall through to the reload below.
            } else {
                return;
            }
        }

        *is_loading = true;
        let self_clone = self.clone();
        thread::spawn(move || {
            if reload_pending {
                self_clone
                    .reload_model_on_next_use
                    .store(false, Ordering::Release);
            }
            let settings = get_settings(&self_clone.app_handle);
            match self_clone.load_model(&settings.selected_model) {
                Ok(()) if removed_device => {
                    self_clone.emit_device_fallback_notification(&settings.selected_model);
                }
                Ok(()) => {}
                Err(e) => error!("Failed to load model: {}", e),
            }
            let mut is_loading = self_clone.is_loading.lock().unwrap();
            *is_loading = false;
            self_clone.loading_condvar.notify_all();
        });
    }

    /// Whether the loaded transcribe-cpp model's device binding no longer
    /// matches current hardware: the bound device disappeared (laptop dGPU
    /// powered off). Keep a working binding, including an explicit CPU or iGPU
    /// choice, until a setting change or restart. CPU bindings return without
    /// probing Vulkan. GPU bindings require a fresh probe; without one this
    /// returns `None`, keeping the pre-hot-plug behavior.
    fn device_binding_stale(&self) -> Option<StaleDeviceBinding> {
        let bound = {
            let engine = self.lock_engine();
            let Some(LoadedEngine::TranscribeCpp(session)) = engine.as_ref() else {
                return None;
            };
            match session.model().device() {
                Ok(device) if is_transcribe_gpu_device(&device) => device,
                _ => return None,
            }
        };

        let Some(probed) = vulkan_probe::probe_gpu_device_names() else {
            return None; // no fresh information; keep the current binding
        };

        if device_binding_removed(
            is_transcribe_gpu_device(&bound),
            &transcribe_device_label(&bound),
            &probed,
        ) {
            info!(
                "Bound GPU device '{}' is no longer present on the system",
                transcribe_device_label(&bound)
            );
            return Some(StaleDeviceBinding::Removed);
        }

        None
    }

    pub fn get_current_model(&self) -> Option<String> {
        let current_model = self.current_model_id.lock().unwrap();
        current_model.clone()
    }

    /// The compute backend the currently-loaded engine is bound to, for
    /// diagnostics (e.g. confirming `--device-index` actually bound a GPU rather
    /// than falling back to CPU/auto). transcribe-cpp (whisper-family) reports
    /// its real backend string; ONNX engines report "onnx"; `None` when no
    /// model is loaded.
    pub fn current_backend(&self) -> Option<String> {
        match self.lock_engine().as_ref() {
            Some(LoadedEngine::TranscribeCpp(session)) => {
                Some(session.model().backend().to_string())
            }
            Some(_) => Some("onnx".to_string()),
            None => None,
        }
    }

    /// Whether a live streaming run is currently in flight.
    pub fn is_streaming(&self) -> bool {
        self.stream_active.load(Ordering::Acquire)
    }

    /// Shared handle to the stream router, used by the audio recorder to feed
    /// real-time frames without going through Tauri state on every frame.
    pub fn stream_router(&self) -> Arc<StreamRouter> {
        Arc::clone(&self.router)
    }

    /// Begin a live streaming transcription on the held engine's session.
    /// Audio frames pushed via [`StreamRouter::feed`] (captured directly by the
    /// audio recorder) are decoded incrementally and emitted to the overlay as
    /// [`StreamTextEvent`].
    ///
    /// Non-blocking: spawns a worker that waits for any in-progress model load,
    /// verifies the model supports streaming, then begins the stream. If the
    /// model can't stream, the worker idles until finalize/cancel and reports
    /// `None` so the caller falls back to batch transcription. Frames sent
    /// before the stream begins queue on the channel and are not lost.
    pub fn start_stream(&self) {
        if self.router.is_open() || self.active_stream_worker.load(Ordering::Acquire) != 0 {
            warn!("start_stream called while a stream worker is already active");
            return;
        }
        let worker_id = self.next_stream_worker_id.fetch_add(1, Ordering::Relaxed);
        if self
            .active_stream_worker
            .compare_exchange(0, worker_id, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            warn!("start_stream lost a race with another stream worker");
            return;
        }
        let rx = self.router.open();
        self.stream_active.store(false, Ordering::Release);

        let manager = self.clone();
        thread::spawn(move || manager.run_stream_worker(rx, worker_id));
    }

    fn run_stream_worker(&self, rx: mpsc::Receiver<StreamCmd>, worker_id: u64) {
        let _worker = StreamWorkerGuard {
            worker_id,
            active_stream_worker: Arc::clone(&self.active_stream_worker),
            active_engine_lease: Arc::clone(&self.active_engine_lease),
            stream_active: Arc::clone(&self.stream_active),
        };

        // Wait for any in-progress model load to finish (start_stream races the
        // background load kicked off when recording starts).
        {
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }
        }

        let model_id = self.get_current_model().unwrap_or_default();

        // Take the engine out of the mutex so we own it during streaming,
        // structurally excluding any concurrent batch transcription (which
        // transcribe-cpp's compute_lock would refuse anyway). Returned when the
        // worker exits, or dropped if the model was switched/unloaded mid-stream.
        if self
            .active_engine_lease
            .compare_exchange(0, worker_id, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            warn!("Live preview: another worker already holds the transcription engine");
            self.router.clear();
            drain_until_finalize(rx);
            return;
        }
        let mut engine = match self.lock_engine().take() {
            Some(e) => e,
            None => {
                info!(
                    "Live preview: model '{}' was unloaded before streaming could begin; \
                     falling back to batch transcription",
                    model_id
                );
                let _ = self.active_engine_lease.compare_exchange(
                    worker_id,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                self.router.clear();
                drain_until_finalize(rx);
                return;
            }
        };

        // Only transcribe-cpp models expose streaming; ONNX engines fall back to
        // batch. The loaded session (not the ModelManager copy) is the source of
        // truth for run-path capabilities.
        let (supports_streaming, supports_translate, languages) = match &engine {
            LoadedEngine::TranscribeCpp(session) => {
                let model = session.model();
                let caps = model.capabilities();
                info!(
                    "Live preview: model '{}' arch='{}' variant='{}' supports_streaming={} \
                     supports_translate={} languages={:?}",
                    model_id,
                    model.arch(),
                    model.variant(),
                    caps.supports_streaming,
                    caps.supports_translate,
                    caps.languages,
                );
                (
                    caps.supports_streaming,
                    caps.supports_translate,
                    caps.languages,
                )
            }
            _ => {
                info!(
                    "Live preview: model '{}' is not a transcribe-cpp model; \
                     streaming is unavailable, using batch transcription",
                    model_id
                );
                (false, false, Vec::new())
            }
        };

        if !supports_streaming {
            self.return_engine(engine, &model_id);
            self.router.clear();
            drain_until_finalize(rx);
            return;
        }

        // Build run options mirroring the offline transcribe-cpp path: task +
        // language gated against what the model actually advertises.
        let settings = get_settings(&self.app_handle);
        let effective_language =
            effective_language_for_model(&settings, self.model_manager.as_ref(), &model_id);
        let run_plan = transcribe_cpp_run_plan(
            settings.translate_to_english,
            &effective_language,
            &languages,
            supports_translate,
        );
        let output_language = resolve_output_language_evidence(
            &settings,
            run_plan.language.as_deref(),
            &languages,
            run_plan.target_language.as_deref() == Some("en"),
        );
        let run_options = RunOptions {
            task: run_plan.task,
            language: run_plan.language,
            target_language: run_plan.target_language,
            ..Default::default()
        };

        // Run the stream on the held session. The Stream borrows the session
        // (and thus the engine) for its lifetime, so the feed/finalize loop
        // lives in a labeled block — when it exits, the borrow is released and
        // the engine can be moved into return_engine().
        let mut preview_script = PreviewScript::new(settings.chinese_script, &output_language);
        let mut finalize_reply: Option<mpsc::Sender<StreamFinalizeReply>> = None;
        let mut finalize_result: Option<Option<FinalizedStreamText>> = None;
        // Set when the compute backend dies mid-stream (device removed, driver
        // reset, …). Further work on the dead device is skipped — feeding (and
        // even finalize) can abort the process in ggml otherwise — the dead
        // engine is dropped, and the model reloads on the best device so the
        // batch fallback below still produces text.
        let mut gpu_backend_failed = false;
        let stream_started = 'stream: {
            let session = match &mut engine {
                LoadedEngine::TranscribeCpp(s) => s,
                _ => break 'stream false,
            };

            // Read the backend string before beginning the stream — the
            // `Stream` borrows `session` mutably for its lifetime, so we can't
            // call `session.model()` once it exists.
            let backend = session.model().backend();
            let bound_gpu_label = session
                .model()
                .device()
                .ok()
                .filter(is_transcribe_gpu_device)
                .map(|device| transcribe_device_label(&device));

            // StreamOptions::default() uses CommitPolicy::Auto and lets the
            // family pick its own streaming strategy (no family-specific ext).
            let mut stream = match session.stream(&run_options, &StreamOptions::default()) {
                Ok(s) => s,
                Err(e) => {
                    if transcribe_cpp_failure_requires_recovery(&e, bound_gpu_label.as_deref()) {
                        error!("Failed to begin stream (backend failure): {}", e);
                        gpu_backend_failed = true;
                    } else {
                        error!("Failed to begin stream: {}", e);
                    }
                    break 'stream false;
                }
            };

            self.stream_active.store(true, Ordering::Release);
            self.touch_activity();
            info!(
                "Live streaming transcription started (model '{}', backend '{}')",
                model_id, backend
            );

            let mut perf = StreamPerf::new();
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    StreamCmd::Feed(pcm) => {
                        if gpu_backend_failed {
                            // Device is gone; drain frames without touching it.
                            continue;
                        }
                        self.touch_activity();
                        perf.record_feed(pcm.len());
                        let feed_start = Instant::now();
                        match stream.feed(&pcm) {
                            Ok(update) => {
                                perf.record_compute(feed_start.elapsed());
                                perf.record_update(
                                    update.revision,
                                    update.input_received_ms,
                                    update.audio_committed_ms,
                                    update.buffered_ms,
                                );
                                if update.committed_changed || update.tentative_changed {
                                    let text = stream.text();
                                    perf.record_emit();
                                    let (committed, tentative) = preview_script.convert(
                                        &text.committed,
                                        &text.tentative,
                                        &languages,
                                    );
                                    self.emit_stream_text(&committed, &tentative);
                                }
                                perf.maybe_log();
                            }
                            Err(e) => {
                                perf.record_compute(feed_start.elapsed());
                                if transcribe_cpp_failure_requires_recovery(
                                    &e,
                                    bound_gpu_label.as_deref(),
                                ) {
                                    error!(
                                        "stream feed failed fatally ({}); abandoning live stream",
                                        e
                                    );
                                    gpu_backend_failed = true;
                                } else {
                                    warn!("stream feed failed: {}", e);
                                }
                            }
                        }
                    }
                    StreamCmd::Finalize(reply) => {
                        let finalize_start = Instant::now();
                        // With a dead backend there is nothing to finalize;
                        // report "no stream text" so the caller falls back to
                        // batch transcription on a recovered engine.
                        let result = if gpu_backend_failed {
                            perf.record_compute(finalize_start.elapsed());
                            None
                        } else {
                            match stream.finalize() {
                                // After finalize the committed prefix holds the full
                                // text; display() = committed + tentative is the safe read.
                                Ok(update) => {
                                    perf.record_compute(finalize_start.elapsed());
                                    perf.record_update(
                                        update.revision,
                                        update.input_received_ms,
                                        update.audio_committed_ms,
                                        update.buffered_ms,
                                    );
                                    // In auto mode the model's own LID is the best
                                    // remaining evidence; the snapshot is only
                                    // materialized when it can change the outcome.
                                    let output_language = match &output_language {
                                        OutputLanguageEvidence::Unknown => {
                                            with_model_detected_language(
                                                OutputLanguageEvidence::Unknown,
                                                stream.snapshot().language,
                                            )
                                        }
                                        resolved => resolved.clone(),
                                    };
                                    Some(FinalizedStreamText {
                                        text: stream.text().full,
                                        output_language,
                                        supported_languages: languages.clone(),
                                    })
                                }
                                Err(e) => {
                                    perf.record_compute(finalize_start.elapsed());
                                    if transcribe_cpp_failure_requires_recovery(
                                        &e,
                                        bound_gpu_label.as_deref(),
                                    ) {
                                        gpu_backend_failed = true;
                                    }
                                    error!(
                                        "stream finalize failed: {}; falling back to batch transcription",
                                        e
                                    );
                                    None
                                }
                            }
                        };
                        let chars = match &result {
                            Some(finalized) => finalized.text.len(),
                            _ => 0,
                        };
                        perf.log_finalized(chars);
                        finalize_reply = Some(reply);
                        finalize_result = Some(result);
                        break;
                    }
                    StreamCmd::Cancel => {
                        if !gpu_backend_failed {
                            stream.reset();
                        }
                        break;
                    }
                }
            }

            true
        };
        // `stream` + the `&mut engine` borrow are released here.

        if !stream_started {
            // Stream never began (model doesn't support streaming or begin
            // failed); drain so the finalize handshake still completes and the
            // caller falls back to batch transcription. Return the engine first
            // so the fallback can immediately use it — unless the backend
            // failed, in which case the dead engine is dropped and recovered
            // onto the best available device instead.
            if gpu_backend_failed {
                let failed_gpu_key = loaded_engine_gpu_key(&engine);
                self.note_failed_compute_device(failed_gpu_key.as_deref());
                drop(engine);
                drop(_worker);
                drain_until_finalize_with_reply(
                    rx,
                    StreamFinalizeReply::NeedsRecovery {
                        model_id,
                        failed_gpu_key,
                    },
                );
            } else {
                self.return_engine(engine, &model_id);
                drop(_worker);
                drain_until_finalize(rx);
            }
            return;
        }

        let result = if gpu_backend_failed {
            // Drop the engine bound to the dead device (do not return it to
            // the pool) before reloading on the best available device.
            // The caller recovers outside the timed finalize handshake.
            let failed_gpu_key = loaded_engine_gpu_key(&engine);
            self.note_failed_compute_device(failed_gpu_key.as_deref());
            drop(engine);
            StreamFinalizeReply::NeedsRecovery {
                model_id,
                failed_gpu_key,
            }
        } else {
            self.return_engine(engine, &model_id);
            StreamFinalizeReply::Finished(finalize_result.flatten())
        };
        // Release the lease before waking a caller that may start batch work.
        drop(_worker);
        if let Some(reply) = finalize_reply {
            let _ = reply.send(result);
        }
    }

    /// Return the leased engine to the mutex, unless the model was switched or
    /// unloaded during transcription (in which case the stale engine is dropped).
    fn return_engine(&self, engine: LoadedEngine, expected_model_id: &str) {
        let still_current =
            self.current_model_id.lock().unwrap().as_deref() == Some(expected_model_id);
        if still_current {
            *self.lock_engine() = Some(engine);
        } else {
            info!(
                "Model changed/unloaded during transcription; dropping stale engine (was '{}')",
                expected_model_id
            );
            // `engine` drops here, freeing its resources.
        }
    }

    /// Flush the active stream and return its final, post-filtered text.
    ///
    /// `Ok(None)` means no usable stream was active and the caller may fall back
    /// to batch transcription. `Err` means finalize itself failed or timed out.
    /// A timeout may still leave the worker holding the engine, so callers
    /// should surface it instead of immediately starting a batch fallback.
    pub fn finalize_stream(&self) -> Result<Option<String>> {
        let Some(tx) = self.router.take() else {
            return Ok(None);
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.send(StreamCmd::Finalize(reply_tx)).is_err() {
            return Ok(None);
        }
        let finalized = match receive_stream_finalization(
            reply_rx,
            STREAM_FINALIZE_REPLY_TIMEOUT,
            |model_id, failed_gpu_key| self.recover_backend_failure(model_id, failed_gpu_key),
        ) {
            Ok(Some(finalized)) => finalized,
            Ok(None) => return Ok(None),
            Err(error) => {
                self.stream_active.store(false, Ordering::Release);
                return Err(error);
            }
        };

        let settings = get_settings(&self.app_handle);
        // Streaming models do not receive a decode prompt, so custom words
        // always go through the shared fuzzy post-correction path.
        let filtered = post_process_transcription_text(
            finalized.text,
            &settings,
            false,
            &finalized.output_language,
            &finalized.supported_languages,
        );

        self.maybe_unload_immediately("streaming transcription");
        Ok(Some(filtered))
    }

    /// Abandon any active stream without producing text (e.g. on cancel).
    pub fn cancel_stream(&self) {
        if let Some(tx) = self.router.take() {
            let _ = tx.send(StreamCmd::Cancel);
        }
        self.stream_active.store(false, Ordering::Release);
    }

    /// Emit a working-phase event to the streaming overlay (spinner + label).
    pub fn emit_stream_working(&self, kind: StreamWorkKind) {
        let _ = StreamPhaseEvent {
            phase: StreamPhase::Working,
            kind: Some(kind),
        }
        .emit(&self.app_handle);
    }

    fn emit_stream_text(&self, committed: &str, tentative: &str) {
        let _ = StreamTextEvent {
            committed: committed.to_string(),
            tentative: tentative.to_string(),
        }
        .emit(&self.app_handle);
    }

    /// Run batch transcription, recovering automatically if the GPU compute
    /// backend dies mid-run (e.g. ggml-vulkan device loss after a Windows
    /// driver reset/TDR on some AMD iGPUs). On such a failure the
    /// dead engine is dropped, the model reloads on the best available device,
    /// and the audio is transcribed once more so the current utterance is not
    /// lost. Failed GPU loads start the temporary GPU retry cooldown.
    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        match self.transcribe_inner(audio.clone()) {
            Err(e) if is_gpu_backend_failure_error(&e) => {
                let model_id = self.get_current_model().unwrap_or_default();
                let failed_gpu_key = e
                    .downcast_ref::<FailedGpuDeviceKey>()
                    .map(|key| key.0.clone());
                if self.recover_backend_failure(&model_id, failed_gpu_key) {
                    self.transcribe_inner(audio)
                } else {
                    Err(e)
                }
            }
            other => other,
        }
    }

    fn transcribe_inner(&self, audio: Vec<f32>) -> Result<String> {
        #[cfg(debug_assertions)]
        if std::env::var("HANDY_FORCE_TRANSCRIPTION_FAILURE").is_ok() {
            return Err(anyhow::anyhow!(
                "Simulated transcription failure (HANDY_FORCE_TRANSCRIPTION_FAILURE)"
            ));
        }

        // Update last activity timestamp
        self.touch_activity();

        let st = std::time::Instant::now();
        let audio_len = audio.len();

        debug!("Audio vector length: {}", audio_len);

        if audio.is_empty() {
            debug!("Empty audio vector");
            self.maybe_unload_immediately("empty audio");
            return Ok(String::new());
        }

        // Check if model is loaded, if not try to load it
        {
            // If the model is loading, wait for it to complete.
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }

            let engine_guard = self.lock_engine();
            if engine_guard.is_none() {
                return Err(anyhow::anyhow!("Model is not loaded for transcription."));
            }
        }

        // Get current settings for configuration
        let settings = get_settings(&self.app_handle);

        // Validate selected language against the model's supported languages.
        // If the language isn't supported, fall back to "auto" to prevent errors.
        // Validate against the model that's actually loaded (which can differ
        // from settings.selected_model when a caller loaded a specific model —
        // e.g. the --transcribe-file path's --model), not the persisted
        // selection.
        let active_model = self
            .get_current_model()
            .unwrap_or_else(|| settings.selected_model.clone());
        // Resolve the persisted language *intent* into the language this model
        // will actually use. The coercion is capability-aware (a must-pick model
        // never receives "auto") and computed fresh here — it is never written
        // back to settings, so the intent survives switching models and back.
        let validated_language =
            effective_language_for_model(&settings, self.model_manager.as_ref(), &active_model);
        if validated_language != settings.selected_language {
            debug!(
                "Language intent '{}' resolved to '{}' for model '{}'",
                settings.selected_language, validated_language, active_model
            );
        }

        // Whether the loaded transcribe-cpp model advertises
        // Feature::InitialPrompt. Informational (logged below); the whisper
        // run extension and the fuzzy-correction skip are gated on
        // `model_is_whisper` instead, since non-whisper archs can advertise
        // the feature while rejecting the whisper-kind extension.
        let mut model_takes_initial_prompt = false;
        // Whether the loaded model is actually whisper-family (arch string).
        // Non-whisper archs (e.g. Voxtral Small) can advertise
        // Feature::InitialPrompt yet reject the whisper-kind run extension
        // with INVALID_ARG, so the whisper extension must be gated on the
        // arch, not on the feature (see #1601).
        let mut model_is_whisper = false;

        // Perform transcription with the appropriate engine.
        // We use catch_unwind to prevent engine panics from poisoning the mutex,
        // which would make the app hang indefinitely on subsequent operations.
        let (result, output_language, model_languages) = {
            let mut engine_guard = self.lock_engine();

            // Take the engine out so we own it during transcription.
            // If the engine panics, we simply don't put it back (effectively unloading it)
            // instead of poisoning the mutex.
            let mut engine = match engine_guard.take() {
                Some(e) => e,
                None => {
                    return Err(anyhow::anyhow!(
                        "Model failed to load after auto-load attempt. Please check your model settings."
                    ));
                }
            };

            // Release the lock before transcribing — no mutex held during the engine call
            drop(engine_guard);

            // Probe live transcribe-cpp capabilities once (cheap GGUF-metadata
            // reads); the loaded session is the source of truth, not the
            // ModelManager copy. The whisper run extension is kind-tagged, so
            // non-whisper archs (parakeet, voxtral, …) reject it with
            // INVALID_ARG; attach it — and translate — only where supported.
            let mut model_supports_translate = false;
            let mut model_languages = self
                .model_manager
                .get_model_info(&active_model)
                .map(|info| info.supported_languages)
                .unwrap_or_default();
            let mut output_was_translated = false;
            let mut applied_language_hint: Option<String> = None;
            let mut model_detected_language: Option<String> = None;
            let mut bound_gpu_label = None;
            if let LoadedEngine::TranscribeCpp(session) = &engine {
                let model = session.model();
                bound_gpu_label = model
                    .device()
                    .ok()
                    .filter(is_transcribe_gpu_device)
                    .map(|device| transcribe_device_label(&device));
                let caps = model.capabilities();
                model_takes_initial_prompt = model.supports(Feature::InitialPrompt);
                model_is_whisper = model.arch() == "whisper";
                model_supports_translate = caps.supports_translate;
                model_languages = caps.languages;
                debug!(
                    "transcribe-cpp model '{}' on '{}': initial_prompt={}, translate={}, languages={:?}",
                    settings.selected_model,
                    model.backend(),
                    model_takes_initial_prompt,
                    model_supports_translate,
                    model_languages
                );
            }

            let transcribe_result = catch_unwind(AssertUnwindSafe(|| -> Result<String> {
                match &mut engine {
                    LoadedEngine::TranscribeCpp(session) => {
                        // Custom words become the initial prompt ONLY for models
                        // that accept one (whisper family). Attaching the
                        // whisper run extension to a non-whisper arch is rejected
                        // with INVALID_ARG, so skip it there and let the fuzzy
                        // post-correction handle custom words instead.
                        let family = if settings.custom_words.is_empty() || !model_is_whisper {
                            None
                        } else {
                            Some(RunExtension::Whisper(WhisperRunOptions {
                                initial_prompt: Some(settings.custom_words.join(", ")),
                                ..Default::default()
                            }))
                        };

                        let run_plan = transcribe_cpp_run_plan(
                            settings.translate_to_english,
                            &validated_language,
                            &model_languages,
                            model_supports_translate,
                        );
                        output_was_translated = run_plan.target_language.as_deref() == Some("en");
                        applied_language_hint = run_plan.language.clone();

                        let run_options = RunOptions {
                            task: run_plan.task,
                            language: run_plan.language,
                            target_language: run_plan.target_language,
                            family,
                            ..Default::default()
                        };

                        debug!(
                            "transcribe-cpp run: task={:?}, language={:?}, initial_prompt={}",
                            run_options.task,
                            run_options.language,
                            run_options.family.is_some()
                        );

                        session
                            .run(&audio, &run_options)
                            .map(|t| {
                                // Whisper's audio-based LID (auto mode only;
                                // `None` when a language hint was passed).
                                model_detected_language = t.language;
                                t.text
                            })
                            // Preserve the typed error so GPU backend failures
                            // (Error::Backend, e.g. Vulkan device loss) can be
                            // detected and recovered from by `transcribe`.
                            .map_err(|e| {
                                let removed_gpu = !is_transcribe_cpp_backend_failure(&e)
                                    && transcribe_cpp_failure_requires_recovery(
                                        &e,
                                        bound_gpu_label.as_deref(),
                                    );
                                let error = anyhow::Error::new(e);
                                let error = if removed_gpu {
                                    error.context(RemovedGpuDevice(
                                        bound_gpu_label.clone().unwrap_or_default(),
                                    ))
                                } else {
                                    error
                                };
                                error.context("transcribe-cpp transcription failed")
                            })
                    }
                    LoadedEngine::Parakeet(parakeet_engine) => {
                        let params = ParakeetParams {
                            timestamp_granularity: Some(TimestampGranularity::Segment),
                            ..Default::default()
                        };
                        parakeet_engine
                            .transcribe_with(&audio, &params)
                            .map(|r| r.text)
                            .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {}", e))
                    }
                    LoadedEngine::Moonshine(moonshine_engine) => moonshine_engine
                        .transcribe(&audio, &TranscribeOptions::default())
                        .map(|r| r.text)
                        .map_err(|e| anyhow::anyhow!("Moonshine transcription failed: {}", e)),
                    LoadedEngine::MoonshineStreaming(streaming_engine) => streaming_engine
                        .transcribe(&audio, &TranscribeOptions::default())
                        .map(|r| r.text)
                        .map_err(|e| {
                            anyhow::anyhow!("Moonshine streaming transcription failed: {}", e)
                        }),
                    LoadedEngine::SenseVoice(sense_voice_engine) => {
                        let language = match validated_language.as_str() {
                            "zh" => Some("zh".to_string()),
                            "en" => Some("en".to_string()),
                            "ja" => Some("ja".to_string()),
                            "ko" => Some("ko".to_string()),
                            "yue" => Some("yue".to_string()),
                            _ => None,
                        };
                        applied_language_hint = language.clone();
                        let params = SenseVoiceParams {
                            language,
                            use_itn: Some(true),
                        };
                        sense_voice_engine
                            .transcribe_with(&audio, &params)
                            .map(|r| r.text)
                            .map_err(|e| anyhow::anyhow!("SenseVoice transcription failed: {}", e))
                    }
                    LoadedEngine::GigaAM(gigaam_engine) => gigaam_engine
                        .transcribe(&audio, &TranscribeOptions::default())
                        .map(|r| r.text)
                        .map_err(|e| anyhow::anyhow!("GigaAM transcription failed: {}", e)),
                    LoadedEngine::Canary(canary_engine) => {
                        output_was_translated = settings.translate_to_english;
                        let lang = if validated_language == "auto" {
                            None
                        } else {
                            Some(validated_language.clone())
                        };
                        applied_language_hint = lang.clone();
                        let options = TranscribeOptions {
                            language: lang,
                            translate: settings.translate_to_english,
                            ..Default::default()
                        };
                        canary_engine
                            .transcribe(&audio, &options)
                            .map(|r| r.text)
                            .map_err(|e| anyhow::anyhow!("Canary transcription failed: {}", e))
                    }
                    LoadedEngine::Cohere(cohere_engine) => {
                        let lang = if validated_language == "auto" {
                            None
                        } else {
                            Some(validated_language.clone())
                        };
                        applied_language_hint = lang.clone();
                        let options = TranscribeOptions {
                            language: lang,
                            ..Default::default()
                        };
                        cohere_engine
                            .transcribe(&audio, &options)
                            .map(|r| r.text)
                            .map_err(|e| anyhow::anyhow!("Cohere transcription failed: {}", e))
                    }
                }
            }));

            let text = match transcribe_result {
                Ok(mut inner_result) => {
                    // A backend failure invalidates the native context, including
                    // on the final retry. Never return that engine to the pool.
                    if inner_result
                        .as_ref()
                        .is_err_and(is_gpu_backend_failure_error)
                    {
                        let failed_gpu_key = loaded_engine_gpu_key(&engine);
                        self.note_failed_compute_device(failed_gpu_key.as_deref());
                        if let Some(key) = failed_gpu_key {
                            inner_result = inner_result
                                .map_err(|error| error.context(FailedGpuDeviceKey(key)));
                        }
                        drop(engine);
                    } else {
                        self.return_engine(engine, &active_model);
                    }
                    inner_result?
                }
                Err(panic_payload) => {
                    // Engine panicked — do NOT put it back (it's in an unknown state).
                    // The engine is dropped here, effectively unloading it.
                    let panic_msg = panic_payload_message(panic_payload.as_ref());
                    error!(
                        "Transcription engine panicked: {}. Model has been unloaded.",
                        panic_msg
                    );

                    // Clear the model ID so it will be reloaded on next attempt
                    {
                        let mut current_model = self
                            .current_model_id
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *current_model = None;
                    }

                    let _ = self.app_handle.emit(
                        "model-state-changed",
                        ModelStateEvent {
                            event_type: "unloaded".to_string(),
                            model_id: None,
                            model_name: None,
                            device_name: None,
                            error: Some(format!("Engine panicked: {}", panic_msg)),
                        },
                    );

                    return Err(anyhow::anyhow!(
                        "Transcription engine panicked: {}. The model has been unloaded and will reload on next attempt.",
                        panic_msg
                    ));
                }
            };

            let output_language = with_model_detected_language(
                resolve_output_language_evidence(
                    &settings,
                    applied_language_hint.as_deref(),
                    &model_languages,
                    output_was_translated,
                ),
                model_detected_language,
            );
            debug!("Output language evidence: {:?}", output_language);

            (text, output_language, model_languages)
        };

        // Apply fuzzy word correction if custom words are configured — UNLESS the
        // words were already handed to the model as an initial prompt (whisper
        // family). We don't pass a prompt to non-whisper models (it requires the
        // whisper-kind run extension), so they still get fuzzy correction here,
        // same as the ONNX engines.
        let filtered_result = post_process_transcription_text(
            result,
            &settings,
            model_is_whisper,
            &output_language,
            &model_languages,
        );

        let et = std::time::Instant::now();
        let translation_note = if settings.translate_to_english {
            " (translated)"
        } else {
            ""
        };
        // Real-time factor. Input PCM is 16 kHz mono, so audio length in seconds
        // is samples / 16000. `speedup` is audio_secs / elapsed_secs — e.g. 4.00x
        // means transcribed 4x faster than real time
        let elapsed_secs = (et - st).as_secs_f64();
        let audio_secs = audio_len as f64 / 16_000.0;
        let speedup = real_time_factor(audio_secs, elapsed_secs);
        info!(
            "Transcription completed in {:.2}s for {:.2}s of audio ({:.2}x real-time){}",
            elapsed_secs, audio_secs, speedup, translation_note
        );

        let final_result = filtered_result;

        if final_result.is_empty() {
            info!("Transcription result is empty");
        } else {
            info!(
                "Transcription result: {}",
                crate::utils::redact_text(&final_result)
            );
        }

        self.maybe_unload_immediately("transcription");

        Ok(final_result)
    }
}

struct StreamPerf {
    feed_count: u64,
    emit_count: u64,
    streamed_samples: u64,
    stream_compute_elapsed: Duration,
    last_log: Instant,
    latest_revision: i32,
    latest_input_received_ms: i64,
    latest_audio_committed_ms: i64,
    latest_buffered_ms: i64,
}

impl StreamPerf {
    fn new() -> Self {
        Self {
            feed_count: 0,
            emit_count: 0,
            streamed_samples: 0,
            stream_compute_elapsed: Duration::ZERO,
            last_log: Instant::now(),
            latest_revision: 0,
            latest_input_received_ms: 0,
            latest_audio_committed_ms: 0,
            latest_buffered_ms: 0,
        }
    }

    fn record_feed(&mut self, samples: usize) {
        self.feed_count += 1;
        self.streamed_samples += samples as u64;
    }

    fn record_compute(&mut self, elapsed: Duration) {
        self.stream_compute_elapsed += elapsed;
    }

    fn record_update(
        &mut self,
        revision: i32,
        input_received_ms: i64,
        audio_committed_ms: i64,
        buffered_ms: i64,
    ) {
        self.latest_revision = revision;
        self.latest_input_received_ms = input_received_ms;
        self.latest_audio_committed_ms = audio_committed_ms;
        self.latest_buffered_ms = buffered_ms;
    }

    fn record_emit(&mut self) {
        self.emit_count += 1;
    }

    fn maybe_log(&mut self) {
        if self.last_log.elapsed() < STREAM_PERF_LOG_INTERVAL {
            return;
        }

        let audio_secs = self.audio_secs();
        let compute_secs = self.compute_secs();
        debug!(
            "Live preview perf: {:.2}s streamed audio, {:.2}s model compute ({:.2}x real-time), \
             input_received={:.2}s, committed_audio={:.2}s, buffered={}ms, revision={}, \
             {} frames fed, {} updates emitted",
            audio_secs,
            compute_secs,
            real_time_factor(audio_secs, compute_secs),
            self.latest_input_received_ms as f64 / 1000.0,
            self.latest_audio_committed_ms as f64 / 1000.0,
            self.latest_buffered_ms,
            self.latest_revision,
            self.feed_count,
            self.emit_count,
        );
        self.last_log = Instant::now();
    }

    fn log_finalized(&self, chars: usize) {
        let audio_secs = self.audio_secs();
        let compute_secs = self.compute_secs();
        info!(
            "Live preview finalized in {:.2}s model compute for {:.2}s streamed audio ({:.2}x real-time): \
             input_received={:.2}s, committed_audio={:.2}s, buffered={}ms, revision={}, \
             {} frames fed, {} updates emitted, {} chars",
            compute_secs,
            audio_secs,
            real_time_factor(audio_secs, compute_secs),
            self.latest_input_received_ms as f64 / 1000.0,
            self.latest_audio_committed_ms as f64 / 1000.0,
            self.latest_buffered_ms,
            self.latest_revision,
            self.feed_count,
            self.emit_count,
            chars
        );
    }

    fn audio_secs(&self) -> f64 {
        self.streamed_samples as f64 / 16_000.0
    }

    fn compute_secs(&self) -> f64 {
        self.stream_compute_elapsed.as_secs_f64()
    }
}

fn real_time_factor(audio_secs: f64, compute_secs: f64) -> f64 {
    if compute_secs > 0.0 {
        audio_secs / compute_secs
    } else {
        0.0
    }
}

/// Resolve the persisted language intent into the language a specific model can
/// use without writing the coerced value back to settings.
fn effective_language_for_model(
    settings: &AppSettings,
    model_manager: &ModelManager,
    model_id: &str,
) -> String {
    match model_manager.get_model_info(model_id) {
        Some(info) => crate::managers::model::effective_language(
            &settings.selected_language,
            &info.supported_languages,
            info.supports_language_detection,
        ),
        None => settings.selected_language.clone(),
    }
}

/// Resolve how confidently Handy knows the language of the text produced by a
/// transcription run. The UI language is deliberately not part of this
/// decision.
fn resolve_output_language_evidence(
    settings: &AppSettings,
    applied_language_hint: Option<&str>,
    supported_languages: &[String],
    translated_to_english: bool,
) -> OutputLanguageEvidence {
    if translated_to_english {
        return OutputLanguageEvidence::TranslatedToEnglish;
    }

    // Stored language intent is only evidence when this specific engine run
    // actually received the hint. Some multilingual engines (notably Parakeet
    // V3) always auto-detect and ignore Handy's selection; transcribe-cpp also
    // drops a requested hint when the loaded model does not advertise it.
    if let Some(language) = applied_language_hint.filter(|lang| !lang.is_empty() && *lang != "auto")
    {
        if settings.selected_language != "auto"
            && crate::managers::model::canonical_language_code(&settings.selected_language)
                == crate::managers::model::canonical_language_code(language)
        {
            return OutputLanguageEvidence::UserSelected(language.to_string());
        }

        // The engine may have required a concrete fallback even though the
        // user's persisted language was auto or unsupported.
        return OutputLanguageEvidence::ModelConstrained(language.to_string());
    }

    // A single-language model has a known output language without needing a
    // selectable language hint.
    if let [language] = supported_languages {
        return OutputLanguageEvidence::ModelConstrained(language.clone());
    }

    OutputLanguageEvidence::Unknown
}

/// Upgrade [`OutputLanguageEvidence::Unknown`] with the language the model
/// itself detected during the run (audio-based LID, e.g. Whisper in auto
/// mode). Stronger evidence resolved before the run is never overridden.
fn with_model_detected_language(
    evidence: OutputLanguageEvidence,
    detected: Option<String>,
) -> OutputLanguageEvidence {
    match (evidence, detected) {
        (OutputLanguageEvidence::Unknown, Some(language))
            if !language.is_empty() && language != "auto" =>
        {
            OutputLanguageEvidence::ModelDetected(language)
        }
        (evidence, _) => evidence,
    }
}

struct TranscribeCppRunPlan {
    task: Task,
    language: Option<String>,
    target_language: Option<String>,
}

/// Build the transcribe-cpp language/task options shared by batch and live
/// streaming paths.
fn transcribe_cpp_run_plan(
    translate_to_english: bool,
    effective_language: &str,
    model_languages: &[String],
    model_supports_translate: bool,
) -> TranscribeCppRunPlan {
    let requested_language = match effective_language {
        "auto" => None,
        other => Some(other.to_string()),
    };
    // Only pass a language the loaded model actually advertises (per
    // capabilities().languages); otherwise auto-detect rather than failing with
    // UNSUPPORTED_LANGUAGE. Language-agnostic models report an empty list, so
    // they always stay on auto.
    let language = requested_language.filter(|lang| model_languages.iter().any(|l| l == lang));
    let (task, target_language) = cpp_translation_task(
        translate_to_english,
        model_supports_translate,
        language.as_deref(),
    );

    TranscribeCppRunPlan {
        task,
        language,
        target_language,
    }
}

fn post_process_transcription_text(
    raw: String,
    settings: &AppSettings,
    custom_words_already_prompted: bool,
    output_language: &OutputLanguageEvidence,
    supported_languages: &[String],
) -> String {
    let converts_script = settings.chinese_script != ChineseScript::AsTranscribed;
    fail_open_text_transform(raw, |raw| {
        // Last-resort language evidence: confidence-gated detection from the
        // transcribed text itself, constrained to the model's languages. Only
        // consulted when it can change the outcome (built-in gated fillers or
        // Chinese script conversion).
        let output_language = match output_language {
            OutputLanguageEvidence::Unknown
                if converts_script
                    || (settings.filler_word_removal_enabled
                        && settings.custom_filler_words.is_none()) =>
            {
                match detect_output_language(&raw, supported_languages) {
                    Some(language) => {
                        debug!("Text-based language detection resolved '{}'", language);
                        OutputLanguageEvidence::TextDetected(language)
                    }
                    None => OutputLanguageEvidence::Unknown,
                }
            }
            other => other.clone(),
        };

        // Convert the script before custom words so they match in the script
        // the user writes in. Only output known to be Chinese is touched, so
        // e.g. Japanese kanji are never rewritten.
        let variety = output_language
            .language()
            .and_then(ChineseVariety::from_language);
        let raw = match variety {
            Some(variety) if converts_script => {
                convert_chinese_script(&raw, variety, settings.chinese_script)
            }
            _ => raw,
        };

        let corrected = if !settings.custom_words.is_empty() && !custom_words_already_prompted {
            apply_custom_words(
                &raw,
                &settings.custom_words,
                settings.word_correction_threshold,
            )
        } else {
            raw
        };

        let without_fillers = remove_filler_words(
            &corrected,
            &output_language,
            &settings.custom_filler_words,
            settings.filler_word_removal_enabled,
        );

        normalize_transcription_output(&without_fillers)
    })
}

/// Characters to wait for before detecting the preview's language. A lone
/// Chinese character reads as Mandarin, but Japanese often opens with kanji
/// before any kana; waiting a few characters keeps those from locking in.
const PREVIEW_DETECTION_MIN_CHARS: usize = 6;

/// Converts live-preview text into the configured Chinese script as it streams.
///
/// The preview is cosmetic: the final paste re-resolves the language and
/// converts on its own, so this only has to look right while speaking.
struct PreviewScript {
    script: ChineseScript,
    variety: Option<ChineseVariety>,
    /// The language wasn't known when the stream started (auto-detect), so it
    /// is detected from the streamed text until it turns out to be Chinese.
    detect: bool,
}

impl PreviewScript {
    fn new(script: ChineseScript, output_language: &OutputLanguageEvidence) -> Self {
        let enabled = script != ChineseScript::AsTranscribed;
        Self {
            script,
            variety: output_language
                .language()
                .and_then(ChineseVariety::from_language)
                .filter(|_| enabled),
            detect: enabled && *output_language == OutputLanguageEvidence::Unknown,
        }
    }

    /// Converts the model's raw committed/tentative text. Always fed the raw
    /// text, never a previous conversion, so it stays consistent with the
    /// final conversion. Once Chinese is detected it sticks for the rest of
    /// the stream so the preview doesn't flip back and forth.
    fn convert(
        &mut self,
        committed: &str,
        tentative: &str,
        supported_languages: &[String],
    ) -> (String, String) {
        if self.variety.is_none() && self.detect {
            let text = format!("{committed}{tentative}");
            if text.chars().count() >= PREVIEW_DETECTION_MIN_CHARS {
                self.variety = detect_output_language(&text, supported_languages)
                    .as_deref()
                    .and_then(ChineseVariety::from_language);
            }
        }

        match self.variety {
            Some(variety) => (
                convert_chinese_script(committed, variety, self.script),
                convert_chinese_script(tentative, variety, self.script),
            ),
            None => (committed.to_string(), tentative.to_string()),
        }
    }
}

/// Optional text cleanup must never discard a successful model result. The
/// transform is pure and owns its input, so recovering the untouched text is
/// safe even if a bug in custom-word or filler filtering unwinds.
fn fail_open_text_transform<F>(raw: String, transform: F) -> String
where
    F: FnOnce(String) -> String,
{
    let fallback = raw.clone();
    match catch_unwind(AssertUnwindSafe(|| transform(raw))) {
        Ok(processed) => processed,
        Err(payload) => {
            error!(
                "Optional transcription text post-processing panicked: {}; using the raw transcription",
                panic_payload_message(payload.as_ref())
            );
            fallback
        }
    }
}

/// Decide a transcribe-cpp run's task + translation target from settings.
///
/// "Translate to English" only fires where the model advertises translation.
/// Unlike transcribe-rs (which forces the target to English itself when its
/// `translate` flag is set), transcribe-cpp requires an explicit
/// `target_language`: a null target defaults to the *source*, so a non-English
/// source silently becomes e.g. es→es and Canary rejects the unadvertised pair.
/// An English source is skipped entirely — en→en is not a real translation, and
/// it's reachable by default since auto-detect-less models coerce intent to "en".
///
/// Returns `(task, target_language)` ready to drop into `RunOptions`.
fn cpp_translation_task(
    translate_to_english: bool,
    model_supports_translate: bool,
    source_language: Option<&str>,
) -> (Task, Option<String>) {
    let translate_to_en =
        translate_to_english && model_supports_translate && source_language != Some("en");
    if translate_to_en {
        (Task::Translate, Some("en".to_string()))
    } else {
        (Task::Transcribe, None)
    }
}

/// Drain a stream command channel, ignoring fed audio, until the caller
/// finalizes or cancels. Used when streaming can't actually run (model not
/// loaded / not streaming-capable) so the finalize handshake still completes
/// and the caller falls back to batch transcription.
fn drain_until_finalize(rx: mpsc::Receiver<StreamCmd>) {
    drain_until_finalize_with_reply(rx, StreamFinalizeReply::Finished(None));
}

fn drain_until_finalize_with_reply(rx: mpsc::Receiver<StreamCmd>, result: StreamFinalizeReply) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            StreamCmd::Feed(_) => {}
            StreamCmd::Finalize(reply) => {
                let _ = reply.send(result);
                break;
            }
            StreamCmd::Cancel => break,
        }
    }
}

/// Only the worker handshake is timed. Recovery starts after the failed engine
/// and its lease have been released, so a slow fallback load cannot consume
/// the finalize deadline and prevent the recorded utterance's batch retry.
fn receive_stream_finalization(
    rx: mpsc::Receiver<StreamFinalizeReply>,
    timeout: Duration,
    recover: impl FnOnce(&str, Option<String>) -> bool,
) -> Result<Option<FinalizedStreamText>> {
    match rx.recv_timeout(timeout) {
        Ok(StreamFinalizeReply::Finished(result)) => Ok(result),
        Ok(StreamFinalizeReply::NeedsRecovery {
            model_id,
            failed_gpu_key,
        }) => {
            if recover(&model_id, failed_gpu_key) {
                Ok(None)
            } else {
                Err(anyhow::anyhow!(
                    "Failed to recover transcription after a GPU failure"
                ))
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!(
            "Timed out waiting {:?} for live transcription to finalize",
            timeout
        )),
    }
}

/// Initialize the transcribe-cpp native backend once at startup: route native +
/// ggml diagnostics into the `log` facade and register compute backend modules.
/// In a static build (macOS Metal) `init_backends_default` is a harmless no-op;
/// in a `dynamic-backends` build it loads the per-ISA CPU / GPU modules. Must run
/// before the first model load.
pub fn init_transcribe_backend() {
    transcribe_cpp::init_logging();
    match transcribe_cpp::init_backends_default() {
        Ok(()) => {
            if transcribe_gpu_disabled_for_host() {
                warn!(
                    "Windows x64 build is running under emulation on an ARM64 host; \
                     disabling transcribe.cpp GPU acceleration and using CPU"
                );
            }
        }
        Err(e) => warn!("Failed to initialize transcribe-cpp backends: {}", e),
    }
}

/// Log the compute devices [`init_transcribe_backend`] registered.
///
/// Listing devices is what first opens the GPU. On macOS that loads ggml's
/// Metal library, which is compiled from source whenever the system's shader
/// cache does not hold it yet (the first launch after an install or update),
/// so the app calls this from a background thread instead of its startup
/// path. A model load that comes first waits on the same one-time compile.
pub fn report_compute_devices() {
    let devices = transcribe_compute_devices();
    info!(
        "transcribe-cpp initialized with {} compute device(s): [{}]",
        devices.len(),
        devices
            .iter()
            .map(|d| format!("{} ({})", d.name, d.kind))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Human-readable list of the transcribe-cpp compute devices registered at
/// startup, for the `--list-devices` flag. The reported `index` is the
/// value to pass to `--device-index`. Backends must be initialized first
/// (see [`init_transcribe_backend`]).
pub fn describe_compute_devices() -> Vec<String> {
    transcribe_compute_devices()
        .into_iter()
        .map(|d| {
            let idx = d
                .index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "-".to_string());
            let name = if d.description.is_empty() {
                d.name
            } else {
                d.description
            };
            let vram_mb = d.memory_total / (1024 * 1024);
            format!(
                "index={} kind={} name={} vram={}MB",
                idx, d.kind, name, vram_mb
            )
        })
        .collect()
}

/// Resolve a `--list-devices` registry index to an exact opaque device handle
/// for a transcribe-cpp model load (the `--device-index` flag). In 0.2 index 0
/// is an exact selection too; only an omitted index requests automatic device
/// selection. Errors if the index isn't a registered, loadable primary device.
fn resolve_device_index(index: usize) -> Result<(Backend, Option<transcribe_cpp::Device>)> {
    let device = transcribe_compute_devices()
        .into_iter()
        .find(|d| d.index == Some(index))
        .ok_or_else(|| {
            anyhow::anyhow!("No compute device with index {index} (see --list-devices)")
        })?;
    if matches!(
        device.device_type,
        transcribe_cpp::DeviceType::Accel | transcribe_cpp::DeviceType::Unknown
    ) {
        return Err(anyhow::anyhow!(
            "Device index {index} ({}) cannot host a model",
            device.kind
        ));
    }

    // 0.2's opaque handle makes every index, including zero, an exact
    // selection. Backend::Auto accepts any primary device and cannot conflict
    // with the selected device's vendor backend.
    Ok((Backend::Auto, Some(device)))
}

/// Resolve the user's persisted GPU identity to a fresh opaque 0.2 device
/// handle. Registry indices and handles are process-local, so settings store a
/// key based on the backend's stable `device_id` (falling back to name for
/// backends such as Metal that do not report one). The hot-plug load plan
/// (`build_load_plan`) orders candidates and prefers this stored device when
/// it is still present.
fn transcribe_device_key(device: &transcribe_cpp::Device) -> String {
    let (identity_kind, identity) = match device.device_id.as_deref() {
        Some(device_id) => ("id", device_id),
        None => ("name", device.name.as_str()),
    };
    serde_json::to_string(&(device.kind.as_str(), identity_kind, identity))
        .expect("transcribe device identity is always JSON serializable")
}

fn transcribe_device_label(device: &transcribe_cpp::Device) -> String {
    if device.description.is_empty() {
        device.name.clone()
    } else {
        device.description.clone()
    }
}

/// Apply the user's ORT accelerator preference to the transcribe-rs global.
/// Called on startup and before loading a model.
///
/// The transcribe.cpp (whisper-family) backend is no longer set here: it is
/// chosen at model-load time against the devices that currently exist (see
/// `build_load_plan`), so changing the accelerator only needs a model reload
/// (see `reload_model_on_next_use`).
pub fn apply_accelerator_settings(app: &tauri::AppHandle) {
    use transcribe_rs::accel;

    let settings = get_settings(app);

    info!(
        "transcribe.cpp accelerator preference: {:?} (applied on next model load)",
        settings.transcribe_accelerator
    );

    let ort_pref = match settings.ort_accelerator {
        OrtAcceleratorSetting::Auto => accel::OrtAccelerator::Auto,
        OrtAcceleratorSetting::Cpu => accel::OrtAccelerator::CpuOnly,
        OrtAcceleratorSetting::Cuda => accel::OrtAccelerator::Cuda,
        OrtAcceleratorSetting::DirectMl => accel::OrtAccelerator::DirectMl,
        OrtAcceleratorSetting::Rocm => accel::OrtAccelerator::Rocm,
    };
    accel::set_ort_accelerator(ort_pref);
    info!("ORT accelerator set to: {}", ort_pref);
}

#[derive(Serialize, Clone, Debug, Type)]
pub struct GpuDeviceOption {
    pub id: String,
    pub name: String,
    pub total_vram_mb: usize,
}

static GPU_DEVICES: OnceLock<Vec<GpuDeviceOption>> = OnceLock::new();

fn transcribe_gpu_disabled_for_host() -> bool {
    crate::utils::is_windows_x64_emulated_on_arm64()
}

fn is_transcribe_gpu_device(device: &transcribe_cpp::Device) -> bool {
    matches!(
        device.device_type,
        transcribe_cpp::DeviceType::Gpu | transcribe_cpp::DeviceType::Igpu
    )
}

fn transcribe_device_allowed(kind: &str, gpu_disabled: bool) -> bool {
    !gpu_disabled || matches!(kind, "cpu" | "accel")
}

fn transcribe_compute_devices() -> Vec<transcribe_cpp::Device> {
    let devices = transcribe_cpp::devices();
    let gpu_disabled = transcribe_gpu_disabled_for_host();
    if !gpu_disabled {
        return devices;
    }

    devices
        .into_iter()
        .filter(|device| transcribe_device_allowed(&device.kind, gpu_disabled))
        .collect()
}

fn available_transcribe_accelerators(gpu_disabled: bool) -> Vec<String> {
    if gpu_disabled {
        vec!["cpu".to_string()]
    } else {
        vec!["auto".to_string(), "cpu".to_string(), "gpu".to_string()]
    }
}

fn cached_gpu_devices() -> &'static [GpuDeviceOption] {
    // GPU compute devices transcribe-cpp registered at startup. `id` is a
    // persistent identity key, never the process-local registry index. It uses
    // the backend's device_id where available and its name otherwise (Metal).
    // `total_vram_mb` is 0 when the backend does not report capacity.
    GPU_DEVICES.get_or_init(|| {
        transcribe_compute_devices()
            .into_iter()
            .filter(is_transcribe_gpu_device)
            .map(|d| GpuDeviceOption {
                id: transcribe_device_key(&d),
                name: transcribe_device_label(&d),
                total_vram_mb: (d.memory_total / (1024 * 1024)) as usize,
            })
            .collect()
    })
}

#[derive(Serialize, Clone, Debug, Type)]
pub struct AvailableAccelerators {
    pub transcribe: Vec<String>,
    pub ort: Vec<String>,
    pub gpu_devices: Vec<GpuDeviceOption>,
}

/// Return the accelerators available to this process on its current host.
pub fn get_available_accelerators() -> AvailableAccelerators {
    use transcribe_rs::accel::OrtAccelerator;

    let ort_options: Vec<String> = OrtAccelerator::available()
        .into_iter()
        .map(|a| a.to_string())
        .collect();

    let transcribe_options = available_transcribe_accelerators(transcribe_gpu_disabled_for_host());

    AvailableAccelerators {
        transcribe: transcribe_options,
        ort: ort_options,
        gpu_devices: cached_gpu_devices().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|code| (*code).to_string()).collect()
    }

    #[test]
    fn backend_recovery_excludes_allocation_and_request_errors() {
        assert!(is_transcribe_cpp_backend_failure(
            &transcribe_cpp::Error::Backend("device lost".to_string())
        ));
        for error in [
            transcribe_cpp::Error::OutOfMemory("allocation failed".to_string()),
            transcribe_cpp::Error::InvalidArgument("invalid request".to_string()),
            transcribe_cpp::Error::Unsupported("unsupported language".to_string()),
            transcribe_cpp::Error::ModelLoad("invalid model".to_string()),
        ] {
            assert!(!is_transcribe_cpp_backend_failure(&error), "{error}");
        }
    }

    #[test]
    fn batch_backend_failure_survives_error_context() {
        let error = anyhow::Error::new(transcribe_cpp::Error::Backend("device lost".to_string()))
            .context(FailedGpuDeviceKey("vulkan:rtx".to_string()))
            .context("transcribe-cpp transcription failed")
            .context("history retranscription");
        assert!(is_gpu_backend_failure_error(&error));
        assert_eq!(
            error.downcast_ref::<FailedGpuDeviceKey>().unwrap().0,
            "vulkan:rtx"
        );
        assert!(matches!(
            error.downcast_ref::<transcribe_cpp::Error>(),
            Some(transcribe_cpp::Error::Backend(_))
        ));
        assert!(!is_gpu_backend_failure_error(&anyhow::anyhow!(
            "backend error: device lost"
        )));
    }

    #[test]
    fn allocation_recovery_requires_confirmed_removal_of_the_bound_gpu() {
        let error = transcribe_cpp::Error::OutOfMemory("graph allocation failed".to_string());
        let present = vec!["RTX".to_string(), "Radeon".to_string()];
        let removed = vec!["Radeon".to_string()];
        assert!(allocation_failed_on_removed_gpu(
            &error,
            Some("RTX"),
            Some(&removed)
        ));
        assert!(!allocation_failed_on_removed_gpu(
            &error,
            Some("RTX"),
            Some(&present)
        ));
        assert!(!allocation_failed_on_removed_gpu(&error, Some("RTX"), None));
        assert!(!allocation_failed_on_removed_gpu(
            &error,
            None,
            Some(&removed)
        ));
        assert!(!allocation_failed_on_removed_gpu(
            &transcribe_cpp::Error::InvalidArgument("bad request".to_string()),
            Some("RTX"),
            Some(&removed)
        ));
    }

    #[test]
    fn removed_gpu_evidence_preserves_the_original_allocation_error() {
        let error = anyhow::Error::new(transcribe_cpp::Error::OutOfMemory(
            "graph allocation failed".to_string(),
        ))
        .context(RemovedGpuDevice("RTX".to_string()))
        .context("transcribe-cpp transcription failed");
        assert!(is_gpu_backend_failure_error(&error));
        assert!(matches!(
            error.downcast_ref::<transcribe_cpp::Error>(),
            Some(transcribe_cpp::Error::OutOfMemory(_))
        ));
    }

    #[test]
    fn normal_hosts_preserve_every_transcribe_accelerator_setting() {
        assert_eq!(
            available_transcribe_accelerators(false),
            ["auto", "cpu", "gpu"]
        );
        // Without a probe result the pre-hot-plug behavior is preserved:
        // Auto/Gpu defer to ggml's automatic selection, Cpu is strict.
        for (setting, expected) in [
            (TranscribeAcceleratorSetting::Auto, Backend::Auto),
            (TranscribeAcceleratorSetting::Gpu, Backend::Auto),
        ] {
            assert_eq!(
                build_load_plan(setting, false, None, &[], None, false, &[]),
                vec![(expected, None)]
            );
        }
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Cpu,
                false,
                None,
                &[],
                None,
                false,
                &[]
            ),
            vec![(Backend::Cpu, None)]
        );
        for kind in ["cpu", "accel", "metal", "cuda", "vulkan", "gpu"] {
            assert!(transcribe_device_allowed(kind, false));
        }
    }

    #[test]
    fn emulated_x64_on_arm64_forces_every_transcribe_setting_to_cpu() {
        for setting in [
            TranscribeAcceleratorSetting::Auto,
            TranscribeAcceleratorSetting::Cpu,
            TranscribeAcceleratorSetting::Gpu,
        ] {
            assert_eq!(
                build_load_plan(setting, true, None, &[], None, false, &[]),
                vec![(Backend::Cpu, None)]
            );
        }
        assert_eq!(available_transcribe_accelerators(true), ["cpu"]);
        assert!(transcribe_device_allowed("cpu", true));
        assert!(transcribe_device_allowed("accel", true));
        for kind in ["metal", "cuda", "vulkan", "gpu", "unknown"] {
            assert!(!transcribe_device_allowed(kind, true));
        }
    }

    fn gpu_candidate(index: usize, label: &str, rank: u8) -> GpuCandidate {
        GpuCandidate {
            index,
            key: format!("key-{label}"),
            label: label.to_string(),
            rank,
        }
    }

    #[test]
    fn load_plan_orders_probed_gpus_and_falls_back_to_cpu() {
        let candidates = vec![
            gpu_candidate(0, "AMD Radeon(TM) Graphics", 1),
            gpu_candidate(1, "NVIDIA GeForce RTX 3060", 2),
        ];
        let probed = vec![
            "AMD Radeon(TM) Graphics".to_string(),
            "NVIDIA GeForce RTX 3060".to_string(),
        ];
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probed),
                &candidates,
                None,
                false,
                &[]
            ),
            vec![
                (Backend::Auto, Some(1)),
                (Backend::Auto, Some(0)),
                (Backend::Cpu, None)
            ]
        );
    }

    #[test]
    fn load_plan_skips_devices_missing_from_probe() {
        // dGPU powered off: the registry still lists it, the probe doesn't
        // see it, so the plan binds the iGPU and keeps a CPU terminal entry.
        let candidates = vec![
            gpu_candidate(0, "NVIDIA GeForce RTX 3060", 2),
            gpu_candidate(1, "AMD Radeon(TM) Graphics", 1),
        ];
        let probed = vec!["AMD Radeon(TM) Graphics".to_string()];
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probed),
                &candidates,
                None,
                false,
                &[]
            ),
            vec![(Backend::Auto, Some(1)), (Backend::Cpu, None)]
        );
    }

    #[test]
    fn load_plan_prefers_stored_device_and_respects_cooldown() {
        let candidates = vec![
            gpu_candidate(1, "AMD Radeon(TM) Graphics", 1),
            gpu_candidate(0, "NVIDIA GeForce RTX 3060", 2),
        ];
        let probed = vec![
            "AMD Radeon(TM) Graphics".to_string(),
            "NVIDIA GeForce RTX 3060".to_string(),
        ];
        let plan = build_load_plan(
            TranscribeAcceleratorSetting::Gpu,
            false,
            Some(&probed),
            &candidates,
            Some("key-NVIDIA GeForce RTX 3060"),
            false,
            &[],
        );
        assert_eq!(plan.first(), Some(&(Backend::Auto, Some(0))));

        // While the retry cooldown is active, GPU attempts are skipped
        // entirely and the plan is CPU-only.
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probed),
                &candidates,
                None,
                true,
                &[]
            ),
            vec![(Backend::Cpu, None)]
        );
    }

    #[test]
    fn load_plan_is_cpu_only_when_probe_sees_no_gpus() {
        let candidates = vec![gpu_candidate(0, "NVIDIA GeForce RTX 3060", 2)];
        let probed = vec!["AMD Radeon(TM) Graphics".to_string()];
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probed),
                &candidates,
                None,
                false,
                &[]
            ),
            vec![(Backend::Cpu, None)]
        );
    }

    #[test]
    fn explicit_igpu_preference_precedes_a_higher_ranked_gpu() {
        let candidates = vec![gpu_candidate(0, "RTX", 2), gpu_candidate(1, "Radeon", 1)];
        let probe = vec!["RTX".to_string(), "Radeon".to_string()];
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Gpu,
                false,
                Some(&probe),
                &candidates,
                Some("key-Radeon"),
                false,
                &[],
            ),
            vec![
                (Backend::Auto, Some(1)),
                (Backend::Auto, Some(0)),
                (Backend::Cpu, None)
            ]
        );
    }

    #[test]
    fn compute_failure_excludes_a_gpu_that_is_still_present() {
        let candidates = vec![gpu_candidate(0, "RTX", 2), gpu_candidate(1, "Radeon", 1)];
        let probe = vec!["RTX".to_string(), "Radeon".to_string()];
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probe),
                &candidates,
                Some("key-RTX"),
                false,
                &["key-RTX".to_string()],
            ),
            vec![(Backend::Auto, Some(1)), (Backend::Cpu, None)]
        );
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                Some(&probe),
                &candidates,
                None,
                false,
                &["key-RTX".to_string(), "key-Radeon".to_string()],
            ),
            vec![(Backend::Cpu, None)]
        );
        // Without a fresh probe, Auto could select the failed device again.
        assert_eq!(
            build_load_plan(
                TranscribeAcceleratorSetting::Auto,
                false,
                None,
                &candidates,
                None,
                false,
                &["key-RTX".to_string()],
            ),
            vec![(Backend::Cpu, None)]
        );
    }

    #[test]
    fn working_cpu_and_igpu_bindings_stay_loaded_with_a_discrete_gpu_present() {
        let probe = vec!["RTX".to_string(), "Radeon".to_string()];
        assert!(!device_binding_removed(false, "CPU", &probe));
        assert!(!device_binding_removed(true, "Radeon", &probe));
        assert!(device_binding_removed(true, "Removed GPU", &probe));
    }

    #[test]
    fn stream_recovery_runs_after_the_handshake_without_its_timeout() {
        let (tx, rx) = mpsc::channel();
        let active_worker = Arc::new(AtomicU64::new(1));
        let lease = Arc::new(AtomicU64::new(1));
        let guard = StreamWorkerGuard {
            worker_id: 1,
            active_stream_worker: active_worker.clone(),
            active_engine_lease: lease.clone(),
            stream_active: Arc::new(AtomicBool::new(true)),
        };
        drop(guard);
        tx.send(StreamFinalizeReply::NeedsRecovery {
            model_id: "test-model".to_string(),
            failed_gpu_key: Some("key-Radeon".to_string()),
        })
        .unwrap();
        let result = receive_stream_finalization(rx, Duration::from_millis(1), |model, key| {
            assert_eq!(model, "test-model");
            assert_eq!(key.as_deref(), Some("key-Radeon"));
            assert_eq!(active_worker.load(Ordering::Acquire), 0);
            assert_eq!(lease.load(Ordering::Acquire), 0);
            thread::sleep(Duration::from_millis(10));
            true
        });
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn failed_stream_recovery_is_reported_instead_of_starting_batch_work() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamFinalizeReply::NeedsRecovery {
            model_id: "test-model".to_string(),
            failed_gpu_key: None,
        })
        .unwrap();
        assert!(receive_stream_finalization(rx, Duration::from_secs(1), |_, _| false).is_err());
    }

    #[test]
    fn optional_text_transform_falls_back_to_raw_text_after_panic() {
        let raw = "原始轉錄。".to_string();
        let result = fail_open_text_transform(raw.clone(), |_| {
            panic!("simulated optional cleanup failure")
        });

        assert_eq!(result, raw);
    }

    #[test]
    fn non_chinese_output_is_never_converted() {
        let settings = AppSettings {
            chinese_script: ChineseScript::Traditional,
            ..Default::default()
        };
        for evidence in [
            OutputLanguageEvidence::ModelDetected("ja".to_string()),
            OutputLanguageEvidence::UserSelected("en".to_string()),
            OutputLanguageEvidence::TranslatedToEnglish,
        ] {
            let result = post_process_transcription_text(
                "学校に行きます".to_string(),
                &settings,
                false,
                &evidence,
                &languages(&["zh", "ja", "en"]),
            );

            assert_eq!(result, "学校に行きます", "{evidence:?}");
        }
    }

    #[test]
    fn auto_detected_chinese_text_is_converted() {
        let settings = AppSettings {
            chinese_script: ChineseScript::Simplified,
            filler_word_removal_enabled: false,
            ..Default::default()
        };
        let result = post_process_transcription_text(
            "我們今天下午一起去學校圖書館看書，然後再去吃晚飯。".to_string(),
            &settings,
            false,
            &OutputLanguageEvidence::Unknown,
            &languages(&["zh", "en", "ja"]),
        );

        assert_eq!(result, "我们今天下午一起去学校图书馆看书，然后再去吃晚饭。");
    }

    #[test]
    fn portuguese_transcription_does_not_use_english_ui_filler_words() {
        let settings = AppSettings {
            app_language: "en".to_string(),
            selected_language: "pt-BR".to_string(),
            ..Default::default()
        };
        let supported = languages(&["en", "pt"]);
        let evidence = resolve_output_language_evidence(&settings, Some("pt"), &supported, false);

        let result = post_process_transcription_text(
            "eu vi um carro".to_string(),
            &settings,
            false,
            &evidence,
            &supported,
        );

        assert_eq!(
            evidence,
            OutputLanguageEvidence::UserSelected("pt".to_string())
        );
        assert_eq!(result, "eu vi um carro");
    }

    #[test]
    fn norwegian_alias_is_recorded_as_user_selected_evidence() {
        let settings = AppSettings {
            selected_language: "no".to_string(),
            ..Default::default()
        };

        let evidence =
            resolve_output_language_evidence(&settings, Some("nb"), &languages(&["nb"]), false);

        assert_eq!(
            evidence,
            OutputLanguageEvidence::UserSelected("nb".to_string())
        );
    }

    #[test]
    fn auto_language_without_detection_skips_gated_filler_removal() {
        let settings = AppSettings {
            selected_language: "auto".to_string(),
            ..Default::default()
        };
        let evidence =
            resolve_output_language_evidence(&settings, None, &languages(&["en", "pt"]), false);

        // Too short for a reliable text detection, so the gated "um" must
        // survive; the universal "uhm" is removed regardless.
        let result = post_process_transcription_text(
            "um uhm ok".to_string(),
            &settings,
            false,
            &evidence,
            &languages(&["en", "pt"]),
        );

        assert_eq!(evidence, OutputLanguageEvidence::Unknown);
        assert_eq!(result, "um ok");
    }

    #[test]
    fn unknown_evidence_with_confident_text_detection_removes_gated_fillers() {
        let settings = AppSettings {
            selected_language: "auto".to_string(),
            ..Default::default()
        };

        let result = post_process_transcription_text(
            "um so the weather forecast said it would probably rain throughout the whole weekend"
                .to_string(),
            &settings,
            false,
            &OutputLanguageEvidence::Unknown,
            &languages(&["en", "pt", "es", "de"]),
        );

        assert_eq!(
            result,
            "so the weather forecast said it would probably rain throughout the whole weekend"
        );
    }

    #[test]
    fn unknown_evidence_with_portuguese_text_preserves_um() {
        let settings = AppSettings {
            selected_language: "auto".to_string(),
            ..Default::default()
        };

        let result = post_process_transcription_text(
            "eu vi um carro na rua ontem de manhã quando fui ao mercado".to_string(),
            &settings,
            false,
            &OutputLanguageEvidence::Unknown,
            &languages(&["en", "pt", "es", "de"]),
        );

        assert_eq!(
            result,
            "eu vi um carro na rua ontem de manhã quando fui ao mercado"
        );
    }

    #[test]
    fn model_detected_language_upgrades_unknown_evidence_only() {
        assert_eq!(
            with_model_detected_language(OutputLanguageEvidence::Unknown, Some("en".to_string())),
            OutputLanguageEvidence::ModelDetected("en".to_string())
        );
        assert_eq!(
            with_model_detected_language(OutputLanguageEvidence::Unknown, Some("auto".to_string())),
            OutputLanguageEvidence::Unknown
        );
        assert_eq!(
            with_model_detected_language(OutputLanguageEvidence::Unknown, None),
            OutputLanguageEvidence::Unknown
        );
        assert_eq!(
            with_model_detected_language(
                OutputLanguageEvidence::UserSelected("pt".to_string()),
                Some("en".to_string())
            ),
            OutputLanguageEvidence::UserSelected("pt".to_string())
        );
    }

    #[test]
    fn auto_language_uses_single_language_model_as_evidence() {
        let settings = AppSettings {
            selected_language: "auto".to_string(),
            ..Default::default()
        };

        let evidence =
            resolve_output_language_evidence(&settings, None, &languages(&["en"]), false);

        assert_eq!(
            evidence,
            OutputLanguageEvidence::ModelConstrained("en".to_string())
        );
    }

    #[test]
    fn unsupported_explicit_language_uses_model_fallback_as_evidence() {
        let settings = AppSettings {
            selected_language: "pt".to_string(),
            ..Default::default()
        };

        let evidence = resolve_output_language_evidence(
            &settings,
            Some("en"),
            &languages(&["en", "de"]),
            false,
        );

        assert_eq!(
            evidence,
            OutputLanguageEvidence::ModelConstrained("en".to_string())
        );
    }

    #[test]
    fn ignored_user_language_is_not_output_evidence() {
        let settings = AppSettings {
            // Parakeet V3 ignores language hints and auto-detects even when a
            // selection from the previously active model remains persisted.
            selected_language: "en".to_string(),
            ..Default::default()
        };
        let supported = languages(&["en", "de", "pt"]);

        let evidence = resolve_output_language_evidence(&settings, None, &supported, false);
        assert_eq!(evidence, OutputLanguageEvidence::Unknown);

        let result = post_process_transcription_text(
            "eu vi um carro".to_string(),
            &settings,
            false,
            &evidence,
            &supported,
        );
        assert_eq!(result, "eu vi um carro");
    }

    #[test]
    fn unapplied_transcribe_cpp_language_is_not_output_evidence() {
        let settings = AppSettings {
            selected_language: "en".to_string(),
            ..Default::default()
        };
        let supported = languages(&[]);
        let plan = transcribe_cpp_run_plan(false, "en", &supported, false);

        assert_eq!(plan.language, None);
        assert_eq!(
            resolve_output_language_evidence(
                &settings,
                plan.language.as_deref(),
                &supported,
                false,
            ),
            OutputLanguageEvidence::Unknown
        );
    }

    #[test]
    fn translated_output_is_treated_as_english() {
        let settings = AppSettings {
            selected_language: "pt".to_string(),
            ..Default::default()
        };

        let evidence = resolve_output_language_evidence(
            &settings,
            Some("pt"),
            &languages(&["en", "pt"]),
            true,
        );

        assert_eq!(evidence, OutputLanguageEvidence::TranslatedToEnglish);
    }

    #[test]
    fn transcribe_cpp_run_plan_skips_english_translation() {
        let plan = transcribe_cpp_run_plan(true, "en", &languages(&["en", "es"]), true);

        assert!(matches!(plan.task, Task::Transcribe));
        assert_eq!(plan.language.as_deref(), Some("en"));
        assert_eq!(plan.target_language, None);
    }

    #[test]
    fn transcribe_cpp_run_plan_translates_supported_non_english() {
        let plan = transcribe_cpp_run_plan(true, "es", &languages(&["en", "es"]), true);

        assert!(matches!(plan.task, Task::Translate));
        assert_eq!(plan.language.as_deref(), Some("es"));
        assert_eq!(plan.target_language.as_deref(), Some("en"));
    }

    #[test]
    fn transcribe_cpp_run_plan_requires_model_translation_support() {
        let plan = transcribe_cpp_run_plan(true, "es", &languages(&["en", "es"]), false);

        assert!(matches!(plan.task, Task::Transcribe));
        assert_eq!(plan.language.as_deref(), Some("es"));
        assert_eq!(plan.target_language, None);
    }
}

impl Drop for TranscriptionManager {
    fn drop(&mut self) {
        // Skip shutdown unless this is the very last clone. TranscriptionManager
        // is cloned by initiate_model_load() and the watcher thread — those
        // clones dropping must not kill the watcher. The watcher thread holds
        // its own clone, so engine's strong_count is always >= 2 while the
        // watcher is alive. When it reaches 1, only this instance remains
        // and we can safely shut down.
        if Arc::strong_count(&self.engine) > 1 {
            return;
        }

        // Signal the watcher thread to shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Wait for the thread to finish gracefully.
        // Use match instead of unwrap to avoid panicking if the mutex is
        // poisoned — a panic inside Drop calls abort().
        let mut guard = match self.watcher_handle.lock() {
            Ok(g) => g,
            Err(e) => {
                warn!("Recovered poisoned watcher_handle mutex during TranscriptionManager drop — a panic occurred earlier this session");
                e.into_inner()
            }
        };
        if let Some(handle) = guard.take() {
            if let Err(e) = handle.join() {
                warn!("Failed to join idle watcher thread: {:?}", e);
            } else {
                debug!("Idle watcher thread joined successfully");
            }
        }
    }
}
