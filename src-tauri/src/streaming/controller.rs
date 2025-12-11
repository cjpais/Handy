//! Streaming transcription controller.
//!
//! Coordinates pause detection, transcription, and text output for
//! real-time streaming transcription mode.

use super::pause_detector::PauseDetector;
use super::text_replacer::TextReplacer;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::AppSettings;
use log::{debug, error, info};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// Events emitted during streaming transcription.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamingEvent {
    /// Intermediate transcription result (while still recording)
    IntermediateResult { text: String, is_final: bool },
    /// Streaming session started
    Started,
    /// Streaming session ended
    Ended { final_text: String },
    /// Error occurred during streaming
    Error { message: String },
}

/// Current state of the streaming controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingState {
    /// Not currently streaming
    Idle,
    /// Recording and waiting for speech
    WaitingForSpeech,
    /// Recording with active speech
    Recording,
    /// Speech paused, transcription in progress
    Transcribing,
}

/// Configuration for streaming mode.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Pause detection threshold in milliseconds
    pub pause_threshold_ms: u32,
    /// Whether streaming mode is enabled
    pub enabled: bool,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            pause_threshold_ms: 400,
            enabled: false,
        }
    }
}

impl StreamingConfig {
    /// Load config from app settings.
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            pause_threshold_ms: settings.streaming_pause_threshold_ms,
            enabled: settings.streaming_mode_enabled,
        }
    }
}

/// Controller for streaming transcription mode.
///
/// Manages the lifecycle of a streaming transcription session,
/// coordinating pause detection, transcription, and text output.
pub struct StreamingController {
    /// Current state
    state: Arc<Mutex<StreamingState>>,

    /// Pause detector instance
    pause_detector: Arc<Mutex<PauseDetector>>,

    /// Text replacer for output management
    text_replacer: Arc<Mutex<TextReplacer>>,

    /// Accumulated audio samples for the current session
    audio_buffer: Arc<Mutex<Vec<f32>>>,

    /// Reference to the transcription manager
    transcription_manager: Arc<TranscriptionManager>,

    /// App handle for emitting events and clipboard access
    app_handle: AppHandle,

    /// Configuration
    config: StreamingConfig,

    /// Flag to track if we're currently processing a transcription
    is_transcribing: Arc<AtomicBool>,

    /// The last transcription result (for comparison)
    last_transcription: Arc<Mutex<String>>,

    /// Index in audio_buffer where the last transcription ended
    /// Used for append mode - we only transcribe new audio after this point
    last_transcribed_index: Arc<Mutex<usize>>,
}

impl StreamingController {
    /// Create a new streaming controller.
    pub fn new(
        app_handle: &AppHandle,
        transcription_manager: Arc<TranscriptionManager>,
        config: StreamingConfig,
    ) -> Self {
        let pause_detector = PauseDetector::new(
            config.pause_threshold_ms,
            30, // VAD frame duration is 30ms
        );

        Self {
            state: Arc::new(Mutex::new(StreamingState::Idle)),
            pause_detector: Arc::new(Mutex::new(pause_detector)),
            text_replacer: Arc::new(Mutex::new(TextReplacer::new(5))),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            transcription_manager,
            app_handle: app_handle.clone(),
            config,
            is_transcribing: Arc::new(AtomicBool::new(false)),
            last_transcription: Arc::new(Mutex::new(String::new())),
            last_transcribed_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Start a streaming transcription session.
    pub fn start(&self) {
        debug!("StreamingController: Starting session");

        // Reset all state
        {
            let mut state = self.state.lock().unwrap();
            *state = StreamingState::WaitingForSpeech;
        }
        self.pause_detector.lock().unwrap().reset();
        self.text_replacer.lock().unwrap().reset();
        self.audio_buffer.lock().unwrap().clear();
        *self.last_transcription.lock().unwrap() = String::new();
        *self.last_transcribed_index.lock().unwrap() = 0;
        self.is_transcribing.store(false, Ordering::SeqCst);

        // Emit started event
        let _ = self
            .app_handle
            .emit("streaming-event", StreamingEvent::Started);

        info!("Streaming transcription session started");
    }

    /// Stop the streaming session and return the final transcription.
    ///
    /// # Arguments
    /// * `final_samples` - Optional final audio samples (if different from buffer)
    ///
    /// Returns the final transcription text.
    pub fn stop(&self, final_samples: Option<Vec<f32>>) -> Result<String, String> {
        debug!("StreamingController: Stopping session");

        // Use final_samples if provided, otherwise use buffer
        let full_samples = if let Some(s) = final_samples {
            s
        } else {
            self.audio_buffer.lock().unwrap().clone()
        };

        // Get any remaining audio after the last transcription
        let last_index = *self.last_transcribed_index.lock().unwrap();
        let remaining_samples = if last_index < full_samples.len() {
            full_samples[last_index..].to_vec()
        } else {
            Vec::new()
        };

        // Update state
        {
            let mut state = self.state.lock().unwrap();
            *state = StreamingState::Idle;
        }

        // Transcribe any remaining audio and append it
        if !remaining_samples.is_empty() {
            match self.transcription_manager.transcribe(remaining_samples) {
                Ok(text) => {
                    if !text.is_empty() {
                        // Append to last_transcription
                        {
                            let mut last = self.last_transcription.lock().unwrap();
                            if !last.is_empty() {
                                last.push(' ');
                            }
                            last.push_str(&text);
                        }

                        // Append to output
                        let text_to_append = {
                            let replacer = self.text_replacer.lock().unwrap();
                            if replacer.total_chars_output() > 0 {
                                format!(" {}", text)
                            } else {
                                text.clone()
                            }
                        };

                        if let Err(e) = self
                            .text_replacer
                            .lock()
                            .unwrap()
                            .append(&text_to_append, &self.app_handle)
                        {
                            error!("Failed to append final text: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Final transcription failed: {}", e);
                }
            }
        }

        let final_text = self.last_transcription.lock().unwrap().clone();

        // Emit ended event
        let _ = self.app_handle.emit(
            "streaming-event",
            StreamingEvent::Ended {
                final_text: final_text.clone(),
            },
        );

        info!("Streaming transcription session ended: '{}'", final_text);

        Ok(final_text)
    }

    /// Process a VAD result from the audio pipeline.
    ///
    /// Call this for each VAD frame during recording.
    /// Returns true if a pause was detected and transcription was triggered.
    pub fn on_vad_result(&self, is_speech: bool, frame_samples: &[f32]) -> bool {
        // Don't process if we're not in an active streaming session
        let current_state = *self.state.lock().unwrap();
        if current_state == StreamingState::Idle {
            return false;
        }

        // Add samples to buffer
        self.audio_buffer.lock().unwrap().extend_from_slice(frame_samples);

        // Update state based on speech
        if is_speech {
            let mut state = self.state.lock().unwrap();
            if *state == StreamingState::WaitingForSpeech {
                *state = StreamingState::Recording;
                debug!("StreamingController: Speech detected, now recording");
            }
        }

        // Check for pause
        let pause_detected = self.pause_detector.lock().unwrap().on_vad_result(is_speech);

        if pause_detected {
            debug!("StreamingController: Pause detected, triggering transcription");
            self.trigger_intermediate_transcription();
            return true;
        }

        false
    }

    /// Trigger an intermediate transcription.
    ///
    /// In append mode, we only transcribe audio since the last pause,
    /// then append it to the output (no backspacing needed).
    fn trigger_intermediate_transcription(&self) {
        // Don't start if already transcribing
        if self.is_transcribing.swap(true, Ordering::SeqCst) {
            debug!("Already transcribing, skipping");
            return;
        }

        // Get only the NEW audio since last transcription
        let (samples, new_end_index) = {
            let buffer = self.audio_buffer.lock().unwrap();
            let start_index = *self.last_transcribed_index.lock().unwrap();
            let end_index = buffer.len();

            if start_index >= end_index {
                self.is_transcribing.store(false, Ordering::SeqCst);
                return;
            }

            (buffer[start_index..end_index].to_vec(), end_index)
        };

        if samples.is_empty() {
            self.is_transcribing.store(false, Ordering::SeqCst);
            return;
        }

        // Update state
        {
            let mut state = self.state.lock().unwrap();
            *state = StreamingState::Transcribing;
        }

        let transcription_manager = self.transcription_manager.clone();
        let text_replacer = self.text_replacer.clone();
        let last_transcription = self.last_transcription.clone();
        let last_transcribed_index = self.last_transcribed_index.clone();
        let is_transcribing = self.is_transcribing.clone();
        let state = self.state.clone();
        let app_handle = self.app_handle.clone();

        // Run transcription in background to not block audio processing
        std::thread::spawn(move || {
            let start = Instant::now();

            match transcription_manager.transcribe(samples) {
                Ok(text) => {
                    let elapsed = start.elapsed();
                    info!(
                        "Intermediate transcription completed in {}ms: '{}'",
                        elapsed.as_millis(),
                        text
                    );

                    // Update last transcription (append to existing)
                    {
                        let mut last = last_transcription.lock().unwrap();
                        if !last.is_empty() && !text.is_empty() {
                            last.push(' ');
                        }
                        last.push_str(&text);
                    }

                    // Update the index so next transcription starts from here
                    *last_transcribed_index.lock().unwrap() = new_end_index;

                    // Append output text (no backspacing!)
                    if !text.is_empty() {
                        // Add a space before if we have existing output
                        let text_to_append = {
                            let replacer = text_replacer.lock().unwrap();
                            if replacer.total_chars_output() > 0 {
                                format!(" {}", text)
                            } else {
                                text.clone()
                            }
                        };

                        if let Err(e) = text_replacer.lock().unwrap().append(&text_to_append, &app_handle)
                        {
                            error!("Failed to append text: {}", e);
                        }
                    }

                    // Emit event
                    let _ = app_handle.emit(
                        "streaming-event",
                        StreamingEvent::IntermediateResult {
                            text: text.clone(),
                            is_final: false,
                        },
                    );
                }
                Err(e) => {
                    error!("Intermediate transcription failed: {}", e);
                    let _ = app_handle.emit(
                        "streaming-event",
                        StreamingEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            }

            // Reset state
            {
                let mut s = state.lock().unwrap();
                if *s == StreamingState::Transcribing {
                    *s = StreamingState::Recording;
                }
            }
            is_transcribing.store(false, Ordering::SeqCst);
        });
    }

    /// Get the current state.
    pub fn state(&self) -> StreamingState {
        *self.state.lock().unwrap()
    }

    /// Check if streaming is currently active.
    pub fn is_active(&self) -> bool {
        *self.state.lock().unwrap() != StreamingState::Idle
    }

    /// Get the current output text.
    pub fn get_output_text(&self) -> String {
        self.text_replacer.lock().unwrap().get_output_text()
    }

    /// Check if streaming mode is enabled in config.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert_eq!(config.pause_threshold_ms, 400);
        assert!(!config.enabled);
    }
}
