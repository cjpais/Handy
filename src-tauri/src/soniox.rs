use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use reqwest::blocking::{multipart, Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct SonioxClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    timeout: Duration,
}

impl SonioxClient {
    pub fn new(
        base_url: String,
        api_key: String,
        model: String,
        timeout_seconds: u64,
    ) -> Result<Self> {
        let timeout = Duration::from_secs(timeout_seconds.max(1));
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("Failed to build Soniox HTTP client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            timeout,
        })
    }

    pub fn transcribe_wav(&self, wav_path: &Path) -> Result<String> {
        let mut file_id: Option<String> = None;
        let mut transcription_id: Option<String> = None;

        let result = (|| -> Result<String> {
            let uploaded_file = self.upload_file(wav_path)?;
            file_id = Some(uploaded_file.id.clone());

            let transcription = self.create_transcription(&uploaded_file.id)?;
            transcription_id = Some(transcription.id.clone());

            self.wait_until_completed(&transcription.id)?;
            let transcript = self.get_transcript(&transcription.id)?;
            Ok(transcript.text)
        })();

        if let Some(id) = transcription_id {
            if let Err(error) = self.delete_transcription(&id) {
                warn!("Failed to delete Soniox transcription '{}': {}", id, error);
            }
        }

        if let Some(id) = file_id {
            if let Err(error) = self.delete_file(&id) {
                debug!("Failed to delete Soniox file '{}': {}", id, error);
            }
        }

        result
    }

    fn upload_file(&self, wav_path: &Path) -> Result<UploadedFile> {
        let part = multipart::Part::file(wav_path)
            .with_context(|| format!("Failed to read WAV file for Soniox upload: {:?}", wav_path))?
            .file_name("ixiwhisper-recording.wav")
            .mime_str("audio/wav")
            .context("Failed to set Soniox upload MIME type")?;

        let form = multipart::Form::new().part("file", part);
        let response = self
            .auth(self.client.post(self.endpoint("/v1/files")))
            .multipart(form)
            .send()
            .context("Soniox file upload request failed")?;

        parse_json_response(response, "Soniox file upload")
    }

    fn create_transcription(&self, file_id: &str) -> Result<TranscriptionStatus> {
        let request = CreateTranscriptionRequest {
            model: &self.model,
            file_id,
            language_hints: &["en"],
            language_hints_strict: true,
            enable_language_identification: false,
        };
        let response = self
            .auth(self.client.post(self.endpoint("/v1/transcriptions")))
            .json(&request)
            .send()
            .context("Soniox transcription creation request failed")?;

        parse_json_response(response, "Soniox transcription creation")
    }

    fn wait_until_completed(&self, transcription_id: &str) -> Result<()> {
        let deadline = Instant::now() + self.timeout;

        loop {
            let status = self.get_transcription(transcription_id)?;
            match status.status.as_str() {
                "completed" => return Ok(()),
                "error" | "failed" => {
                    return Err(anyhow!(
                        "Soniox transcription failed: {}",
                        status
                            .error_message
                            .as_deref()
                            .or(status.error_type.as_deref())
                            .unwrap_or("unknown error")
                    ));
                }
                "queued" | "processing" | "downloading" | "transcribing" => {
                    if Instant::now() >= deadline {
                        return Err(anyhow!(
                            "Soniox transcription timed out after {} seconds",
                            self.timeout.as_secs()
                        ));
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                other => {
                    return Err(anyhow!("Soniox returned unexpected status '{}'", other));
                }
            }
        }
    }

    fn get_transcription(&self, transcription_id: &str) -> Result<TranscriptionStatus> {
        let response = self
            .auth(
                self.client
                    .get(self.endpoint(&format!("/v1/transcriptions/{transcription_id}"))),
            )
            .send()
            .context("Soniox transcription status request failed")?;

        parse_json_response(response, "Soniox transcription status")
    }

    fn get_transcript(&self, transcription_id: &str) -> Result<Transcript> {
        let response = self
            .auth(
                self.client.get(
                    self.endpoint(&format!("/v1/transcriptions/{transcription_id}/transcript")),
                ),
            )
            .send()
            .context("Soniox transcript request failed")?;

        parse_json_response(response, "Soniox transcript")
    }

    fn delete_transcription(&self, transcription_id: &str) -> Result<()> {
        let response = self
            .auth(
                self.client
                    .delete(self.endpoint(&format!("/v1/transcriptions/{transcription_id}"))),
            )
            .send()
            .context("Soniox transcription delete request failed")?;

        parse_empty_response(response, "Soniox transcription delete")
    }

    fn delete_file(&self, file_id: &str) -> Result<()> {
        let response = self
            .auth(
                self.client
                    .delete(self.endpoint(&format!("/v1/files/{file_id}"))),
            )
            .send()
            .context("Soniox file delete request failed")?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }

        parse_empty_response(response, "Soniox file delete")
    }

    fn auth(&self, request: RequestBuilder) -> RequestBuilder {
        request.bearer_auth(&self.api_key)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: Response,
    context: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(anyhow!("{context} failed ({status}): {body}"));
    }

    response
        .json::<T>()
        .with_context(|| format!("Failed to parse {context} response"))
}

fn parse_empty_response(response: Response, context: &str) -> Result<()> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(anyhow!("{context} failed ({status}): {body}"));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct UploadedFile {
    id: String,
}

#[derive(Debug, Serialize)]
struct CreateTranscriptionRequest<'a> {
    model: &'a str,
    file_id: &'a str,
    language_hints: &'a [&'a str],
    language_hints_strict: bool,
    enable_language_identification: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionStatus {
    id: String,
    status: String,
    error_type: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Transcript {
    text: String,
}
