pub mod audio;
pub mod constants;
pub mod lang_id;
pub mod text;
pub mod utils;
pub mod vad;

pub use audio::{
    is_effectively_silent, is_microphone_access_denied, is_no_input_device_error,
    list_input_devices, list_output_devices, peak_dbfs, read_wav_samples, save_wav_file,
    verify_wav_file, AudioRecorder, CpalDeviceInfo, VadPolicy, SILENCE_THRESHOLD_DBFS,
};
pub use lang_id::detect_output_language;
pub use text::{
    apply_custom_words, normalize_transcription_output, remove_filler_words, OutputLanguageEvidence,
};
pub use utils::get_cpal_host;
pub use vad::{EarshotVad, SileroVad, VoiceActivityDetector};
