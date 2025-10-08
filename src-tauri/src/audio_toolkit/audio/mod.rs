// Re-export all audio components
pub mod device;
pub mod recorder;
pub mod resampler;
pub mod utils;
pub mod visualizer;

pub use device::{list_input_devices, list_output_devices, CpalDeviceInfo};
pub use recorder::AudioRecorder;
pub use resampler::FrameResampler;
pub use utils::save_wav_file;
pub use visualizer::AudioVisualiser;
