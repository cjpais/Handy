//! Discord Sidecar Process
//!
//! This is a separate process that handles Discord bot functionality using Serenity + Songbird
//! for voice channel integration with KBVE speech coaching.
//!
//! Communication is via JSON over stdin/stdout:
//! - Requests are JSON objects on stdin (one per line)
//! - Responses are JSON objects on stdout (one per line)
//!
//! Audio flow:
//! - Voice audio from Discord users is collected and sent to main app for processing
//! - TTS audio from main app is played back to the voice channel

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GatewayIntents, GuildId, Ready, VoiceState};
use serenity::all::Guild;
use serenity::cache::Settings as CacheSettings;
use serenity::client::{Client, Context, EventHandler};
use serenity::prelude::TypeMapKey;
use songbird::driver::DecodeMode;
use songbird::events::{Event, EventContext, EventHandler as VoiceEventHandler};
use songbird::input::{Input, RawAdapter};
use songbird::{Config, CoreEvent, SerenityInit};
use std::collections::HashMap;
use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use symphonia_core::io::MediaSource;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

// ============================================================================
// Request/Response Types for IPC
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "connect")]
    Connect { token: String },
    #[serde(rename = "disconnect")]
    Disconnect,
    #[serde(rename = "join_voice")]
    JoinVoice { guild_id: String, channel_id: String },
    #[serde(rename = "leave_voice")]
    LeaveVoice { guild_id: String },
    #[serde(rename = "get_guilds")]
    GetGuilds,
    #[serde(rename = "get_channels")]
    GetChannels { guild_id: String },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "enable_listening")]
    EnableListening,
    #[serde(rename = "disable_listening")]
    DisableListening,
    #[serde(rename = "play_audio")]
    PlayAudio {
        /// Base64 encoded PCM audio (f32 samples, 16kHz mono)
        audio_base64: String,
        sample_rate: u32,
    },
    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
enum Response {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "status")]
    Status {
        connected: bool,
        in_voice: bool,
        listening: bool,
        guild_name: Option<String>,
        channel_name: Option<String>,
    },
    #[serde(rename = "guilds")]
    Guilds { guilds: Vec<GuildInfo> },
    #[serde(rename = "channels")]
    Channels { channels: Vec<ChannelInfo> },
    #[serde(rename = "user_audio")]
    UserAudio {
        user_id: String,
        /// Base64 encoded PCM audio (f32 samples, 48kHz stereo -> converted to 16kHz mono)
        audio_base64: String,
        sample_rate: u32,
    },
    #[serde(rename = "user_started_speaking")]
    UserStartedSpeaking { user_id: String },
    #[serde(rename = "user_stopped_speaking")]
    UserStoppedSpeaking { user_id: String },
}

#[derive(Debug, Serialize, Clone)]
struct GuildInfo {
    id: String,
    name: String,
}

#[derive(Debug, Serialize, Clone)]
struct ChannelInfo {
    id: String,
    name: String,
    kind: String,
}

// ============================================================================
// Commands sent to the Discord client task
// ============================================================================

enum DiscordCommand {
    GetGuilds {
        respond: oneshot::Sender<Vec<GuildInfo>>,
    },
    GetChannels {
        guild_id: String,
        respond: oneshot::Sender<Vec<ChannelInfo>>,
    },
    JoinVoice {
        guild_id: String,
        channel_id: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    LeaveVoice {
        guild_id: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    GetStatus {
        respond: oneshot::Sender<(bool, bool, Option<String>, Option<String>)>,
    },
    EnableListening,
    DisableListening,
    PlayAudio {
        audio_data: Vec<f32>,
        sample_rate: u32,
        respond: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

// ============================================================================
// Audio buffer for tracking user speech
// ============================================================================

struct UserAudioBuffer {
    samples: Vec<i16>,
    last_packet_time: Instant,
    is_speaking: bool,
}

impl UserAudioBuffer {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            last_packet_time: Instant::now(),
            is_speaking: false,
        }
    }
}

// ============================================================================
// Shared State
// ============================================================================

struct BotState {
    in_voice: bool,
    listening: bool,
    current_guild: Option<GuildId>,
    current_channel: Option<ChannelId>,
    guild_name: Option<String>,
    channel_name: Option<String>,
    command_rx: Option<mpsc::Receiver<DiscordCommand>>,
    user_audio_buffers: HashMap<u32, UserAudioBuffer>, // ssrc -> buffer
    ssrc_to_user: HashMap<u32, String>,                // ssrc -> user_id string
}

impl BotState {
    fn new(command_rx: mpsc::Receiver<DiscordCommand>) -> Self {
        Self {
            in_voice: false,
            listening: false,
            current_guild: None,
            current_channel: None,
            guild_name: None,
            channel_name: None,
            command_rx: Some(command_rx),
            user_audio_buffers: HashMap::new(),
            ssrc_to_user: HashMap::new(),
        }
    }
}

// TypeMapKey for storing state in serenity's context
struct BotStateKey;
impl TypeMapKey for BotStateKey {
    type Value = Arc<RwLock<BotState>>;
}

// Key for storing the call handle
struct CallKey;
impl TypeMapKey for CallKey {
    type Value = Arc<Mutex<Option<Arc<tokio::sync::Mutex<songbird::Call>>>>>;
}

// ============================================================================
// Voice Event Handler - Receives audio from Discord
// ============================================================================

#[derive(Clone)]
struct VoiceReceiver {
    state: Arc<RwLock<BotState>>,
}

impl VoiceReceiver {
    fn new(state: Arc<RwLock<BotState>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl VoiceEventHandler for VoiceReceiver {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                let mut state = self.state.write().await;

                // Map SSRC to user ID if we have it
                if let Some(user_id) = speaking.user_id {
                    state.ssrc_to_user.insert(speaking.ssrc, user_id.to_string());
                    log::debug!("Mapped SSRC {} to user {}", speaking.ssrc, user_id);
                }

                let is_speaking = speaking.speaking.bits() > 0;
                let user_id = state
                    .ssrc_to_user
                    .get(&speaking.ssrc)
                    .cloned()
                    .unwrap_or_else(|| speaking.ssrc.to_string());

                // Get listening state before mutable borrow of buffer
                let listening = state.listening;

                log::debug!(
                    "SpeakingStateUpdate: user={}, speaking={}, listening={}",
                    user_id,
                    is_speaking,
                    listening
                );

                // Track speaking state changes
                let buffer = state
                    .user_audio_buffers
                    .entry(speaking.ssrc)
                    .or_insert_with(UserAudioBuffer::new);

                if is_speaking && !buffer.is_speaking {
                    buffer.is_speaking = true;
                    log::info!("User {} started speaking (listening={})", user_id, listening);
                    if listening {
                        send_response(&Response::UserStartedSpeaking {
                            user_id: user_id.clone(),
                        });
                    }
                } else if !is_speaking && buffer.is_speaking {
                    buffer.is_speaking = false;
                    log::info!(
                        "User {} stopped speaking, buffer has {} samples (listening={})",
                        user_id,
                        buffer.samples.len(),
                        listening
                    );

                    // User stopped speaking - send collected audio if we have enough
                    if listening && !buffer.samples.is_empty() {
                        log::info!(
                            "Sending {} audio samples from user {}",
                            buffer.samples.len(),
                            user_id
                        );
                        // Convert i16 stereo 48kHz to f32 mono 16kHz
                        let mono_16k = convert_audio(&buffer.samples, 48000, 16000);

                        // Encode as base64
                        let bytes: Vec<u8> = mono_16k
                            .iter()
                            .flat_map(|&s| s.to_le_bytes())
                            .collect();
                        let audio_base64 = BASE64.encode(&bytes);

                        log::info!(
                            "Sending UserAudio: {} mono samples, {} base64 bytes",
                            mono_16k.len(),
                            audio_base64.len()
                        );

                        send_response(&Response::UserAudio {
                            user_id: user_id.clone(),
                            audio_base64,
                            sample_rate: 16000,
                        });

                        send_response(&Response::UserStoppedSpeaking { user_id });
                    }

                    // Clear buffer
                    buffer.samples.clear();
                }
            }
            EventContext::VoiceTick(tick) => {
                let mut state = self.state.write().await;

                if !state.listening {
                    return None;
                }

                // Log speaking users count occasionally
                if !tick.speaking.is_empty() {
                    log::debug!(
                        "VoiceTick: {} speaking, {} silent",
                        tick.speaking.len(),
                        tick.silent.len()
                    );
                }

                // Collect audio from speaking users
                for (ssrc, data) in &tick.speaking {
                    // Check if we have decoded audio
                    if let Some(decoded) = data.decoded_voice.as_ref() {
                        // Look up user_id before taking mutable borrow on user_audio_buffers
                        let user_id_for_log = state
                            .ssrc_to_user
                            .get(ssrc)
                            .cloned()
                            .unwrap_or_else(|| ssrc.to_string());

                        let buffer = state
                            .user_audio_buffers
                            .entry(*ssrc)
                            .or_insert_with(UserAudioBuffer::new);

                        // Mark as speaking when we receive audio (in case SpeakingStateUpdate didn't fire)
                        if !buffer.is_speaking {
                            buffer.is_speaking = true;
                            log::info!("User {} started speaking (detected from audio)", user_id_for_log);
                        }

                        buffer.samples.extend_from_slice(decoded);
                        buffer.last_packet_time = Instant::now();

                        // Log progress at meaningful intervals (every ~1 second of audio at 48kHz stereo)
                        if buffer.samples.len() % 96000 < decoded.len() {
                            log::info!(
                                "VoiceTick: collected {:.1}s of audio for SSRC {} ({} samples)",
                                buffer.samples.len() as f32 / 96000.0,
                                ssrc,
                                buffer.samples.len()
                            );
                        }
                    } else {
                        // No decoded audio - this could mean decode failed or decode mode is wrong
                        log::debug!(
                            "VoiceTick: SSRC {} speaking but no decoded audio (has packet: {})",
                            ssrc,
                            data.packet.is_some()
                        );
                    }
                }

                // Check for users who have been silent for too long (timeout-based speech end detection)
                // This handles cases where SpeakingStateUpdate doesn't fire reliably
                let silence_timeout = Duration::from_millis(1500); // 1.5 seconds of silence = end of speech
                let min_samples_to_send = 16000; // At least ~0.17s of audio at 48kHz stereo

                // First, collect SSRCs that need to be processed (immutable borrow)
                let ssrcs_to_process: Vec<u32> = state
                    .user_audio_buffers
                    .iter()
                    .filter_map(|(ssrc, buffer)| {
                        if buffer.is_speaking
                            && buffer.last_packet_time.elapsed() > silence_timeout
                            && buffer.samples.len() >= min_samples_to_send
                        {
                            Some(*ssrc)
                        } else {
                            None
                        }
                    })
                    .collect();

                // Now process each SSRC separately (no overlapping borrows)
                let mut to_send: Vec<(u32, String, Vec<i16>)> = Vec::new();

                for ssrc in ssrcs_to_process {
                    // Look up user_id first
                    let user_id = state
                        .ssrc_to_user
                        .get(&ssrc)
                        .cloned()
                        .unwrap_or_else(|| ssrc.to_string());

                    // Now get mutable access to the buffer
                    if let Some(buffer) = state.user_audio_buffers.get_mut(&ssrc) {
                        log::info!(
                            "Silence timeout: sending {} audio samples from user {} (SSRC {})",
                            buffer.samples.len(),
                            user_id,
                            ssrc
                        );

                        // Take the samples and reset the buffer
                        let samples = std::mem::take(&mut buffer.samples);
                        buffer.is_speaking = false;

                        to_send.push((ssrc, user_id, samples));
                    }
                }

                // Now send the audio outside of the mutable borrow
                for (_ssrc, user_id, samples) in to_send {
                    // Convert i16 stereo 48kHz to f32 mono 16kHz
                    let mono_16k = convert_audio(&samples, 48000, 16000);

                    // Encode as base64
                    let bytes: Vec<u8> = mono_16k
                        .iter()
                        .flat_map(|&s| s.to_le_bytes())
                        .collect();
                    let audio_base64 = BASE64.encode(&bytes);

                    log::info!(
                        "Sending UserAudio (timeout): {} mono samples, {} base64 bytes",
                        mono_16k.len(),
                        audio_base64.len()
                    );

                    send_response(&Response::UserAudio {
                        user_id: user_id.clone(),
                        audio_base64,
                        sample_rate: 16000,
                    });

                    send_response(&Response::UserStoppedSpeaking { user_id });
                }
            }
            _ => {}
        }
        None
    }
}

/// Convert stereo audio to mono and resample
fn convert_audio(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<f32> {
    // First convert stereo to mono (average channels)
    let mono: Vec<f32> = samples
        .chunks(2)
        .map(|chunk| {
            let left = chunk.get(0).copied().unwrap_or(0) as f32 / 32768.0;
            let right = chunk.get(1).copied().unwrap_or(0) as f32 / 32768.0;
            (left + right) / 2.0
        })
        .collect();

    // Simple linear resampling
    if from_rate == to_rate {
        return mono;
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (mono.len() as f64 / ratio) as usize;

    (0..new_len)
        .map(|i| {
            let src_idx = (i as f64 * ratio) as usize;
            mono.get(src_idx).copied().unwrap_or(0.0)
        })
        .collect()
}

// ============================================================================
// Serenity Event Handler
// ============================================================================

struct Handler;

// Track when guilds are ready
struct GuildsReady;
impl TypeMapKey for GuildsReady {
    type Value = Arc<tokio::sync::RwLock<bool>>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("Bot connected as {}", ready.user.name);
        log::info!("Guilds in ready payload: {} (these are UnavailableGuild - full data comes via GUILD_CREATE)", ready.guilds.len());

        // Log guild IDs from ready payload
        for guild in &ready.guilds {
            log::info!("  - Guild ID from ready: {}", guild.id);
        }

        // Note: Don't send response here - the main loop's Connect handler will do it
        // after detecting the connection is ready
        log::info!("Ready event processed, waiting for main loop to send response");

        // Start command processing loop
        let data = ctx.data.read().await;
        if let Some(state) = data.get::<BotStateKey>() {
            let state = state.clone();
            let call_holder = data.get::<CallKey>().cloned();
            drop(data);

            // Take the receiver out of the state
            let mut state_write = state.write().await;
            if let Some(mut command_rx) = state_write.command_rx.take() {
                drop(state_write);

                // Spawn command handler
                let ctx_clone = ctx.clone();
                let state_clone = state.clone();
                tokio::spawn(async move {
                    while let Some(cmd) = command_rx.recv().await {
                        handle_discord_command(&ctx_clone, &state_clone, call_holder.as_ref(), cmd).await;
                    }
                    log::info!("Command receiver closed");
                });
            }
        }
    }

    async fn voice_state_update(&self, _ctx: Context, _old: Option<VoiceState>, _new: VoiceState) {
        // Track voice state changes if needed
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        log::info!("Guild cached: {} ({})", guild.name, guild.id);

        // Log total cached guilds
        let cached_count = ctx.cache.guilds().len();
        log::info!("Total guilds in cache: {}", cached_count);

        // Mark guilds as ready
        let data = ctx.data.read().await;
        if let Some(ready_flag) = data.get::<GuildsReady>() {
            *ready_flag.write().await = true;
        }
    }
}

// ============================================================================
// Command Handler
// ============================================================================

async fn handle_discord_command(
    ctx: &Context,
    state: &Arc<RwLock<BotState>>,
    call_holder: Option<&Arc<Mutex<Option<Arc<tokio::sync::Mutex<songbird::Call>>>>>>,
    cmd: DiscordCommand,
) {
    match cmd {
        DiscordCommand::GetGuilds { respond } => {
            let guilds = get_guilds(ctx);
            let _ = respond.send(guilds);
        }
        DiscordCommand::GetChannels { guild_id, respond } => {
            let channels = get_voice_channels(ctx, &guild_id);
            let _ = respond.send(channels);
        }
        DiscordCommand::JoinVoice {
            guild_id,
            channel_id,
            respond,
        } => {
            let result = join_voice(ctx, state, call_holder, &guild_id, &channel_id).await;
            let _ = respond.send(result);
        }
        DiscordCommand::LeaveVoice { guild_id, respond } => {
            let result = leave_voice(ctx, state, &guild_id).await;
            let _ = respond.send(result);
        }
        DiscordCommand::GetStatus { respond } => {
            let state_guard = state.read().await;
            let _ = respond.send((
                state_guard.in_voice,
                state_guard.listening,
                state_guard.guild_name.clone(),
                state_guard.channel_name.clone(),
            ));
        }
        DiscordCommand::EnableListening => {
            let mut state_guard = state.write().await;
            state_guard.listening = true;
            log::info!("Listening enabled");
        }
        DiscordCommand::DisableListening => {
            let mut state_guard = state.write().await;
            state_guard.listening = false;
            // Clear all audio buffers
            state_guard.user_audio_buffers.clear();
            log::info!("Listening disabled");
        }
        DiscordCommand::PlayAudio {
            audio_data,
            sample_rate,
            respond,
        } => {
            let result = play_audio(call_holder, &audio_data, sample_rate).await;
            let _ = respond.send(result);
        }
        DiscordCommand::Shutdown => {
            log::info!("Shutdown command received in Discord task");
        }
    }
}

fn get_guilds(ctx: &Context) -> Vec<GuildInfo> {
    let cache = &ctx.cache;
    let guild_ids: Vec<GuildId> = cache.guilds();

    log::info!("get_guilds called - found {} guild IDs in cache", guild_ids.len());

    let mut guilds = Vec::new();
    for guild_id in &guild_ids {
        log::info!("Checking guild ID: {}", guild_id);
        if let Some(guild) = cache.guild(*guild_id) {
            log::info!("Found guild in cache: {} ({})", guild.name, guild_id);
            guilds.push(GuildInfo {
                id: guild_id.to_string(),
                name: guild.name.clone(),
            });
        } else {
            log::warn!("Guild ID {} not found in cache details", guild_id);
        }
    }

    log::info!("Returning {} guilds", guilds.len());
    guilds
}

fn get_voice_channels(ctx: &Context, guild_id: &str) -> Vec<ChannelInfo> {
    let guild_id: u64 = match guild_id.parse() {
        Ok(id) => id,
        Err(_) => return vec![],
    };
    let guild_id = GuildId::new(guild_id);

    let cache = &ctx.cache;
    let mut channels = Vec::new();

    if let Some(guild) = cache.guild(guild_id) {
        for (channel_id, channel) in guild.channels.iter() {
            if channel.kind == serenity::model::channel::ChannelType::Voice {
                channels.push(ChannelInfo {
                    id: channel_id.to_string(),
                    name: channel.name.clone(),
                    kind: "voice".to_string(),
                });
            }
        }
    }
    channels
}

async fn join_voice(
    ctx: &Context,
    state: &Arc<RwLock<BotState>>,
    call_holder: Option<&Arc<Mutex<Option<Arc<tokio::sync::Mutex<songbird::Call>>>>>>,
    guild_id: &str,
    channel_id: &str,
) -> Result<(), String> {
    log::info!("join_voice called: guild={}, channel={}", guild_id, channel_id);

    let guild_id: u64 = guild_id
        .parse()
        .map_err(|_| "Invalid guild ID".to_string())?;
    let channel_id: u64 = channel_id
        .parse()
        .map_err(|_| "Invalid channel ID".to_string())?;

    let guild_id = GuildId::new(guild_id);
    let channel_id = ChannelId::new(channel_id);

    log::info!("Joining voice channel: guild={}, channel={}", guild_id, channel_id);

    let manager = songbird::get(ctx)
        .await
        .ok_or_else(|| "Songbird not initialized".to_string())?;

    log::info!("Got Songbird manager, joining channel...");

    let call_lock = manager
        .join(guild_id, channel_id)
        .await
        .map_err(|e| format!("Failed to join voice channel: {:?}", e))?;

    log::info!("Successfully joined voice channel! Setting up audio receiver...");

    // Store the call handle for later audio playback
    if let Some(holder) = call_holder {
        let mut holder_guard = holder.lock().await;
        *holder_guard = Some(call_lock.clone());
    }

    // Set up voice receiver to capture audio events
    // Note: DecodeMode is configured at Songbird initialization time
    {
        let mut call = call_lock.lock().await;

        let receiver = VoiceReceiver::new(state.clone());
        call.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
        call.add_global_event(CoreEvent::VoiceTick.into(), receiver);

        log::info!("Voice receiver registered for audio capture");
    }

    // Update state
    {
        let mut state_guard = state.write().await;
        state_guard.in_voice = true;
        state_guard.current_guild = Some(guild_id);
        state_guard.current_channel = Some(channel_id);

        // Get names from cache
        if let Some(guild) = ctx.cache.guild(guild_id) {
            state_guard.guild_name = Some(guild.name.clone());
            if let Some(channel) = guild.channels.get(&channel_id) {
                state_guard.channel_name = Some(channel.name.clone());
            }
        }
    }

    Ok(())
}

async fn leave_voice(
    ctx: &Context,
    state: &Arc<RwLock<BotState>>,
    guild_id: &str,
) -> Result<(), String> {
    let guild_id: u64 = guild_id
        .parse()
        .map_err(|_| "Invalid guild ID".to_string())?;
    let guild_id = GuildId::new(guild_id);

    let manager = songbird::get(ctx)
        .await
        .ok_or_else(|| "Songbird not initialized".to_string())?;

    manager
        .remove(guild_id)
        .await
        .map_err(|e| format!("Failed to leave voice: {:?}", e))?;

    // Update state
    {
        let mut state_guard = state.write().await;
        state_guard.in_voice = false;
        state_guard.listening = false;
        state_guard.current_guild = None;
        state_guard.current_channel = None;
        state_guard.guild_name = None;
        state_guard.channel_name = None;
        state_guard.user_audio_buffers.clear();
        state_guard.ssrc_to_user.clear();
    }

    Ok(())
}

/// A simple MediaSource wrapper around a Cursor<Vec<u8>> for in-memory audio
struct MemoryAudioSource {
    cursor: Cursor<Vec<u8>>,
    len: u64,
}

impl MemoryAudioSource {
    fn new(data: Vec<u8>) -> Self {
        let len = data.len() as u64;
        Self {
            cursor: Cursor::new(data),
            len,
        }
    }
}

impl Read for MemoryAudioSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl Seek for MemoryAudioSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl MediaSource for MemoryAudioSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

async fn play_audio(
    call_holder: Option<&Arc<Mutex<Option<Arc<tokio::sync::Mutex<songbird::Call>>>>>>,
    audio_data: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    let call_holder = call_holder.ok_or_else(|| "No call holder".to_string())?;
    let holder_guard = call_holder.lock().await;
    let call_lock = holder_guard
        .as_ref()
        .ok_or_else(|| "Not in a voice channel".to_string())?;

    // Songbird expects stereo 48kHz f32 PCM
    // First resample to 48kHz if needed, then duplicate to stereo
    let resampled = if sample_rate != 48000 {
        let ratio = 48000.0 / sample_rate as f64;
        let new_len = (audio_data.len() as f64 * ratio) as usize;
        (0..new_len)
            .map(|i| {
                let src_idx = (i as f64 / ratio) as usize;
                audio_data.get(src_idx).copied().unwrap_or(0.0)
            })
            .collect::<Vec<f32>>()
    } else {
        audio_data.to_vec()
    };

    // Convert mono to stereo (interleaved f32)
    let stereo: Vec<f32> = resampled
        .iter()
        .flat_map(|&sample| [sample, sample]) // duplicate for L+R
        .collect();

    // Convert f32 samples to bytes (little-endian)
    let audio_bytes: Vec<u8> = stereo
        .iter()
        .flat_map(|&s| s.to_le_bytes())
        .collect();

    // Create the memory source
    let source = MemoryAudioSource::new(audio_bytes);

    // Create RawAdapter - expects f32 interleaved PCM
    let adapter = RawAdapter::new(source, 48000, 2);

    // Convert to Input
    let input: Input = adapter.into();

    // Play the audio
    let mut call = call_lock.lock().await;
    call.play_input(input);

    Ok(())
}


// ============================================================================
// IPC Communication
// ============================================================================

fn send_response(response: &Response) {
    let json = serde_json::to_string(response).unwrap();
    println!("{}", json);
    io::stdout().flush().unwrap();
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    // Configure logging: show info for our code, warn for noisy libraries
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(
            "info,serenity=warn,songbird=warn,tracing=warn,rustls=warn,h2=warn,hyper=warn,tungstenite=warn"
        )
    )
    .format_timestamp(None)
    .format_target(false)
    .init();

    log::info!("Discord sidecar starting...");

    // Signal ready
    send_response(&Response::Ok {
        message: "Discord sidecar ready".to_string(),
    });

    // Channel for sending commands to Discord client
    let (command_tx, command_rx) = mpsc::channel::<DiscordCommand>(32);
    let command_tx = Arc::new(tokio::sync::Mutex::new(Some(command_tx)));

    // Track if we're connected
    let connected = Arc::new(tokio::sync::RwLock::new(false));
    let client_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Spawn stdin reader task
    let command_tx_clone = command_tx.clone();
    let connected_clone = connected.clone();
    let client_handle_clone = client_handle.clone();

    // We need to read stdin in a blocking manner but process commands async
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(32);

    // Spawn blocking stdin reader
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if stdin_tx.blocking_send(l).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    log::error!("Failed to read stdin: {}", e);
                    break;
                }
            }
        }
    });

    // Process commands
    loop {
        let line = match stdin_rx.recv().await {
            Some(l) => l,
            None => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                send_response(&Response::Error {
                    message: format!("Invalid JSON: {}", e),
                });
                continue;
            }
        };

        match request {
            Request::Connect { token } => {
                log::info!("Connecting to Discord...");

                let is_connected = *connected_clone.read().await;
                if is_connected {
                    send_response(&Response::Ok {
                        message: "Already connected".to_string(),
                    });
                    continue;
                }

                // Create new command channel for this connection
                let (new_tx, new_rx) = mpsc::channel::<DiscordCommand>(32);
                {
                    let mut tx_guard = command_tx_clone.lock().await;
                    *tx_guard = Some(new_tx);
                }

                // GUILDS intent is required to receive GUILD_CREATE events
                // which populate the cache with guild information
                let intents = GatewayIntents::GUILDS
                    | GatewayIntents::GUILD_VOICE_STATES
                    | GatewayIntents::GUILD_MESSAGES
                    | GatewayIntents::MESSAGE_CONTENT; // Additional intent for debugging

                // Create shared state
                let state = Arc::new(RwLock::new(BotState::new(new_rx)));
                let call_holder = Arc::new(Mutex::new(None));
                let guilds_ready = Arc::new(tokio::sync::RwLock::new(false));

                // Configure cache to store guild information
                let cache_settings = CacheSettings::default();

                // Configure Songbird with DecodeMode::Decode to receive audio
                // This MUST be done at initialization using register_songbird_from_config
                let songbird_config = Config::default().decode_mode(DecodeMode::Decode);

                let mut client = match Client::builder(&token, intents)
                    .event_handler(Handler)
                    .register_songbird_from_config(songbird_config)
                    .cache_settings(cache_settings)
                    .type_map_insert::<BotStateKey>(state)
                    .type_map_insert::<CallKey>(call_holder)
                    .type_map_insert::<GuildsReady>(guilds_ready)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Failed to create client: {}", e),
                        });
                        continue;
                    }
                };

                let connected_inner = connected_clone.clone();

                // Start client in background task
                let handle = tokio::spawn(async move {
                    *connected_inner.write().await = true;
                    if let Err(e) = client.start().await {
                        log::error!("Client error: {}", e);
                    }
                    *connected_inner.write().await = false;
                });

                *client_handle_clone.lock().await = Some(handle);

                // Wait for connection to be established (poll the connected flag)
                let mut attempts = 0;
                let max_attempts = 50; // 5 seconds max
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let is_now_connected = *connected_clone.read().await;
                    if is_now_connected {
                        log::info!("Discord client connected successfully");
                        send_response(&Response::Ok {
                            message: "Connected to Discord".to_string(),
                        });
                        break;
                    }
                    attempts += 1;
                    if attempts >= max_attempts {
                        log::error!("Timeout waiting for Discord connection");
                        send_response(&Response::Error {
                            message: "Timeout waiting for Discord connection".to_string(),
                        });
                        break;
                    }
                }
            }

            Request::Disconnect => {
                log::info!("Disconnecting from Discord...");
                *connected.write().await = false;

                // Abort client task if running
                if let Some(handle) = client_handle.lock().await.take() {
                    handle.abort();
                }

                send_response(&Response::Ok {
                    message: "Disconnected".to_string(),
                });
            }

            Request::GetGuilds => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx.send(DiscordCommand::GetGuilds { respond: respond_tx }).await.is_ok() {
                        match tokio::time::timeout(std::time::Duration::from_secs(5), respond_rx).await {
                            Ok(Ok(guilds)) => {
                                send_response(&Response::Guilds { guilds });
                            }
                            _ => {
                                send_response(&Response::Guilds { guilds: vec![] });
                            }
                        }
                    } else {
                        send_response(&Response::Guilds { guilds: vec![] });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::GetChannels { guild_id } => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx
                        .send(DiscordCommand::GetChannels {
                            guild_id,
                            respond: respond_tx,
                        })
                        .await
                        .is_ok()
                    {
                        match tokio::time::timeout(std::time::Duration::from_secs(5), respond_rx).await {
                            Ok(Ok(channels)) => {
                                send_response(&Response::Channels { channels });
                            }
                            _ => {
                                send_response(&Response::Channels { channels: vec![] });
                            }
                        }
                    } else {
                        send_response(&Response::Channels { channels: vec![] });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::JoinVoice {
                guild_id,
                channel_id,
            } => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx
                        .send(DiscordCommand::JoinVoice {
                            guild_id,
                            channel_id,
                            respond: respond_tx,
                        })
                        .await
                        .is_ok()
                    {
                        match tokio::time::timeout(std::time::Duration::from_secs(10), respond_rx).await {
                            Ok(Ok(Ok(()))) => {
                                send_response(&Response::Ok {
                                    message: "Joined voice channel".to_string(),
                                });
                            }
                            Ok(Ok(Err(e))) => {
                                send_response(&Response::Error { message: e });
                            }
                            _ => {
                                send_response(&Response::Error {
                                    message: "Timeout joining voice".to_string(),
                                });
                            }
                        }
                    } else {
                        send_response(&Response::Error {
                            message: "Failed to send command".to_string(),
                        });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::LeaveVoice { guild_id } => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx
                        .send(DiscordCommand::LeaveVoice {
                            guild_id,
                            respond: respond_tx,
                        })
                        .await
                        .is_ok()
                    {
                        match tokio::time::timeout(std::time::Duration::from_secs(5), respond_rx).await {
                            Ok(Ok(Ok(()))) => {
                                send_response(&Response::Ok {
                                    message: "Left voice channel".to_string(),
                                });
                            }
                            Ok(Ok(Err(e))) => {
                                send_response(&Response::Error { message: e });
                            }
                            _ => {
                                send_response(&Response::Error {
                                    message: "Timeout leaving voice".to_string(),
                                });
                            }
                        }
                    } else {
                        send_response(&Response::Error {
                            message: "Failed to send command".to_string(),
                        });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::Status => {
                let is_connected = *connected.read().await;
                if !is_connected {
                    send_response(&Response::Status {
                        connected: false,
                        in_voice: false,
                        listening: false,
                        guild_name: None,
                        channel_name: None,
                    });
                    continue;
                }

                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx.send(DiscordCommand::GetStatus { respond: respond_tx }).await.is_ok() {
                        match tokio::time::timeout(std::time::Duration::from_secs(5), respond_rx).await {
                            Ok(Ok((in_voice, listening, guild_name, channel_name))) => {
                                send_response(&Response::Status {
                                    connected: true,
                                    in_voice,
                                    listening,
                                    guild_name,
                                    channel_name,
                                });
                            }
                            _ => {
                                send_response(&Response::Status {
                                    connected: true,
                                    in_voice: false,
                                    listening: false,
                                    guild_name: None,
                                    channel_name: None,
                                });
                            }
                        }
                    } else {
                        send_response(&Response::Status {
                            connected: false,
                            in_voice: false,
                            listening: false,
                            guild_name: None,
                            channel_name: None,
                        });
                    }
                } else {
                    send_response(&Response::Status {
                        connected: false,
                        in_voice: false,
                        listening: false,
                        guild_name: None,
                        channel_name: None,
                    });
                }
            }

            Request::EnableListening => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    if tx.send(DiscordCommand::EnableListening).await.is_ok() {
                        send_response(&Response::Ok {
                            message: "Listening enabled".to_string(),
                        });
                    } else {
                        send_response(&Response::Error {
                            message: "Failed to enable listening".to_string(),
                        });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::DisableListening => {
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    if tx.send(DiscordCommand::DisableListening).await.is_ok() {
                        send_response(&Response::Ok {
                            message: "Listening disabled".to_string(),
                        });
                    } else {
                        send_response(&Response::Error {
                            message: "Failed to disable listening".to_string(),
                        });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::PlayAudio {
                audio_base64,
                sample_rate,
            } => {
                // Decode base64 audio
                let audio_bytes = match BASE64.decode(&audio_base64) {
                    Ok(b) => b,
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Invalid base64 audio: {}", e),
                        });
                        continue;
                    }
                };

                // Convert bytes to f32 samples
                let audio_data: Vec<f32> = audio_bytes
                    .chunks(4)
                    .filter_map(|chunk| {
                        if chunk.len() == 4 {
                            Some(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        } else {
                            None
                        }
                    })
                    .collect();

                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let (respond_tx, respond_rx) = oneshot::channel();
                    if tx
                        .send(DiscordCommand::PlayAudio {
                            audio_data,
                            sample_rate,
                            respond: respond_tx,
                        })
                        .await
                        .is_ok()
                    {
                        match tokio::time::timeout(std::time::Duration::from_secs(30), respond_rx).await {
                            Ok(Ok(Ok(()))) => {
                                send_response(&Response::Ok {
                                    message: "Audio playback started".to_string(),
                                });
                            }
                            Ok(Ok(Err(e))) => {
                                send_response(&Response::Error { message: e });
                            }
                            _ => {
                                send_response(&Response::Error {
                                    message: "Timeout playing audio".to_string(),
                                });
                            }
                        }
                    } else {
                        send_response(&Response::Error {
                            message: "Failed to send command".to_string(),
                        });
                    }
                } else {
                    send_response(&Response::Error {
                        message: "Not connected".to_string(),
                    });
                }
            }

            Request::Shutdown => {
                log::info!("Shutdown requested");

                // Send shutdown to Discord task
                let tx_guard = command_tx.lock().await;
                if let Some(tx) = tx_guard.as_ref() {
                    let _ = tx.send(DiscordCommand::Shutdown).await;
                }

                // Abort client task if running
                if let Some(handle) = client_handle.lock().await.take() {
                    handle.abort();
                }

                send_response(&Response::Ok {
                    message: "Shutting down".to_string(),
                });
                break;
            }
        }
    }

    log::info!("Discord sidecar exiting");
}
