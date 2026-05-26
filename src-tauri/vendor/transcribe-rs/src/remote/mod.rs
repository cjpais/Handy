use std::path::Path;

use async_trait::async_trait;

use crate::{TranscribeError, TranscriptionResult};

pub mod openai;

/// Common interface for speech transcription through remote APIs.
///
/// Unlike local inference engines, remote APIs can handle concurren requests
/// and can switch models without any cost.
#[async_trait]
pub trait RemoteTranscriptionEngine: Send + Sync {
    type RequestParams: Send + Sync;

    async fn transcribe_file(
        &self,
        wav_path: &Path,
        params: Self::RequestParams,
    ) -> Result<TranscriptionResult, TranscribeError>;
}
