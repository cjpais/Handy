// Re-export all audio components
mod device;
#[cfg(target_os = "linux")]
mod pipewire_recorder;
mod recorder;
mod recorder_backend;
mod resampler;
mod utils;
mod visualizer;

#[cfg(target_os = "linux")]
pub use device::resolve_pipewire_target;
pub use device::{
    list_input_devices, list_input_devices_for_backend, list_output_devices, CpalDeviceInfo,
    InputDeviceInfo,
};
pub use recorder::{
    is_microphone_access_denied, is_no_input_device_error, AudioRecorder, VadPolicy,
};
// Shared parts used by the `Recorder` seam / manager to build backends.
pub(crate) use recorder::{AudioFrameCallback, VadConfig};
pub use recorder_backend::{resolve_capture_backend, CaptureBackend, Recorder};
pub use resampler::FrameResampler;
pub use utils::{read_wav_samples, save_wav_file, verify_wav_file};
pub use visualizer::AudioVisualiser;
