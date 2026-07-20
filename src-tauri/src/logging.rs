//! Structured event logging for Handy.
//!
//! This module provides a structured logging layer on top of the existing `log`
//! crate macros. While `debug!()`/`info!()` produce free-form text for
//! human-readable console output, this module emits **structured JSONL events**
//! to a dedicated file, each carrying a session ID, event type, and typed
//! context fields.
//!
//! An AI coding agent (or debugging human) can grep the JSONL log by session
//! ID (`sid`), event type (`evt`), or log level (`lvl`) to quickly diagnose
//! issues without reading thousands of unstructured lines.

use chrono::Utc;
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Unique identifier for a recording→transcription session.
/// Generated from a timestamp so it's human-sortable and unique per session.
pub type SessionId = String;

/// Generate a new session ID from the current UTC timestamp.
pub fn new_session_id() -> SessionId {
    format!("s-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"))
}

// ── Structured event types ─────────────────────────────────────────

/// Structured event types that matter for diagnostics.
/// Each variant captures the context needed to understand *what happened*
/// without reading free-form log messages.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "evt")]
#[allow(dead_code)] // Variants used selectively via emit(); some reserved for future instrumentation
pub enum AppEvent {
    // ── Recording lifecycle ──────────────────────────────────────
    RecordingStarted {
        sid: SessionId,
        mic: String,
        always_on: bool,
    },
    RecordingStopped {
        sid: SessionId,
        sample_count: usize,
        duration_ms: u64,
    },
    RecordingFailed {
        sid: SessionId,
        error: String,
        error_type: String,
    },

    // ── Transcription lifecycle ──────────────────────────────────
    TranscriptionStarted {
        sid: SessionId,
        model_id: String,
    },
    TranscriptionCompleted {
        sid: SessionId,
        model_id: String,
        text_length: usize,
        duration_ms: u64,
    },
    TranscriptionFailed {
        sid: SessionId,
        model_id: String,
        error: String,
        duration_ms: u64,
    },

    // ── Model management ────────────────────────────────────────
    ModelLoadStarted {
        model_id: String,
    },
    ModelLoadCompleted {
        model_id: String,
        duration_ms: u64,
    },
    ModelLoadFailed {
        model_id: String,
        error: String,
        duration_ms: u64,
    },
    ModelUnloaded {
        model_id: String,
        reason: String,
        idle_secs: u64,
    },
    ModelSwitched {
        old_model_id: Option<String>,
        new_model_id: String,
    },

    // ── Post-processing ─────────────────────────────────────────
    PostProcessStarted {
        sid: SessionId,
        provider: String,
    },
    PostProcessCompleted {
        sid: SessionId,
        provider: String,
        duration_ms: u64,
    },
    PostProcessFailed {
        sid: SessionId,
        provider: String,
        error: String,
    },

    // ── Audio/device events ──────────────────────────────────────
    MicDeviceChanged {
        old: Option<String>,
        new: String,
    },
    UsbWatchdogCycle {
        device: String,
        success: bool,
    },
    AudioFeedback {
        sound_type: String,
    },

    // ── Clipboard/paste ─────────────────────────────────────────
    PasteSucceeded {
        sid: SessionId,
        duration_ms: u64,
    },
    PasteFailed {
        sid: SessionId,
        error: String,
    },

    // ── App lifecycle ────────────────────────────────────────────
    AppStarted {
        version: String,
        platform: String,
    },
    ShortcutTriggered {
        binding_id: String,
        action: String,
    },
    SettingsChanged {
        setting: String,
    },
    CancelTriggered {
        recording_was_active: bool,
    },

    // ── Crash diagnostics ─────────────────────────────────────────
    /// Captured from the global panic hook before the process terminates.
    AppCrashed {
        message: String,
        location: String,
        thread: String,
    },
}

impl AppEvent {
    /// Map each event variant to a log level for the `log` crate.
    /// Failures → Error, completions → Info, starts → Debug.
    pub fn log_level(&self) -> log::Level {
        match self {
            // Errors
            Self::RecordingFailed { .. }
            | Self::TranscriptionFailed { .. }
            | Self::ModelLoadFailed { .. }
            | Self::PostProcessFailed { .. }
            | Self::PasteFailed { .. }
            | Self::AppCrashed { .. } => log::Level::Error,

            // Warnings
            Self::UsbWatchdogCycle { success: false, .. } => log::Level::Warn,

            // Info — completions and significant state changes
            Self::RecordingStarted { .. }
            | Self::TranscriptionCompleted { .. }
            | Self::ModelLoadCompleted { .. }
            | Self::ModelUnloaded { .. }
            | Self::ModelSwitched { .. }
            | Self::PostProcessCompleted { .. }
            | Self::PasteSucceeded { .. }
            | Self::AppStarted { .. }
            | Self::MicDeviceChanged { .. } => log::Level::Info,

            // Debug — starts and routine events
            Self::RecordingStopped { .. }
            | Self::TranscriptionStarted { .. }
            | Self::ModelLoadStarted { .. }
            | Self::PostProcessStarted { .. }
            | Self::AudioFeedback { .. }
            | Self::ShortcutTriggered { .. }
            | Self::SettingsChanged { .. }
            | Self::CancelTriggered { .. }
            | Self::UsbWatchdogCycle { success: true, .. } => log::Level::Debug,
        }
    }
}

// ── JSONL log writer ────────────────────────────────────────────────

/// Wrapper type so we can store the file handle in a Mutex.
struct LogWriter {
    file: File,
    _path: PathBuf,
}

/// Global structured log writer. Set once during app setup.
/// `None` before initialization; `Some` after the JSONL file is opened.
static STRUCTURED_LOGGER: std::sync::OnceLock<Mutex<Option<LogWriter>>> =
    std::sync::OnceLock::new();

/// Initialise the structured JSONL logger.
///
/// Creates (or appends to) a `handy-events.jsonl` file in the app's log
/// directory. Must be called once during `setup()`.
pub fn init(app_handle: &tauri::AppHandle) {
    let log_dir = match crate::portable::app_log_dir(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("Failed to resolve log directory for structured logging: {e}");
            return;
        }
    };

    // Create the directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        log::error!("Failed to create log directory {:?}: {e}", log_dir);
        return;
    }

    let path = log_dir.join("handy-events.jsonl");

    let file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to open structured log file {:?}: {e}", path);
            return;
        }
    };

    log::info!("Structured event log: {}", path.display());

    let _ = STRUCTURED_LOGGER.set(Mutex::new(Some(LogWriter { file, _path: path })));
}

/// Emit a structured event to the JSONL log file **and** the standard `log`
/// crate at the event's appropriate level.
///
/// The JSONL line includes:
/// - `ts` — ISO 8601 UTC timestamp
/// - `lvl` — log level (trace/debug/info/warn/error)
/// - The serialised `AppEvent` (tagged with `evt`)
pub fn emit(event: AppEvent) {
    let level = event.log_level();

    // Also emit to the standard log crate so it appears in the console and
    // the existing tauri-plugin-log file output.
    let event_json = match serde_json::to_string(&event) {
        Ok(j) => j,
        Err(e) => {
            log::error!("Failed to serialise structured event: {e}");
            return;
        }
    };

    log::log!(level, "[event] {}", event_json);

    // Write to the dedicated JSONL file
    if let Some(logger) = STRUCTURED_LOGGER.get() {
        let mut guard = logger.lock();

        if let Some(ref mut writer) = *guard {
            let line = serde_json::json!({
                "ts": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                "lvl": level.as_str(),
            })
            .to_string();

            // Merge the event JSON into the envelope.
            // We produce a single JSON line by combining the envelope fields
            // with the event fields. This keeps the line self-contained.
            let mut envelope: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            let event_val: serde_json::Value =
                serde_json::from_str(&event_json).unwrap_or_default();

            if let (serde_json::Value::Object(ref mut map), serde_json::Value::Object(evt_map)) =
                (&mut envelope, event_val)
            {
                for (k, v) in evt_map {
                    map.insert(k, v);
                }
            }

            if let Err(e) = writeln!(writer.file, "{}", envelope) {
                log::error!("Failed to write structured event: {e}");
            }
            // Flush on error/warn events so they're captured before a crash
            if level <= log::Level::Warn {
                let _ = writer.file.flush();
            }
        }
    }
}

/// Install a global panic hook that captures panic information and writes
/// it to both the standard log file and the structured JSONL event log
/// before the process terminates.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<unknown panic payload>".to_string());

        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());

        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();

        log::error!("PANIC in thread '{}': {} at {}", thread, message, location);

        emit(AppEvent::AppCrashed {
            message,
            location,
            thread,
        });

        // Force-flush both log files so crash is captured before process dies
        if let Some(logger) = STRUCTURED_LOGGER.get() {
            let guard = logger.lock();
            if let Some(ref writer) = *guard {
                let _ = writer.file.sync_all();
            }
        }
        log::logger().flush();
    }));
}