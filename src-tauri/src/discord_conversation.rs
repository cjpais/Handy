//! Discord Conversation Manager
//!
//! Manages continuous conversation mode for Discord voice channels.
//! Listens for audio from Discord users and processes it through the Onichan pipeline
//! (Whisper transcription → LLM response → TTS playback to Discord).
//!
//! Features:
//! - Per-user audio buffering for parallel handling of multiple speakers
//! - Queued processing: while processing one user, other users' audio is buffered
//! - All transcripts are stored to memory with Discord user IDs

use crate::discord::DiscordManager;
use crate::managers::transcription::TranscriptionManager;
use crate::memory::{is_content_worth_storing, MemoryManager};
use crate::onichan::OnichanManager;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Minimum audio samples to consider for transcription (~300ms at 16kHz)
/// Lowered from 8000 (0.5s) to 4800 (0.3s) to capture shorter utterances
const MIN_AUDIO_SAMPLES: usize = 4800;

/// Maximum audio samples per chunk for transcription (~5s at 16kHz)
/// This prevents transcription from getting too slow for long speeches
/// Audio longer than this will be chunked to the last 5 seconds
/// Matches the Discord sidecar's max buffer of 5 seconds
const MAX_AUDIO_SAMPLES: usize = 80000;

/// Cooldown period after bot speaks to avoid self-triggering (in milliseconds)
/// This prevents the bot from responding to its own speech picked up by other mics
const POST_SPEECH_COOLDOWN_MS: u64 = 2000;

/// Per-user audio buffer for parallel handling
#[derive(Debug)]
struct UserAudioBuffer {
    /// Accumulated audio samples (f32, 16kHz mono)
    samples: Vec<f32>,
    /// Sample rate of the audio
    sample_rate: u32,
}

impl UserAudioBuffer {
    fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }
}

/// Manages Discord voice conversation mode
pub struct DiscordConversationManager {
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    discord_manager: Arc<DiscordManager>,
    memory_manager: Arc<Mutex<Option<Arc<MemoryManager>>>>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
    /// Timestamp (ms since epoch) when bot last finished speaking - used for cooldown
    last_speech_time: Arc<AtomicU64>,
    worker_handle: Arc<std::sync::Mutex<Option<thread::JoinHandle<()>>>>,
    /// Per-user audio queue for parallel handling
    /// Key: user_id, Value: queued audio buffers
    audio_queue: Arc<Mutex<HashMap<String, Vec<UserAudioBuffer>>>>,
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
            memory_manager: Arc::new(Mutex::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            last_speech_time: Arc::new(AtomicU64::new(0)),
            worker_handle: Arc::new(std::sync::Mutex::new(None)),
            audio_queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set the memory manager for storing transcripts
    pub fn set_memory_manager(&self, manager: Arc<MemoryManager>) {
        *self.memory_manager.lock().unwrap() = Some(manager);
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

        // Clear any old audio queue
        self.audio_queue.lock().unwrap().clear();

        // First, enable listening on the Discord sidecar
        self.discord_manager.enable_listening()?;

        info!("Starting Discord conversation mode");
        self.is_running.store(true, Ordering::Relaxed);

        let app_handle = self.app_handle.clone();
        let transcription_manager = self.transcription_manager.clone();
        let onichan_manager = self.onichan_manager.clone();
        let discord_manager = self.discord_manager.clone();
        let memory_manager = self.memory_manager.clone();
        let is_running = self.is_running.clone();
        let is_processing = self.is_processing.clone();
        let last_speech_time = self.last_speech_time.clone();
        let audio_queue = self.audio_queue.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = run_discord_conversation_loop(
                app_handle,
                transcription_manager,
                onichan_manager,
                discord_manager,
                memory_manager,
                is_running,
                is_processing,
                last_speech_time,
                audio_queue,
            ) {
                error!("Discord conversation loop error: {}", e);
            }
        });

        *self.worker_handle.lock().unwrap() = Some(handle);

        // Emit state change
        let _ = self
            .app_handle
            .emit("discord-conversation-state", "listening");

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

        let _ = self
            .app_handle
            .emit("discord-conversation-state", "stopped");
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

/// Transcription result from parallel processing
#[derive(Debug)]
struct TranscriptionResult {
    user_id: String,
    text: String,
}

/// Main conversation loop that processes Discord audio with parallel user handling
///
/// Architecture for parallel processing:
/// 1. Audio events are received and decoded immediately
/// 2. Transcription runs in parallel using tokio::spawn_blocking (CPU-bound)
/// 3. LLM responses are generated sequentially (one at a time)
/// 4. TTS playback is sequential (Discord can only play one audio at a time)
/// 5. All transcripts are stored to memory with Discord user IDs
fn run_discord_conversation_loop(
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    discord_manager: Arc<DiscordManager>,
    memory_manager: Arc<Mutex<Option<Arc<MemoryManager>>>>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
    last_speech_time: Arc<AtomicU64>,
    audio_queue: Arc<Mutex<HashMap<String, Vec<UserAudioBuffer>>>>,
) -> Result<(), String> {
    // Create a tokio runtime for async operations
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    rt.block_on(async {
        run_discord_conversation_loop_async(
            app_handle,
            transcription_manager,
            onichan_manager,
            discord_manager,
            memory_manager,
            is_running,
            is_processing,
            last_speech_time,
            audio_queue,
        )
        .await
    })
}

/// Async version of the conversation loop using tokio for parallel transcription
async fn run_discord_conversation_loop_async(
    app_handle: AppHandle,
    transcription_manager: Arc<TranscriptionManager>,
    onichan_manager: Arc<OnichanManager>,
    discord_manager: Arc<DiscordManager>,
    memory_manager: Arc<Mutex<Option<Arc<MemoryManager>>>>,
    is_running: Arc<AtomicBool>,
    is_processing: Arc<AtomicBool>,
    last_speech_time: Arc<AtomicU64>,
    audio_queue: Arc<Mutex<HashMap<String, Vec<UserAudioBuffer>>>>,
) -> Result<(), String> {
    use crate::discord::SidecarResponse;
    use tokio::sync::mpsc;

    info!("Discord conversation loop started with parallel audio handling (tokio)");

    // Channel for transcription results - transcriptions happen in parallel,
    // but LLM responses are generated one at a time
    let (transcription_tx, mut transcription_rx) = mpsc::channel::<TranscriptionResult>(32);

    // Track active transcription tasks per user
    let active_transcriptions: Arc<Mutex<std::collections::HashSet<String>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));

    // Unbounded channel for audio events - prevents drops when main loop is busy
    let (audio_event_tx, mut audio_event_rx) = mpsc::unbounded_channel::<(String, Vec<f32>, u32)>();

    // Spawn dedicated audio receiver task that continuously drains events from sidecar
    // This ensures we never miss audio even when the main loop is busy with transcription/LLM
    let dm = discord_manager.clone();
    let running_clone = is_running.clone();
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        info!("Audio receiver task started");
        while running_clone.load(Ordering::Relaxed) {
            // Drain ALL available events each iteration (not just one)
            loop {
                match dm.recv_event_timeout(Duration::from_millis(5)) {
                    Some(SidecarResponse::UserAudio {
                        user_id,
                        audio_base64,
                        sample_rate,
                    }) => {
                        // Decode and forward to main loop via channel
                        match decode_audio(&audio_base64, sample_rate) {
                            Ok(samples) => {
                                if audio_event_tx
                                    .send((user_id, samples, sample_rate))
                                    .is_err()
                                {
                                    // Channel closed, main loop stopped
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to decode audio: {}", e);
                            }
                        }
                    }
                    Some(SidecarResponse::UserStartedSpeaking { user_id }) => {
                        debug!("User {} started speaking", user_id);
                        let _ = app_clone.emit(
                            "discord-user-speaking",
                            serde_json::json!({ "user_id": user_id, "speaking": true }),
                        );
                    }
                    Some(SidecarResponse::UserStoppedSpeaking { user_id }) => {
                        debug!("User {} stopped speaking", user_id);
                        let _ = app_clone.emit(
                            "discord-user-speaking",
                            serde_json::json!({ "user_id": user_id, "speaking": false }),
                        );
                    }
                    Some(_) => {}  // Ignore other events
                    None => break, // No more events available
                }
            }
            // Small sleep to prevent busy-loop when no events
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        info!("Audio receiver task stopped");
    });

    while is_running.load(Ordering::Relaxed) {
        // Check if we're in a voice channel
        let status = discord_manager.status();
        if !status.in_voice {
            debug!("Not in voice channel, waiting...");
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Check cooldown - ignore audio if bot recently spoke (to prevent self-triggering)
        let last_spoke = last_speech_time.load(Ordering::Relaxed);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let in_cooldown = last_spoke > 0 && now_ms < last_spoke + POST_SPEECH_COOLDOWN_MS;

        // Non-blocking check for transcription results
        while let Ok(result) = transcription_rx.try_recv() {
            // Remove from active transcriptions
            {
                let mut active = active_transcriptions.lock().unwrap();
                active.remove(&result.user_id);
            }

            // Process the transcription result (LLM + TTS)
            if !in_cooldown {
                is_processing.store(true, Ordering::Relaxed);

                if let Err(e) = process_transcription_result(
                    &app_handle,
                    &onichan_manager,
                    &discord_manager,
                    &memory_manager,
                    &last_speech_time,
                    &result,
                )
                .await
                {
                    error!(
                        "Failed to process transcription from {}: {}",
                        result.user_id, e
                    );
                }

                is_processing.store(false, Ordering::Relaxed);
            }
        }

        // Drain all buffered audio from the receiver task
        // This processes ALL audio that arrived while we were busy, preventing drops
        while let Ok((user_id, samples, sample_rate)) = audio_event_rx.try_recv() {
            if in_cooldown {
                debug!("In cooldown, ignoring audio from {}", user_id);
                continue;
            }

            // Queue the audio for this user
            let mut queue = audio_queue.lock().unwrap();
            let buffer = UserAudioBuffer::new(samples, sample_rate);
            queue.entry(user_id.clone()).or_default().push(buffer);
            debug!(
                "Queued audio for user {} ({} users in queue)",
                user_id,
                queue.len()
            );
        }

        // Spawn parallel transcription tasks for users with queued audio
        // (only if not already being transcribed)
        if !in_cooldown {
            let users_to_transcribe: Vec<(String, Vec<UserAudioBuffer>)> = {
                let mut queue = audio_queue.lock().unwrap();
                let active = active_transcriptions.lock().unwrap();

                let mut to_process = Vec::new();
                let users: Vec<String> = queue.keys().cloned().collect();

                for user_id in users {
                    if !active.contains(&user_id) {
                        if let Some(buffers) = queue.remove(&user_id) {
                            to_process.push((user_id, buffers));
                        }
                    }
                }
                to_process
            };

            for (user_id, buffers) in users_to_transcribe {
                // Mark as actively transcribing
                {
                    let mut active = active_transcriptions.lock().unwrap();
                    active.insert(user_id.clone());
                }

                // Merge buffers for this user
                let sample_rate = buffers.first().map(|b| b.sample_rate).unwrap_or(16000);
                let mut merged_samples = Vec::new();
                for buffer in buffers {
                    merged_samples.extend(buffer.samples);
                }

                // Chunk long audio to keep transcription fast
                // Only use the last MAX_AUDIO_SAMPLES if audio is too long
                let samples_to_transcribe = if merged_samples.len() > MAX_AUDIO_SAMPLES {
                    info!(
                        "Audio from user {} is {:.2}s, chunking to last {:.2}s",
                        user_id,
                        merged_samples.len() as f32 / sample_rate as f32,
                        MAX_AUDIO_SAMPLES as f32 / sample_rate as f32
                    );
                    // Take the last chunk (most recent speech)
                    merged_samples[merged_samples.len() - MAX_AUDIO_SAMPLES..].to_vec()
                } else {
                    merged_samples
                };

                info!(
                    "Starting parallel transcription for user {} ({:.2}s of audio)",
                    user_id,
                    samples_to_transcribe.len() as f32 / sample_rate as f32
                );

                // Spawn transcription task
                let tx = transcription_tx.clone();
                let tm = transcription_manager.clone();
                let uid = user_id.clone();
                let app = app_handle.clone();

                // Clone active_transcriptions for the spawned task to clean up on completion/error
                let active_transcriptions_clone = active_transcriptions.clone();

                tokio::spawn(async move {
                    let _ = app.emit("discord-conversation-state", "transcribing");

                    // Run transcription in blocking task (CPU-bound)
                    let transcription_result =
                        tokio::task::spawn_blocking(move || tm.transcribe(samples_to_transcribe))
                            .await;

                    match transcription_result {
                        Ok(Ok(text)) => {
                            if !text.trim().is_empty() {
                                info!("Transcription for user {}: {}", uid, text);
                                let _ = tx
                                    .send(TranscriptionResult {
                                        user_id: uid.clone(),
                                        text,
                                    })
                                    .await;
                            } else {
                                debug!("Empty transcription for user {}", uid);
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Transcription failed for user {}: {}", uid, e);
                        }
                        Err(e) => {
                            error!("Transcription task panicked for user {}: {}", uid, e);
                        }
                    }

                    // Always remove user from active_transcriptions when done (success or failure)
                    // This ensures the user isn't blocked from future transcriptions
                    {
                        let mut active = active_transcriptions_clone.lock().unwrap();
                        active.remove(&uid);
                    }
                });
            }
        }

        // Small yield to prevent busy-looping
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    info!("Discord conversation loop stopped");
    Ok(())
}

/// Process a transcription result: check wake word, generate LLM response, play TTS
async fn process_transcription_result(
    app_handle: &AppHandle,
    onichan_manager: &OnichanManager,
    discord_manager: &DiscordManager,
    memory_manager: &Arc<Mutex<Option<Arc<MemoryManager>>>>,
    last_speech_time: &Arc<AtomicU64>,
    result: &TranscriptionResult,
) -> Result<(), String> {
    let text = &result.text;
    let user_id = &result.user_id;

    // Store the transcription to memory (regardless of wake word)
    // But only if the content is meaningful (not just "yeah", "ok", etc.)
    if is_content_worth_storing(text) {
        if let Some(mm) = memory_manager.lock().unwrap().as_ref() {
            if let Err(e) = mm.store_message(user_id, text, false) {
                warn!("Failed to store user message to memory: {}", e);
            } else {
                debug!("Stored transcript to memory for user {}", user_id);
            }
        }
    } else {
        debug!(
            "Skipping memory storage for short/filler content: '{}'",
            text
        );
    }

    // Check for wake words - only respond if addressed
    const WAKE_WORDS: &[&str] = &["chan", "omni", "oni", "onichan", "amy"];
    let text_lower = text.to_lowercase();
    let has_wake_word = WAKE_WORDS.iter().any(|w| text_lower.contains(w));

    // Emit the transcription for UI
    let _ = app_handle.emit(
        "discord-user-speech",
        serde_json::json!({
            "user_id": user_id,
            "text": text,
            "skipped": !has_wake_word
        }),
    );

    if !has_wake_word {
        info!(
            "No wake word detected from user {}, skipping response",
            user_id
        );
        return Ok(());
    }

    // Process with LLM
    let _ = app_handle.emit("discord-conversation-state", "thinking");

    // Set the current user for memory association
    onichan_manager.set_current_user(Some(user_id.to_string()));

    let response = onichan_manager.process_input(text.to_string()).await?;

    info!("LLM response for {}: {}", user_id, response);

    // Store bot response to memory (if meaningful)
    if is_content_worth_storing(&response) {
        if let Some(mm) = memory_manager.lock().unwrap().as_ref() {
            if let Err(e) = mm.store_message(user_id, &response, true) {
                warn!("Failed to store bot response to memory: {}", e);
            }
        }
    }

    // Generate TTS audio
    let _ = app_handle.emit("discord-conversation-state", "speaking");

    let (tts_base64, tts_sample_rate) = onichan_manager.synthesize_speech(&response)?;

    // Play audio on Discord
    discord_manager.play_audio(&tts_base64, tts_sample_rate)?;

    // Record when we finished speaking to enable cooldown period
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    last_speech_time.store(now_ms, Ordering::Relaxed);
    info!(
        "Bot finished speaking, cooldown active for {}ms",
        POST_SPEECH_COOLDOWN_MS
    );

    let _ = app_handle.emit("discord-conversation-state", "listening");

    Ok(())
}

/// Decode base64 audio into f32 samples
fn decode_audio(audio_base64: &str, sample_rate: u32) -> Result<Vec<f32>, String> {
    let audio_bytes = BASE64
        .decode(audio_base64)
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

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
        return Err(format!(
            "Audio too short: {} samples (min {})",
            samples.len(),
            MIN_AUDIO_SAMPLES
        ));
    }

    // Resample to 16kHz if needed
    let samples_16k = if sample_rate != 16000 {
        resample(&samples, sample_rate, 16000)
    } else {
        samples
    };

    Ok(samples_16k)
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
