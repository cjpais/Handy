use crate::filler_detector::{detect_filler_words, FillerWordCount, FillerWordMatch};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Partial transcription result sent during live coaching
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PartialTranscription {
    /// The transcribed text so far
    pub text: String,
    /// Number of filler words detected in this chunk
    pub filler_count: usize,
    /// Total words in the transcription
    pub word_count: usize,
    /// Filler percentage (0.0 - 100.0)
    pub filler_percentage: f32,
    /// Filler word matches with positions for highlighting
    pub matches: Vec<FillerWordMatch>,
    /// Breakdown of filler word usage
    pub filler_breakdown: Vec<FillerWordCount>,
}

/// Manages live coaching during recording
pub struct LiveCoachingManager {
    app_handle: AppHandle,
    audio_manager: Arc<AudioRecordingManager>,
    transcription_manager: Arc<TranscriptionManager>,
    is_active: Arc<AtomicBool>,
    worker_handle: Arc<std::sync::Mutex<Option<thread::JoinHandle<()>>>>,
}

impl LiveCoachingManager {
    pub fn new(
        app_handle: &AppHandle,
        audio_manager: Arc<AudioRecordingManager>,
        transcription_manager: Arc<TranscriptionManager>,
    ) -> Self {
        Self {
            app_handle: app_handle.clone(),
            audio_manager,
            transcription_manager,
            is_active: Arc::new(AtomicBool::new(false)),
            worker_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Start live coaching - periodically peek at audio and transcribe
    pub fn start(&self) {
        if self.is_active.load(Ordering::Relaxed) {
            debug!("Live coaching already active");
            return;
        }

        self.is_active.store(true, Ordering::Relaxed);
        info!("Starting live coaching");

        let app_handle = self.app_handle.clone();
        let audio_manager = Arc::clone(&self.audio_manager);
        let transcription_manager = Arc::clone(&self.transcription_manager);
        let is_active = Arc::clone(&self.is_active);

        let handle = thread::spawn(move || {
            let mut last_sample_count = 0;
            let chunk_interval = Duration::from_secs(3); // Transcribe every 3 seconds
            let min_samples_for_transcription = 16000 * 2; // At least 2 seconds of audio

            while is_active.load(Ordering::Relaxed) {
                thread::sleep(chunk_interval);

                // Check if still active and recording
                if !is_active.load(Ordering::Relaxed) || !audio_manager.is_recording() {
                    debug!("Live coaching stopped or not recording");
                    break;
                }

                // Peek at current samples
                if let Some(samples) = audio_manager.peek_samples() {
                    let current_count = samples.len();

                    // Only transcribe if we have enough new samples
                    if current_count > last_sample_count + min_samples_for_transcription {
                        debug!(
                            "Live coaching: transcribing {} samples (new: {})",
                            current_count,
                            current_count - last_sample_count
                        );

                        // Transcribe the full audio so far
                        match transcription_manager.transcribe(samples) {
                            Ok(text) => {
                                if !text.is_empty() {
                                    // Analyze for filler words
                                    let settings = get_settings(&app_handle);
                                    let custom_fillers = if settings.custom_filler_words.is_empty()
                                    {
                                        None
                                    } else {
                                        Some(settings.custom_filler_words.as_slice())
                                    };

                                    let analysis = detect_filler_words(&text, custom_fillers);

                                    let partial = PartialTranscription {
                                        text: text.clone(),
                                        filler_count: analysis.filler_count,
                                        word_count: analysis.total_words,
                                        filler_percentage: analysis.filler_percentage,
                                        matches: analysis.matches.clone(),
                                        filler_breakdown: analysis.filler_breakdown.clone(),
                                    };

                                    debug!(
                                        "Live coaching: {} words, {} fillers ({:.1}%)",
                                        partial.word_count,
                                        partial.filler_count,
                                        partial.filler_percentage
                                    );

                                    // Emit partial transcription to frontend
                                    if let Err(e) =
                                        app_handle.emit("partial-transcription", partial)
                                    {
                                        error!("Failed to emit partial transcription: {}", e);
                                    }

                                    last_sample_count = current_count;
                                }
                            }
                            Err(e) => {
                                debug!("Live coaching transcription failed: {}", e);
                            }
                        }
                    }
                }
            }

            info!("Live coaching worker stopped");
        });

        *self.worker_handle.lock().unwrap() = Some(handle);
    }

    /// Stop live coaching
    pub fn stop(&self) {
        if !self.is_active.load(Ordering::Relaxed) {
            return;
        }

        info!("Stopping live coaching");
        self.is_active.store(false, Ordering::Relaxed);

        // Wait for worker to finish
        if let Some(handle) = self.worker_handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }

    /// Check if live coaching is currently active
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }
}

impl Drop for LiveCoachingManager {
    fn drop(&mut self) {
        self.stop();
    }
}
