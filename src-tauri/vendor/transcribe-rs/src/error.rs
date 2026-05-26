use std::path::PathBuf;

/// Errors produced by transcribe-rs engines.
#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("model not found: {0}")]
    ModelNotFound(PathBuf),

    #[error("inference error: {0}")]
    Inference(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error("config error: {0}")]
    Config(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

// ---- From impls for common error types ----

impl From<hound::Error> for TranscribeError {
    fn from(e: hound::Error) -> Self {
        TranscribeError::Audio(e.to_string())
    }
}

impl From<serde_json::Error> for TranscribeError {
    fn from(e: serde_json::Error) -> Self {
        TranscribeError::Config(e.to_string())
    }
}

#[cfg(feature = "onnx")]
impl From<ort::Error> for TranscribeError {
    fn from(e: ort::Error) -> Self {
        TranscribeError::Inference(e.to_string())
    }
}

#[cfg(feature = "audio-features")]
impl From<ndarray::ShapeError> for TranscribeError {
    fn from(e: ndarray::ShapeError) -> Self {
        TranscribeError::Inference(e.to_string())
    }
}
