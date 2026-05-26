use serde::{Deserialize, Serialize};

/// Streamed during a model download. Host emits this through `EventSink`
/// to keep the UI in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}
