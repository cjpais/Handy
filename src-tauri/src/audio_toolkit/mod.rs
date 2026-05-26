//! Desktop audio toolkit. Pure logic lives in `handy-core`; cpal-bound code
//! stays here.
//!
//! The thin shim files under this module re-export from `handy_core` so that
//! existing callers within `src-tauri/` keep working. New desktop code should
//! prefer importing directly from `handy_core::...`.

pub mod audio;
pub mod constants;
pub mod text;
pub mod utils;
pub mod vad;

pub use audio::{
    find_ai_mouse_microphone_name, is_microphone_access_denied, list_input_devices,
    list_output_devices, save_wav_file, AudioRecorder, CpalDeviceInfo,
};
pub use handy_core::text::{apply_custom_words, filter_transcription_output};
pub use handy_core::vad::{SileroVad, VoiceActivityDetector};
pub use utils::get_cpal_host;
