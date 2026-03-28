// CI-only mock StreamingSession - avoids transcribe-rs dependency.
// This file is copied over streaming.rs during CI tests.

use std::sync::mpsc;

use crate::managers::transcription::TranscriptionManager;
use crate::settings::TranscriptionMode;
use std::sync::Arc;
use tauri::AppHandle;

pub struct StreamingFinishResult {
    pub audio: Vec<f32>,
    pub combined_text: String,
}

pub struct StreamingSession;

impl StreamingSession {
    pub fn start(
        _mode: TranscriptionMode,
        _tm: Arc<TranscriptionManager>,
        _app: AppHandle,
        _chunk_rx: mpsc::Receiver<Vec<f32>>,
        _vad_model_path: String,
        _realtime_chunk_duration_secs: f32,
    ) -> Self {
        Self
    }

    pub fn finish(self) -> StreamingFinishResult {
        StreamingFinishResult {
            audio: Vec::new(),
            combined_text: String::new(),
        }
    }
}

/// No-op in CI mock.
pub fn transcribe_chunked(
    _tm: &TranscriptionManager,
    _samples: &[f32],
    _vad_model_path: &str,
) -> Result<String, anyhow::Error> {
    Ok(String::new())
}
