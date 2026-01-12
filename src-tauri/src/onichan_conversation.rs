use crate::audio_toolkit::{list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad};
use crate::managers::transcription::TranscriptionManager;
use crate::onichan::OnichanManager;
use crate::settings::get_settings;
use crate::vad_model;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Minimum audio length to consider for transcription (500ms)
const MIN_AUDIO_SAMPLES: usize = 16000 / 2;
/// Check interval for silence detection
const CHECK_INTERVAL_MS: u64 = 100;

/// Manages continuous conversation mode for Onichan
/// Listens continuously and automatically processes speech when silence is detected
pub struct OnichanConversationManager {
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
    worker_handle: Arc<std::sync::Mutex<Option<thread::JoinHandle<()>>>>,
}

impl OnichanConversationManager {
    pub fn new(
        app_handle: &AppHandle,
        transcription_manager: Arc<TranscriptionManager>,
        onichan_manager: Arc<OnichanManager>,
    ) -> Self {
        Self {
            app_handle: app_handle.clone(),
            transcription_manager,
            onichan_manager,
            is_running: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            worker_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Start continuous conversation mode
    pub fn start(&self) -> Result<(), String> {
        if self.is_running.load(Ordering::Relaxed) {
            debug!("Conversation mode already running");
            return Ok(());
        }

        info!("Starting Onichan continuous conversation mode");
        self.is_running.store(true, Ordering::Relaxed);

        let app_handle = self.app_handle.clone();
        let transcription_manager = self.transcription_manager.clone();
        let onichan_manager = self.onichan_manager.clone();
        let is_running = self.is_running.clone();
        let is_processing = self.is_processing.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = run_conversation_loop(
                app_handle,
                transcription_manager,
                onichan_manager,
                is_running,
                is_processing,
            ) {
                error!("Conversation loop error: {}", e);
            }
        });

        *self.worker_handle.lock().unwrap() = Some(handle);
        Ok(())
    }

    /// Stop continuous conversation mode
    pub fn stop(&self) {
        if !self.is_running.load(Ordering::Relaxed) {
            return;
        }

        info!("Stopping Onichan continuous conversation mode");
        self.is_running.store(false, Ordering::Relaxed);

        // Wait for worker to finish (with timeout)
        if let Some(handle) = self.worker_handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }

    /// Check if conversation mode is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Check if currently processing (transcribing/thinking/speaking)
    pub fn is_processing(&self) -> bool {
        self.is_processing.load(Ordering::Relaxed)
    }
}

impl Drop for OnichanConversationManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// State machine for conversation
#[derive(Debug, Clone, Copy, PartialEq)]
enum ConversationState {
    /// Waiting for speech to start
    Listening,
    /// Currently collecting speech
    CollectingSpeech,
    /// Processing the collected speech
    Processing,
}

fn run_conversation_loop(
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
) -> Result<(), String> {
    // Get VAD model path
    let vad_path = vad_model::get_best_vad_model_path(&app_handle)
        .map_err(|e| format!("VAD model not available: {}", e))?;

    // Create a fresh VAD for conversation detection
    let silero = SileroVad::new(vad_path.to_str().unwrap(), 0.3)
        .map_err(|e| format!("Failed to create VAD: {}", e))?;
    // More sensitive settings for conversation: less prefill, quicker response
    let vad = SmoothedVad::new(Box::new(silero), 10, 10, 2);

    // Create audio recorder
    let mut recorder = AudioRecorder::new()
        .map_err(|e| format!("Failed to create recorder: {}", e))?
        .with_vad(Box::new(vad));

    // Get selected microphone
    let settings = get_settings(&app_handle);
    let selected_device = settings.selected_microphone.as_ref().and_then(|name| {
        list_input_devices()
            .ok()?
            .into_iter()
            .find(|d| &d.name == name)
            .map(|d| d.device)
    });

    // Open the recorder
    recorder
        .open(selected_device)
        .map_err(|e| format!("Failed to open recorder: {}", e))?;

    info!("Conversation loop started, listening...");

    let mut state = ConversationState::Listening;
    let mut last_speech_time = Instant::now();
    let mut collected_samples: Vec<f32> = Vec::new();

    // Emit initial listening state
    let _ = app_handle.emit("onichan-conversation-state", "listening");

    while is_running.load(Ordering::Relaxed) {
        // Check if onichan is still active
        if !onichan_manager.is_active() {
            debug!("Onichan disabled, stopping conversation loop");
            break;
        }

        match state {
            ConversationState::Listening => {
                // Start recording to detect speech
                if recorder.start().is_err() {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }

                // Wait a bit then check for speech
                thread::sleep(Duration::from_millis(CHECK_INTERVAL_MS));

                if let Ok(samples) = recorder.peek() {
                    if !samples.is_empty() {
                        // VAD detected speech, transition to collecting
                        info!("Speech detected, collecting...");
                        state = ConversationState::CollectingSpeech;
                        last_speech_time = Instant::now();
                        collected_samples = samples;
                        let _ = app_handle.emit("onichan-conversation-state", "collecting");
                    }
                }
            }

            ConversationState::CollectingSpeech => {
                thread::sleep(Duration::from_millis(CHECK_INTERVAL_MS));

                if let Ok(samples) = recorder.peek() {
                    let prev_len = collected_samples.len();
                    collected_samples = samples;

                    // Check if we got new samples (speech continuing)
                    if collected_samples.len() > prev_len {
                        last_speech_time = Instant::now();
                    }

                    // Check for silence (no new samples for threshold duration)
                    let silence_duration = last_speech_time.elapsed();
                    // Get the silence threshold from settings (allows user to adjust)
                    let silence_threshold_ms = get_settings(&app_handle).onichan_silence_threshold;
                    if silence_duration.as_millis() >= silence_threshold_ms as u128 {
                        // Silence detected - stop recording and process
                        if collected_samples.len() >= MIN_AUDIO_SAMPLES {
                            info!(
                                "Silence detected after {:.1}s, processing {} samples",
                                silence_duration.as_secs_f32(),
                                collected_samples.len()
                            );

                            // Stop and get final samples
                            if let Ok(final_samples) = recorder.stop() {
                                collected_samples = final_samples;
                            }

                            state = ConversationState::Processing;
                            is_processing.store(true, Ordering::Relaxed);
                            let _ = app_handle.emit("onichan-conversation-state", "processing");
                        } else {
                            // Too short, reset to listening
                            debug!("Speech too short, ignoring");
                            let _ = recorder.stop();
                            collected_samples.clear();
                            state = ConversationState::Listening;
                        }
                    }
                }
            }

            ConversationState::Processing => {
                // Process the collected speech
                let result = process_speech(
                    &app_handle,
                    &transcription_manager,
                    &onichan_manager,
                    &collected_samples,
                );

                if let Err(e) = result {
                    warn!("Speech processing failed: {}", e);
                }

                // Reset state
                collected_samples.clear();
                is_processing.store(false, Ordering::Relaxed);
                state = ConversationState::Listening;
                let _ = app_handle.emit("onichan-conversation-state", "listening");

                // Small delay before listening again (to avoid catching echo of TTS)
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    // Cleanup
    let _ = recorder.stop();
    let _ = recorder.close();
    info!("Conversation loop stopped");

    Ok(())
}

fn process_speech(
    app_handle: &AppHandle,
    transcription_manager: &TranscriptionManager,
    onichan_manager: &OnichanManager,
    samples: &[f32],
) -> Result<(), String> {
    // Transcribe
    let _ = app_handle.emit("onichan-conversation-state", "transcribing");
    let text = transcription_manager
        .transcribe(samples.to_vec())
        .map_err(|e| format!("Transcription failed: {}", e))?;

    if text.trim().is_empty() {
        return Err("Empty transcription".to_string());
    }

    info!("Transcribed: {}", text);

    // Emit the transcription for UI
    let _ = app_handle.emit("onichan-user-speech", text.clone());

    // Process with LLM (need to use tokio runtime)
    let _ = app_handle.emit("onichan-conversation-state", "thinking");

    // Use a blocking runtime to call async functions
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create runtime: {}", e))?;

    let response = rt.block_on(async { onichan_manager.process_input(text).await })?;

    // Speak the response
    let _ = app_handle.emit("onichan-conversation-state", "speaking");
    rt.block_on(async { onichan_manager.speak(&response).await })?;

    Ok(())
}
