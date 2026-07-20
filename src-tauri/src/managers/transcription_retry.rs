//! Transcription Retry Queue Manager
//!
//! This module provides robust retry handling for failed transcriptions.
//! Key features:
//! - Classifies failure types for intelligent retry strategies
//! - Persists retry queue to disk (survives app restarts)
//! - Exponential backoff to avoid overwhelming the system
//! - Automatic retry with fallback models
//! - Silent audio detection (no retry needed)

use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// Classification of transcription failure types.
/// Different failures require different retry strategies.
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
pub enum TranscriptionFailure {
    /// Model failed to load (wrong ID, corrupted, missing files)
    ModelLoadFailure { model_id: String, error: String },
    /// Transcription inference failed (engine error)
    InferenceFailure { model_id: String, error: String },
    /// Engine panicked during transcription
    EnginePanic { model_id: String },
    /// Operation timed out (took too long)
    Timeout {
        model_id: String,
        duration_secs: u64,
    },
    /// GPU/memory resource unavailable
    ResourceUnavailable { resource: String, error: String },
    /// Audio was silent/empty (no retry needed)
    SilentAudio,
    /// Unknown/uncategorized failure
    Unknown { error: String },
}

impl TranscriptionFailure {
    /// Check if this failure type should trigger automatic retry.
    /// Returns false for failures that won't benefit from retrying.
    pub fn should_auto_retry(&self) -> bool {
        match self {
            // These failures might succeed on retry
            Self::ModelLoadFailure { .. } => true, // Can succeed with fallback model
            Self::InferenceFailure { .. } => true,
            Self::Timeout { .. } => true,
            Self::ResourceUnavailable { .. } => true,
            Self::Unknown { .. } => true,

            // These won't succeed on retry
            Self::EnginePanic { .. } => false, // Panic indicates serious issue
            Self::SilentAudio => false,
        }
    }

    /// Check if this failure could benefit from trying a different model.
    pub fn should_try_fallback_model(&self) -> bool {
        match self {
            // These might work with a different model
            Self::InferenceFailure { .. } => true,
            Self::Timeout { .. } => true,
            Self::ModelLoadFailure { .. } => true,
            Self::ResourceUnavailable { .. } => false, // Resource issue, not model issue
            Self::Unknown { .. } => true,
            Self::EnginePanic { .. } => false, // Panic indicates instability
            Self::SilentAudio => false,
        }
    }

    /// Get a human-readable description of the failure.
    pub fn description(&self) -> String {
        match self {
            Self::ModelLoadFailure { model_id, error } => {
                format!("Failed to load model '{}': {}", model_id, error)
            }
            Self::InferenceFailure { model_id, error } => {
                format!("Transcription failed with model '{}': {}", model_id, error)
            }
            Self::EnginePanic { model_id } => {
                format!("Model '{}' crashed during transcription", model_id)
            }
            Self::Timeout {
                model_id,
                duration_secs,
            } => {
                format!("Model '{}' timed out after {}s", model_id, duration_secs)
            }
            Self::ResourceUnavailable { resource, error } => {
                format!("{} unavailable: {}", resource, error)
            }
            Self::SilentAudio => "Audio was silent or empty".to_string(),
            Self::Unknown { error } => {
                format!("Unknown error: {}", error)
            }
        }
    }
}

/// Represents a pending transcription retry.
/// Contains all information needed to re-attempt transcription.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RetryableTranscription {
    /// Unique identifier for this retry entry
    pub id: String,
    /// Path to the saved audio file
    pub audio_path: PathBuf,
    /// Unix timestamp when the audio was recorded
    pub timestamp: i64,
    /// Model ID to use for transcription
    pub model_id: String,
    /// Fallback models to try if primary fails
    pub fallback_models: Vec<String>,
    /// Index into fallback_models (0 = primary, 1+ = fallbacks)
    pub current_model_index: usize,
    /// Number of retry attempts made
    pub retry_count: u32,
    /// Maximum retry attempts allowed
    pub max_retries: u32,
    /// Last failure type
    pub last_failure: Option<TranscriptionFailure>,
    /// Error message from last attempt
    pub last_error: String,
    /// When to retry next (None = ready immediately)
    pub next_retry_at: Option<i64>,
    /// Whether post-processing was requested
    pub post_process: bool,
    /// Post-processing prompt if any
    pub post_process_prompt: Option<String>,
    /// History entry ID (if created before failure)
    pub history_entry_id: Option<i64>,
    /// Whether this entry is currently being processed
    pub is_processing: bool,
}

impl RetryableTranscription {
    /// Create a new retryable transcription from a failed attempt.
    pub fn new(
        audio_path: PathBuf,
        model_id: String,
        fallback_models: Vec<String>,
        failure: TranscriptionFailure,
        post_process: bool,
        post_process_prompt: Option<String>,
        history_entry_id: Option<i64>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now().timestamp(),
            audio_path,
            model_id,
            fallback_models,
            current_model_index: 0,
            retry_count: 0,
            max_retries: 3,
            last_failure: Some(failure.clone()),
            last_error: failure.description(),
            next_retry_at: None,
            post_process,
            post_process_prompt,
            history_entry_id,
            is_processing: false,
        }
    }

    /// Check if this entry is ready for retry.
    pub fn is_ready(&self) -> bool {
        if self.is_processing {
            return false;
        }

        if self.retry_count >= self.max_retries {
            return false;
        }

        if let Some(next_retry) = self.next_retry_at {
            let now = Utc::now().timestamp();
            return now >= next_retry;
        }

        true
    }

    /// Get the next model to try.
    /// Returns None if all models have been tried.
    #[allow(dead_code)]
    pub fn get_next_model(&self) -> Option<String> {
        let models = std::iter::once(self.model_id.clone())
            .chain(self.fallback_models.clone())
            .collect::<Vec<_>>();

        models.into_iter().nth(self.current_model_index)
    }

    /// Advance to the next fallback model.
    pub fn advance_to_fallback(&mut self) -> bool {
        if self.current_model_index < self.fallback_models.len() {
            self.current_model_index += 1;
            true
        } else {
            false
        }
    }

    /// Schedule the next retry with exponential backoff.
    pub fn schedule_retry(&mut self, base_delay_secs: u64, multiplier: f64) {
        self.retry_count += 1;

        // Exponential backoff: delay * (multiplier ^ retry_count)
        let delay_secs = (base_delay_secs as f64) * multiplier.powi(self.retry_count as i32 - 1);

        // Cap at 5 minutes
        let delay_secs = delay_secs.min(300.0) as u64;

        self.next_retry_at = Some(Utc::now().timestamp() + delay_secs as i64);
    }

    /// Mark this entry as successfully completed.
    #[allow(dead_code)]
    pub fn mark_completed(&mut self) {
        self.last_failure = None;
        self.next_retry_at = None;
        self.is_processing = false;
    }
}

/// Manages the queue of pending transcription retries.
#[derive(Clone)]
pub struct TranscriptionRetryQueue {
    pending: Arc<Mutex<VecDeque<RetryableTranscription>>>,
    queue_file_path: PathBuf,
    _app_handle: AppHandle,
}

impl TranscriptionRetryQueue {
    /// Create a new retry queue manager.
    pub fn new(app_handle: AppHandle) -> Result<Self> {
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?;

        let queue_file_path = app_data_dir.join("transcription_retry_queue.json");

        let queue = Self {
            pending: Arc::new(Mutex::new(VecDeque::new())),
            queue_file_path,
            _app_handle: app_handle,
        };

        // Load existing queue from disk
        queue.load_from_disk()?;

        Ok(queue)
    }

    /// Add a failed transcription to the retry queue.
    /// Returns the retry entry ID.
    pub fn add_failed_transcription(
        &self,
        audio_path: PathBuf,
        model_id: String,
        fallback_models: Vec<String>,
        failure: TranscriptionFailure,
        post_process: bool,
        post_process_prompt: Option<String>,
        history_entry_id: Option<i64>,
    ) -> Result<String> {
        let entry = RetryableTranscription::new(
            audio_path,
            model_id,
            fallback_models,
            failure,
            post_process,
            post_process_prompt,
            history_entry_id,
        );

        let entry_id = entry.id.clone();

        // Check if we should auto-retry
        let should_retry = entry
            .last_failure
            .as_ref()
            .map(|f| f.should_auto_retry())
            .unwrap_or(false);

        if !should_retry {
            debug!(
                "Failure type {:?} should not auto-retry, but keeping in queue for manual retry",
                entry.last_failure
            );
        }

        {
            let mut pending = self.pending.lock();
            pending.push_back(entry);
        }

        self.save_to_disk()?;

        info!("Added transcription to retry queue: {}", entry_id);

        Ok(entry_id)
    }

    /// Get the next entry ready for retry.
    /// Returns None if no entries are ready.
    #[allow(dead_code)]
    pub fn get_next_retry(&self) -> Option<RetryableTranscription> {
        let mut pending = self.pending.lock();

        for entry in pending.iter_mut() {
            if entry.is_ready() {
                entry.is_processing = true;
                return Some(entry.clone());
            }
        }

        None
    }

    /// Mark a retry as successfully completed.
    /// Removes the entry from the queue.
    pub fn mark_retry_complete(&self, entry_id: &str) -> Result<()> {
        {
            let mut pending = self.pending.lock();
            pending.retain(|e| e.id != entry_id);
        }

        self.save_to_disk()?;

        info!("Removed completed retry entry: {}", entry_id);

        Ok(())
    }

    /// Mark a retry as failed and schedule the next attempt.
    /// Returns false if max retries exceeded.
    pub fn mark_retry_failed(&self, entry_id: &str, failure: TranscriptionFailure) -> Result<bool> {
        let can_retry = {
            let mut pending = self.pending.lock();

            if let Some(entry) = pending.iter_mut().find(|e| e.id == entry_id) {
                entry.is_processing = false;
                entry.last_failure = Some(failure.clone());
                entry.last_error = failure.description();

                // Check if we should try a fallback model first
                if failure.should_try_fallback_model() && entry.advance_to_fallback() {
                    // We have a fallback model to try
                    debug!("Advancing to fallback model for entry {}", entry_id);
                    entry.next_retry_at = None; // Ready immediately
                    true
                } else if entry.retry_count < entry.max_retries {
                    // Schedule retry with exponential backoff
                    entry.schedule_retry(5, 2.0);
                    debug!(
                        "Scheduled retry {} for entry {} at {:?}",
                        entry.retry_count, entry_id, entry.next_retry_at
                    );
                    true
                } else {
                    // Max retries exceeded
                    warn!(
                        "Max retries ({}) exceeded for entry {}",
                        entry.max_retries, entry_id
                    );
                    false
                }
            } else {
                warn!("Entry {} not found in retry queue", entry_id);
                return Ok(false);
            }
        };

        self.save_to_disk()?;

        Ok(can_retry)
    }

    /// Get all pending retry entries (for UI display).
    pub fn get_all_pending(&self) -> Vec<RetryableTranscription> {
        let pending = self.pending.lock();
        pending.iter().cloned().collect()
    }

    /// Remove a specific entry from the queue.
    pub fn remove_entry(&self, entry_id: &str) -> Result<bool> {
        let removed = {
            let mut pending = self.pending.lock();
            let initial_len = pending.len();
            pending.retain(|e| e.id != entry_id);
            pending.len() < initial_len
        };

        if removed {
            self.save_to_disk()?;
            info!("Manually removed retry entry: {}", entry_id);
        }

        Ok(removed)
    }

    /// Clear all pending retry entries.
    pub fn clear_all(&self) -> Result<()> {
        {
            let mut pending = self.pending.lock();
            pending.clear();
        }

        self.save_to_disk()?;

        info!("Cleared all retry entries");

        Ok(())
    }

    /// Get the count of pending retries.
    pub fn count(&self) -> usize {
        let pending = self.pending.lock();
        pending.len()
    }

    /// Persist the queue to disk.
    fn save_to_disk(&self) -> Result<()> {
        let pending = self.pending.lock();
        let entries: Vec<_> = pending.iter().collect();

        if let Err(e) = std::fs::write(
            &self.queue_file_path,
            serde_json::to_string_pretty(&entries)?,
        ) {
            error!("Failed to save retry queue to disk: {}", e);
            return Err(anyhow::anyhow!("Failed to save retry queue: {}", e));
        }

        debug!("Saved {} retry entries to disk", entries.len());

        Ok(())
    }

    /// Load the queue from disk.
    fn load_from_disk(&self) -> Result<()> {
        if !self.queue_file_path.exists() {
            debug!("No existing retry queue file found");
            return Ok(());
        }

        let contents = match std::fs::read_to_string(&self.queue_file_path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read retry queue file: {}", e);
                return Ok(());
            }
        };

        let entries: Vec<RetryableTranscription> = match serde_json::from_str(&contents) {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to parse retry queue file: {}", e);
                // Try to backup the corrupted file
                let backup_path = format!("{}.corrupted", self.queue_file_path.display());
                let _ = std::fs::rename(&self.queue_file_path, backup_path);
                return Ok(());
            }
        };

        {
            let mut pending = self.pending.lock();
            for entry in entries {
                // Reset processing flag (in case app crashed during processing)
                let mut entry = entry;
                entry.is_processing = false;
                pending.push_back(entry);
            }
        }

        info!("Loaded {} retry entries from disk", self.count());

        Ok(())
    }

    /// Process all pending retries that are ready.
    /// This should be called periodically or after app startup.
    #[allow(dead_code)]
    pub fn process_pending_retries<F>(&self, mut process_fn: F) -> Result<Vec<String>>
    where
        F: FnMut(RetryableTranscription) -> Result<()>,
    {
        let mut processed = Vec::new();

        // Keep processing until no more ready entries
        while let Some(entry) = self.get_next_retry() {
            match process_fn(entry.clone()) {
                Ok(()) => {
                    self.mark_retry_complete(&entry.id)?;
                    processed.push(entry.id);
                }
                Err(e) => {
                    // Determine failure type from error
                    let failure = TranscriptionFailure::Unknown {
                        error: e.to_string(),
                    };

                    if !self.mark_retry_failed(&entry.id, failure)? {
                        // Max retries exceeded, remove from queue
                        warn!(
                            "Entry {} failed after max retries, removing from queue",
                            entry.id
                        );
                        self.remove_entry(&entry.id)?;
                    }
                }
            }
        }

        Ok(processed)
    }
}

// ── Tauri commands for the retry queue ──────────────────────────────────

/// Get all pending retry entries (for UI display).
#[tauri::command]
#[specta::specta]
pub fn get_retry_entries(
    retry_queue: State<'_, Arc<Mutex<TranscriptionRetryQueue>>>,
) -> Vec<RetryableTranscription> {
    retry_queue.lock().get_all_pending()
}

/// Remove a specific entry from the retry queue.
#[tauri::command]
#[specta::specta]
pub fn remove_retry_entry(
    retry_queue: State<'_, Arc<Mutex<TranscriptionRetryQueue>>>,
    entry_id: String,
) -> Result<bool, String> {
    retry_queue
        .lock()
        .remove_entry(&entry_id)
        .map_err(|e| e.to_string())
}

/// Clear all pending retry entries.
#[tauri::command]
#[specta::specta]
pub fn clear_retry_entries(
    retry_queue: State<'_, Arc<Mutex<TranscriptionRetryQueue>>>,
) -> Result<(), String> {
    retry_queue
        .lock()
        .clear_all()
        .map_err(|e| e.to_string())
}

/// Get the count of pending retry entries.
#[tauri::command]
#[specta::specta]
pub fn get_retry_count(
    retry_queue: State<'_, Arc<Mutex<TranscriptionRetryQueue>>>,
) -> usize {
    retry_queue.lock().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_classification() {
        let failure = TranscriptionFailure::InferenceFailure {
            model_id: "turbo".to_string(),
            error: "OOM".to_string(),
        };

        assert!(failure.should_auto_retry());
        assert!(failure.should_try_fallback_model());
        assert!(!failure.description().is_empty());
    }

    #[test]
    fn test_silent_audio_no_retry() {
        let failure = TranscriptionFailure::SilentAudio;

        assert!(!failure.should_auto_retry());
        assert!(!failure.should_try_fallback_model());
    }

    #[test]
    fn test_retryable_transcription() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/test.wav"),
            "turbo".to_string(),
            vec!["small".to_string(), "medium".to_string()],
            TranscriptionFailure::InferenceFailure {
                model_id: "turbo".to_string(),
                error: "Failed".to_string(),
            },
            false,
            None,
            None,
        );

        // Initially ready
        assert!(entry.is_ready());

        // Get next model (primary)
        assert_eq!(entry.get_next_model(), Some("turbo".to_string()));

        // Advance to fallback
        assert!(entry.advance_to_fallback());
        assert_eq!(entry.get_next_model(), Some("small".to_string()));

        // Schedule retry
        entry.schedule_retry(5, 2.0);
        assert!(entry.next_retry_at.is_some());
    }

    #[test]
    fn test_exponential_backoff() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/test.wav"),
            "turbo".to_string(),
            vec![],
            TranscriptionFailure::Timeout {
                model_id: "turbo".to_string(),
                duration_secs: 30,
            },
            false,
            None,
            None,
        );

        entry.max_retries = 5;

        // First retry: 5s
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 1);

        // Second retry: 10s
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 2);

        // Third retry: 20s
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 3);

        // Fourth retry: 40s
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 4);
    }

    // ── Additional tests ───────────────────────────────────────────

    // ── Failure type display messages ──────────────────────────────

    #[test]
    fn display_model_load_failure() {
        let f = TranscriptionFailure::ModelLoadFailure {
            model_id: "turbo".into(),
            error: "weights missing".into(),
        };
        assert_eq!(f.description(), "Failed to load model 'turbo': weights missing");
    }

    #[test]
    fn display_inference_failure() {
        let f = TranscriptionFailure::InferenceFailure {
            model_id: "small".into(),
            error: "OOM".into(),
        };
        assert_eq!(
            f.description(),
            "Transcription failed with model 'small': OOM"
        );
    }

    #[test]
    fn display_engine_panic() {
        let f = TranscriptionFailure::EnginePanic {
            model_id: "turbo".into(),
        };
        assert_eq!(
            f.description(),
            "Model 'turbo' crashed during transcription"
        );
    }

    #[test]
    fn display_timeout() {
        let f = TranscriptionFailure::Timeout {
            model_id: "medium".into(),
            duration_secs: 45,
        };
        assert_eq!(
            f.description(),
            "Model 'medium' timed out after 45s"
        );
    }

    #[test]
    fn display_resource_unavailable() {
        let f = TranscriptionFailure::ResourceUnavailable {
            resource: "GPU".into(),
            error: "device busy".into(),
        };
        assert_eq!(f.description(), "GPU unavailable: device busy");
    }

    #[test]
    fn display_silent_audio() {
        let f = TranscriptionFailure::SilentAudio;
        assert_eq!(f.description(), "Audio was silent or empty");
    }

    #[test]
    fn display_unknown() {
        let f = TranscriptionFailure::Unknown {
            error: "something weird".into(),
        };
        assert_eq!(f.description(), "Unknown error: something weird");
    }

    // ── should_auto_retry for all variants ─────────────────────────

    #[test]
    fn engine_panic_no_auto_retry() {
        let f = TranscriptionFailure::EnginePanic {
            model_id: "turbo".into(),
        };
        assert!(!f.should_auto_retry());
    }

    #[test]
    fn model_load_failure_auto_retry() {
        let f = TranscriptionFailure::ModelLoadFailure {
            model_id: "turbo".into(),
            error: "corrupt".into(),
        };
        assert!(f.should_auto_retry());
    }

    #[test]
    fn timeout_auto_retry() {
        let f = TranscriptionFailure::Timeout {
            model_id: "turbo".into(),
            duration_secs: 30,
        };
        assert!(f.should_auto_retry());
    }

    #[test]
    fn resource_unavailable_auto_retry() {
        let f = TranscriptionFailure::ResourceUnavailable {
            resource: "GPU".into(),
            error: "busy".into(),
        };
        assert!(f.should_auto_retry());
    }

    #[test]
    fn unknown_auto_retry() {
        let f = TranscriptionFailure::Unknown {
            error: "mystery".into(),
        };
        assert!(f.should_auto_retry());
    }

    // ── should_try_fallback_model for all variants ─────────────────

    #[test]
    fn engine_panic_no_fallback() {
        let f = TranscriptionFailure::EnginePanic {
            model_id: "turbo".into(),
        };
        assert!(!f.should_try_fallback_model());
    }

    #[test]
    fn resource_unavailable_no_fallback() {
        let f = TranscriptionFailure::ResourceUnavailable {
            resource: "GPU".into(),
            error: "busy".into(),
        };
        assert!(!f.should_try_fallback_model());
    }

    #[test]
    fn model_load_failure_fallback() {
        let f = TranscriptionFailure::ModelLoadFailure {
            model_id: "turbo".into(),
            error: "missing".into(),
        };
        assert!(f.should_try_fallback_model());
    }

    #[test]
    fn inference_failure_fallback() {
        let f = TranscriptionFailure::InferenceFailure {
            model_id: "turbo".into(),
            error: "OOM".into(),
        };
        assert!(f.should_try_fallback_model());
    }

    // ── RetryableTranscription edge cases ──────────────────────────

    #[test]
    fn retryable_new_defaults() {
        let entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        assert_eq!(entry.retry_count, 0);
        assert_eq!(entry.max_retries, 3);
        assert!(!entry.is_processing);
        assert!(entry.next_retry_at.is_none());
        assert!(entry.is_ready());
    }

    #[test]
    fn not_ready_when_processing() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        entry.is_processing = true;
        assert!(!entry.is_ready());
    }

    #[test]
    fn not_ready_when_max_retries_exceeded() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        entry.retry_count = 3;
        entry.max_retries = 3;
        assert!(!entry.is_ready());
    }

    #[test]
    fn get_next_model_primary() {
        let entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec!["model-b".into()],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        assert_eq!(entry.get_next_model(), Some("model-a".into()));
    }

    #[test]
    fn get_next_model_fallback() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec!["model-b".into(), "model-c".into()],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        entry.current_model_index = 1;
        assert_eq!(entry.get_next_model(), Some("model-b".into()));
    }

    #[test]
    fn get_next_model_none_when_exhausted() {
        let entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        // Primary at index 0 exists
        assert_eq!(entry.get_next_model(), Some("model-a".into()));

        let mut entry2 = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        entry2.current_model_index = 1; // past end
        assert_eq!(entry2.get_next_model(), None);
    }

    #[test]
    fn advance_to_fallback_returns_false_when_exhausted() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec!["model-b".into()],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        // First advance: 0 -> 1
        assert!(entry.advance_to_fallback());
        // Second advance: 1 -> 2, but fallback_models.len() == 1
        assert!(!entry.advance_to_fallback());
    }

    #[test]
    fn mark_completed_clears_failure() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::InferenceFailure {
                model_id: "model-a".into(),
                error: "fail".into(),
            },
            false,
            None,
            None,
        );
        entry.mark_completed();
        assert!(entry.last_failure.is_none());
        assert!(entry.next_retry_at.is_none());
        assert!(!entry.is_processing);
    }

    #[test]
    fn schedule_retry_caps_at_5_minutes() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        // Large base delay with high multiplier — should cap at 300s
        entry.retry_count = 10; // Already many retries
        entry.schedule_retry(60, 10.0);

        // next_retry_at should be at most 300s from now
        let now = Utc::now().timestamp();
        let next = entry.next_retry_at.unwrap();
        let delay = next - now;
        assert!(delay <= 301, "delay {} should be <= 300", delay);
    }

    #[test]
    fn retryable_transcription_serde_roundtrip() {
        let entry = RetryableTranscription::new(
            PathBuf::from("/tmp/test.wav"),
            "turbo".into(),
            vec!["small".into()],
            TranscriptionFailure::InferenceFailure {
                model_id: "turbo".into(),
                error: "OOM".into(),
            },
            true,
            Some("fix grammar".into()),
            Some(42),
        );

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: RetryableTranscription = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.model_id, "turbo");
        assert_eq!(deserialized.fallback_models, vec!["small"]);
        assert_eq!(deserialized.post_process, true);
        assert_eq!(
            deserialized.post_process_prompt,
            Some("fix grammar".into())
        );
        assert_eq!(deserialized.history_entry_id, Some(42));
    }

    #[test]
    fn failure_serde_roundtrip() {
        let failures = vec![
            TranscriptionFailure::SilentAudio,
            TranscriptionFailure::EnginePanic {
                model_id: "turbo".into(),
            },
            TranscriptionFailure::InferenceFailure {
                model_id: "small".into(),
                error: "OOM".into(),
            },
            TranscriptionFailure::Timeout {
                model_id: "medium".into(),
                duration_secs: 30,
            },
            TranscriptionFailure::ResourceUnavailable {
                resource: "GPU".into(),
                error: "busy".into(),
            },
            TranscriptionFailure::Unknown {
                error: "mystery".into(),
            },
            TranscriptionFailure::ModelLoadFailure {
                model_id: "turbo".into(),
                error: "corrupt".into(),
            },
        ];

        for f in &failures {
            let json = serde_json::to_string(f).unwrap();
            let deserialized: TranscriptionFailure = serde_json::from_str(&json).unwrap();
            assert_eq!(*f, deserialized, "Roundtrip failed for {:?}", f);
        }
    }

    #[test]
    fn new_retries_increment_correctly() {
        let mut entry = RetryableTranscription::new(
            PathBuf::from("/tmp/a.wav"),
            "model-a".into(),
            vec![],
            TranscriptionFailure::SilentAudio,
            false,
            None,
            None,
        );
        assert_eq!(entry.retry_count, 0);
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 1);
        entry.schedule_retry(5, 2.0);
        assert_eq!(entry.retry_count, 2);
    }
}
