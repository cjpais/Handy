use crate::audio_toolkit::{apply_custom_words, filter_transcription_output};
use crate::settings::AppSettings;
use hound::{SampleFormat, WavSpec, WavWriter};
use log::debug;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::io::Cursor;

#[derive(Debug, Deserialize)]
struct RemoteWhisperResponse {
    text: String,
}

fn samples_to_wav_bytes(samples: &[f32]) -> Result<Vec<u8>, String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut cursor, spec)
        .map_err(|e| format!("Failed to initialize WAV writer: {}", e))?;

    for sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer
            .write_sample(sample_i16)
            .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV data: {}", e))?;

    Ok(cursor.into_inner())
}

pub async fn transcribe(samples: &[f32], settings: &AppSettings) -> Result<String, String> {
    if samples.is_empty() {
        return Ok(String::new());
    }

    let base_url = settings.remote_whisper_base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Remote Whisper base URL is empty".to_string());
    }

    let model = settings.remote_whisper_model.trim();
    if model.is_empty() {
        return Err("Remote Whisper model is empty".to_string());
    }

    let url = format!("{}/audio/transcriptions", base_url);
    debug!("Sending remote transcription request to: {}", url);

    let wav_bytes = samples_to_wav_bytes(samples)?;
    let file_part = Part::bytes(wav_bytes)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to build multipart audio part: {}", e))?;

    let mut form = Form::new()
        .part("file", file_part)
        .text("model", model.to_string())
        .text("response_format", "json".to_string())
        .text(
            "temperature",
            settings.remote_whisper_temperature.to_string(),
        );

    let prompt = settings.remote_whisper_prompt.trim();
    if !prompt.is_empty() {
        form = form.text("prompt", prompt.to_string());
    }

    let language = settings.remote_whisper_language.trim();
    if !language.is_empty() && language != "auto" {
        form = form.text("language", language.to_string());
    }

    let client = reqwest::Client::new();
    let mut request = client.post(&url).multipart(form);

    let api_key = settings.remote_whisper_api_key.trim();
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Remote transcription HTTP request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(format!(
            "Remote transcription failed with status {}: {}",
            status, error_text
        ));
    }

    let response_body: RemoteWhisperResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse remote transcription response: {}", e))?;

    let corrected_result = if !settings.custom_words.is_empty() {
        apply_custom_words(
            &response_body.text,
            &settings.custom_words,
            settings.word_correction_threshold,
        )
    } else {
        response_body.text
    };

    Ok(filter_transcription_output(&corrected_result))
}
