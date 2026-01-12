//! Memory Manager - Communicates with memory sidecar for semantic memory storage
//!
//! Provides long-term conversation memory via vector search.
//! Communication is via JSON over stdin/stdout pipes.

use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Request types sent to the sidecar
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SidecarRequest {
    #[serde(rename = "store")]
    Store {
        user_id: String,
        content: String,
        is_bot: bool,
    },
    #[serde(rename = "query")]
    Query {
        user_id: String,
        text: String,
        limit: usize,
    },
    #[serde(rename = "query_all")]
    QueryAll { text: String, limit: usize },
    #[serde(rename = "browse_recent")]
    BrowseRecent {
        limit: usize,
        user_id: Option<String>,
        is_bot: Option<bool>,
    },
    #[serde(rename = "list_users")]
    ListUsers,
    #[serde(rename = "count")]
    Count,
    #[serde(rename = "clear_all")]
    ClearAll,
    #[serde(rename = "cleanup")]
    Cleanup { ttl_days: u32 },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "list_models")]
    ListModels,
    #[serde(rename = "load_model")]
    LoadModel { model_id: String },
    #[serde(rename = "get_current_model")]
    GetCurrentModel,
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Embedding model info returned from sidecar
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct EmbeddingModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub dimension: usize,
    pub size_mb: u32,
    pub is_downloaded: bool,
    pub is_loaded: bool,
}

/// User info with memory count
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct MemoryUserInfo {
    pub user_id: String,
    pub memory_count: usize,
}

/// Response types from the sidecar
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SidecarResponse {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "stored")]
    Stored { id: String },
    #[serde(rename = "results")]
    Results { messages: Vec<MemoryMessage> },
    #[serde(rename = "users")]
    Users { users: Vec<MemoryUserInfo> },
    #[serde(rename = "count")]
    Count { total: usize },
    #[serde(rename = "cleared")]
    Cleared { deleted: u32 },
    #[serde(rename = "cleanup_done")]
    CleanupDone { deleted: u32 },
    #[serde(rename = "status")]
    Status {
        ready: bool,
        model_loaded: bool,
        current_model_id: Option<String>,
    },
    #[serde(rename = "models")]
    Models { models: Vec<EmbeddingModelInfo> },
    #[serde(rename = "current_model")]
    CurrentModel { model: Option<EmbeddingModelInfo> },
    #[serde(rename = "model_loaded")]
    ModelLoaded { model_id: String },
    #[serde(rename = "error")]
    Error { message: String },
}

/// A memory message from the sidecar
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct MemoryMessage {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub is_bot: bool,
    pub timestamp: i64,
    pub similarity: Option<f32>,
}

/// Manages communication with the memory sidecar process
struct SidecarProcess {
    child: Child,
}

impl SidecarProcess {
    fn spawn(sidecar_path: &Path) -> Result<Self, String> {
        info!("Spawning memory sidecar from: {:?}", sidecar_path);

        let mut child = Command::new(sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;

        // Wait for the ready message
        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| "Failed to get sidecar stdout".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read sidecar ready message: {}", e))?;

        let response: SidecarResponse =
            serde_json::from_str(&line).map_err(|e| format!("Invalid sidecar response: {}", e))?;

        match response {
            SidecarResponse::Ok { message } => {
                info!("Memory sidecar ready: {}", message);
            }
            SidecarResponse::Error { message } => {
                return Err(format!("Memory sidecar failed to start: {}", message));
            }
            _ => {
                return Err("Unexpected response from memory sidecar".to_string());
            }
        }

        Ok(Self { child })
    }

    fn send_request(&mut self, request: &SidecarRequest) -> Result<SidecarResponse, String> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| "Sidecar stdin not available".to_string())?;

        let json = serde_json::to_string(request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        writeln!(stdin, "{}", json).map_err(|e| format!("Failed to write to sidecar: {}", e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush sidecar stdin: {}", e))?;

        // Read response
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| "Sidecar stdout not available".to_string())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read sidecar response: {}", e))?;

        // Check for empty response (sidecar likely crashed)
        if line.trim().is_empty() {
            return Err("Sidecar returned empty response (may have crashed)".to_string());
        }

        serde_json::from_str(&line).map_err(|e| format!("Invalid sidecar response: {}", e))
    }

    /// Check if the sidecar process is still alive
    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false, // Process has exited
            Ok(None) => true,     // Process is still running
            Err(_) => false,      // Error checking - assume dead
        }
    }

    fn store(&mut self, user_id: &str, content: &str, is_bot: bool) -> Result<String, String> {
        let response = self.send_request(&SidecarRequest::Store {
            user_id: user_id.to_string(),
            content: content.to_string(),
            is_bot,
        })?;

        match response {
            SidecarResponse::Stored { id } => Ok(id),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn query(
        &mut self,
        user_id: &str,
        text: &str,
        limit: usize,
    ) -> Result<Vec<MemoryMessage>, String> {
        let response = self.send_request(&SidecarRequest::Query {
            user_id: user_id.to_string(),
            text: text.to_string(),
            limit,
        })?;

        match response {
            SidecarResponse::Results { messages } => Ok(messages),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn cleanup(&mut self, ttl_days: u32) -> Result<u32, String> {
        let response = self.send_request(&SidecarRequest::Cleanup { ttl_days })?;

        match response {
            SidecarResponse::CleanupDone { deleted } => Ok(deleted),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn query_all(&mut self, text: &str, limit: usize) -> Result<Vec<MemoryMessage>, String> {
        let response = self.send_request(&SidecarRequest::QueryAll {
            text: text.to_string(),
            limit,
        })?;

        match response {
            SidecarResponse::Results { messages } => Ok(messages),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn browse_recent(
        &mut self,
        limit: usize,
        user_id: Option<String>,
        is_bot: Option<bool>,
    ) -> Result<Vec<MemoryMessage>, String> {
        let response = self.send_request(&SidecarRequest::BrowseRecent {
            limit,
            user_id,
            is_bot,
        })?;

        match response {
            SidecarResponse::Results { messages } => Ok(messages),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn list_users(&mut self) -> Result<Vec<MemoryUserInfo>, String> {
        let response = self.send_request(&SidecarRequest::ListUsers)?;

        match response {
            SidecarResponse::Users { users } => Ok(users),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn get_count(&mut self) -> Result<usize, String> {
        let response = self.send_request(&SidecarRequest::Count)?;

        match response {
            SidecarResponse::Count { total } => Ok(total),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn clear_all(&mut self) -> Result<u32, String> {
        let response = self.send_request(&SidecarRequest::ClearAll)?;

        match response {
            SidecarResponse::Cleared { deleted } => Ok(deleted),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn list_models(&mut self) -> Result<Vec<EmbeddingModelInfo>, String> {
        let response = self.send_request(&SidecarRequest::ListModels)?;

        match response {
            SidecarResponse::Models { models } => Ok(models),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn load_model(&mut self, model_id: &str) -> Result<String, String> {
        let response = self.send_request(&SidecarRequest::LoadModel {
            model_id: model_id.to_string(),
        })?;

        match response {
            SidecarResponse::ModelLoaded { model_id } => Ok(model_id),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn get_current_model(&mut self) -> Result<Option<EmbeddingModelInfo>, String> {
        let response = self.send_request(&SidecarRequest::GetCurrentModel)?;

        match response {
            SidecarResponse::CurrentModel { model } => Ok(model),
            SidecarResponse::Error { message } => Err(message),
            _ => Err("Unexpected response from sidecar".to_string()),
        }
    }

    fn shutdown(&mut self) {
        if let Err(e) = self.send_request(&SidecarRequest::Shutdown) {
            warn!("Error sending shutdown to memory sidecar: {}", e);
        }
        if let Err(e) = self.child.wait() {
            warn!("Error waiting for memory sidecar to exit: {}", e);
        }
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        info!("Dropping memory sidecar process");
        self.shutdown();
    }
}

/// Thread-safe manager for the memory sidecar
pub struct MemoryManager {
    sidecar: Mutex<Option<SidecarProcess>>,
    sidecar_path: PathBuf,
}

impl MemoryManager {
    pub fn new(sidecar_path: PathBuf) -> Self {
        Self {
            sidecar: Mutex::new(None),
            sidecar_path,
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
                warn!("Memory sidecar crashed, respawning...");
                *guard = None;
            }

            info!("Starting memory sidecar process...");
            let sidecar = SidecarProcess::spawn(&self.sidecar_path)?;

            // Give it a moment to stabilize
            if was_running {
                thread::sleep(Duration::from_millis(500));
            }

            *guard = Some(sidecar);
        }
        Ok(())
    }

    /// Store a message in memory
    pub fn store_message(
        &self,
        user_id: &str,
        content: &str,
        is_bot: bool,
    ) -> Result<String, String> {
        self.ensure_sidecar()?;

        let result = {
            let mut guard = self.sidecar.lock().unwrap();
            let sidecar = guard
                .as_mut()
                .ok_or_else(|| "Sidecar not available".to_string())?;
            sidecar.store(user_id, content, is_bot)
        };

        // Handle crash recovery
        if let Err(ref e) = result {
            if e.contains("Broken pipe")
                || e.contains("empty response")
                || e.contains("crashed")
                || e.contains("Invalid sidecar response")
            {
                warn!("Memory sidecar appears to have crashed, attempting recovery...");

                {
                    let mut guard = self.sidecar.lock().unwrap();
                    *guard = None;
                }

                self.ensure_sidecar()?;
                thread::sleep(Duration::from_millis(100));

                let mut guard = self.sidecar.lock().unwrap();
                let sidecar = guard
                    .as_mut()
                    .ok_or_else(|| "Sidecar not available after recovery".to_string())?;

                return sidecar.store(user_id, content, is_bot);
            }
        }

        result
    }

    /// Query for relevant context
    pub fn query_context(
        &self,
        user_id: &str,
        text: &str,
        limit: usize,
    ) -> Result<Vec<MemoryMessage>, String> {
        self.ensure_sidecar()?;

        let result = {
            let mut guard = self.sidecar.lock().unwrap();
            let sidecar = guard
                .as_mut()
                .ok_or_else(|| "Sidecar not available".to_string())?;
            sidecar.query(user_id, text, limit)
        };

        // Handle crash recovery
        if let Err(ref e) = result {
            if e.contains("Broken pipe")
                || e.contains("empty response")
                || e.contains("crashed")
                || e.contains("Invalid sidecar response")
            {
                warn!(
                    "Memory sidecar appears to have crashed during query, attempting recovery..."
                );

                {
                    let mut guard = self.sidecar.lock().unwrap();
                    *guard = None;
                }

                self.ensure_sidecar()?;
                thread::sleep(Duration::from_millis(100));

                let mut guard = self.sidecar.lock().unwrap();
                let sidecar = guard
                    .as_mut()
                    .ok_or_else(|| "Sidecar not available after recovery".to_string())?;

                return sidecar.query(user_id, text, limit);
            }
        }

        result
    }

    /// Clean up old messages
    pub fn cleanup_expired(&self, ttl_days: u32) -> Result<u32, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.cleanup(ttl_days)
    }

    /// Check if the sidecar is currently running
    pub fn is_running(&self) -> bool {
        let mut guard = self.sidecar.lock().unwrap();
        match guard.as_mut() {
            Some(sidecar) => sidecar.is_alive(),
            None => false,
        }
    }

    /// Get total count of all memories
    pub fn get_total_count(&self) -> Result<usize, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.get_count()
    }

    /// Clear all memories
    pub fn clear_all(&self) -> Result<u32, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.clear_all()
    }

    /// Query all memories (not filtered by user)
    pub fn query_all(&self, text: &str, limit: usize) -> Result<Vec<MemoryMessage>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.query_all(text, limit)
    }

    /// Browse recent memories without semantic search (for UI browsing)
    pub fn browse_recent(
        &self,
        limit: usize,
        user_id: Option<String>,
        is_bot: Option<bool>,
    ) -> Result<Vec<MemoryMessage>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.browse_recent(limit, user_id, is_bot)
    }

    /// List unique users with memory counts
    pub fn list_users(&self) -> Result<Vec<MemoryUserInfo>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.list_users()
    }

    /// List available embedding models
    pub fn list_models(&self) -> Result<Vec<EmbeddingModelInfo>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.list_models()
    }

    /// Load an embedding model by ID
    pub fn load_model(&self, model_id: &str) -> Result<String, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.load_model(model_id)
    }

    /// Get the currently loaded embedding model
    pub fn get_current_model(&self) -> Result<Option<EmbeddingModelInfo>, String> {
        self.ensure_sidecar()?;

        let mut guard = self.sidecar.lock().unwrap();
        let sidecar = guard
            .as_mut()
            .ok_or_else(|| "Sidecar not available".to_string())?;
        sidecar.get_current_model()
    }

    /// Shutdown the sidecar process
    pub fn shutdown(&self) {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(mut sidecar) = guard.take() {
            sidecar.shutdown();
        }
    }
}

impl Drop for MemoryManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Minimum word count for content to be worth storing in memory
const MIN_WORDS_FOR_STORAGE: usize = 4;

/// Minimum character count for content to be worth storing
const MIN_CHARS_FOR_STORAGE: usize = 15;

/// Common filler/acknowledgment phrases that shouldn't be stored
const SKIP_PHRASES: &[&str] = &[
    "yeah",
    "yes",
    "no",
    "okay",
    "ok",
    "uh",
    "um",
    "hmm",
    "hm",
    "mhm",
    "uh-huh",
    "sure",
    "right",
    "alright",
    "yep",
    "nope",
    "hey",
    "hi",
    "hello",
    "bye",
    "goodbye",
    "thanks",
    "thank you",
    "cool",
    "nice",
    "great",
    "good",
    "fine",
    "amen",
    "what",
    "huh",
    "oh",
    "ah",
];

/// Check if content is meaningful enough to store in long-term memory.
/// Returns true if the content should be stored, false if it should be skipped.
pub fn is_content_worth_storing(content: &str) -> bool {
    let trimmed = content.trim();

    // Skip empty content
    if trimmed.is_empty() {
        return false;
    }

    // Skip very short content
    if trimmed.len() < MIN_CHARS_FOR_STORAGE {
        return false;
    }

    // Count words (split on whitespace)
    let word_count = trimmed.split_whitespace().count();
    if word_count < MIN_WORDS_FOR_STORAGE {
        return false;
    }

    // Check if it's just a common filler phrase
    let lower = trimmed.to_lowercase();
    for phrase in SKIP_PHRASES {
        // Exact match or phrase with punctuation
        if lower == *phrase
            || lower == format!("{}.", phrase)
            || lower == format!("{}!", phrase)
            || lower == format!("{}?", phrase)
        {
            return false;
        }
    }

    // Check if content is mostly filler words (> 75% filler)
    let filler_count = trimmed
        .split_whitespace()
        .filter(|word| {
            let w = word.to_lowercase();
            let clean = w.trim_matches(|c: char| !c.is_alphabetic());
            SKIP_PHRASES.contains(&clean)
        })
        .count();

    if word_count > 0 && (filler_count as f32 / word_count as f32) > 0.75 {
        return false;
    }

    true
}

/// Format memory context for inclusion in a prompt
pub fn format_memory_context(memories: &[MemoryMessage]) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    for memory in memories {
        let speaker = if memory.is_bot { "You" } else { "User" };
        let timestamp = chrono::DateTime::from_timestamp(memory.timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        result.push_str(&format!(
            "[{}] {}: {}\n",
            timestamp, speaker, memory.content
        ));
    }
    result
}
