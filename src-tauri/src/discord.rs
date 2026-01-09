//! Discord Bot Manager - Communicates with Discord sidecar process
//!
//! The Discord bot runs in a separate process to isolate dependencies.
//! Communication is via JSON over stdin/stdout pipes.

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

/// Request types sent to the sidecar
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SidecarRequest {
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
        audio_base64: String,
        sample_rate: u32,
    },
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Response types from the sidecar
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum SidecarResponse {
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
        audio_base64: String,
        sample_rate: u32,
    },
    #[serde(rename = "user_started_speaking")]
    UserStartedSpeaking { user_id: String },
    #[serde(rename = "user_stopped_speaking")]
    UserStoppedSpeaking { user_id: String },
}

#[derive(Debug, Deserialize, Serialize, Clone, specta::Type)]
pub struct GuildInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, specta::Type)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// Discord state for frontend
#[derive(Debug, Serialize, Clone, specta::Type)]
pub struct DiscordState {
    pub connected: bool,
    pub in_voice: bool,
    pub listening: bool,
    pub guild_name: Option<String>,
    pub channel_name: Option<String>,
    pub error: Option<String>,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self {
            connected: false,
            in_voice: false,
            listening: false,
            guild_name: None,
            channel_name: None,
            error: None,
        }
    }
}

/// Channel message types for sidecar communication
#[derive(Debug)]
pub enum SidecarMessage {
    Response(SidecarResponse),
    Event(SidecarResponse), // Async events like UserAudio
}

/// Manages communication with the Discord sidecar process
struct SidecarProcess {
    child: Child,
    token: Option<String>,
    response_rx: mpsc::Receiver<SidecarResponse>,
    event_tx: Option<mpsc::Sender<SidecarResponse>>,
    reader_running: Arc<AtomicBool>,
    _reader_handle: Option<thread::JoinHandle<()>>,
}

impl SidecarProcess {
    fn spawn(sidecar_path: &Path, event_tx: Option<mpsc::Sender<SidecarResponse>>) -> Result<Self, String> {
        info!("Spawning Discord sidecar from: {:?}", sidecar_path);

        let mut child = Command::new(sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn Discord sidecar: {}", e))?;

        // Take stdout for the reader thread
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to get sidecar stdout".to_string())?;

        // Create channels for response communication
        let (response_tx, response_rx) = mpsc::channel();
        let reader_running = Arc::new(AtomicBool::new(true));
        let reader_running_clone = reader_running.clone();
        let event_tx_clone = event_tx.clone();

        // Spawn reader thread
        let reader_handle = thread::spawn(move || {
            Self::reader_loop(stdout, response_tx, event_tx_clone, reader_running_clone);
        });

        // Wait for the ready message
        let response = response_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| format!("Failed to receive ready message: {}", e))?;

        match response {
            SidecarResponse::Ok { message } => {
                info!("Discord sidecar ready: {}", message);
            }
            SidecarResponse::Error { message } => {
                return Err(format!("Discord sidecar failed to start: {}", message));
            }
            _ => {
                return Err("Unexpected response from Discord sidecar".to_string());
            }
        }

        Ok(Self {
            child,
            token: None,
            response_rx,
            event_tx,
            reader_running,
            _reader_handle: Some(reader_handle),
        })
    }

    /// Reader thread that processes all stdout messages
    fn reader_loop(
        stdout: ChildStdout,
        response_tx: mpsc::Sender<SidecarResponse>,
        event_tx: Option<mpsc::Sender<SidecarResponse>>,
        running: Arc<AtomicBool>,
    ) {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        while running.load(Ordering::Relaxed) {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF
                    debug!("Sidecar stdout closed");
                    break;
                }
                Ok(_) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<SidecarResponse>(&line) {
                        Ok(response) => {
                            // Check if this is an async event or a response
                            match &response {
                                SidecarResponse::UserAudio { .. }
                                | SidecarResponse::UserStartedSpeaking { .. }
                                | SidecarResponse::UserStoppedSpeaking { .. } => {
                                    // These are async events - send to event channel
                                    if let Some(ref tx) = event_tx {
                                        if let Err(e) = tx.send(response.clone()) {
                                            debug!("Failed to send event: {}", e);
                                        }
                                    }
                                }
                                _ => {
                                    // This is a response to a request
                                    if let Err(e) = response_tx.send(response) {
                                        debug!("Failed to send response: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse sidecar message: {} - line: {}", e, line.trim());
                        }
                    }
                }
                Err(e) => {
                    if running.load(Ordering::Relaxed) {
                        error!("Error reading from sidecar: {}", e);
                    }
                    break;
                }
            }
        }
        debug!("Reader thread exiting");
    }

    fn send_request(&mut self, request: &SidecarRequest) -> Result<SidecarResponse, String> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| "Sidecar stdin not available".to_string())?;

        let json = serde_json::to_string(request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        writeln!(stdin, "{}", json)
            .map_err(|e| format!("Failed to write to sidecar: {}", e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush sidecar stdin: {}", e))?;

        // Wait for response from the reader thread via channel
        self.response_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|e| format!("Failed to receive response: {}", e))
    }

    fn connect(&mut self, token: &str) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::Connect {
            token: token.to_string(),
        })?;

        match response {
            SidecarResponse::Ok { .. } => {
                self.token = Some(token.to_string());
                Ok(())
            }
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn disconnect(&mut self) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::Disconnect)?;

        match response {
            SidecarResponse::Ok { .. } => {
                self.token = None;
                Ok(())
            }
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn join_voice(&mut self, guild_id: &str, channel_id: &str) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::JoinVoice {
            guild_id: guild_id.to_string(),
            channel_id: channel_id.to_string(),
        })?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn leave_voice(&mut self, guild_id: &str) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::LeaveVoice {
            guild_id: guild_id.to_string(),
        })?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn get_guilds(&mut self) -> Result<Vec<GuildInfo>, String> {
        let response = self.send_request(&SidecarRequest::GetGuilds)?;

        match response {
            SidecarResponse::Guilds { guilds } => Ok(guilds),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn get_channels(&mut self, guild_id: &str) -> Result<Vec<ChannelInfo>, String> {
        let response = self.send_request(&SidecarRequest::GetChannels {
            guild_id: guild_id.to_string(),
        })?;

        match response {
            SidecarResponse::Channels { channels } => Ok(channels),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn status(&mut self) -> Result<DiscordState, String> {
        let response = self.send_request(&SidecarRequest::Status)?;

        match response {
            SidecarResponse::Status {
                connected,
                in_voice,
                listening,
                guild_name,
                channel_name,
            } => Ok(DiscordState {
                connected,
                in_voice,
                listening,
                guild_name,
                channel_name,
                error: None,
            }),
            SidecarResponse::Error { message } => Ok(DiscordState {
                connected: false,
                in_voice: false,
                listening: false,
                guild_name: None,
                channel_name: None,
                error: Some(message),
            }),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn enable_listening(&mut self) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::EnableListening)?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn disable_listening(&mut self) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::DisableListening)?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn play_audio(&mut self, audio_base64: &str, sample_rate: u32) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::PlayAudio {
            audio_base64: audio_base64.to_string(),
            sample_rate,
        })?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn shutdown(&mut self) {
        // Stop the reader thread first
        self.reader_running.store(false, Ordering::Relaxed);

        if let Err(e) = self.send_request(&SidecarRequest::Shutdown) {
            warn!("Error sending shutdown to Discord sidecar: {}", e);
        }
        if let Err(e) = self.child.wait() {
            warn!("Error waiting for Discord sidecar to exit: {}", e);
        }
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        info!("Dropping Discord sidecar process");
        self.shutdown();
    }
}

/// Thread-safe manager for the Discord sidecar
pub struct DiscordManager {
    sidecar: Mutex<Option<SidecarProcess>>,
    sidecar_path: PathBuf,
    bot_token: Mutex<Option<String>>,
    event_tx: Mutex<Option<mpsc::Sender<SidecarResponse>>>,
    event_rx: Mutex<Option<mpsc::Receiver<SidecarResponse>>>,
}

impl DiscordManager {
    pub fn new(sidecar_path: PathBuf) -> Self {
        // Create event channel
        let (event_tx, event_rx) = mpsc::channel();

        Self {
            sidecar: Mutex::new(None),
            sidecar_path,
            bot_token: Mutex::new(None),
            event_tx: Mutex::new(Some(event_tx)),
            event_rx: Mutex::new(Some(event_rx)),
        }
    }

    /// Get the sidecar, spawning it if necessary
    fn ensure_sidecar(&self) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();
        if guard.is_none() {
            info!("Starting Discord sidecar process...");
            let event_tx = self.event_tx.lock().unwrap().clone();
            let sidecar = SidecarProcess::spawn(&self.sidecar_path, event_tx)?;
            *guard = Some(sidecar);
        }
        Ok(())
    }

    /// Receive a pending event (non-blocking)
    /// Returns None if no event is available
    pub fn try_recv_event(&self) -> Option<SidecarResponse> {
        let rx_guard = self.event_rx.lock().unwrap();
        if let Some(ref rx) = *rx_guard {
            rx.try_recv().ok()
        } else {
            None
        }
    }

    /// Receive a pending event (blocking with timeout)
    pub fn recv_event_timeout(&self, timeout: std::time::Duration) -> Option<SidecarResponse> {
        let rx_guard = self.event_rx.lock().unwrap();
        if let Some(ref rx) = *rx_guard {
            rx.recv_timeout(timeout).ok()
        } else {
            None
        }
    }

    pub fn set_token(&self, token: String) -> Result<(), String> {
        let mut token_guard = self.bot_token.lock().unwrap();
        *token_guard = Some(token);
        Ok(())
    }

    pub fn get_token(&self) -> Option<String> {
        self.bot_token.lock().unwrap().clone()
    }

    pub fn connect(&self) -> Result<(), String> {
        let token = self
            .bot_token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "No bot token set".to_string())?;

        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.connect(&token)
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            sidecar.disconnect()?;
        }
        Ok(())
    }

    pub fn join_voice(&self, guild_id: &str, channel_id: &str) -> Result<(), String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.join_voice(guild_id, channel_id)
    }

    pub fn leave_voice(&self, guild_id: &str) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            sidecar.leave_voice(guild_id)?;
        }
        Ok(())
    }

    pub fn get_guilds(&self) -> Result<Vec<GuildInfo>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.get_guilds()
    }

    pub fn get_channels(&self, guild_id: &str) -> Result<Vec<ChannelInfo>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.get_channels(guild_id)
    }

    pub fn status(&self) -> DiscordState {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            sidecar.status().unwrap_or_default()
        } else {
            DiscordState::default()
        }
    }

    pub fn enable_listening(&self) -> Result<(), String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.enable_listening()
    }

    pub fn disable_listening(&self) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            sidecar.disable_listening()?;
        }
        Ok(())
    }

    pub fn play_audio(&self, audio_base64: &str, sample_rate: u32) -> Result<(), String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.play_audio(audio_base64, sample_rate)
    }

    pub fn speak(&self, _text: &str) -> Result<(), String> {
        // TODO: This should use the TTS system to convert text to audio,
        // then call play_audio with the result
        Err("speak() not yet implemented - use play_audio() with TTS output".to_string())
    }

    pub fn shutdown(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(mut sidecar) = guard.take() {
            sidecar.shutdown();
        }
    }
}

impl Default for DiscordManager {
    fn default() -> Self {
        Self::new(PathBuf::from("discord-sidecar"))
    }
}

impl Drop for DiscordManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
