use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const SONIOX_API_URL: &str = "https://api.soniox.com/v1";
const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY_MS: u64 = 500;
const MAX_RETRY_DELAY_MS: u64 = 5000;

#[derive(Serialize)]
struct CreateTranscriptionRequest {
    file_id: String,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_hints: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
struct FileUploadResponse {
    id: String,
}

#[derive(Deserialize, Debug)]
struct CreateTranscriptionResponse {
    id: String,
}

#[derive(Deserialize, Debug)]
struct TranscriptionStatus {
    status: String,
}

#[derive(Deserialize, Debug)]
struct TranscriptResponse {
    text: String,
}

pub struct SonioxClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout_seconds: u32,
}

impl SonioxClient {
    /// Create a new SonioxClient with default timeout (120 seconds)
    #[allow(dead_code)]
    pub fn new(api_key: String, model: String) -> Self {
        Self::with_timeout(api_key, model, 120)
    }

    pub fn with_timeout(api_key: String, model: String, timeout_seconds: u32) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            timeout_seconds,
        }
    }

    /// Validate an API key before saving
    pub fn validate_api_key(key: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("API key cannot be empty".to_string());
        }
        if key.len() < 20 {
            return Err("API key seems too short".to_string());
        }
        Ok(())
    }

    /// Execute an async operation with exponential backoff retry logic
    async fn with_retry<F, Fut, T>(operation_name: &str, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut delay = Duration::from_millis(INITIAL_RETRY_DELAY_MS);

        for attempt in 0..MAX_RETRIES {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if attempt < MAX_RETRIES - 1 => {
                    warn!(
                        "{} attempt {} failed: {}, retrying in {:?}...",
                        operation_name,
                        attempt + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, Duration::from_millis(MAX_RETRY_DELAY_MS));
                }
                Err(e) => {
                    return Err(anyhow!(
                        "{} failed after {} attempts: {}",
                        operation_name,
                        MAX_RETRIES,
                        e
                    ));
                }
            }
        }
        unreachable!()
    }

    /// Convert f32 audio samples to WAV bytes
    fn convert_audio_to_wav(audio: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
            for &sample in audio {
                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                writer.write_sample(sample_i16)?;
            }
            writer.finalize()?;
        }

        Ok(cursor.into_inner())
    }

    /// Upload audio file to Soniox (internal implementation without retry)
    async fn upload_file_impl(&self, audio_data: &[u8]) -> Result<String> {
        let part = multipart::Part::bytes(audio_data.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = multipart::Form::new().part("file", part);

        let response = self
            .client
            .post(format!("{}/files", SONIOX_API_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to upload file: {}", error_text));
        }

        let upload_response: FileUploadResponse = response.json().await?;
        debug!("Uploaded file with id: {}", upload_response.id);
        Ok(upload_response.id)
    }

    /// Upload audio file to Soniox with retry logic
    async fn upload_file(&self, audio_data: Vec<u8>) -> Result<String> {
        Self::with_retry("File upload", || self.upload_file_impl(&audio_data)).await
    }

    /// Create a transcription job (internal implementation without retry)
    async fn create_transcription_impl(
        &self,
        file_id: &str,
        language_hints: Option<Vec<String>>,
    ) -> Result<String> {
        let request = CreateTranscriptionRequest {
            file_id: file_id.to_string(),
            model: self.model.clone(),
            language_hints,
        };

        let response = self
            .client
            .post(format!("{}/transcriptions", SONIOX_API_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to create transcription: {}", error_text));
        }

        let transcription_response: CreateTranscriptionResponse = response.json().await?;
        debug!(
            "Created transcription with id: {}",
            transcription_response.id
        );
        Ok(transcription_response.id)
    }

    /// Create a transcription job with retry logic
    async fn create_transcription(
        &self,
        file_id: String,
        language: Option<&str>,
    ) -> Result<String> {
        let language_hints = language.filter(|l| *l != "auto" && !l.is_empty()).map(|l| {
            // Normalize Chinese variants
            if l == "zh-Hans" || l == "zh-Hant" {
                vec!["zh".to_string()]
            } else {
                vec![l.to_string()]
            }
        });

        Self::with_retry("Create transcription", || {
            self.create_transcription_impl(&file_id, language_hints.clone())
        })
        .await
    }

    /// Poll for transcription completion with exponential backoff
    async fn wait_for_completion(&self, transcription_id: &str) -> Result<()> {
        let timeout = Duration::from_secs(self.timeout_seconds as u64);
        let start_time = std::time::Instant::now();
        let mut poll_interval = Duration::from_millis(500);
        let max_poll_interval = Duration::from_secs(5);
        let mut attempt = 0;

        while start_time.elapsed() < timeout {
            attempt += 1;
            let response = self
                .client
                .get(format!(
                    "{}/transcriptions/{}",
                    SONIOX_API_URL, transcription_id
                ))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(anyhow!(
                    "Failed to check transcription status: {}",
                    error_text
                ));
            }

            let status: TranscriptionStatus = response.json().await?;
            debug!(
                "Transcription status (attempt {}, elapsed {:?}): {}",
                attempt,
                start_time.elapsed(),
                status.status
            );

            match status.status.as_str() {
                "completed" => return Ok(()),
                "error" => return Err(anyhow!("Transcription failed")),
                _ => {
                    tokio::time::sleep(poll_interval).await;
                    // Exponential backoff with cap
                    poll_interval = std::cmp::min(poll_interval * 2, max_poll_interval);
                }
            }
        }

        Err(anyhow!(
            "Transcription timed out after {} seconds",
            self.timeout_seconds
        ))
    }

    /// Get the transcript result (internal implementation without retry)
    async fn get_transcript_impl(&self, transcription_id: &str) -> Result<String> {
        let response = self
            .client
            .get(format!(
                "{}/transcriptions/{}/transcript",
                SONIOX_API_URL, transcription_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get transcript: {}", error_text));
        }

        // Get raw text first for debugging
        let raw_text = response.text().await?;
        info!(
            "Raw transcript response: {}",
            &raw_text[..raw_text.len().min(500)]
        );

        let transcript: TranscriptResponse = serde_json::from_str(&raw_text).map_err(|e| {
            anyhow!(
                "Failed to parse transcript: {} - Response: {}",
                e,
                &raw_text[..raw_text.len().min(200)]
            )
        })?;

        // Use the full text field directly
        Ok(transcript.text.trim().to_string())
    }

    /// Get the transcript result with retry logic
    async fn get_transcript(&self, transcription_id: &str) -> Result<String> {
        let id = transcription_id.to_string();
        Self::with_retry("Get transcript", || self.get_transcript_impl(&id)).await
    }

    /// Delete an uploaded file (single attempt)
    async fn delete_file(&self, file_id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/files/{}", SONIOX_API_URL, file_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to delete file {}: {}",
                file_id,
                response.status()
            ));
        }
        Ok(())
    }

    /// Delete an uploaded file with retry logic for robust cleanup
    async fn delete_file_with_retry(&self, file_id: &str) {
        const CLEANUP_RETRIES: u32 = 3;
        const CLEANUP_DELAY: Duration = Duration::from_secs(1);

        for attempt in 0..CLEANUP_RETRIES {
            match self.delete_file(file_id).await {
                Ok(_) => {
                    debug!("Successfully deleted file {}", file_id);
                    return;
                }
                Err(e) => {
                    warn!(
                        "Failed to delete file {} (attempt {}/{}): {}",
                        file_id,
                        attempt + 1,
                        CLEANUP_RETRIES,
                        e
                    );
                    if attempt < CLEANUP_RETRIES - 1 {
                        tokio::time::sleep(CLEANUP_DELAY).await;
                    }
                }
            }
        }
        log::error!(
            "Failed to delete file {} after {} attempts",
            file_id,
            CLEANUP_RETRIES
        );
    }

    /// Transcribe audio using Soniox async API
    pub async fn transcribe(&self, audio: Vec<f32>, language: Option<&str>) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let start_time = std::time::Instant::now();
        info!(
            "Starting Soniox async transcription with {} samples",
            audio.len()
        );

        // Convert audio to WAV format (16kHz mono)
        let wav_data = Self::convert_audio_to_wav(&audio, 16000)?;
        info!("Converted audio to WAV: {} bytes", wav_data.len());

        // Upload file
        info!("Uploading file to Soniox...");
        let file_id = self.upload_file(wav_data).await?;
        info!("File uploaded with id: {}", file_id);

        // Create transcription
        info!("Creating transcription job...");
        let transcription_id = self.create_transcription(file_id.clone(), language).await?;
        info!("Transcription job created: {}", transcription_id);

        // Wait for completion
        info!("Waiting for transcription to complete...");
        self.wait_for_completion(&transcription_id).await?;
        info!("Transcription completed");

        // Get transcript
        info!("Fetching transcript...");
        let transcript = self.get_transcript(&transcription_id).await?;

        // Cleanup: delete the uploaded file with retry
        self.delete_file_with_retry(&file_id).await;

        let duration = start_time.elapsed();
        info!(
            "Soniox transcription completed in {}ms: '{}'",
            duration.as_millis(),
            transcript
        );

        Ok(transcript)
    }
}
