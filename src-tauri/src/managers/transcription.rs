use crate::audio_toolkit::{apply_custom_words, VoiceActivityDetector};
use crate::managers::model::{EngineType, ModelManager};
use crate::settings::{get_settings, ModelUnloadTimeout};
use anyhow::Result;
use log::{debug as log_debug, error, info, warn};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use transcribe_rs::{
    engines::{
        parakeet::{
            ParakeetEngine, ParakeetInferenceParams, ParakeetModelParams, TimestampGranularity,
        },
        whisper::{WhisperEngine, WhisperInferenceParams},
    },
    TranscriptionEngine,
};

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub error: Option<String>,
}

enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
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
}

impl TranscriptionManager {
    pub fn new(app_handle: &AppHandle, model_manager: Arc<ModelManager>) -> Result<Self> {
        let manager = Self {
            engine: Arc::new(Mutex::new(None)),
            model_manager,
            app_handle: app_handle.clone(),
            current_model_id: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(AtomicU64::new(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            )),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            watcher_handle: Arc::new(Mutex::new(None)),
            is_loading: Arc::new(Mutex::new(false)),
            loading_condvar: Arc::new(Condvar::new()),
        };

        // Start the idle watcher
        {
            let app_handle_cloned = app_handle.clone();
            let manager_cloned = manager.clone();
            let shutdown_signal = manager.shutdown_signal.clone();
            let handle = thread::spawn(move || {
                while !shutdown_signal.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(10)); // Check every 10 seconds

                    // Check shutdown signal again after sleep
                    if shutdown_signal.load(Ordering::Relaxed) {
                        break;
                    }

                    let settings = get_settings(&app_handle_cloned);
                    let timeout_seconds = settings.model_unload_timeout.to_seconds();

                    if let Some(limit_seconds) = timeout_seconds {
                        // Skip polling-based unloading for immediate timeout since it's handled directly in transcribe()
                        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately {
                            continue;
                        }

                        let last = manager_cloned.last_activity.load(Ordering::Relaxed);
                        let now_ms = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        if now_ms.saturating_sub(last) > limit_seconds * 1000 {
                            // idle -> unload
                            if manager_cloned.is_model_loaded() {
                                let unload_start = std::time::Instant::now();
                                log_debug!("Starting to unload model due to inactivity");

                                if let Ok(()) = manager_cloned.unload_model() {
                                    let _ = app_handle_cloned.emit(
                                        "model-state-changed",
                                        ModelStateEvent {
                                            event_type: "unloaded".to_string(),
                                            model_id: None,
                                            model_name: None,
                                            error: None,
                                        },
                                    );
                                    let unload_duration = unload_start.elapsed();
                                    log_debug!(
                                        "Model unloaded due to inactivity (took {}ms)",
                                        unload_duration.as_millis()
                                    );
                                }
                            }
                        }
                    }
                }
                log_debug!("Idle watcher thread shutting down gracefully");
            });
            *manager.watcher_handle.lock().unwrap() = Some(handle);
        }

        Ok(manager)
    }

    pub fn is_model_loaded(&self) -> bool {
        let engine = self.engine.lock().unwrap();
        engine.is_some()
    }

    pub fn unload_model(&self) -> Result<()> {
        let unload_start = std::time::Instant::now();
        log_debug!("Starting to unload model");

        {
            let mut engine = self.engine.lock().unwrap();
            if let Some(ref mut loaded_engine) = *engine {
                match loaded_engine {
                    LoadedEngine::Whisper(ref mut whisper) => whisper.unload_model(),
                    LoadedEngine::Parakeet(ref mut parakeet) => parakeet.unload_model(),
                }
            }
            *engine = None; // Drop the engine to free memory
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
        log_debug!(
            "Model unloaded manually (took {}ms)",
            unload_duration.as_millis()
        );
        Ok(())
    }

    pub fn load_model(&self, model_id: &str) -> Result<()> {
        let load_start = std::time::Instant::now();
        log_debug!("Starting to load model: {}", model_id);

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

        // Create appropriate engine based on model type
        let loaded_engine = match model_info.engine_type {
            EngineType::Whisper => {
                let mut engine = WhisperEngine::new();
                engine.load_model(&model_path).map_err(|e| {
                    let error_msg = format!("Failed to load whisper model {}: {}", model_id, e);
                    let _ = self.app_handle.emit(
                        "model-state-changed",
                        ModelStateEvent {
                            event_type: "loading_failed".to_string(),
                            model_id: Some(model_id.to_string()),
                            model_name: Some(model_info.name.clone()),
                            error: Some(error_msg.clone()),
                        },
                    );
                    anyhow::anyhow!(error_msg)
                })?;
                LoadedEngine::Whisper(engine)
            }
            EngineType::Parakeet => {
                let mut engine = ParakeetEngine::new();
                engine
                    .load_model_with_params(&model_path, ParakeetModelParams::int8())
                    .map_err(|e| {
                        let error_msg =
                            format!("Failed to load parakeet model {}: {}", model_id, e);
                        let _ = self.app_handle.emit(
                            "model-state-changed",
                            ModelStateEvent {
                                event_type: "loading_failed".to_string(),
                                model_id: Some(model_id.to_string()),
                                model_name: Some(model_info.name.clone()),
                                error: Some(error_msg.clone()),
                            },
                        );
                        anyhow::anyhow!(error_msg)
                    })?;
                LoadedEngine::Parakeet(engine)
            }
        };

        // Update the current engine and model ID
        {
            let mut engine = self.engine.lock().unwrap();
            *engine = Some(loaded_engine);
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = Some(model_id.to_string());
        }

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
        log_debug!(
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

    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        // Update last activity timestamp
        self.last_activity.store(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::Relaxed,
        );

        let st = std::time::Instant::now();

        log_debug!("Audio vector length: {}", audio.len());

        if audio.len() == 0 {
            log_debug!("Empty audio vector");
            return Ok(String::new());
        }

        // Check if model is loaded, if not try to load it
        {
            // If the model is loading, wait for it to complete.
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }

            let engine_guard = self.engine.lock().unwrap();
            if engine_guard.is_none() {
                drop(engine_guard); // Drop lock before loading
                let settings = get_settings(&self.app_handle);
                info!(
                    "Model not loaded, auto-loading: {}",
                    settings.selected_model
                );
                self.load_model(&settings.selected_model)?;
            }
        }

        // Get current settings for configuration
        let settings = get_settings(&self.app_handle);

        // Smart Chunking Configuration
        const TARGET_CHUNK_DURATION_SECONDS: usize = 30;
        const SAMPLE_RATE: usize = 16000;
        const TARGET_CHUNK_SIZE: usize = TARGET_CHUNK_DURATION_SECONDS * SAMPLE_RATE;

        // Initialize VAD
        let resource_path = self
            .app_handle
            .path()
            .resource_dir()
            .unwrap()
            .join("resources/models/silero_vad_v4.onnx");

        let mut vad = crate::audio_toolkit::vad::SileroVad::new(resource_path, 0.5)
            .map_err(|e| anyhow::anyhow!("Failed to initialize VAD: {}", e))?;

        let total_samples = audio.len();
        let mut full_transcription = String::new();
        let mut processed_samples = 0;
        let mut start_idx = 0;

        while start_idx < total_samples {
            // Determine the end index for this chunk
            let end_idx = if start_idx + TARGET_CHUNK_SIZE >= total_samples {
                total_samples
            } else {
                // Look for a silence point around the target chunk size
                // We'll search in a window of +/- 5 seconds around the 30s mark
                let search_window_samples = 5 * SAMPLE_RATE;
                let target_end = start_idx + TARGET_CHUNK_SIZE;
                let search_start = target_end
                    .saturating_sub(search_window_samples)
                    .max(start_idx);
                let search_end = (target_end + search_window_samples).min(total_samples);

                let mut best_cut_idx = target_end.min(total_samples);
                let mut found_silence = false;

                // Iterate through frames in the search window to find silence
                // Silero VAD expects 30ms frames (480 samples at 16kHz)
                const VAD_FRAME_SIZE: usize = 480; // 30ms * 16000Hz / 1000

                // Align search start to frame boundary relative to start_idx
                let aligned_search_start =
                    start_idx + ((search_start - start_idx) / VAD_FRAME_SIZE) * VAD_FRAME_SIZE;

                for current_pos in (aligned_search_start..search_end).step_by(VAD_FRAME_SIZE) {
                    if current_pos + VAD_FRAME_SIZE > total_samples {
                        break;
                    }

                    let frame = &audio[current_pos..current_pos + VAD_FRAME_SIZE];
                    match vad.push_frame(frame) {
                        Ok(frame_type) => {
                            if !frame_type.is_speech() {
                                // Found silence! Use the end of this frame as cut point
                                best_cut_idx = current_pos + VAD_FRAME_SIZE;
                                found_silence = true;
                                // Prefer cuts closer to the target duration?
                                // For now, taking the first silence after the minimum duration might be safer
                                // or finding the longest silence.
                                // Let's just take the first valid silence we find in the window for simplicity/speed
                                // But ideally we want to be as close to 30s as possible.

                                // Let's try to find silence closest to target_end
                                if (best_cut_idx as i64 - target_end as i64).abs()
                                    < (target_end as i64 - best_cut_idx as i64).abs()
                                {
                                    // This logic is a bit flawed in a loop.
                                    // Let's just break on first silence found in the window?
                                    // Or maybe search backwards from max window?
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("VAD error at sample {}: {}", current_pos, e);
                        }
                    }
                }

                // If no silence found, just cut at target size
                if !found_silence {
                    log_debug!("No silence found in search window, hard cutting at 30s");
                    target_end.min(total_samples)
                } else {
                    log_debug!("Found silence at sample {}, cutting there", best_cut_idx);
                    best_cut_idx
                }
            };

            let chunk_len = end_idx - start_idx;
            let chunk_vec = audio[start_idx..end_idx].to_vec();

            log_debug!(
                "Processing chunk: start={}, len={} samples",
                start_idx,
                chunk_len
            );

            // Perform transcription for the current chunk
            let chunk_result = {
                let mut engine_guard = self.engine.lock().unwrap();
                let engine = engine_guard.as_mut().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Model failed to load after auto-load attempt. Please check your model settings."
                    )
                })?;

                match engine {
                    LoadedEngine::Whisper(whisper_engine) => {
                        // Normalize language code for Whisper
                        let whisper_language = if settings.selected_language == "auto" {
                            None
                        } else {
                            let normalized = if settings.selected_language == "zh-Hans"
                                || settings.selected_language == "zh-Hant"
                            {
                                "zh".to_string()
                            } else {
                                settings.selected_language.clone()
                            };
                            Some(normalized)
                        };

                        let params = WhisperInferenceParams {
                            language: whisper_language,
                            translate: settings.translate_to_english,
                            ..Default::default()
                        };

                        whisper_engine
                            .transcribe_samples(chunk_vec, Some(params))
                            .map_err(|e| anyhow::anyhow!("Whisper transcription failed: {}", e))?
                    }
                    LoadedEngine::Parakeet(parakeet_engine) => {
                        let params = ParakeetInferenceParams {
                            timestamp_granularity: TimestampGranularity::Segment,
                            ..Default::default()
                        };

                        parakeet_engine
                            .transcribe_samples(chunk_vec, Some(params))
                            .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {}", e))?
                    }
                }
            };

            // Append chunk result to full transcription
            if !full_transcription.is_empty() {
                full_transcription.push(' ');
            }
            full_transcription.push_str(&chunk_result.text);

            // Update progress
            processed_samples = end_idx;
            let progress = (processed_samples as f64 / total_samples as f64) * 100.0;
            let _ = self.app_handle.emit("transcription-progress", progress);

            // Update last activity to prevent timeout during long loops
            self.last_activity.store(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
                Ordering::Relaxed,
            );

            // Move to next chunk
            start_idx = end_idx;
        }

        // Apply word correction if custom words are configured
        let corrected_result = if !settings.custom_words.is_empty() {
            apply_custom_words(
                &full_transcription,
                &settings.custom_words,
                settings.word_correction_threshold,
            )
        } else {
            full_transcription
        };

        let et = std::time::Instant::now();
        let translation_note = if settings.translate_to_english {
            " (translated)"
        } else {
            ""
        };
        info!(
            "Transcription completed in {}ms{}",
            (et - st).as_millis(),
            translation_note
        );

        let final_result = corrected_result.trim().to_string();

        if final_result.is_empty() {
            info!("Transcription result is empty");
        } else {
            info!("Transcription result: {}", final_result);
        }

        // Check if we should immediately unload the model after transcription
        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately {
            info!("Immediately unloading model after transcription");
            if let Err(e) = self.unload_model() {
                error!("Failed to immediately unload model: {}", e);
            }
        }

        Ok(final_result)
    }
}

impl Drop for TranscriptionManager {
    fn drop(&mut self) {
        log_debug!("Shutting down TranscriptionManager");

        // Signal the watcher thread to shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Wait for the thread to finish gracefully
        if let Some(handle) = self.watcher_handle.lock().unwrap().take() {
            if let Err(e) = handle.join() {
                warn!("Failed to join idle watcher thread: {:?}", e);
            } else {
                log_debug!("Idle watcher thread joined successfully");
            }
        }
    }
}
