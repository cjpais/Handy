// Re-export all audio components
mod device;
mod recorder;
mod resampler;
mod utils;
mod visualizer;

pub use device::{list_input_devices, list_output_devices, CpalDeviceInfo};
pub use recorder::{is_microphone_access_denied, AudioRecorder};
pub use resampler::FrameResampler;
pub use utils::{load_wav_samples, save_wav_file, wav_duration_secs};
pub use visualizer::AudioVisualiser;
