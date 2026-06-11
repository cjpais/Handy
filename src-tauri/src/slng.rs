use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{multipart, Client, Response};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct SlngClient {
    client: Client,
    endpoint: String,
    api_key: String,
    language: String,
}

impl SlngClient {
    pub fn new(
        endpoint: String,
        api_key: String,
        language: String,
        timeout_seconds: u64,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()
            .context("Failed to build SLNG HTTP client")?;

        Ok(Self {
            client,
            endpoint: endpoint.trim().to_string(),
            api_key,
            language,
        })
    }

    pub fn transcribe_wav(&self, wav_path: &Path) -> Result<String> {
        if self.endpoint.is_empty() {
            return Err(anyhow!("SLNG endpoint is not configured"));
        }

        let part = multipart::Part::file(wav_path)
            .with_context(|| format!("Failed to read WAV file for SLNG upload: {:?}", wav_path))?
            .file_name("ixiwhisper-recording.wav")
            .mime_str("audio/wav")
            .context("Failed to set SLNG upload MIME type")?;

        let form = multipart::Form::new()
            .part("audio", part)
            .text("language", self.language.clone());

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .context("SLNG transcription request failed")?;

        parse_transcript_response(response)
    }
}

fn parse_transcript_response(response: Response) -> Result<String> {
    let status = response.status();
    let body = response
        .text()
        .context("Failed to read SLNG transcription response")?;

    if !status.is_success() {
        return Err(anyhow!(
            "SLNG transcription failed ({}): {}",
            status,
            preview_body(&body)
        ));
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("SLNG transcription response was empty"));
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => extract_transcript(&value).ok_or_else(|| {
            anyhow!(
                "SLNG response did not include a transcript field: {}",
                preview_body(trimmed)
            )
        }),
        Err(_) => Ok(trimmed.to_string()),
    }
}

fn extract_transcript(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return clean_text(text);
    }

    for pointer in [
        "/text",
        "/transcript",
        "/transcription",
        "/data/text",
        "/data/transcript",
        "/data/transcription",
        "/result/text",
        "/result/transcript",
        "/result/transcription",
        "/results/channels/0/alternatives/0/transcript",
        "/channels/0/alternatives/0/transcript",
        "/alternatives/0/transcript",
    ] {
        if let Some(text) = value.pointer(pointer).and_then(Value::as_str) {
            if let Some(cleaned) = clean_text(text) {
                return Some(cleaned);
            }
        }
    }

    None
}

fn clean_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn preview_body(body: &str) -> String {
    let mut preview = body
        .chars()
        .take(512)
        .collect::<String>()
        .replace(['\n', '\r'], " ");
    if body.chars().count() > 512 {
        preview.push_str("...");
    }
    preview
}
