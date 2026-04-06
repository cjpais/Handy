use crate::settings::{
    AppSettings, ELEVENLABS_DEFAULT_MODEL_ID, ELEVENLABS_TRANSCRIPTION_PROVIDER_ID,
};
use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use reqwest::{Error as ReqwestError, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::io::Cursor;
use std::time::Duration;

const ELEVENLABS_SPEECH_TO_TEXT_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const ELEVENLABS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const ELEVENLABS_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const ELEVENLABS_PERMISSION_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const ELEVENLABS_PERMISSION_PROBE_SAMPLES: usize = 1_600;
const MAX_KEYTERMS: usize = 1000;

#[derive(Deserialize)]
struct ElevenLabsTranscriptionResponse {
    text: String,
}

pub fn verify_api_key(api_key: &str) -> Result<()> {
    let api_key = api_key.trim();

    if api_key.is_empty() {
        bail!("Enter your ElevenLabs API key.");
    }

    let wav_bytes = encode_wav(&vec![0.0; ELEVENLABS_PERMISSION_PROBE_SAMPLES])?;
    let audio_part = Part::bytes(wav_bytes)
        .file_name("permission-probe.wav")
        .mime_str("audio/wav")
        .context("Failed to prepare ElevenLabs verification payload")?;

    let form = Form::new()
        .text("model_id", ELEVENLABS_DEFAULT_MODEL_ID.to_string())
        .part("file", audio_part);

    let client = build_client(ELEVENLABS_PERMISSION_PROBE_TIMEOUT)?;
    let response = client
        .post(ELEVENLABS_SPEECH_TO_TEXT_URL)
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .map_err(|error| anyhow::anyhow!(build_request_error_message(&error)))?;

    let status = response.status();
    let body = response
        .text()
        .context("Failed to read ElevenLabs verification response body")?;

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        bail!("{}", build_error_message(status, &body));
    }

    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        bail!("{}", build_error_message(status, &body));
    }

    if matches!(
        status,
        StatusCode::BAD_REQUEST
            | StatusCode::UNSUPPORTED_MEDIA_TYPE
            | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return Ok(());
    }

    if !status.is_success() {
        bail!("{}", build_error_message(status, &body));
    }

    Ok(())
}

pub fn transcribe(audio: &[f32], settings: &AppSettings) -> Result<String> {
    let api_key = settings
        .transcription_api_keys
        .get(ELEVENLABS_TRANSCRIPTION_PROVIDER_ID)
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if api_key.is_empty() {
        bail!(
            "ElevenLabs API key is missing. Add it in Settings > Models > External Speech-to-Text Providers."
        );
    }

    let wav_bytes = encode_wav(audio)?;
    let audio_part = Part::bytes(wav_bytes)
        .file_name("handy-recording.wav")
        .mime_str("audio/wav")
        .context("Failed to prepare ElevenLabs audio payload")?;

    let mut form = Form::new()
        .text("model_id", ELEVENLABS_DEFAULT_MODEL_ID.to_string())
        .part("file", audio_part);

    if let Some(language_code) = normalize_language_code(&settings.selected_language) {
        form = form.text("language_code", language_code);
    }

    for keyterm in collect_keyterms(&settings.custom_words) {
        form = form.text("keyterms", keyterm);
    }

    let client = build_client(ELEVENLABS_REQUEST_TIMEOUT)?;

    let response = client
        .post(ELEVENLABS_SPEECH_TO_TEXT_URL)
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .map_err(|error| anyhow::anyhow!(build_request_error_message(&error)))?;

    let status = response.status();
    let body = response
        .text()
        .context("Failed to read ElevenLabs response body")?;

    if !status.is_success() {
        bail!("{}", build_error_message(status, &body));
    }

    let payload: ElevenLabsTranscriptionResponse =
        serde_json::from_str(&body).context("Failed to parse ElevenLabs response")?;

    Ok(payload.text.trim().to_string())
}

fn build_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .connect_timeout(ELEVENLABS_CONNECT_TIMEOUT)
        .timeout(timeout)
        .build()
        .context("Failed to create ElevenLabs HTTP client")
}

fn encode_wav(audio: &[f32]) -> Result<Vec<u8>> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer =
            WavWriter::new(&mut cursor, spec).context("Failed to create WAV encoder")?;

        for sample in audio {
            let sample = sample.clamp(-1.0, 1.0);
            writer
                .write_sample((sample * i16::MAX as f32) as i16)
                .context("Failed to encode WAV sample")?;
        }

        writer
            .finalize()
            .context("Failed to finalize WAV payload")?;
    }

    Ok(cursor.into_inner())
}

fn normalize_language_code(language: &str) -> Option<String> {
    match language.trim() {
        "" | "auto" => None,
        "zh-Hans" | "zh-Hant" => Some("zh".to_string()),
        "jw" => Some("jv".to_string()),
        language => Some(language.to_string()),
    }
}

fn collect_keyterms(custom_words: &[String]) -> Vec<String> {
    custom_words
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .take(MAX_KEYTERMS)
        .map(ToOwned::to_owned)
        .collect()
}

fn build_error_message(status: StatusCode, body: &str) -> String {
    let detail = extract_provider_detail(body);

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        if is_missing_speech_to_text_permission(body) {
            return "ElevenLabs API key is missing speech-to-text permissions.".to_string();
        }

        if let Some(detail) = detail {
            return format!("ElevenLabs authentication failed: {}", detail);
        }

        return "ElevenLabs authentication failed. Check the configured API key.".to_string();
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        return detail
            .map(|detail| format!("ElevenLabs rate limit exceeded: {}", detail))
            .unwrap_or_else(|| "ElevenLabs rate limit exceeded.".to_string());
    }

    if status.is_server_error() {
        return detail
            .map(|detail| format!("ElevenLabs is currently unavailable: {}", detail))
            .unwrap_or_else(|| {
                "ElevenLabs is currently unavailable. Please try again in a moment.".to_string()
            });
    }

    detail
        .map(|detail| {
            format!(
                "ElevenLabs request failed ({}): {}",
                status.as_u16(),
                detail
            )
        })
        .unwrap_or_else(|| format!("ElevenLabs request failed with status {}.", status.as_u16()))
}

fn build_request_error_message(error: &ReqwestError) -> String {
    if error.is_timeout() {
        return "ElevenLabs request timed out. Please try again.".to_string();
    }

    if error.is_connect() {
        return "Couldn't reach ElevenLabs. Check your internet connection and try again."
            .to_string();
    }

    format!("Failed to reach ElevenLabs speech-to-text API: {}", error)
}

fn extract_provider_detail(body: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(body).ok()?;

    match value.get("detail") {
        Some(Value::String(message)) => Some(message.clone()),
        Some(Value::Object(detail)) => detail
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
    .or_else(|| {
        let trimmed = body.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn is_missing_speech_to_text_permission(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };

    let Some(detail) = value.get("detail").and_then(Value::as_object) else {
        return false;
    };

    let Some(status) = detail.get("status").and_then(Value::as_str) else {
        return false;
    };
    let Some(message) = detail.get("message").and_then(Value::as_str) else {
        return false;
    };

    status == "missing_permissions" && message.contains("speech_to_text")
}

#[cfg(test)]
mod tests {
    use super::{
        build_error_message, collect_keyterms, is_missing_speech_to_text_permission,
        normalize_language_code,
    };
    use reqwest::StatusCode;

    #[test]
    fn language_code_normalization_handles_auto_and_chinese() {
        assert_eq!(normalize_language_code("auto"), None);
        assert_eq!(normalize_language_code("zh-Hans"), Some("zh".to_string()));
        assert_eq!(normalize_language_code("zh-Hant"), Some("zh".to_string()));
        assert_eq!(normalize_language_code("de"), Some("de".to_string()));
    }

    #[test]
    fn collect_keyterms_ignores_blank_entries() {
        let keyterms =
            collect_keyterms(&["alpha".to_string(), "  ".to_string(), "beta".to_string()]);

        assert_eq!(keyterms, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn build_error_message_detects_missing_speech_to_text_permission() {
        let body = r#"{
          "detail": {
            "status": "missing_permissions",
            "message": "missing permission: speech_to_text"
          }
        }"#;

        assert!(is_missing_speech_to_text_permission(body));
        assert_eq!(
            build_error_message(StatusCode::FORBIDDEN, body),
            "ElevenLabs API key is missing speech-to-text permissions."
        );
    }

    #[test]
    fn build_error_message_surfaces_authentication_detail() {
        let body = r#"{"detail":"invalid api key"}"#;

        assert_eq!(
            build_error_message(StatusCode::UNAUTHORIZED, body),
            "ElevenLabs authentication failed: invalid api key"
        );
    }

    #[test]
    fn build_error_message_surfaces_rate_limit_detail() {
        let body = r#"{"detail":"quota exceeded"}"#;

        assert_eq!(
            build_error_message(StatusCode::TOO_MANY_REQUESTS, body),
            "ElevenLabs rate limit exceeded: quota exceeded"
        );
    }

    #[test]
    fn build_error_message_surfaces_provider_unavailability() {
        let body = r#"{"detail":"temporary outage"}"#;

        assert_eq!(
            build_error_message(StatusCode::BAD_GATEWAY, body),
            "ElevenLabs is currently unavailable: temporary outage"
        );
    }
}
