use anyhow::Result;
use base64::Engine;
use hound::{WavSpec, WavWriter};
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Serialize)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Serialize)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<Content>,
}

#[derive(Deserialize)]
struct GenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: Option<String>,
}

fn encode_samples_to_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut buffer);
        let mut writer = WavWriter::new(cursor, spec)?;
        for sample in samples {
            let sample_i16 = (sample * i16::MAX as f32) as i16;
            writer.write_sample(sample_i16)?;
        }
        writer.finalize()?;
    }

    Ok(buffer)
}

pub async fn transcribe_audio(api_key: &str, model: &str, audio_samples: &[f32]) -> Result<String> {
    let wav_bytes = encode_samples_to_wav(audio_samples)?;
    let audio_base64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

    debug!(
        "Gemini transcribe: {} samples, {} bytes WAV, {} bytes base64",
        audio_samples.len(),
        wav_bytes.len(),
        audio_base64.len()
    );

    let url = format!("{}{}:generateContent", GEMINI_API_BASE, model);

    let request = GenerateContentRequest {
        contents: vec![Content {
            parts: vec![
                Part {
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "audio/wav".to_string(),
                        data: audio_base64,
                    }),
                },
                Part {
                    text: Some(
                        "Transcribe this audio. Return only the transcript text, nothing else."
                            .to_string(),
                    ),
                    inline_data: None,
                },
            ],
        }],
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "x-goog-api-key",
        HeaderValue::from_str(api_key)
            .map_err(|e| anyhow::anyhow!("Invalid API key: {}", e))?,
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .headers(headers)
        .json(&request)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Gemini API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(anyhow::anyhow!(
            "Gemini API error ({}): {}",
            status,
            error_text
        ));
    }

    let resp: GenerateContentResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Gemini response: {}", e))?;

    let text = resp
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .and_then(|p| p.into_iter().next())
        .and_then(|p| p.text)
        .unwrap_or_default();

    debug!("Gemini transcription result: {}", text);
    Ok(text.trim().to_string())
}
