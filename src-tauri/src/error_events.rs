//! Recoverable error events for the frontend error dialog system.
//!
//! This module defines the `RecoverableErrorEvent` struct that is emitted via Tauri
//! events when a recoverable error occurs. The frontend listens for these events
//! and shows a dialog with retry/dismiss options.
//!
//! Error types that should trigger recoverable error dialogs:
//! - Model download failures (network issues, verification failures)
//! - Model loading failures (OOM, corrupted model files)
//! - Transcription failures (engine crashes, model not loaded)
//! - Audio device errors (microphone permission denied, no input device)

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

/// Categories of recoverable errors.
/// Each variant maps to a user-friendly i18n key prefix in the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecoverableErrorType {
    /// Model download failed (network error, verification failure, extraction failure)
    ModelDownload,
    /// Model failed to load into memory (OOM, corrupted file, incompatible engine)
    ModelLoad,
    /// Transcription failed (engine panic, model not loaded, inference error)
    Transcription,
    /// Audio device error (permission denied, no device found, device disconnected)
    AudioDevice,
}

/// Whether a recoverable error can be retried automatically or needs user action.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// The operation can be retried directly (e.g., re-download, re-load model)
    Retry,
    /// The operation requires user action before retrying (e.g., grant permissions)
    UserAction,
    /// The error is permanent and cannot be recovered by retrying
    Permanent,
}

/// A recoverable error event payload sent from the backend to the frontend.
///
/// The frontend should display an error dialog with:
/// - `message`: User-friendly error description
/// - `recovery_action`: What kind of recovery is possible
/// - `error_type`: Category of the error (for i18n and icon selection)
/// - `context`: Additional context JSON (e.g., model_id, device name)
/// - `retry_command`: The Tauri command name to call on retry (if applicable)
/// - `retry_args`: Arguments for the retry command (JSON string)
/// - `technical_detail`: Technical error info for "Show Details" toggle
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct RecoverableErrorEvent {
    /// Unique identifier for this error occurrence (for dedup/tracking retry count)
    pub error_id: String,
    /// Category of the error
    pub error_type: RecoverableErrorType,
    /// Whether retry is possible and what kind
    pub recovery_action: RecoveryAction,
    /// User-friendly error message (already localized key path or plain text)
    pub message: String,
    /// Optional i18n key for the message (e.g., "errors.modelDownloadFailed")
    pub message_key: Option<String>,
    /// Optional i18n interpolation parameters as JSON (e.g., {"model": "Whisper Small"})
    pub message_params: Option<String>,
    /// Additional context as JSON (model_id, device name, etc.)
    pub context: Option<String>,
    /// Tauri command name to invoke on retry (e.g., "download_model", "set_active_model")
    pub retry_command: Option<String>,
    /// JSON-encoded arguments for the retry command
    pub retry_args: Option<String>,
    /// Technical error detail for "Show Details" toggle
    pub technical_detail: Option<String>,
}

/// Generate a short unique ID for an error event.
fn generate_error_id() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("err-{}", duration.as_millis())
}

/// Emit a recoverable error event to the frontend.
///
/// This is the primary entry point for the backend to notify the frontend
/// about recoverable errors that should show a dialog with retry options.
pub fn emit_recoverable_error(app: &AppHandle, event: RecoverableErrorEvent) {
    if let Err(e) = app.emit("recoverable-error", &event) {
        log::warn!("Failed to emit recoverable error event: {}", e);
    }
}

/// Helper to create and emit a model download error.
pub fn emit_model_download_error(
    app: &AppHandle,
    model_id: &str,
    model_name: &str,
    error: &str,
    can_retry: bool,
) {
    let recovery_action = if can_retry {
        RecoveryAction::Retry
    } else {
        RecoveryAction::Permanent
    };

    let event = RecoverableErrorEvent {
        error_id: generate_error_id(),
        error_type: RecoverableErrorType::ModelDownload,
        recovery_action,
        message: format!("Failed to download model {}", model_name),
        message_key: Some("errors.recoverable.modelDownloadFailed".to_string()),
        message_params: Some(serde_json::json!({ "model": model_name }).to_string()),
        context: Some(
            serde_json::json!({ "model_id": model_id, "model_name": model_name }).to_string(),
        ),
        retry_command: Some("download_model".to_string()),
        retry_args: Some(serde_json::json!({ "model_id": model_id }).to_string()),
        technical_detail: Some(error.to_string()),
    };

    emit_recoverable_error(app, event);
}

/// Helper to create and emit a model load error.
pub fn emit_model_load_error(
    app: &AppHandle,
    model_id: &str,
    model_name: &str,
    error: &str,
    can_retry: bool,
) {
    let recovery_action = if can_retry {
        RecoveryAction::Retry
    } else {
        RecoveryAction::Permanent
    };

    let event = RecoverableErrorEvent {
        error_id: generate_error_id(),
        error_type: RecoverableErrorType::ModelLoad,
        recovery_action,
        message: format!("Failed to load model {}", model_name),
        message_key: Some("errors.recoverable.modelLoadFailed".to_string()),
        message_params: Some(serde_json::json!({ "model": model_name }).to_string()),
        context: Some(
            serde_json::json!({ "model_id": model_id, "model_name": model_name }).to_string(),
        ),
        retry_command: Some("set_active_model".to_string()),
        retry_args: Some(serde_json::json!({ "model_id": model_id }).to_string()),
        technical_detail: Some(error.to_string()),
    };

    emit_recoverable_error(app, event);
}

/// Helper to create and emit a transcription error.
pub fn emit_transcription_error(
    app: &AppHandle,
    error: &str,
    model_id: Option<&str>,
    can_retry: bool,
) {
    let recovery_action = if can_retry {
        RecoveryAction::Retry
    } else {
        RecoveryAction::Permanent
    };

    let context = model_id.map(|mid| serde_json::json!({ "model_id": mid }).to_string());

    let event = RecoverableErrorEvent {
        error_id: generate_error_id(),
        error_type: RecoverableErrorType::Transcription,
        recovery_action,
        message: "Transcription failed".to_string(),
        message_key: Some("errors.recoverable.transcriptionFailed".to_string()),
        message_params: None,
        context,
        retry_command: None, // Transcription retries use different mechanism
        retry_args: None,
        technical_detail: Some(error.to_string()),
    };

    emit_recoverable_error(app, event);
}

/// Helper to create and emit an audio device error.
pub fn emit_audio_device_error(
    app: &AppHandle,
    error_type: &str,
    error: &str,
    needs_user_action: bool,
) {
    let recovery_action = if needs_user_action {
        RecoveryAction::UserAction
    } else {
        RecoveryAction::Retry
    };

    let message_key = match error_type {
        "microphone_permission_denied" => "errors.recoverable.microphonePermissionDenied",
        "no_input_device" => "errors.recoverable.noInputDevice",
        _ => "errors.recoverable.audioDeviceError",
    };

    let event = RecoverableErrorEvent {
        error_id: generate_error_id(),
        error_type: RecoverableErrorType::AudioDevice,
        recovery_action,
        message: error.to_string(),
        message_key: Some(message_key.to_string()),
        message_params: None,
        context: Some(serde_json::json!({ "error_type": error_type }).to_string()),
        retry_command: None, // Audio device errors typically need user action
        retry_args: None,
        technical_detail: Some(error.to_string()),
    };

    emit_recoverable_error(app, event);
}