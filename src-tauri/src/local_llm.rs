//! Local LLM Manager - Communicates with LLM sidecar process
//!
//! The LLM runs in a separate process to avoid GGML symbol conflicts with whisper-rs.
//! Communication is via JSON over stdin/stdout pipes.

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Request types sent to the sidecar
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SidecarRequest {
    #[serde(rename = "load")]
    Load { model_path: String },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "chat")]
    Chat {
        system_prompt: String,
        user_message: String,
        max_tokens: u32,
    },
    #[serde(rename = "generate")]
    Generate { prompt: String, max_tokens: u32 },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Response types from the sidecar
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SidecarResponse {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "result")]
    Result { text: String },
    #[serde(rename = "status")]
    Status {
        loaded: bool,
        model_path: Option<String>,
    },
}

/// Manages communication with the LLM sidecar process
struct SidecarProcess {
    child: Child,
    model_path: Option<String>,
}

impl SidecarProcess {
    fn spawn(sidecar_path: &Path) -> Result<Self, String> {
        info!("Spawning LLM sidecar from: {:?}", sidecar_path);

        let mut child = Command::new(sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Let stderr go to parent for debugging
            .spawn()
            .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;

        // Wait for the ready message
        let stdout = child.stdout.as_mut()
            .ok_or_else(|| "Failed to get sidecar stdout".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line)
            .map_err(|e| format!("Failed to read sidecar ready message: {}", e))?;

        let response: SidecarResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid sidecar response: {}", e))?;

        match response {
            SidecarResponse::Ok { message } => {
                info!("Sidecar ready: {}", message);
            }
            SidecarResponse::Error { message } => {
                return Err(format!("Sidecar failed to start: {}", message));
            }
            _ => {
                return Err("Unexpected response from sidecar".to_string());
            }
        }

        Ok(Self {
            child,
            model_path: None,
        })
    }

    fn send_request(&mut self, request: &SidecarRequest) -> Result<SidecarResponse, String> {
        let stdin = self.child.stdin.as_mut()
            .ok_or_else(|| "Sidecar stdin not available".to_string())?;

        let json = serde_json::to_string(request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        writeln!(stdin, "{}", json)
            .map_err(|e| format!("Failed to write to sidecar: {}", e))?;
        stdin.flush()
            .map_err(|e| format!("Failed to flush sidecar stdin: {}", e))?;

        // Read response
        let stdout = self.child.stdout.as_mut()
            .ok_or_else(|| "Sidecar stdout not available".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line)
            .map_err(|e| format!("Failed to read sidecar response: {}", e))?;

        serde_json::from_str(&line)
            .map_err(|e| format!("Invalid sidecar response: {}", e))
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
            _ => Err("Unexpected response from sidecar".to_string()),
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
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn chat(&mut self, system_prompt: &str, user_message: &str, max_tokens: u32) -> Result<String, String> {
        let response = self.send_request(&SidecarRequest::Chat {
            system_prompt: system_prompt.to_string(),
            user_message: user_message.to_string(),
            max_tokens,
        })?;

        match response {
            SidecarResponse::Result { text } => Ok(text),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn generate(&mut self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        let response = self.send_request(&SidecarRequest::Generate {
            prompt: prompt.to_string(),
            max_tokens,
        })?;

        match response {
            SidecarResponse::Result { text } => Ok(text),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn shutdown(&mut self) {
        if let Err(e) = self.send_request(&SidecarRequest::Shutdown) {
            warn!("Error sending shutdown to sidecar: {}", e);
        }
        if let Err(e) = self.child.wait() {
            warn!("Error waiting for sidecar to exit: {}", e);
        }
    }

    fn is_loaded(&self) -> bool {
        self.model_path.is_some()
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        info!("Dropping sidecar process");
        self.shutdown();
    }
}

/// Thread-safe manager for the LLM sidecar
pub struct LocalLlmManager {
    sidecar: Mutex<Option<SidecarProcess>>,
    sidecar_path: PathBuf,
}

impl LocalLlmManager {
    pub fn new(sidecar_path: PathBuf) -> Self {
        Self {
            sidecar: Mutex::new(None),
            sidecar_path,
        }
    }

    /// Get the sidecar, spawning it if necessary
    fn ensure_sidecar(&self) -> Result<(), String> {
        let mut guard = self.sidecar.lock().unwrap();
        if guard.is_none() {
            info!("Starting LLM sidecar process...");
            let sidecar = SidecarProcess::spawn(&self.sidecar_path)?;
            *guard = Some(sidecar);
        }
        Ok(())
    }

    pub fn load_model(&self, model_path: &Path) -> Result<(), String> {
        info!("LocalLlmManager::load_model() called with path: {:?}", model_path);

        // Verify the file exists first
        if !model_path.exists() {
            let err = format!("Model file does not exist: {:?}", model_path);
            error!("{}", err);
            return Err(err);
        }

        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard.as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.load_model(&model_path.to_string_lossy())
    }

    pub fn unload_model(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            if let Err(e) = sidecar.unload_model() {
                error!("Failed to unload model: {}", e);
            }
        }
    }

    pub fn is_loaded(&self) -> bool {
        let guard = self.sidecar.lock().unwrap();
        guard.as_ref().map(|s| s.is_loaded()).unwrap_or(false)
    }

    pub fn chat(&self, system_prompt: &str, user_message: &str, max_tokens: u32) -> Result<String, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard.as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.chat(system_prompt, user_message, max_tokens)
    }

    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard.as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;

        sidecar.generate(prompt, max_tokens)
    }

    /// Shutdown the sidecar process
    pub fn shutdown(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(mut sidecar) = guard.take() {
            sidecar.shutdown();
        }
    }
}

impl Default for LocalLlmManager {
    fn default() -> Self {
        // Default path - will be set properly from tauri config
        Self::new(PathBuf::from("llm-sidecar"))
    }
}

impl Drop for LocalLlmManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
