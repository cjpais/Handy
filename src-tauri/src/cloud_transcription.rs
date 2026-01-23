use crate::settings::CloudTranscriptionProvider;
use hound::{SampleFormat, WavSpec, WavWriter};
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::multipart::{Form, Part};
use std::io::Cursor;

const SAMPLE_RATE: u32 = 16000;

fn samples_to_wav(samples: &[f32]) -> Result<Vec<u8>, String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut buffer, spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        for &sample in samples {
            let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer
                .write_sample(sample_i16)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;
    }

    Ok(buffer.into_inner())
}

fn build_headers(api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    if !api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| format!("Invalid authorization header value: {}", e))?,
        );
    }

    Ok(headers)
}

pub async fn transcribe_audio(
    provider: &CloudTranscriptionProvider,
    api_key: String,
    model: &str,
    audio_samples: Vec<f32>,
    language: Option<String>,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err(format!(
            "API key is required for cloud transcription with {}",
            provider.label
        ));
    }

    if audio_samples.is_empty() {
        return Err(
            "No audio was recorded. Please try speaking longer or check your microphone."
                .to_string(),
        );
    }

    debug!(
        "Starting cloud transcription with provider '{}' (model: {}, samples: {})",
        provider.id,
        model,
        audio_samples.len()
    );

    let wav_data = samples_to_wav(&audio_samples)?;
    debug!("Converted audio to WAV format ({} bytes)", wav_data.len());

    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/audio/transcriptions", base_url);
    debug!("Sending transcription request to: {}", url);

    let headers = build_headers(&api_key)?;
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let audio_part = Part::bytes(wav_data)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to create audio part: {}", e))?;

    let mut form = Form::new()
        .part("file", audio_part)
        .text("model", model.to_string())
        .text("response_format", "text")
        .text("temperature", "0");

    if let Some(lang) = language {
        if !lang.is_empty() && lang != "auto" {
            let whisper_lang = match lang.as_str() {
                "zh-Hans" | "zh-Hant" => "zh".to_string(),
                other => other.to_string(),
            };
            form = form.text("language", whisper_lang);
        }
    }

    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(format!(
            "Cloud transcription failed with status {}: {}",
            status, error_text
        ));
    }

    let transcription = response
        .text()
        .await
        .map_err(|e| format!("Failed to read transcription response: {}", e))?;

    debug!(
        "Cloud transcription completed. Output length: {} chars",
        transcription.len()
    );

    Ok(transcription.trim().to_string())
}
