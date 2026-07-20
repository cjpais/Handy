//! Centralized error types for the Handy application.
//!
//! Uses `thiserror` for structured error definitions with user-friendly messages.
//! Internal error propagation uses `anyhow`; conversion to `AppError` happens at
//! API boundaries (Tauri commands, manager public methods). `AppError` converts
//! to `String` via `From<AppError> for String` for Tauri command compatibility.

/// Unified error type for the Handy application.
/// Each variant corresponds to a domain area and carries enough context
/// for user-friendly error messages. The `#[error(...)]` attribute
/// provides the display message; `#[source]` marks the underlying cause
/// for error chaining.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // ── Audio ──────────────────────────────────────────────────────────
    /// Errors from audio recording, device enumeration, or stream setup.
    #[error("Audio error: {message}")]
    Audio {
        message: String,
        #[source]
        source: anyhow::Error,
    },

    /// No audio input device available or selected.
    #[error("No audio input device available")]
    AudioNoDevice,

    // ── Transcription ──────────────────────────────────────────────────
    /// Errors from the transcription engine (model loading, inference, etc.).
    #[error("Transcription error: {message}")]
    Transcription {
        message: String,
        #[source]
        source: anyhow::Error,
    },

    /// The transcription engine panicked (e.g., segfault in native code).
    #[error("Transcription engine panicked: {0}. The model has been unloaded and will reload on next attempt.")]
    TranscriptionPanic(String),

    /// Another transcription is already in progress.
    #[error("Another transcription is in progress. Please wait and try again.")]
    TranscriptionBusy,

    /// Timed out waiting for model to load.
    #[error("Timed out waiting for model to load. Please try again.")]
    TranscriptionLoadTimeout,

    /// Model not loaded when transcription was requested.
    #[error("Model is not loaded for transcription.")]
    ModelNotLoaded,

    // ── Model ───────────────────────────────────────────────────────────
    /// Model not found in the available models list.
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// Model exists but has not been downloaded yet.
    #[error("Model not downloaded: {0}")]
    ModelNotDownloaded(String),

    /// Download verification (SHA256) failed.
    #[error("Download verification failed for model {model_id}: file is corrupt. Please retry.")]
    ModelVerificationFailed {
        model_id: String,
        #[source]
        source: anyhow::Error,
    },

    /// Network or I/O error during model download.
    #[error("Failed to download model {model_id}: {message}")]
    ModelDownloadFailed {
        model_id: String,
        message: String,
        #[source]
        source: anyhow::Error,
    },

    /// Download was cancelled by the user.
    #[error("Download cancelled for: {0}")]
    ModelDownloadCancelled(String),

    /// Failed to extract a downloaded model archive.
    #[error("Failed to extract model {model_id}: {message}")]
    ModelExtractionFailed {
        model_id: String,
        message: String,
        #[source]
        source: anyhow::Error,
    },

    /// Failed to load a model into memory for transcription.
    #[error("Failed to load {engine} model {model_id}: {message}")]
    ModelLoadFailed {
        engine: String,
        model_id: String,
        message: String,
        #[source]
        source: anyhow::Error,
    },

    /// No model files found to delete.
    #[error("No model files found to delete")]
    ModelNoFilesToDelete,

    /// Model is currently downloading and cannot be used.
    #[error("Model is currently downloading: {0}")]
    ModelCurrentlyDownloading(String),

    /// Model file/directory not found on disk.
    #[error("Complete model {kind} not found: {model_id}")]
    ModelPathNotFound { kind: String, model_id: String },

    // ── Settings ────────────────────────────────────────────────────────
    /// Error persisting or loading application settings.
    #[error("Settings error: {0}")]
    Settings(String),

    // ── History / Database ──────────────────────────────────────────────
    /// SQLite or database-related errors.
    #[error("Database error: {message}")]
    Database {
        message: String,
        #[source]
        source: anyhow::Error,
    },

    // ── I/O ─────────────────────────────────────────────────────────────
    /// General file-system or I/O errors.
    #[error("I/O error: {message}")]
    Io {
        message: String,
        #[source]
        source: std::io::Error,
    },

    /// Path resolution error.
    #[error("Failed to resolve path: {0}")]
    PathResolution(String),

    // ── Catch-all ───────────────────────────────────────────────────────
    /// Errors that don't fit a specific category.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// ── Conversions from underlying error types ─────────────────────────

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io {
            message: err.to_string(),
            source: err,
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database {
            message: err.to_string(),
            source: anyhow::anyhow!("{}", err),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Other(anyhow::anyhow!("{}", err))
    }
}

// ── Conversion to String for Tauri command boundary ─────────────────
//
// Tauri commands return `Result<T, String>`. This impl lets us write
// `result.map_err(AppError::from)?` or `?.to_string()` at the command
// boundary without boilerplate.

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}

// ── Convenience constructors ────────────────────────────────────────

impl AppError {
    /// Create an audio error from a message and any underlying error.
    pub fn audio(msg: impl Into<String>, source: anyhow::Error) -> Self {
        AppError::Audio {
            message: msg.into(),
            source,
        }
    }

    /// Create a transcription error from a message and any underlying error.
    pub fn transcription(msg: impl Into<String>, source: anyhow::Error) -> Self {
        AppError::Transcription {
            message: msg.into(),
            source,
        }
    }

    /// Create a model-load error with the engine name, model id, and underlying error.
    pub fn model_load(
        engine: impl Into<String>,
        model_id: impl Into<String>,
        message: impl Into<String>,
        source: anyhow::Error,
    ) -> Self {
        AppError::ModelLoadFailed {
            engine: engine.into(),
            model_id: model_id.into(),
            message: message.into(),
            source,
        }
    }

    /// Create a download failure error.
    pub fn model_download(
        model_id: impl Into<String>,
        message: impl Into<String>,
        source: anyhow::Error,
    ) -> Self {
        AppError::ModelDownloadFailed {
            model_id: model_id.into(),
            message: message.into(),
            source,
        }
    }

    /// Create a model extraction failure error.
    pub fn model_extraction(
        model_id: impl Into<String>,
        message: impl Into<String>,
        source: anyhow::Error,
    ) -> Self {
        AppError::ModelExtractionFailed {
            model_id: model_id.into(),
            message: message.into(),
            source,
        }
    }

    /// Create a database error from a message and any underlying error.
    pub fn database(msg: impl Into<String>, source: anyhow::Error) -> Self {
        AppError::Database {
            message: msg.into(),
            source,
        }
    }

    /// Create a settings error.
    pub fn settings(msg: impl Into<String>) -> Self {
        AppError::Settings(msg.into())
    }

    /// Create an I/O error with context.
    pub fn io(msg: impl Into<String>, source: std::io::Error) -> Self {
        AppError::Io {
            message: msg.into(),
            source,
        }
    }

    /// Create a path resolution error.
    pub fn path_resolution(msg: impl Into<String>) -> Self {
        AppError::PathResolution(msg.into())
    }
}

/// Result type alias used throughout the app for functions that can fail
/// with a structured `AppError`.
pub type AppResult<T> = Result<T, AppError>;

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    // ── Display messages ───────────────────────────────────────────

    #[test]
    fn display_audio_no_device() {
        let err = AppError::AudioNoDevice;
        assert_eq!(err.to_string(), "No audio input device available");
    }

    #[test]
    fn display_transcription_busy() {
        let err = AppError::TranscriptionBusy;
        assert_eq!(
            err.to_string(),
            "Another transcription is in progress. Please wait and try again."
        );
    }

    #[test]
    fn display_transcription_load_timeout() {
        let err = AppError::TranscriptionLoadTimeout;
        assert_eq!(
            err.to_string(),
            "Timed out waiting for model to load. Please try again."
        );
    }

    #[test]
    fn display_model_not_loaded() {
        let err = AppError::ModelNotLoaded;
        assert_eq!(err.to_string(), "Model is not loaded for transcription.");
    }

    #[test]
    fn display_model_not_found() {
        let err = AppError::ModelNotFound("whisper-turbo".into());
        assert_eq!(err.to_string(), "Model not found: whisper-turbo");
    }

    #[test]
    fn display_model_not_downloaded() {
        let err = AppError::ModelNotDownloaded("whisper-large".into());
        assert_eq!(err.to_string(), "Model not downloaded: whisper-large");
    }

    #[test]
    fn display_model_download_cancelled() {
        let err = AppError::ModelDownloadCancelled("whisper-small".into());
        assert_eq!(err.to_string(), "Download cancelled for: whisper-small");
    }

    #[test]
    fn display_model_no_files_to_delete() {
        let err = AppError::ModelNoFilesToDelete;
        assert_eq!(err.to_string(), "No model files found to delete");
    }

    #[test]
    fn display_model_currently_downloading() {
        let err = AppError::ModelCurrentlyDownloading("whisper-turbo".into());
        assert_eq!(
            err.to_string(),
            "Model is currently downloading: whisper-turbo"
        );
    }

    #[test]
    fn display_settings_error() {
        let err = AppError::Settings("key missing".into());
        assert_eq!(err.to_string(), "Settings error: key missing");
    }

    #[test]
    fn display_path_resolution() {
        let err = AppError::PathResolution("/foo/bar".into());
        assert_eq!(
            err.to_string(),
            "Failed to resolve path: /foo/bar"
        );
    }

    #[test]
    fn display_transcription_panic() {
        let err = AppError::TranscriptionPanic("segfault".into());
        assert_eq!(
            err.to_string(),
            "Transcription engine panicked: segfault. The model has been unloaded and will reload on next attempt."
        );
    }

    #[test]
    fn display_model_path_not_found() {
        let err = AppError::ModelPathNotFound {
            kind: "whisper".into(),
            model_id: "turbo".into(),
        };
        assert_eq!(err.to_string(), "Complete model whisper not found: turbo");
    }

    // ── Structured errors with source ──────────────────────────────

    #[test]
    fn display_audio_error_with_source() {
        let source = anyhow::anyhow!("device busy");
        let err = AppError::Audio {
            message: "Failed to open mic".into(),
            source,
        };
        assert_eq!(err.to_string(), "Audio error: Failed to open mic");
        assert!(err.source().is_some());
    }

    #[test]
    fn display_transcription_error_with_source() {
        let source = anyhow::anyhow!("inference failed");
        let err = AppError::Transcription {
            message: "decode error".into(),
            source,
        };
        assert_eq!(err.to_string(), "Transcription error: decode error");
        assert!(err.source().is_some());
    }

    #[test]
    fn display_database_error_with_source() {
        let source = anyhow::anyhow!("locked");
        let err = AppError::Database {
            message: "table busy".into(),
            source,
        };
        assert_eq!(err.to_string(), "Database error: table busy");
        assert!(err.source().is_some());
    }

    #[test]
    fn display_model_verification_failed() {
        let source = anyhow::anyhow!("SHA mismatch");
        let err = AppError::ModelVerificationFailed {
            model_id: "turbo".into(),
            source,
        };
        assert_eq!(
            err.to_string(),
            "Download verification failed for model turbo: file is corrupt. Please retry."
        );
    }

    #[test]
    fn display_model_download_failed() {
        let source = anyhow::anyhow!("connection refused");
        let err = AppError::ModelDownloadFailed {
            model_id: "small".into(),
            message: "network error".into(),
            source,
        };
        assert_eq!(
            err.to_string(),
            "Failed to download model small: network error"
        );
    }

    #[test]
    fn display_model_extraction_failed() {
        let source = anyhow::anyhow!("bad tar");
        let err = AppError::ModelExtractionFailed {
            model_id: "medium".into(),
            message: "invalid archive".into(),
            source,
        };
        assert_eq!(
            err.to_string(),
            "Failed to extract model medium: invalid archive"
        );
    }

    #[test]
    fn display_model_load_failed() {
        let source = anyhow::anyhow!("file missing");
        let err = AppError::ModelLoadFailed {
            engine: "whisper".into(),
            model_id: "turbo".into(),
            message: "weights not found".into(),
            source,
        };
        assert_eq!(
            err.to_string(),
            "Failed to load whisper model turbo: weights not found"
        );
    }

    #[test]
    fn display_io_error_with_source() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = AppError::Io {
            message: "read failed".into(),
            source,
        };
        assert_eq!(err.to_string(), "I/O error: read failed");
    }

    #[test]
    fn display_other_anyhow() {
        let source = anyhow::anyhow!("something broke");
        let err = AppError::Other(source);
        assert_eq!(err.to_string(), "something broke");
    }

    // ── Convenience constructors ───────────────────────────────────

    #[test]
    fn constructor_audio() {
        let err = AppError::audio("test msg", anyhow::anyhow!("root cause"));
        assert_eq!(err.to_string(), "Audio error: test msg");
        assert!(err.source().is_some());
    }

    #[test]
    fn constructor_transcription() {
        let err = AppError::transcription("decode fail", anyhow::anyhow!("oom"));
        assert_eq!(err.to_string(), "Transcription error: decode fail");
    }

    #[test]
    fn constructor_model_load() {
        let err = AppError::model_load(
            "whisper",
            "turbo",
            "weights missing",
            anyhow::anyhow!("io"),
        );
        assert_eq!(
            err.to_string(),
            "Failed to load whisper model turbo: weights missing"
        );
    }

    #[test]
    fn constructor_model_download() {
        let err = AppError::model_download("small", "timeout", anyhow::anyhow!("timed out"));
        assert_eq!(
            err.to_string(),
            "Failed to download model small: timeout"
        );
    }

    #[test]
    fn constructor_model_extraction() {
        let err =
            AppError::model_extraction("medium", "bad archive", anyhow::anyhow!("corrupt"));
        assert_eq!(
            err.to_string(),
            "Failed to extract model medium: bad archive"
        );
    }

    #[test]
    fn constructor_database() {
        let err = AppError::database("table locked", anyhow::anyhow!("busy"));
        assert_eq!(err.to_string(), "Database error: table locked");
    }

    #[test]
    fn constructor_settings() {
        let err = AppError::settings("missing key");
        assert_eq!(err.to_string(), "Settings error: missing key");
    }

    #[test]
    fn constructor_io() {
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = AppError::io("write failed", source);
        assert_eq!(err.to_string(), "I/O error: write failed");
    }

    #[test]
    fn constructor_path_resolution() {
        let err = AppError::path_resolution("~/missing");
        assert_eq!(err.to_string(), "Failed to resolve path: ~/missing");
    }

    // ── From conversions ───────────────────────────────────────────

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
        let app_err: AppError = io_err.into();
        assert!(matches!(app_err, AppError::Io { .. }));
        assert!(app_err.to_string().contains("pipe broke"));
    }

    #[test]
    fn from_rusqlite_error() {
        let sqlite_err = rusqlite::Error::ExecuteReturnedResults;
        let app_err: AppError = sqlite_err.into();
        assert!(matches!(app_err, AppError::Database { .. }));
    }

    #[test]
    fn from_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("generic failure");
        let app_err: AppError = anyhow_err.into();
        assert!(matches!(app_err, AppError::Other(_)));
        assert_eq!(app_err.to_string(), "generic failure");
    }

    #[test]
    fn from_app_error_to_string() {
        let err = AppError::Settings("test".into());
        let s: String = err.into();
        assert_eq!(s, "Settings error: test");
    }

    #[test]
    fn from_io_error_to_app_error_to_string() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let app_err: AppError = io_err.into();
        let s: String = app_err.into();
        assert!(s.contains("gone"));
    }

    // ── Error variant discrimination ───────────────────────────────

    #[test]
    fn debug_format_is_useful() {
        let err = AppError::TranscriptionBusy;
        let debug = format!("{:?}", err);
        assert!(debug.contains("TranscriptionBusy"));
    }

    #[test]
    fn source_chaining_works() {
        let source = anyhow::anyhow!("root");
        let err = AppError::Audio {
            message: "msg".into(),
            source,
        };
        let src = err.source().unwrap();
        assert_eq!(src.to_string(), "root");
    }

    #[test]
    fn multiple_errors_have_distinct_display() {
        let errors: Vec<AppError> = vec![
            AppError::AudioNoDevice,
            AppError::TranscriptionBusy,
            AppError::ModelNotLoaded,
            AppError::ModelNoFilesToDelete,
            AppError::TranscriptionLoadTimeout,
        ];
        let displays: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let unique: std::collections::HashSet<&str> =
            displays.iter().map(|s| s.as_str()).collect();
        assert_eq!(unique.len(), displays.len(), "All display messages should be unique");
    }
}