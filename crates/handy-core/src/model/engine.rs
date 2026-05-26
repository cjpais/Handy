use serde::{Deserialize, Serialize};

/// All supported speech-recognition engines. Host managers map this to the
/// concrete `transcribe_rs` engine variant when loading models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineType {
    Whisper,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
}
