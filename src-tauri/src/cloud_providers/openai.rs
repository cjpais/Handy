use anyhow::Result;
use log::warn;

use super::{with_retry, CloudProvider, MODEL_ID_CLOUD};
use crate::settings::AppSettings;

pub struct OpenAiProvider;

/// Parse a JSON string into a serde_json::Value object, ignoring invalid input.
fn parse_extra_params(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str(trimmed) {
        Ok(v @ serde_json::Value::Object(_)) => Some(v),
        _ => {
            warn!("cloud_transcription_extra_params is not a valid JSON object, ignoring");
            None
        }
    }
}

/// POST audio to an OpenAI-compatible `/audio/transcriptions` endpoint.
async fn call_cloud_api(
    base_url: &str,
    api_key: &str,
    model_name: &str,
    wav_bytes: Vec<u8>,
    language: Option<&str>,
    extra_params: Option<serde_json::Value>,
) -> Result<String> {
    use reqwest::multipart;

    let file_part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let mut form = multipart::Form::new()
        .part("file", file_part)
        .text("model", model_name.to_string())
        .text("response_format", "json");

    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    // Merge extra_params into form fields — overriding reserved keys is intentional
    if let Some(serde_json::Value::Object(map)) = extra_params {
        for (k, v) in map {
            let val = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            form = form.text(k, val);
        }
    }

    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Network error: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Cloud API {}: {body}", status.as_u16()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {e}"))?;

    json["text"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("No 'text' field in API response"))
}

#[async_trait::async_trait]
impl CloudProvider for OpenAiProvider {
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        _post_process: bool,
        settings: &AppSettings,
    ) -> Result<String> {
        let base_url = settings.cloud_transcription_base_url.clone();
        let api_key = settings.cloud_transcription_api_key.clone();
        let model = settings.cloud_transcription_model.clone();
        let language = match settings.selected_language.as_str() {
            "auto" => None,
            lang => Some(lang.to_string()),
        };
        let extra = parse_extra_params(&settings.cloud_transcription_extra_params);

        with_retry("Cloud", || {
            let base_url = base_url.clone();
            let api_key = api_key.clone();
            let model = model.clone();
            let wav = wav_bytes.clone();
            let lang = language.clone();
            let extra = extra.clone();
            async move {
                call_cloud_api(
                    &base_url,
                    &api_key,
                    &model,
                    wav,
                    lang.as_deref(),
                    extra,
                )
                .await
            }
        })
        .await
    }

    async fn test_connection(&self, settings: &AppSettings) -> Result<()> {
        // Minimal valid 16kHz mono WAV with 0 samples (44-byte header only)
        let silent_wav: Vec<u8> = vec![
            0x52, 0x49, 0x46, 0x46, // "RIFF"
            0x24, 0x00, 0x00, 0x00, // chunk size = 36
            0x57, 0x41, 0x56, 0x45, // "WAVE"
            0x66, 0x6D, 0x74, 0x20, // "fmt "
            0x10, 0x00, 0x00, 0x00, // subchunk1 size = 16
            0x01, 0x00, // PCM
            0x01, 0x00, // 1 channel
            0x80, 0x3E, 0x00, 0x00, // 16000 Hz
            0x00, 0x7D, 0x00, 0x00, // byte rate
            0x02, 0x00, // block align
            0x10, 0x00, // bits per sample = 16
            0x64, 0x61, 0x74, 0x61, // "data"
            0x00, 0x00, 0x00, 0x00, // data size = 0
        ];

        call_cloud_api(
            &settings.cloud_transcription_base_url,
            &settings.cloud_transcription_api_key,
            &settings.cloud_transcription_model,
            silent_wav,
            None,
            None,
        )
        .await?;
        Ok(())
    }

    fn id(&self) -> &'static str {
        MODEL_ID_CLOUD
    }
}
