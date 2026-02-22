use crate::types::HotkeyId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Accessibility permission not granted. Please enable it in System Settings > Privacy & Security > Accessibility")]
    AccessibilityNotGranted,

    #[error("Failed to create event tap: {0}")]
    EventTapCreationFailed(String),

    #[error("Failed to create run loop source")]
    RunLoopSourceCreationFailed,

    #[error("Hotkey with id {0:?} not found")]
    HotkeyNotFound(HotkeyId),

    #[error("Hotkey already registered: {0}")]
    HotkeyAlreadyRegistered(String),

    #[error("Event loop not running")]
    EventLoopNotRunning,

    #[error("Operation timed out")]
    Timeout,

    #[error("Failed to start recording")]
    RecordingFailed,

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Hotkey cannot be empty (must have at least a key or modifiers)")]
    EmptyHotkey,

    #[error("Invalid hotkey format: {0}")]
    InvalidHotkeyFormat(String),

    #[error("Unknown key: {0}")]
    UnknownKey(String),

    #[error("Unknown modifier: {0}")]
    UnknownModifier(String),

    #[error("Internal error: Mutex poisoned")]
    MutexPoisoned,
}

pub type Result<T> = std::result::Result<T, Error>;
