//! Local TTS Manager - Communicates with TTS sidecar process
//!
//! The TTS runs in a separate process to avoid ort version conflicts with vad-rs.
//! Communication is via JSON over stdin/stdout pipes.

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// TTS voice configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsVoiceConfig {
    pub name: String,
    pub language: String,
}

impl Default for TtsVoiceConfig {
    fn default() -> Self {
        Self {
            name: "en_US-amy-medium".to_string(),
            language: "en-US".to_string(),
        }
    }
}

/// Request types sent to the TTS sidecar
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SidecarRequest {
    #[serde(rename = "load")]
    Load { model_path: String },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "speak")]
    Speak {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_device: Option<String>,
        volume: f32,
    },
    #[serde(rename = "synthesize")]
    Synthesize { text: String },
    #[serde(rename = "list_devices")]
    ListDevices,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Response types from the TTS sidecar
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SidecarResponse {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "devices")]
    Devices { devices: Vec<String> },
    #[serde(rename = "status")]
    Status {
        loaded: bool,
        model_path: Option<String>,
    },
    #[serde(rename = "audio")]
    Audio {
        audio_base64: String,
        sample_rate: u32,
    },
}

/// Manages communication with the TTS sidecar process
struct TtsSidecarProcess {
    child: Child,
    model_path: Option<String>,
}

impl TtsSidecarProcess {
    fn spawn(sidecar_path: &Path) -> Result<Self, String> {
        info!("Spawning TTS sidecar from: {:?}", sidecar_path);

        let mut child = Command::new(sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Let stderr go to parent for debugging
            .spawn()
            .map_err(|e| format!("Failed to spawn TTS sidecar: {}", e))?;

        // Wait for the ready message
        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| "Failed to get TTS sidecar stdout".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read TTS sidecar ready message: {}", e))?;

        let response: SidecarResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid TTS sidecar response: {}", e))?;

        match response {
            SidecarResponse::Ok { message } => {
                info!("TTS Sidecar ready: {}", message);
            }
            SidecarResponse::Error { message } => {
                return Err(format!("TTS Sidecar failed to start: {}", message));
            }
            _ => {
                return Err("Unexpected response from TTS sidecar".to_string());
            }
        }

        Ok(Self {
            child,
            model_path: None,
        })
    }

    fn send_request(&mut self, request: &SidecarRequest) -> Result<SidecarResponse, String> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| "TTS sidecar stdin not available".to_string())?;

        let json = serde_json::to_string(request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        writeln!(stdin, "{}", json)
            .map_err(|e| format!("Failed to write to TTS sidecar: {}", e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush TTS sidecar stdin: {}", e))?;

        // Read response
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| "TTS sidecar stdout not available".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read TTS sidecar response: {}", e))?;

        // Check for empty response (sidecar likely crashed)
        if line.trim().is_empty() {
            return Err("TTS sidecar returned empty response (may have crashed)".to_string());
        }

        serde_json::from_str(&line).map_err(|e| format!("Invalid TTS sidecar response: {}", e))
    }

    /// Check if the sidecar process is still alive
    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false, // Process has exited
            Ok(None) => true,     // Process is still running
            Err(_) => false,      // Error checking - assume dead
        }
    }

    fn load_model(&mut self, model_path: &str) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::Load {
            model_path: model_path.to_string(),
        })?;

        match response {
            SidecarResponse::Ok { .. } => {
                self.model_path = Some(model_path.to_string());
                Ok(())
            }
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from TTS sidecar".to_string()),
        }
    }

    fn unload_model(&mut self) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::Unload)?;

        match response {
            SidecarResponse::Ok { .. } => {
                self.model_path = None;
                Ok(())
            }
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from TTS sidecar".to_string()),
        }
    }

    fn speak(
        &mut self,
        text: &str,
        output_device: Option<&str>,
        volume: f32,
    ) -> Result<(), String> {
        let response = self.send_request(&SidecarRequest::Speak {
            text: text.to_string(),
            output_device: output_device.map(|s| s.to_string()),
            volume,
        })?;

        match response {
            SidecarResponse::Ok { .. } => Ok(()),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from TTS sidecar".to_string()),
        }
    }

    /// Synthesize audio and return raw samples as base64
    fn synthesize(&mut self, text: &str) -> Result<(String, u32), String> {
        let response = self.send_request(&SidecarRequest::Synthesize {
            text: text.to_string(),
        })?;

        match response {
            SidecarResponse::Audio {
                audio_base64,
                sample_rate,
            } => Ok((audio_base64, sample_rate)),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from TTS sidecar".to_string()),
        }
    }

    fn list_devices(&mut self) -> Result<Vec<String>, String> {
        let response = self.send_request(&SidecarRequest::ListDevices)?;

        match response {
            SidecarResponse::Devices { devices } => Ok(devices),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from TTS sidecar".to_string()),
        }
    }

    fn shutdown(&mut self) {
        if let Err(e) = self.send_request(&SidecarRequest::Shutdown) {
            warn!("Error sending shutdown to TTS sidecar: {}", e);
        }
        if let Err(e) = self.child.wait() {
            warn!("Error waiting for TTS sidecar to exit: {}", e);
        }
    }

    fn is_loaded(&self) -> bool {
        self.model_path.is_some()
    }
}

impl Drop for TtsSidecarProcess {
    fn drop(&mut self) {
        info!("Dropping TTS sidecar process");
        self.shutdown();
    }
}

/// Thread-safe manager for the TTS sidecar
pub struct LocalTtsManager {
    sidecar: Mutex<Option<TtsSidecarProcess>>,
    sidecar_path: PathBuf,
    output_device: Mutex<Option<String>>,
    /// Track the loaded model path so we can reload after crash
    loaded_model_path: Mutex<Option<PathBuf>>,
}

impl LocalTtsManager {
    pub fn new(sidecar_path: PathBuf) -> Self {
        Self {
            sidecar: Mutex::new(None),
            sidecar_path,
            output_device: Mutex::new(None),
            loaded_model_path: Mutex::new(None),
        }
    }

    /// Get the sidecar, spawning it if necessary or if it crashed
    fn ensure_sidecar(&self) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();

        // Check if existing sidecar is still alive
        let needs_respawn = match guard.as_mut() {
            Some(sidecar) => !sidecar.is_alive(),
            None => true,
        };

        if needs_respawn {
            // Clear the dead sidecar if any
            let was_running = guard.is_some();
            if was_running {
                warn!("TTS sidecar crashed, respawning...");
                *guard = None;
            }

            info!("Starting TTS sidecar process...");
            let mut sidecar = TtsSidecarProcess::spawn(&self.sidecar_path)?;

            // If we had a model loaded before the crash, reload it
            if was_running {
                let model_path_guard = self.loaded_model_path.lock().unwrap();
                if let Some(ref model_path) = *model_path_guard {
                    info!("Reloading TTS model after crash: {:?}", model_path);
                    if let Err(e) = sidecar.load_model(&model_path.to_string_lossy()) {
                        error!("Failed to reload TTS model after crash: {}", e);
                        // Don't fail - the sidecar is running, just without a model
                    }
                }
            }

            *guard = Some(sidecar);
        }
        Ok(())
    }

    pub fn load_model(&self, model_path: &Path) -> Result<(), String> {
        info!(
            "LocalTtsManager::load_model() called with path: {:?}",
            model_path
        );

        // Verify the file exists first (check for both .onnx and .onnx.json)
        let onnx_path = model_path;
        let json_path = model_path.with_extension("onnx.json");

        if !onnx_path.exists() {
            let err = format!("Model file does not exist: {:?}", onnx_path);
            error!("{}", err);
            return Err(err);
        }

        // Check for config file
        if !json_path.exists() {
            // Try the alternate naming convention
            let alt_json_path = PathBuf::from(format!("{}.json", onnx_path.display()));
            if !alt_json_path.exists() {
                let err = format!(
                    "Model config file does not exist: {:?} or {:?}",
                    json_path, alt_json_path
                );
                error!("{}", err);
                return Err(err);
            }
        }

        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "TTS sidecar not available".to_string())?;

        let result = sidecar.load_model(&model_path.to_string_lossy());

        // Track the loaded model path for crash recovery
        if result.is_ok() {
            let mut model_path_guard = self.loaded_model_path.lock().unwrap();
            *model_path_guard = Some(model_path.to_path_buf());
        }

        result
    }

    pub fn unload_model(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            if let Err(e) = sidecar.unload_model() {
                error!("Failed to unload TTS model: {}", e);
            }
        }
        // Clear the tracked model path
        let mut model_path_guard = self.loaded_model_path.lock().unwrap();
        *model_path_guard = None;
    }

    pub fn is_loaded(&self) -> bool {
        let guard = self.sidecar.lock().unwrap();
        guard.as_ref().map(|s| s.is_loaded()).unwrap_or(false)
    }

    /// Set the output device for TTS playback
    pub fn set_output_device(&self, device: Option<String>) {
        let mut guard = self.output_device.lock().unwrap();
        *guard = device;
    }

    /// Get the current output device
    pub fn get_output_device(&self) -> Option<String> {
        let guard = self.output_device.lock().unwrap();
        guard.clone()
    }

    /// Speak the given text
    pub fn speak(&self, text: &str, volume: f32) -> Result<(), String> {
        self.ensure_sidecar()?;

        let output_device = self.output_device.lock().unwrap().clone();

        let result = {
            let mut guard = self.sidecar.lock().unwrap();
            let sidecar = guard
                .as_mut()
                .ok_or_else(|| "TTS sidecar not available".to_string())?;
            sidecar.speak(text, output_device.as_deref(), volume)
        };

        // If we got a broken pipe or empty response, the sidecar crashed - try to recover once
        if let Err(ref e) = result {
            if e.contains("Broken pipe") || e.contains("empty response") || e.contains("crashed") {
                warn!("TTS sidecar appears to have crashed during speak, attempting recovery...");

                // Force respawn by clearing the sidecar
                {
                    let mut guard = self.sidecar.lock().unwrap();
                    *guard = None;
                }

                // Try to respawn and retry
                self.ensure_sidecar()?;

                let output_device = self.output_device.lock().unwrap().clone();
                let mut guard = self.sidecar.lock().unwrap();
                let sidecar = guard
                    .as_mut()
                    .ok_or_else(|| "TTS sidecar not available after recovery".to_string())?;

                return sidecar.speak(text, output_device.as_deref(), volume);
            }
        }

        result
    }

    /// Synthesize audio and return as base64-encoded samples
    /// Returns (audio_base64, sample_rate)
    pub fn synthesize(&self, text: &str) -> Result<(String, u32), String> {
        self.ensure_sidecar()?;

        let result = {
            let mut guard = self.sidecar.lock().unwrap();
            let sidecar = guard
                .as_mut()
                .ok_or_else(|| "TTS sidecar not available".to_string())?;
            sidecar.synthesize(text)
        };

        // If we got a broken pipe or empty response, the sidecar crashed - try to recover once
        if let Err(ref e) = result {
            if e.contains("Broken pipe") || e.contains("empty response") || e.contains("crashed") {
                warn!(
                    "TTS sidecar appears to have crashed during synthesize, attempting recovery..."
                );

                // Force respawn by clearing the sidecar
                {
                    let mut guard = self.sidecar.lock().unwrap();
                    *guard = None;
                }

                // Try to respawn and retry
                self.ensure_sidecar()?;

                let mut guard = self.sidecar.lock().unwrap();
                let sidecar = guard
                    .as_mut()
                    .ok_or_else(|| "TTS sidecar not available after recovery".to_string())?;

                return sidecar.synthesize(text);
            }
        }

        result
    }

    /// List available output devices
    pub fn list_devices(&self) -> Result<Vec<String>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "TTS sidecar not available".to_string())?;

        sidecar.list_devices()
    }

    /// Get sample rate (for compatibility)
    pub fn sample_rate(&self) -> u32 {
        22050 // Piper default sample rate
    }

    /// Shutdown the sidecar process
    pub fn shutdown(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(mut sidecar) = guard.take() {
            sidecar.shutdown();
        }
    }
}

impl Default for LocalTtsManager {
    fn default() -> Self {
        // Default path - will be set properly from tauri config
        Self::new(PathBuf::from("tts-sidecar"))
    }
}

impl Drop for LocalTtsManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
