pub mod gemini;
pub mod openai;

use anyhow::Result;
use log::{debug, warn};
use std::future::Future;
use std::time::Duration;

use crate::settings::AppSettings;

pub const MODEL_ID_CLOUD: &str = "cloud";
pub const MODEL_ID_GEMINI: &str = "gemini";

/// Trait for cloud-based transcription providers (Gemini, OpenAI-compatible, etc.)
#[async_trait::async_trait]
pub trait CloudProvider: Send + Sync {
    /// Transcribe WAV audio bytes.
    /// `post_process` — if true, provider may apply its built-in prompt (e.g. Gemini system_instruction).
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        post_process: bool,
        settings: &AppSettings,
    ) -> Result<String>;

    /// Verify credentials with a minimal request.
    async fn test_connection(&self, settings: &AppSettings) -> Result<()>;

    /// Provider ID matching the model ID constant (e.g. "cloud", "gemini").
    fn id(&self) -> &'static str;
}

const RETRY_DELAYS_MS: &[u64] = &[0, 300, 800];

/// Shared retry wrapper for cloud transcription calls.
pub async fn with_retry<F, Fut>(label: &str, f: F) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut last_error = anyhow::anyhow!("Unknown {} transcription error", label);

    for (attempt, &delay) in RETRY_DELAYS_MS.iter().enumerate() {
        if delay > 0 {
            debug!(
                "{} transcription retry {}/{}, waiting {}ms",
                label,
                attempt + 1,
                RETRY_DELAYS_MS.len(),
                delay
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        match f().await {
            Ok(text) => return Ok(text),
            Err(e) => {
                warn!(
                    "{} transcription attempt {}/{} failed: {}",
                    label,
                    attempt + 1,
                    RETRY_DELAYS_MS.len(),
                    e
                );
                last_error = e;
            }
        }
    }

    Err(last_error)
}

/// Minimal valid 16kHz mono WAV with 0 samples (44-byte header only).
/// Used for test_connection() credential verification.
pub(crate) fn silent_wav() -> Vec<u8> {
    vec![
        0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74,
        0x20, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x80, 0x3E, 0x00, 0x00, 0x00, 0x7D,
        0x00, 0x00, 0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61, 0x00, 0x00, 0x00, 0x00,
    ]
}

/// Strip the `${output}` placeholder from a prompt template to produce a system instruction.
pub(crate) fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

/// Returns true if the model ID corresponds to a cloud provider.
pub fn is_cloud_model(model_id: &str) -> bool {
    matches!(model_id, MODEL_ID_CLOUD | MODEL_ID_GEMINI)
}

/// Resolve a model ID to its CloudProvider implementation.
/// Returns None for local model IDs.
pub fn provider_for_model(model_id: &str) -> Option<Box<dyn CloudProvider>> {
    match model_id {
        MODEL_ID_CLOUD => Some(Box::new(openai::OpenAiProvider)),
        MODEL_ID_GEMINI => Some(Box::new(gemini::GeminiProvider)),
        _ => None,
    }
}
