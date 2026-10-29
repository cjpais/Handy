//! Speech-to-text with the one pinned model.
//!
//! The model is loaded when the app starts and then stays loaded (unless the
//! unload timeout says otherwise). A recording is decoded live while it goes
//! when the model can stream; otherwise the whole recording is decoded when it
//! stops.

use crate::engine_supervisor::{
    CancelMark, DeviceSelector, EngineError, EngineSupervisor, LoadSpec, LoadedInfo, Unloading,
};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::model::ModelManager;
use crate::settings::{get_settings, ModelUnloadTimeout};
use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tauri_specta::Event;
use transcribe_cpp::{Backend, RunOptions, Task, Transcript};

mod live;

const STREAM_PERF_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Language the pinned model is run in.
const MODEL_LANGUAGE: &str = "en";

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub error: Option<String>,
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
    /// Finalizing — show a spinner.
    Working,
}

/// Emitted to switch the streaming overlay to a working spinner.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct StreamPhaseEvent {
    pub phase: StreamPhase,
}

/// Commands sent to the streaming worker thread. Audio frames and the finalize
/// request travel the same channel so FIFO ordering guarantees every fed frame
/// is processed before finalize runs.
enum StreamCmd {
    Feed(Vec<f32>),
    /// Flush the live decode and reply with what it decoded, or `None` if
    /// there is no usable live result (caller should fall back to batch
    /// transcription). `Err(Cancelled)` if the finalize itself was cancelled.
    Finalize {
        reply: mpsc::Sender<StreamFinalizeReply>,
    },
    Cancel,
}

type StreamFinalizeReply = Result<Option<String>, EngineError>;

/// Routes real-time audio frames to the active streaming worker. Shared between
/// the [`TranscriptionManager`] (opens/closes the route) and the audio recorder's
/// per-frame callback (feeds frames). The recorder holds an `Arc<StreamRouter>`
/// directly, so a frame with no stream pending costs a single relaxed atomic
/// load — no Tauri state lookup, no mutex lock.
pub struct StreamRouter {
    /// The active streaming worker's route, present from `start_stream`
    /// until `finalize_live`/`cancel_stream`.
    route: Mutex<Option<Route>>,
    /// True while a stream is pending or active (channel is open). The audio
    /// callback checks this first to avoid the mutex lock when no stream runs.
    open: Arc<AtomicBool>,
}

/// One streaming worker's command channel.
struct Route {
    tx: mpsc::Sender<StreamCmd>,
    /// Set by `cancel_stream`, so the worker drops audio still queued ahead
    /// of the `Cancel` instead of decoding it.
    cancelled: Arc<AtomicBool>,
}

impl StreamRouter {
    fn new() -> Self {
        Self {
            route: Mutex::new(None),
            open: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open a fresh command channel for a new streaming session, returning the
    /// receiver the worker should drain and its cancel flag. Caller must
    /// ensure no prior channel is still open.
    fn open(&self) -> (mpsc::Receiver<StreamCmd>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel::<StreamCmd>();
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.route.lock().unwrap() = Some(Route {
            tx,
            cancelled: Arc::clone(&cancelled),
        });
        self.open.store(true, Ordering::Relaxed);
        (rx, cancelled)
    }

    /// Take the route out (closing the channel to new feeds), so the caller
    /// can send the final `Finalize`/`Cancel` command.
    fn take(&self) -> Option<Route> {
        self.open.store(false, Ordering::Relaxed);
        self.route.lock().unwrap().take()
    }

    /// Drop the channel and mark closed without sending a final command (used
    /// when the worker exits without a finalize/cancel handshake).
    fn clear(&self) {
        self.open.store(false, Ordering::Relaxed);
        *self.route.lock().unwrap() = None;
    }

    /// Forward a 16 kHz frame to the active streaming worker. Cheap no-op (a
    /// single relaxed atomic load) when no stream is pending.
    pub fn feed(&self, frame: &[f32]) {
        if !self.open.load(Ordering::Relaxed) {
            return;
        }
        if let Some(route) = self.route.lock().unwrap().as_ref() {
            let _ = route.tx.send(StreamCmd::Feed(frame.to_vec()));
        }
    }

    /// Whether a stream is pending or active.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }
}

/// RAII guard that clears the `is_loading` flag and notifies waiters on drop.
/// Ensures the loading flag is always reset, even on early returns or panics.
struct LoadingGuard {
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

/// RAII guard that clears the streaming worker flags on any worker exit -
/// normal return, early return, or a panic that unwinds the detached worker
/// thread. Tokens prevent an older worker from clearing a newer worker's
/// state if a start/finalize race ever slips through.
struct StreamWorkerGuard {
    worker_id: u64,
    active_stream_worker: Arc<AtomicU64>,
    stream_active: Arc<AtomicBool>,
}

impl Drop for StreamWorkerGuard {
    fn drop(&mut self) {
        if self.active_stream_worker.load(Ordering::Acquire) == self.worker_id {
            self.stream_active.store(false, Ordering::Release);
        }
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
    /// The transcribe-cpp engine: owns the worker process, its model, and
    /// recovery from crashes and hangs.
    engine: EngineSupervisor,
    model_manager: Arc<ModelManager>,
    app_handle: AppHandle,
    last_activity: Arc<AtomicU64>,
    shutdown_signal: Arc<AtomicBool>,
    watcher_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    is_loading: Arc<Mutex<bool>>,
    loading_condvar: Arc<Condvar>,
    /// Routes real-time audio frames to the active streaming worker; see
    /// [`StreamRouter`]. Shared with the audio recorder so per-frame feeds skip
    /// Tauri state and the manager lock.
    router: Arc<StreamRouter>,
    /// True only while a transcribe-cpp stream is actually in flight (set by
    /// the worker once the stream begins). Used for overlay/UI decisions.
    stream_active: Arc<AtomicBool>,
    /// Streaming uses three independent flags: router open = frames should
    /// route, worker active = no second worker may start, stream active = UI
    /// should show a live session.
    ///
    /// Monotonic id source for stream workers; zero means "no worker".
    next_stream_worker_id: Arc<AtomicU64>,
    /// Nonzero while a stream worker exists, even if it has not leased the engine
    /// yet. This prevents a second worker from starting after finalize/cancel
    /// closes the router but before the first worker has fully exited.
    active_stream_worker: Arc<AtomicU64>,
}

impl TranscriptionManager {
    pub fn new(app_handle: &AppHandle, model_manager: Arc<ModelManager>) -> Result<Self> {
        let manager = Self {
            engine: EngineSupervisor::new(!transcribe_gpu_disabled_for_host()),
            model_manager,
            app_handle: app_handle.clone(),
            last_activity: Arc::new(AtomicU64::new(Self::now_ms())),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            watcher_handle: Arc::new(Mutex::new(None)),
            is_loading: Arc::new(Mutex::new(false)),
            loading_condvar: Arc::new(Condvar::new()),
            router: Arc::new(StreamRouter::new()),
            stream_active: Arc::new(AtomicBool::new(false)),
            next_stream_worker_id: Arc::new(AtomicU64::new(1)),
            active_stream_worker: Arc::new(AtomicU64::new(0)),
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

                    // While recording, keep the idle timer fresh so the model
                    // is never unloaded mid-session.
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

                        if idle_ms > limit_ms && manager_cloned.is_model_loaded() {
                            info!(
                                "Model idle for {}s (limit: {}s), unloading",
                                idle_ms / 1000,
                                limit_seconds
                            );
                            if let Err(e) = manager_cloned.unload_model() {
                                error!("Failed to unload idle model: {}", e);
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

    pub fn is_model_loaded(&self) -> bool {
        // A transcribe-cpp model stays loaded while it is busy, so a batch
        // run or stream in progress never reads as "unloaded".
        self.engine.loaded().is_some()
    }

    /// Unload the model. Returns once the transcribe-cpp worker has exited,
    /// which frees all of its memory.
    pub fn unload_model(&self) -> Result<()> {
        self.begin_unload().wait();
        Ok(())
    }

    /// Unload the model without waiting for the transcribe-cpp worker to
    /// exit, so it never blocks the caller (e.g. the event loop) behind a
    /// transcription in progress. The model reads as unloaded at once.
    pub fn request_unload(&self) {
        drop(self.begin_unload());
    }

    /// Every state change of an unload, done now on the caller's thread: a
    /// dictation started right after sees no model and loads it again, and
    /// its load is queued behind this unload, so the unload can never affect
    /// it. Only the worker's exit is left to wait for.
    fn begin_unload(&self) -> Unloading {
        let unloading = self.engine.unload();
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "unloaded".to_string(),
                error: None,
            },
        );
        unloading
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

    /// Unloads the model immediately if the setting is enabled and the model
    /// is loaded. Never waits for the worker to exit, so it is safe on any
    /// thread, including the event loop.
    pub fn maybe_unload_immediately(&self, context: &str) {
        let settings = get_settings(&self.app_handle);
        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately
            && self.is_model_loaded()
        {
            info!("Immediately unloading model after {}", context);
            self.request_unload();
        }
    }

    /// Load the pinned model into the engine. Emits `model-state-changed`
    /// events around it so the settings window can show progress.
    pub fn load_model(&self) -> Result<()> {
        let load_start = Instant::now();
        debug!("Starting to load the model");

        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_started".to_string(),
                error: None,
            },
        );

        // Every failure after loading starts must emit a terminal event so the
        // frontend can never remain in its loading state.
        let emit_loading_failed = |error_msg: &str| {
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    error: Some(error_msg.to_string()),
                },
            );
        };

        let Some(model_path) = self.model_manager.model_path() else {
            let error_msg = "Model not downloaded";
            emit_loading_failed(error_msg);
            return Err(anyhow::anyhow!(error_msg));
        };

        let backend = if transcribe_gpu_disabled_for_host() {
            Backend::Cpu
        } else {
            Backend::Auto
        };
        let info = self
            .engine
            .load(LoadSpec {
                path: model_path,
                backend,
                device: DeviceSelector::Auto,
            })
            .map_err(|e| {
                let error_msg = format!("Failed to load the model: {}", e);
                emit_loading_failed(&error_msg);
                anyhow::anyhow!(error_msg)
            })?;
        // The bound backend may differ from the request (e.g. CPU fallback
        // under Auto or after a GPU crash); log what loaded.
        info!(
            "Loaded transcribe-cpp model (bound backend '{}', bound device '{}', \
             supports_streaming={}, languages={:?})",
            info.backend,
            info.device,
            info.capabilities.supports_streaming,
            info.capabilities.languages
        );

        // Reset idle timer so the watcher doesn't immediately unload a just-loaded model
        self.touch_activity();

        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_completed".to_string(),
                error: None,
            },
        );

        debug!(
            "Successfully loaded the model (took {}ms)",
            load_start.elapsed().as_millis()
        );
        Ok(())
    }

    /// Kicks off the model loading in a background thread if it's not already loaded
    pub fn initiate_model_load(&self) {
        let mut is_loading = self.is_loading.lock().unwrap();
        if *is_loading || self.is_model_loaded() {
            return;
        }

        *is_loading = true;
        let self_clone = self.clone();
        thread::spawn(move || {
            let _loading = LoadingGuard {
                is_loading: self_clone.is_loading.clone(),
                loading_condvar: self_clone.loading_condvar.clone(),
            };
            if let Err(e) = self_clone.load_model() {
                error!("Failed to load model: {}", e);
            }
        });
    }

    /// Whether a recording is expected to decode live. The loaded model says
    /// so; before it has loaded, the pinned model is known to stream.
    pub fn expects_streaming(&self) -> bool {
        self.engine
            .loaded()
            .map(|info| info.capabilities.supports_streaming)
            .unwrap_or(true)
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

    /// Begin a recording's live decode on the held engine's session. Audio
    /// frames pushed via [`StreamRouter::feed`] (captured directly by the
    /// audio recorder) are streamed, with preview text emitted to the overlay
    /// as [`StreamTextEvent`].
    ///
    /// Non-blocking: spawns a worker that waits for any in-progress model load,
    /// then begins the stream. With a model that can't stream, the worker idles
    /// until finalize/cancel and reports `None` so the caller falls back to
    /// batch transcription. Frames sent before the stream begins queue on the
    /// channel and are not lost.
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
        let (rx, cancelled) = self.router.open();
        self.stream_active.store(false, Ordering::Release);

        let manager = self.clone();
        thread::spawn(move || manager.run_stream_worker(rx, cancelled, worker_id));
    }

    fn run_stream_worker(
        &self,
        rx: mpsc::Receiver<StreamCmd>,
        cancelled: Arc<AtomicBool>,
        worker_id: u64,
    ) {
        let _worker = StreamWorkerGuard {
            worker_id,
            active_stream_worker: Arc::clone(&self.active_stream_worker),
            stream_active: Arc::clone(&self.stream_active),
        };

        // Wait for any in-progress model load to finish (start_stream races the
        // background load).
        {
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }
        }

        // The loaded model is the source of truth for run-path capabilities.
        let Some(info) = self.engine.loaded() else {
            info!("Live preview: the model is not loaded; using batch transcription");
            self.router.clear();
            drain_until_finalize(rx);
            return;
        };
        if !info.capabilities.supports_streaming {
            self.router.clear();
            drain_until_finalize(rx);
            return;
        }
        let Some(mut session) = live::StreamSession::begin(self, &info) else {
            // Nothing to decode live: the recording is transcribed in batch.
            self.router.clear();
            drain_until_finalize(rx);
            return;
        };
        self.touch_activity();

        while let Ok(cmd) = rx.recv() {
            match cmd {
                StreamCmd::Feed(pcm) => {
                    // Audio queued ahead of a cancel is dropped, not decoded.
                    if cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    self.touch_activity();
                    session.feed(pcm);
                }
                StreamCmd::Finalize { reply } => {
                    let _ = reply.send(session.finish());
                    return;
                }
                // Dropping the session resets the worker's stream.
                StreamCmd::Cancel => return,
            }
        }
        // The channel closed without finalize/cancel; dropping the session
        // resets the worker's stream. `_worker` drops after, clearing this
        // worker's flags.
    }

    /// Flush the live decode and return what it produced.
    ///
    /// `Ok(None)` means no usable live result exists and the caller should
    /// fall back to batch transcription. `Err` means the transcription was
    /// cancelled. There is no timeout here: waiting behind a model load is a
    /// queue position, not a failure (#1841), and the engine bounds the
    /// finalize itself by the audio it still has to decode.
    fn finalize_live(&self) -> Result<Option<String>> {
        let Some(route) = self.router.take() else {
            return Ok(None);
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        if route
            .tx
            .send(StreamCmd::Finalize { reply: reply_tx })
            .is_err()
        {
            return Ok(None);
        }
        match reply_rx.recv() {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Ok(None),
        }
    }

    /// Abandon any live decode without producing text (e.g. on cancel).
    pub fn cancel_stream(&self) {
        if let Some(route) = self.router.take() {
            route.cancelled.store(true, Ordering::Release);
            let _ = route.tx.send(StreamCmd::Cancel);
        }
        self.stream_active.store(false, Ordering::Release);
    }

    /// Stop the transcription in progress (a batch run or stream finalize),
    /// for an explicit user cancel. The worker doing it is stopped; the model
    /// stays loaded and the next use starts a fresh worker for it.
    pub fn cancel_transcription(&self) {
        self.engine.cancel();
    }

    /// Emit a working-phase event to the streaming overlay (spinner).
    pub fn emit_stream_working(&self) {
        let _ = StreamPhaseEvent {
            phase: StreamPhase::Working,
        }
        .emit(&self.app_handle);
    }

    /// Transcribe a finished recording: what its live decode produced, or a
    /// batch run of the whole recording when there is no live result.
    pub fn transcribe_recording(&self, samples: Vec<f32>) -> Result<String> {
        let started = Instant::now();
        let live = self.finalize_live()?;
        self.complete(started, samples, live)
    }

    /// The final text of one transcription, from its live result or else a
    /// batch run of `audio`. Every transcription finishes here.
    fn complete(&self, started: Instant, audio: Vec<f32>, live: Option<String>) -> Result<String> {
        #[cfg(debug_assertions)]
        if std::env::var("HANDY_FORCE_TRANSCRIPTION_FAILURE").is_ok() {
            return Err(anyhow::anyhow!(
                "Simulated transcription failure (HANDY_FORCE_TRANSCRIPTION_FAILURE)"
            ));
        }

        let audio_len = audio.len();
        debug!("Audio vector length: {}", audio_len);
        if audio.is_empty() {
            debug!("Empty audio vector");
            self.maybe_unload_immediately("empty audio");
            return Ok(String::new());
        }

        let raw = match live {
            Some(text) => text,
            None => self.decode_batch(audio)?,
        };
        let final_result = normalize_transcription_output(&raw);

        // Real-time factor. Input PCM is 16 kHz mono, so audio length in seconds
        // is samples / 16000. `speedup` is audio_secs / elapsed_secs — e.g. 4.00x
        // means transcribed 4x faster than real time
        let elapsed_secs = started.elapsed().as_secs_f64();
        let audio_secs = audio_len as f64 / 16_000.0;
        let speedup = real_time_factor(audio_secs, elapsed_secs);
        info!(
            "Transcription completed in {:.2}s for {:.2}s of audio ({:.2}x real-time)",
            elapsed_secs, audio_secs, speedup
        );

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

    /// Decode a whole clip with the loaded model, waiting for a model load
    /// that is under way.
    fn decode_batch(&self, audio: Vec<f32>) -> Result<String> {
        // A cancel from here on stops the run.
        let mark = self.engine.cancel_mark();
        self.touch_activity();
        {
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }
        }
        let Some(info) = self.engine.loaded() else {
            return Err(anyhow::anyhow!("Model is not loaded for transcription."));
        };
        let transcript = self
            .run_batch(mark, audio, &run_options(&info))
            .map_err(engine_error)?;
        Ok(transcript.text)
    }

    fn run_batch(
        &self,
        mark: CancelMark,
        pcm: Vec<f32>,
        run_options: &RunOptions,
    ) -> Result<Transcript, EngineError> {
        self.touch_activity();
        self.engine
            .transcribe_since(mark, pcm, run_options.clone())
            .inspect_err(|e| {
                if !matches!(e, EngineError::Cancelled) {
                    warn!("Transcription failed: {}", e);
                }
            })
    }
}

/// How a run on the loaded model is configured: plain transcription, in the
/// model's language when it advertises one.
fn run_options(info: &LoadedInfo) -> RunOptions {
    let language = info
        .capabilities
        .languages
        .iter()
        .any(|l| l == MODEL_LANGUAGE)
        .then(|| MODEL_LANGUAGE.to_string());
    RunOptions {
        task: Task::Transcribe,
        language,
        ..Default::default()
    }
}

/// A batch run's error: a cancel stays recognisable as one.
fn engine_error(e: EngineError) -> anyhow::Error {
    match e {
        EngineError::Cancelled => EngineError::Cancelled.into(),
        e => anyhow::anyhow!("transcribe-cpp transcription failed: {}", e),
    }
}

/// Trim the text and collapse runs of spaces.
fn normalize_transcription_output(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn emit_stream_text(app_handle: &AppHandle, committed: &str, tentative: &str) {
    let _ = StreamTextEvent {
        committed: committed.to_string(),
        tentative: tentative.to_string(),
    }
    .emit(app_handle);
}

fn lock_perf(perf: &Mutex<StreamPerf>) -> MutexGuard<'_, StreamPerf> {
    perf.lock().unwrap_or_else(|e| e.into_inner())
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

    fn record_update(&mut self, update: &transcribe_cpp::StreamUpdate) {
        self.latest_revision = update.revision;
        self.latest_input_received_ms = update.input_received_ms;
        self.latest_audio_committed_ms = update.audio_committed_ms;
        self.latest_buffered_ms = update.buffered_ms;
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

/// Drain a stream command channel, ignoring fed audio, until the caller
/// finalizes or cancels. Used when streaming can't actually run (model not
/// loaded / not streaming-capable) so the finalize handshake still completes
/// and the caller falls back to batch transcription.
fn drain_until_finalize(rx: mpsc::Receiver<StreamCmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            StreamCmd::Feed(_) => {}
            StreamCmd::Finalize { reply } => {
                let _ = reply.send(Ok(None));
                break;
            }
            StreamCmd::Cancel => break,
        }
    }
}

/// The transcribe.cpp GPU path is off for the x64 Windows build running under
/// emulation on an ARM64 host.
fn transcribe_gpu_disabled_for_host() -> bool {
    crate::utils::is_windows_x64_emulated_on_arm64()
}

impl Drop for TranscriptionManager {
    fn drop(&mut self) {
        // Skip shutdown unless this is the very last clone. TranscriptionManager
        // is cloned by initiate_model_load() and the watcher thread — those
        // clones dropping must not kill the watcher. The watcher thread holds
        // its own clone, so `last_activity`'s strong_count is always >= 2 while
        // the watcher is alive. When it reaches 1, only this instance remains
        // and we can safely shut down.
        if Arc::strong_count(&self.last_activity) > 1 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_trimmed_and_single_spaced() {
        assert_eq!(
            normalize_transcription_output("  hello   world \n"),
            "hello world"
        );
        assert_eq!(normalize_transcription_output(""), "");
    }

    #[test]
    fn real_time_factor_handles_zero_compute() {
        assert_eq!(real_time_factor(5.0, 0.0), 0.0);
        assert_eq!(real_time_factor(5.0, 0.5), 10.0);
    }
}
