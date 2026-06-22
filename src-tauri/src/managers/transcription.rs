use crate::audio_toolkit::{apply_custom_words, filter_transcription_output};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::model::{EngineType, ModelManager};
use crate::settings::{
    get_settings, ModelUnloadTimeout, OrtAcceleratorSetting, TranscribeAcceleratorSetting,
};
use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime};
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

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
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
    /// Receiving audio / live text (or waiting for the stream to begin).
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
enum StreamCmd {
    Feed(Vec<f32>),
    /// Flush the stream and reply with the final text, or `None` if no stream
    /// was ever active (caller should fall back to batch transcription).
    Finalize(mpsc::Sender<Option<String>>),
    Cancel,
}

/// Routes real-time audio frames to the active streaming worker. Shared
/// between the [`TranscriptionManager`] (which opens/closes the route) and the
/// audio recorder's per-frame callback (which feeds frames).
///
/// Designed so the per-frame cost when no stream is pending is a single
/// relaxed atomic load — no Tauri state lookup, no mutex lock. The recorder
/// callback captures an `Arc<StreamRouter>` directly (handed to it at recorder
/// creation time) instead of going through `app_handle.try_state()` on every
/// frame.
pub struct StreamRouter {
    /// Command channel to the active streaming worker, present from
    /// `start_stream` until `finalize_stream`/`cancel_stream`.
    tx: Mutex<Option<mpsc::Sender<StreamCmd>>>,
    /// True while a stream is pending or active (channel is open). The audio
    /// callback checks this first to avoid the mutex lock when no stream runs.
    open: Arc<AtomicBool>,
    /// When true, forward every raw frame pre-VAD (continuous mode) so the
    /// streaming model receives uninterrupted audio for timing calibration.
    /// When false, forward only VAD-gated speech frames (matches batch audio).
    /// Shared as `Arc<AtomicBool>` so the audio recorder can read it per-frame
    /// without depending on `StreamRouter` (or `TranscriptionManager`).
    continuous: Arc<AtomicBool>,
}

impl StreamRouter {
    fn new() -> Self {
        Self {
            tx: Mutex::new(None),
            open: Arc::new(AtomicBool::new(false)),
            continuous: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open a fresh command channel for a new streaming session, returning the
    /// receiver the worker should drain. Caller must ensure no prior channel is
    /// still open.
    fn open(&self, continuous: bool) -> mpsc::Receiver<StreamCmd> {
        let (tx, rx) = mpsc::channel::<StreamCmd>();
        *self.tx.lock().unwrap() = Some(tx);
        self.continuous.store(continuous, Ordering::Relaxed);
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

    /// Shared handle to the continuous-mode flag, so the audio recorder can
    /// read it per-frame without going through Tauri state or depending on
    /// `StreamRouter`/`TranscriptionManager`.
    pub fn continuous_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.continuous)
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

/// RAII guard that clears the `is_loading` flag and notifies waiters on drop.
/// Ensures the loading flag is always reset, even on early returns or panics.
pub struct LoadingGuard {
    is_loading: Arc<Mutex<bool>>,
    loading_condvar: Arc<Condvar>,
}

impl Drop for LoadingGuard {
    fn drop(&mut self) {
        let mut is_loading = self.is_loading.lock().unwrap();
        *is_loading = false;
        self.loading_condvar.notify_all();
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
    /// Routes real-time audio frames to the active streaming worker. The audio
    /// recorder captures an `Arc<StreamRouter>` directly (handed to it at
    /// recorder creation time), so the per-frame path never goes through Tauri
    /// state or locks the manager — a single relaxed atomic load when no stream
    /// is pending.
    router: Arc<StreamRouter>,
    /// True only while a transcribe-cpp `Stream` is actually in flight (set by
    /// the worker once `stream()` succeeds). Used for overlay/UI decisions.
    stream_active: Arc<AtomicBool>,
    /// True while the streaming worker has taken the engine out of `engine`
    /// (from the moment it is leased until it is returned or dropped). Kept
    /// distinct from `stream_active` — the engine is leased for the worker's
    /// entire lifetime, but the stream may not start (model not loaded / not
    /// streaming-capable / begin failed). `is_model_loaded()` consults this so
    /// the model still reports "loaded" while the worker holds it.
    engine_leased: Arc<AtomicBool>,
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
            router: Arc::new(StreamRouter::new()),
            stream_active: Arc::new(AtomicBool::new(false)),
            engine_leased: Arc::new(AtomicBool::new(false)),
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
                        .map_or(false, |a| a.is_recording());
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
        // the mutex). It's still loaded — just in use — so report true.
        self.lock_engine().is_some() || self.engine_leased.load(Ordering::Relaxed)
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
        let load_start = std::time::Instant::now();
        debug!("Starting to load model: {}", model_id);

        // Emit loading started event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_started".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: None,
                error: None,
            },
        );

        let model_info = self
            .model_manager
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            let error_msg = "Model not downloaded";
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    model_id: Some(model_id.to_string()),
                    model_name: Some(model_info.name.clone()),
                    error: Some(error_msg.to_string()),
                },
            );
            return Err(anyhow::anyhow!(error_msg));
        }

        let model_path = self.model_manager.get_model_path(model_id)?;

        // Tear down any currently-loaded engine BEFORE creating the new one, so
        // transcribe-cpp frees the previous model's native context (Metal/ggml)
        // first. This avoids holding two models at once (peak memory on large
        // GGUFs) and gives every switch a clean backend rather than building the
        // new model alongside the old one.
        {
            let mut engine = self.lock_engine();
            *engine = None;
        }

        // Create appropriate engine based on model type
        let emit_loading_failed = |error_msg: &str| {
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    model_id: Some(model_id.to_string()),
                    model_name: Some(model_info.name.clone()),
                    error: Some(error_msg.to_string()),
                },
            );
        };

        let loaded_engine = match model_info.engine_type {
            EngineType::TranscribeCpp => {
                // The whisper backend is chosen at load time (transcribe-cpp has
                // no runtime global). Re-read the preference here so an
                // accelerator change — which unloads the model — takes effect on
                // the next load. `gpu_device` must be 0 in transcribe-cpp 0.x.
                let settings = get_settings(&self.app_handle);
                let backend = select_transcribe_backend(settings.transcribe_accelerator);
                let model_options = ModelOptions {
                    backend,
                    gpu_device: 0,
                };
                let model = Model::load_with(&model_path, &model_options).map_err(|e| {
                    let error_msg = format!("Failed to load whisper model {}: {}", model_id, e);
                    emit_loading_failed(&error_msg);
                    anyhow::anyhow!(error_msg)
                })?;
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
                // Record the model's real streaming capability (from GGUF
                // metadata) so the picker badge reflects runtime truth rather
                // than a static guess. The load-completed event below triggers a
                // frontend model refresh that picks this up.
                let caps = session.model().capabilities();
                self.model_manager
                    .set_supports_streaming(model_id, caps.supports_streaming);
                info!(
                    "Loaded whisper model '{}' (requested {:?}, bound backend '{}', \
                     supports_streaming={})",
                    model_id, backend, bound_backend, caps.supports_streaming
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
        if *is_loading || self.is_model_loaded() {
            return;
        }

        *is_loading = true;
        let self_clone = self.clone();
        thread::spawn(move || {
            let settings = get_settings(&self_clone.app_handle);
            if let Err(e) = self_clone.load_model(&settings.selected_model) {
                error!("Failed to load model: {}", e);
            }
            let mut is_loading = self_clone.is_loading.lock().unwrap();
            *is_loading = false;
            self_clone.loading_condvar.notify_all();
        });
    }

    pub fn get_current_model(&self) -> Option<String> {
        let current_model = self.current_model_id.lock().unwrap();
        current_model.clone()
    }

    /// Whether a live streaming run is currently in flight.
    pub fn is_streaming(&self) -> bool {
        self.stream_active.load(Ordering::Relaxed)
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
        if self.router.is_open() {
            warn!("start_stream called while a stream worker is already active");
            return;
        }
        let settings = get_settings(&self.app_handle);
        let rx = self.router.open(settings.live_preview_continuous);
        self.stream_active.store(false, Ordering::Relaxed);

        let manager = self.clone();
        thread::spawn(move || manager.run_stream_worker(rx));
    }

    fn run_stream_worker(&self, rx: mpsc::Receiver<StreamCmd>) {
        // Wait for any in-progress model load to finish (start_stream races the
        // background load kicked off when recording starts).
        {
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }
        }

        let model_id = self.get_current_model().unwrap_or_default();

        // Take the engine out of the mutex so we own it during streaming. This
        // prevents any concurrent batch transcription on a second session of
        // the same model — transcribe-cpp's compute_lock would refuse it
        // anyway, but taking the engine out makes the exclusion structural
        // rather than conventional. The engine is returned (or dropped if the
        // model was switched/unloaded mid-stream) when the worker exits.
        self.engine_leased.store(true, Ordering::Relaxed);
        let mut engine = match self.lock_engine().take() {
            Some(e) => e,
            None => {
                self.engine_leased.store(false, Ordering::Relaxed);
                info!(
                    "Live preview: model '{}' was unloaded before streaming could begin; \
                     falling back to batch transcription",
                    model_id
                );
                self.router.clear();
                drain_until_finalize(rx);
                return;
            }
        };

        // Probe capabilities (immutable borrow, brief). Only transcribe-cpp
        // models expose streaming; ONNX engines fall back to batch.
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
            self.engine_leased.store(false, Ordering::Relaxed);
            self.router.clear();
            drain_until_finalize(rx);
            return;
        }

        // Build run options mirroring the offline transcribe-cpp path: task +
        // language gated against what the model actually advertises.
        let settings = get_settings(&self.app_handle);
        let requested_language = match settings.selected_language.as_str() {
            "auto" => None,
            "zh-Hans" | "zh-Hant" => Some("zh".to_string()),
            other => Some(other.to_string()),
        };
        let language = requested_language.filter(|lang| languages.iter().any(|l| l == lang));
        let task = if settings.translate_to_english && supports_translate {
            Task::Translate
        } else {
            Task::Transcribe
        };
        let run_options = RunOptions {
            task,
            language,
            ..Default::default()
        };

        // Run the stream on the held session. The Stream borrows the session
        // (and thus the engine) for its lifetime, so the feed/finalize loop
        // lives in a labeled block — when it exits, the borrow is released and
        // the engine can be moved into return_engine().
        let stream_started = 'stream: {
            let session = match &mut engine {
                LoadedEngine::TranscribeCpp(s) => s,
                _ => break 'stream false,
            };

            // Read the backend string before beginning the stream — the
            // `Stream` borrows `session` mutably for its lifetime, so we can't
            // call `session.model()` once it exists.
            let backend = session.model().backend();

            // StreamOptions::default() uses CommitPolicy::Auto and lets the
            // family pick its own streaming strategy (no family-specific ext).
            let mut stream = match session.stream(&run_options, &StreamOptions::default()) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to begin stream: {}", e);
                    break 'stream false;
                }
            };

            self.stream_active.store(true, Ordering::Relaxed);
            self.touch_activity();
            info!(
                "Live streaming transcription started (model '{}', backend '{}')",
                model_id, backend
            );

            let mut feed_count: u64 = 0;
            let mut emit_count: u64 = 0;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    StreamCmd::Feed(pcm) => {
                        self.touch_activity();
                        feed_count += 1;
                        match stream.feed(&pcm) {
                            Ok(update) => {
                                if update.committed_changed || update.tentative_changed {
                                    let text = stream.text();
                                    emit_count += 1;
                                    self.emit_stream_text(&text.committed, &text.tentative);
                                }
                            }
                            Err(e) => warn!("stream feed failed: {}", e),
                        }
                    }
                    StreamCmd::Finalize(reply) => {
                        let text = match stream.finalize() {
                            // After finalize the committed prefix holds the full
                            // text; display() = committed + tentative is the safe read.
                            Ok(_) => stream.text().display(),
                            Err(e) => {
                                error!("stream finalize failed: {}", e);
                                String::new()
                            }
                        };
                        info!(
                            "Live preview finalized: {} frames fed, {} updates emitted, {} chars",
                            feed_count,
                            emit_count,
                            text.len()
                        );
                        let _ = reply.send(Some(text));
                        break;
                    }
                    StreamCmd::Cancel => {
                        stream.reset();
                        break;
                    }
                }
            }

            self.stream_active.store(false, Ordering::Relaxed);
            true
        };
        // `stream` + the `&mut engine` borrow are released here.

        if !stream_started {
            // Stream never began (model doesn't support streaming or begin
            // failed); drain so the finalize handshake still completes and the
            // caller falls back to batch transcription.
            drain_until_finalize(rx);
        }

        self.return_engine(engine, &model_id);
        self.engine_leased.store(false, Ordering::Relaxed);
    }

    /// Return the leased engine to the mutex, unless the model was switched or
    /// unloaded during streaming (in which case the stale engine is dropped).
    fn return_engine(&self, engine: LoadedEngine, expected_model_id: &str) {
        let still_current =
            self.current_model_id.lock().unwrap().as_deref() == Some(expected_model_id);
        if still_current {
            *self.lock_engine() = Some(engine);
        } else {
            info!(
                "Model changed/unloaded during streaming; dropping stale engine (was '{}')",
                expected_model_id
            );
            // `engine` drops here, freeing its resources.
        }
    }

    /// Flush the active stream and return its final, post-filtered text. Returns
    /// `None` when no stream was active (caller should batch-transcribe instead).
    pub fn finalize_stream(&self) -> Option<String> {
        let tx = self.router.take()?;
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.send(StreamCmd::Finalize(reply_tx)).is_err() {
            return None;
        }
        let raw = match reply_rx.recv() {
            Ok(Some(text)) => text,
            _ => return None,
        };

        // Apply the same custom-word correction + filler/hallucination filtering
        // the offline path uses. Streaming models are non-whisper (no decode
        // prompt), so custom words always go through fuzzy post-correction.
        let settings = get_settings(&self.app_handle);
        let corrected = if settings.custom_words.is_empty() {
            raw
        } else {
            apply_custom_words(
                &raw,
                &settings.custom_words,
                settings.word_correction_threshold,
            )
        };
        let filtered = filter_transcription_output(
            &corrected,
            &settings.app_language,
            &settings.custom_filler_words,
        );

        self.maybe_unload_immediately("streaming transcription");
        Some(filtered)
    }

    /// Abandon any active stream without producing text (e.g. on cancel).
    pub fn cancel_stream(&self) {
        if let Some(tx) = self.router.take() {
            let _ = tx.send(StreamCmd::Cancel);
        }
        self.stream_active.store(false, Ordering::Relaxed);
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

    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
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
        let validated_language = if settings.selected_language == "auto" {
            "auto".to_string()
        } else {
            let is_supported = self
                .model_manager
                .get_model_info(&settings.selected_model)
                .map(|info| {
                    info.supported_languages.is_empty()
                        || info
                            .supported_languages
                            .contains(&settings.selected_language)
                })
                .unwrap_or(true);

            if is_supported {
                settings.selected_language.clone()
            } else {
                warn!(
                    "Language '{}' not supported by current model, falling back to auto-detect",
                    settings.selected_language
                );
                "auto".to_string()
            }
        };

        // Whether the loaded transcribe-cpp model accepts a decode prompt
        // (whisper family). Gates the whisper-only run extension below, and
        // whether fuzzy custom-word correction still runs afterwards.
        let mut model_takes_initial_prompt = false;

        // Perform transcription with the appropriate engine.
        // We use catch_unwind to prevent engine panics from poisoning the mutex,
        // which would make the app hang indefinitely on subsequent operations.
        let result = {
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

            // Probe transcribe-cpp model capabilities once (cheap GGUF-metadata
            // reads). The whisper run extension is kind-tagged, so non-whisper
            // archs (parakeet, voxtral, …) reject it with INVALID_ARG; only
            // attach it where supported. Translate is gated the same way.
            let mut model_supports_translate = false;
            let mut model_languages: Vec<String> = Vec::new();
            if let LoadedEngine::TranscribeCpp(session) = &engine {
                let model = session.model();
                let caps = model.capabilities();
                model_takes_initial_prompt = model.supports(Feature::InitialPrompt);
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
                        let requested_language = if validated_language == "auto" {
                            None
                        } else if validated_language == "zh-Hans" || validated_language == "zh-Hant"
                        {
                            Some("zh".to_string())
                        } else {
                            Some(validated_language.clone())
                        };
                        // Only pass a language the loaded model actually advertises
                        // (per capabilities().languages); otherwise auto-detect
                        // rather than failing with UNSUPPORTED_LANGUAGE. Language-
                        // agnostic models report an empty list -> always auto.
                        let language = requested_language
                            .filter(|lang| model_languages.iter().any(|l| l == lang));

                        // Custom words become the initial prompt ONLY for models
                        // that accept one (whisper family). Attaching the
                        // whisper run extension to a non-whisper arch is rejected
                        // with INVALID_ARG, so skip it there and let the fuzzy
                        // post-correction handle custom words instead.
                        let family =
                            if settings.custom_words.is_empty() || !model_takes_initial_prompt {
                                None
                            } else {
                                Some(RunExtension::Whisper(WhisperRunOptions {
                                    initial_prompt: Some(settings.custom_words.join(", ")),
                                    ..Default::default()
                                }))
                            };

                        let run_options = RunOptions {
                            // Translate is only valid where the model supports it;
                            // otherwise the dispatcher rejects it (UNSUPPORTED_TASK).
                            task: if settings.translate_to_english && model_supports_translate {
                                Task::Translate
                            } else {
                                Task::Transcribe
                            },
                            language,
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
                            .map(|t| t.text)
                            .map_err(|e| {
                                anyhow::anyhow!("transcribe-cpp transcription failed: {}", e)
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
                            "zh" | "zh-Hans" | "zh-Hant" => Some("zh".to_string()),
                            "en" => Some("en".to_string()),
                            "ja" => Some("ja".to_string()),
                            "ko" => Some("ko".to_string()),
                            "yue" => Some("yue".to_string()),
                            _ => None,
                        };
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
                        let lang = if validated_language == "auto" {
                            None
                        } else {
                            Some(validated_language.clone())
                        };
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
                        } else if validated_language == "zh-Hans" || validated_language == "zh-Hant"
                        {
                            Some("zh".to_string())
                        } else {
                            Some(validated_language.clone())
                        };
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

            match transcribe_result {
                Ok(inner_result) => {
                    // Success or normal error — put the engine back
                    let mut engine_guard = self.lock_engine();
                    *engine_guard = Some(engine);
                    inner_result?
                }
                Err(panic_payload) => {
                    // Engine panicked — do NOT put it back (it's in an unknown state).
                    // The engine is dropped here, effectively unloading it.
                    let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
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
                            error: Some(format!("Engine panicked: {}", panic_msg)),
                        },
                    );

                    return Err(anyhow::anyhow!(
                        "Transcription engine panicked: {}. The model has been unloaded and will reload on next attempt.",
                        panic_msg
                    ));
                }
            }
        };

        // Apply fuzzy word correction if custom words are configured — UNLESS the
        // words were already handed to the model as an initial prompt (whisper
        // family). Non-whisper transcribe-cpp models can't take a prompt, so they
        // still get fuzzy correction here, same as the ONNX engines.
        let corrected_result = if !settings.custom_words.is_empty() && !model_takes_initial_prompt {
            apply_custom_words(
                &result,
                &settings.custom_words,
                settings.word_correction_threshold,
            )
        } else {
            result
        };

        // Filter out filler words and hallucinations
        let filtered_result = filter_transcription_output(
            &corrected_result,
            &settings.app_language,
            &settings.custom_filler_words,
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
        let speedup = if elapsed_secs > 0.0 {
            audio_secs / elapsed_secs
        } else {
            0.0
        };
        info!(
            "Transcription completed in {:.2}s for {:.2}s of audio ({:.2}x real-time){}",
            elapsed_secs, audio_secs, speedup, translation_note
        );

        let final_result = filtered_result;

        if final_result.is_empty() {
            info!("Transcription result is empty");
        } else {
            info!("Transcription result: {}", final_result);
        }

        self.maybe_unload_immediately("transcription");

        Ok(final_result)
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
            StreamCmd::Finalize(reply) => {
                let _ = reply.send(None);
                break;
            }
            StreamCmd::Cancel => break,
        }
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
            let devices = transcribe_cpp::devices();
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
        Err(e) => warn!("Failed to initialize transcribe-cpp backends: {}", e),
    }
}

/// Map Handy's whisper accelerator setting to a transcribe-cpp [`Backend`].
///
/// `Auto` lets the library pick the best device (with CPU fallback). `Cpu` forces
/// strict CPU. `Gpu` requests the platform GPU backend, but only if a device for
/// it is actually registered — otherwise it falls back to `Auto` so the load
/// never fails outright on a machine without that GPU backend.
fn select_transcribe_backend(setting: TranscribeAcceleratorSetting) -> Backend {
    match setting {
        TranscribeAcceleratorSetting::Cpu => Backend::Cpu,
        TranscribeAcceleratorSetting::Auto => Backend::Auto,
        TranscribeAcceleratorSetting::Gpu => {
            #[cfg(target_os = "macos")]
            let candidates = [Backend::Metal];
            #[cfg(not(target_os = "macos"))]
            let candidates = [Backend::Cuda, Backend::Vulkan];

            match candidates
                .into_iter()
                .find(|&b| transcribe_cpp::backend_available(b))
            {
                Some(b) => b,
                None => {
                    warn!("No GPU backend available for transcribe.cpp; falling back to Auto");
                    Backend::Auto
                }
            }
        }
    }
}

/// Apply the user's ORT accelerator preference to the transcribe-rs global.
/// Called on startup and whenever the user changes the setting.
///
/// The transcribe.cpp (whisper-family) backend is no longer set here: it is
/// chosen at model-load time from [`select_transcribe_backend`], so changing the
/// accelerator only needs a model reload (see `apply_and_reload_accelerator`).
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
    pub id: i32,
    pub name: String,
    pub total_vram_mb: usize,
}

static GPU_DEVICES: OnceLock<Vec<GpuDeviceOption>> = OnceLock::new();

fn cached_gpu_devices() -> &'static [GpuDeviceOption] {
    // Reports the GPU compute devices transcribe-cpp registered at startup
    // (see `init_transcribe_backend`). This is informational only: transcribe-cpp
    // 0.x requires `gpu_device == 0`, so per-device selection is not yet honored.
    // `Device` carries no VRAM figure, so `total_vram_mb` is reported as 0.
    GPU_DEVICES.get_or_init(|| {
        transcribe_cpp::devices()
            .into_iter()
            .filter(|d| d.kind != "cpu" && d.kind != "accel")
            .enumerate()
            .map(|(i, d)| GpuDeviceOption {
                id: i as i32,
                name: if d.description.is_empty() {
                    d.name
                } else {
                    d.description
                },
                total_vram_mb: 0,
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

/// Return which accelerators are compiled into this build.
pub fn get_available_accelerators() -> AvailableAccelerators {
    use transcribe_rs::accel::OrtAccelerator;

    let ort_options: Vec<String> = OrtAccelerator::available()
        .into_iter()
        .map(|a| a.to_string())
        .collect();

    let transcribe_options = vec!["auto".to_string(), "cpu".to_string(), "gpu".to_string()];

    AvailableAccelerators {
        transcribe: transcribe_options,
        ort: ort_options,
        gpu_devices: cached_gpu_devices().to_vec(),
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

        // Wait for the thread to finish gracefully
        if let Some(handle) = self.watcher_handle.lock().unwrap().take() {
            if let Err(e) = handle.join() {
                warn!("Failed to join idle watcher thread: {:?}", e);
            } else {
                debug!("Idle watcher thread joined successfully");
            }
        }
    }
}
