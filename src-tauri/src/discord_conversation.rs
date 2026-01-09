//! Discord Conversation Manager
//!
//! Manages continuous conversation mode for Discord voice channels.
//! Listens for audio from Discord users and processes it through the Onichan pipeline
//! (Whisper transcription → LLM response → TTS playback to Discord).

use crate::discord::DiscordManager;
use crate::managers::transcription::TranscriptionManager;
use crate::onichan::OnichanManager;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{debug, error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Minimum audio samples to consider for transcription (~500ms at 16kHz)
const MIN_AUDIO_SAMPLES: usize = 8000;

/// Response types from the Discord sidecar (subset we care about)
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type")]
enum DiscordEvent {
    #[serde(rename = "user_audio")]
    UserAudio {
        user_id: String,
        audio_base64: String,
        sample_rate: u32,
    },
    #[serde(rename = "user_started_speaking")]
    UserStartedSpeaking { user_id: String },
    #[serde(rename = "user_stopped_speaking")]
    UserStoppedSpeaking { user_id: String },
    #[serde(other)]
    Other,
}

/// Manages Discord voice conversation mode
pub struct DiscordConversationManager {
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    discord_manager: Arc<DiscordManager>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
    worker_handle: Arc<std::sync::Mutex<Option<thread::JoinHandle<()>>>>,
}

impl DiscordConversationManager {
    pub fn new(
        app_handle: &AppHandle,
        transcription_manager: Arc<TranscriptionManager>,
        onichan_manager: Arc<OnichanManager>,
        discord_manager: Arc<DiscordManager>,
    ) -> Self {
        Self {
            app_handle: app_handle.clone(),
            transcription_manager,
            onichan_manager,
            discord_manager,
            is_running: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            worker_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Start Discord conversation mode
    /// This enables listening on the Discord sidecar and starts processing audio
    pub fn start(&self) -> Result<(), String> {
        if self.is_running.load(Ordering::Relaxed) {
            debug!("Discord conversation mode already running");
            return Ok(());
        }

        // Enable Onichan mode so process_input will work
        self.onichan_manager.enable();

        // Clear conversation history to start fresh
        self.onichan_manager.clear_history();

        // First, enable listening on the Discord sidecar
        self.discord_manager.enable_listening()?;

        info!("Starting Discord conversation mode");
        self.is_running.store(true, Ordering::Relaxed);

        let app_handle = self.app_handle.clone();
        let transcription_manager = self.transcription_manager.clone();
        let onichan_manager = self.onichan_manager.clone();
        let discord_manager = self.discord_manager.clone();
        let is_running = self.is_running.clone();
        let is_processing = self.is_processing.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = run_discord_conversation_loop(
                app_handle,
                transcription_manager,
                onichan_manager,
                discord_manager,
                is_running,
                is_processing,
            ) {
                error!("Discord conversation loop error: {}", e);
            }
        });

        *self.worker_handle.lock().unwrap() = Some(handle);

        // Emit state change
        let _ = self.app_handle.emit("discord-conversation-state", "listening");

        Ok(())
    }

    /// Stop Discord conversation mode
    pub fn stop(&self) {
        if !self.is_running.load(Ordering::Relaxed) {
            return;
        }

        info!("Stopping Discord conversation mode");
        self.is_running.store(false, Ordering::Relaxed);

        // Disable Onichan mode
        self.onichan_manager.disable();

        // Disable listening on the Discord sidecar
        let _ = self.discord_manager.disable_listening();

        // Wait for worker to finish (with timeout)
        if let Some(handle) = self.worker_handle.lock().unwrap().take() {
            let _ = handle.join();
        }

        let _ = self.app_handle.emit("discord-conversation-state", "stopped");
    }

    /// Check if conversation mode is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Check if currently processing
    pub fn is_processing(&self) -> bool {
        self.is_processing.load(Ordering::Relaxed)
    }
}

impl Drop for DiscordConversationManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Main conversation loop that processes Discord audio
fn run_discord_conversation_loop(
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    discord_manager: Arc<DiscordManager>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
) -> Result<(), String> {
    use crate::discord::SidecarResponse;

    info!("Discord conversation loop started");

    while is_running.load(Ordering::Relaxed) {
        // Check if we're in a voice channel
        let status = discord_manager.status();
        if !status.in_voice {
            debug!("Not in voice channel, waiting...");
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        // Wait for events from the sidecar with a timeout
        let event = discord_manager.recv_event_timeout(Duration::from_millis(100));

        if let Some(event) = event {
            match event {
                SidecarResponse::UserAudio {
                    user_id,
                    audio_base64,
                    sample_rate,
                } => {
                    // Only process if not already processing
                    if !is_processing.load(Ordering::Relaxed) {
                        is_processing.store(true, Ordering::Relaxed);
                        info!("Received audio from user {}, processing...", user_id);

                        // Drain any queued audio events to prevent overlapping responses
                        let mut drained = 0;
                        while let Some(queued) = discord_manager.try_recv_event() {
                            if matches!(queued, SidecarResponse::UserAudio { .. }) {
                                drained += 1;
                            }
                        }
                        if drained > 0 {
                            info!("Drained {} queued audio events before processing", drained);
                        }

                        if let Err(e) = process_discord_audio(
                            &app_handle,
                            &transcription_manager,
                            &onichan_manager,
                            &discord_manager,
                            &user_id,
                            &audio_base64,
                            sample_rate,
                        ) {
                            error!("Failed to process Discord audio: {}", e);
                        }

                        // Drain any audio events that arrived while we were processing/speaking
                        let mut drained_after = 0;
                        while let Some(queued) = discord_manager.try_recv_event() {
                            if matches!(queued, SidecarResponse::UserAudio { .. }) {
                                drained_after += 1;
                            }
                        }
                        if drained_after > 0 {
                            info!("Drained {} audio events that arrived during processing", drained_after);
                        }

                        is_processing.store(false, Ordering::Relaxed);
                    } else {
                        debug!("Skipping audio - already processing");
                    }
                }
                SidecarResponse::UserStartedSpeaking { user_id } => {
                    debug!("User {} started speaking", user_id);
                }
                SidecarResponse::UserStoppedSpeaking { user_id } => {
                    debug!("User {} stopped speaking", user_id);
                }
                _ => {
                    debug!("Received unexpected event type");
                }
            }
        }
    }

    info!("Discord conversation loop stopped");
    Ok(())
}

/// Process audio from a Discord user
/// This is called when we receive user audio from the sidecar
pub fn process_discord_audio(
    app_handle: &AppHandle,
    transcription_manager: &TranscriptionManager,
    onichan_manager: &OnichanManager,
    discord_manager: &DiscordManager,
    user_id: &str,
    audio_base64: &str,
    sample_rate: u32,
) -> Result<(), String> {
    info!("Processing Discord audio from user {}", user_id);

    // Decode the base64 audio
    let audio_bytes = BASE64
        .decode(audio_base64)
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

    // Convert bytes to f32 samples (the sidecar sends f32 little-endian)
    let samples: Vec<f32> = audio_bytes
        .chunks(4)
        .filter_map(|chunk| {
            if chunk.len() == 4 {
                Some(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            } else {
                None
            }
        })
        .collect();

    if samples.len() < MIN_AUDIO_SAMPLES {
        return Err("Audio too short".to_string());
    }

    // Resample from input rate to 16kHz if needed (Whisper expects 16kHz)
    let samples_16k = if sample_rate != 16000 {
        resample(&samples, sample_rate, 16000)
    } else {
        samples
    };

    // Transcribe
    let _ = app_handle.emit("discord-conversation-state", "transcribing");
    let text = transcription_manager
        .transcribe(samples_16k)
        .map_err(|e| format!("Transcription failed: {}", e))?;

    if text.trim().is_empty() {
        return Err("Empty transcription".to_string());
    }

    info!("Discord transcription: {}", text);

    // Emit the transcription for UI
    let _ = app_handle.emit(
        "discord-user-speech",
        serde_json::json!({
            "user_id": user_id,
            "text": text.clone()
        }),
    );

    // Process with LLM
    let _ = app_handle.emit("discord-conversation-state", "thinking");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create runtime: {}", e))?;

    let response = rt.block_on(async { onichan_manager.process_input(text).await })?;

    info!("LLM response: {}", response);

    // Generate TTS audio using synthesize_speech (returns base64 audio)
    let _ = app_handle.emit("discord-conversation-state", "speaking");

    let (tts_base64, tts_sample_rate) = onichan_manager.synthesize_speech(&response)?;

    // Play audio on Discord
    discord_manager.play_audio(&tts_base64, tts_sample_rate)?;

    // Add a small cooldown after speaking to avoid picking up echo/reverb
    // The discord sidecar also pauses listening during playback, but this adds extra safety
    thread::sleep(Duration::from_millis(500));

    let _ = app_handle.emit("discord-conversation-state", "listening");

    Ok(())
}

/// Simple linear resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio) as usize;

    (0..new_len)
        .map(|i| {
            let src_idx = (i as f64 * ratio) as usize;
            samples.get(src_idx).copied().unwrap_or(0.0)
        })
        .collect()
}
