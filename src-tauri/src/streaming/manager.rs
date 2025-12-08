//! Streaming transcription manager.
//!
//! Coordinates streaming transcription mode at a high level,
//! managing the integration between audio recording, transcription,
//! and text output.

use super::controller::{StreamingConfig, StreamingController};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;
use log::{debug, info};
use std::sync::{Arc, Mutex, Weak};
use tauri::AppHandle;

/// Manager for streaming transcription sessions.
///
/// This is a high-level coordinator that owns the StreamingController
/// and provides methods to start/stop streaming sessions.
pub struct StreamingManager {
    /// The streaming controller (created per-session)
    controller: Arc<Mutex<Option<StreamingController>>>,

    /// Reference to transcription manager
    transcription_manager: Arc<TranscriptionManager>,

    /// Weak reference to audio recording manager (set after initialization)
    audio_manager: Mutex<Option<Weak<AudioRecordingManager>>>,

    /// App handle for settings and events
    app_handle: AppHandle,
}

impl StreamingManager {
    /// Create a new streaming manager.
    pub fn new(app_handle: &AppHandle, transcription_manager: Arc<TranscriptionManager>) -> Self {
        Self {
            controller: Arc::new(Mutex::new(None)),
            transcription_manager,
            audio_manager: Mutex::new(None),
            app_handle: app_handle.clone(),
        }
    }

    /// Set the audio recording manager reference.
    ///
    /// This must be called after the AudioRecordingManager is created,
    /// to enable VAD callback wiring.
    pub fn set_audio_manager(&self, audio_manager: Arc<AudioRecordingManager>) {
        *self.audio_manager.lock().unwrap() = Some(Arc::downgrade(&audio_manager));
    }

    /// Check if streaming mode is enabled in settings.
    pub fn is_streaming_enabled(&self) -> bool {
        let settings = get_settings(&self.app_handle);
        settings.streaming_mode_enabled
    }

    /// Set up the VAD callback for streaming mode.
    ///
    /// This should be called BEFORE starting the recording to avoid
    /// recreating the microphone stream and losing samples.
    pub fn setup_vad_callback(&self) {
        // Set up VAD callback to feed frames to the controller
        if let Some(weak_audio) = self.audio_manager.lock().unwrap().as_ref() {
            if let Some(audio_manager) = weak_audio.upgrade() {
                let controller_for_callback = self.controller.clone();

                audio_manager.set_vad_callback(move |is_speech, samples| {
                    let controller_guard = controller_for_callback.lock().unwrap();
                    if let Some(ref ctrl) = *controller_guard {
                        ctrl.on_vad_result(is_speech, samples);
                    }
                });

                debug!("VAD callback set for streaming");
            } else {
                debug!("Audio manager no longer available for VAD callback");
            }
        } else {
            debug!("No audio manager reference available for VAD callback");
        }
    }

    /// Start the streaming controller.
    ///
    /// This should be called AFTER recording has started.
    /// The VAD callback should already be set up via setup_vad_callback().
    pub fn start_controller(&self) {
        let settings = get_settings(&self.app_handle);
        let config = StreamingConfig::from_settings(&settings);

        let controller = StreamingController::new(
            &self.app_handle,
            self.transcription_manager.clone(),
            config,
        );

        controller.start();

        // Store the controller
        *self.controller.lock().unwrap() = Some(controller);

        info!("Streaming controller started");
    }

    /// Start a streaming session (legacy method that combines setup_vad_callback and start_controller).
    ///
    /// NOTE: This method sets up VAD callback after the microphone stream is open,
    /// which may cause issues. Prefer using setup_vad_callback() before starting
    /// recording, then start_controller() after.
    pub fn start_session(&self) -> bool {
        let settings = get_settings(&self.app_handle);

        if !settings.streaming_mode_enabled {
            debug!("Streaming mode is disabled, not starting session");
            return false;
        }

        self.setup_vad_callback();
        self.start_controller();

        info!("Streaming session started");
        true
    }

    /// Stop the streaming session and return the final text.
    ///
    /// If streaming wasn't active, returns None.
    pub fn stop_session(&self, final_samples: Option<Vec<f32>>) -> Option<String> {
        // Clear VAD callback first
        if let Some(weak_audio) = self.audio_manager.lock().unwrap().as_ref() {
            if let Some(audio_manager) = weak_audio.upgrade() {
                audio_manager.clear_vad_callback();
                debug!("VAD callback cleared");
            }
        }

        let mut controller_guard = self.controller.lock().unwrap();

        if let Some(controller) = controller_guard.take() {
            match controller.stop(final_samples) {
                Ok(text) => {
                    info!("Streaming session stopped, final text: '{}'", text);
                    Some(text)
                }
                Err(e) => {
                    debug!("Streaming session stopped with error: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Process a VAD frame during recording.
    ///
    /// This should be called for each VAD frame while recording is active.
    /// It feeds the frame to the StreamingController for pause detection.
    pub fn on_vad_frame(&self, is_speech: bool, samples: &[f32]) {
        let controller_guard = self.controller.lock().unwrap();

        if let Some(ref controller) = *controller_guard {
            controller.on_vad_result(is_speech, samples);
        }
    }

    /// Check if a streaming session is currently active.
    pub fn is_session_active(&self) -> bool {
        let controller_guard = self.controller.lock().unwrap();
        if let Some(ref controller) = *controller_guard {
            controller.is_active()
        } else {
            false
        }
    }

    /// Get the current output text from the streaming session.
    pub fn get_current_output(&self) -> Option<String> {
        let controller_guard = self.controller.lock().unwrap();
        if let Some(ref controller) = *controller_guard {
            Some(controller.get_output_text())
        } else {
            None
        }
    }

    /// Cancel the current streaming session without finalizing.
    pub fn cancel_session(&self) {
        let mut controller_guard = self.controller.lock().unwrap();
        if controller_guard.take().is_some() {
            info!("Streaming session cancelled");
        }
    }
}
