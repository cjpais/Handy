//! macOS-specific audio feedback using system output device to avoid AirPods Handoff.
//!
//! This module provides a Swift bridge to play audio through the system output device
//! (kAudioHardwarePropertyDefaultSystemOutputDevice) instead of the default output device
//! (kAudioHardwarePropertyDefaultOutputDevice). The system output device is used for
//! system alerts and does NOT trigger AirPods Handoff.

use std::ffi::CString;
use std::os::raw::c_char;
use std::path::Path;

// Link to the Swift functions
extern "C" {
    fn play_sound_via_system_output(file_path: *const c_char, volume: f32) -> i32;
    fn is_system_output_available() -> i32;
}

/// Error codes returned by the Swift bridge
#[derive(Debug)]
pub enum AudioFeedbackError {
    /// Failed to get system output device
    NoSystemOutputDevice,
    /// Failed to load audio file
    FileLoadError,
    /// Failed to set output device on audio engine
    DeviceSetupError,
    /// Failed to start audio engine
    EngineStartError,
    /// Failed to create audio buffer
    BufferError,
    /// Failed to read audio file into buffer
    FileReadError,
    /// Path contains invalid characters
    InvalidPath,
    /// Unknown error
    Unknown(i32),
}

impl std::fmt::Display for AudioFeedbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSystemOutputDevice => write!(f, "Failed to get system output device"),
            Self::FileLoadError => write!(f, "Failed to load audio file"),
            Self::DeviceSetupError => write!(f, "Failed to set output device on audio engine"),
            Self::EngineStartError => write!(f, "Failed to start audio engine"),
            Self::BufferError => write!(f, "Failed to create audio buffer"),
            Self::FileReadError => write!(f, "Failed to read audio file"),
            Self::InvalidPath => write!(f, "Path contains invalid characters"),
            Self::Unknown(code) => write!(f, "Unknown error (code: {})", code),
        }
    }
}

impl std::error::Error for AudioFeedbackError {}

/// Checks if the system output device is available.
pub fn is_available() -> bool {
    unsafe { is_system_output_available() == 1 }
}

/// Plays a sound file through the system output device.
///
/// This routes audio through the system output device (used for alerts) instead of
/// the default output device, which avoids triggering AirPods Handoff when a user
/// is listening to audio on another device.
///
/// # Arguments
/// * `path` - Path to the audio file to play
/// * `volume` - Volume level from 0.0 to 1.0
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(AudioFeedbackError)` on failure
pub fn play_as_system_alert(path: &Path, volume: f32) -> Result<(), AudioFeedbackError> {
    let path_str = path.to_str().ok_or(AudioFeedbackError::InvalidPath)?;
    let path_cstring = CString::new(path_str).map_err(|_| AudioFeedbackError::InvalidPath)?;

    let result = unsafe { play_sound_via_system_output(path_cstring.as_ptr(), volume) };

    match result {
        0 => Ok(()),
        -1 => Err(AudioFeedbackError::NoSystemOutputDevice),
        -2 => Err(AudioFeedbackError::FileLoadError),
        -3 => Err(AudioFeedbackError::DeviceSetupError),
        -4 => Err(AudioFeedbackError::EngineStartError),
        -5 => Err(AudioFeedbackError::BufferError),
        -6 => Err(AudioFeedbackError::FileReadError),
        code => Err(AudioFeedbackError::Unknown(code)),
    }
}
