use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use hound::{WavSpec, WavWriter};
use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Serialize, Debug)]
struct ChatMessage {
    role: String,
    content: Vec<MessageContent>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MessageContent {
    Text { text: String },
    InputAudio { input_audio: AudioData },
}

#[derive(Serialize, Debug)]
struct AudioData {
    data: String,
    format: String,
}

#[derive(Serialize, Debug)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize, Debug)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize, Debug)]
struct ResponseMessage {
    content: Option<String>,
}

/// Converts PCM audio samples to base64-encoded WAV format
fn samples_to_base64_wav(samples: Vec<f32>) -> Result<String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec)?;

        for sample in samples {
            let amplitude = i16::MAX as f32;
            let sample_i16 = (sample * amplitude) as i16;
            writer.write_sample(sample_i16)?;
        }

        writer.finalize()?;
    }

    let wav_bytes = cursor.into_inner();
    let base64_audio = STANDARD.encode(&wav_bytes);

    Ok(base64_audio)
}

/// Transcribes audio using the Gemini API (OpenAI-compatible endpoint)
pub async fn transcribe_with_api(
    audio: Vec<f32>,
    api_key: &str,
    api_endpoint: &str,
    api_model: &str,
    language: Option<String>,
) -> Result<String> {
    if audio.is_empty() {
        return Ok(String::new());
    }

    if api_key.is_empty() {
        return Err(anyhow!("API key is not configured"));
    }

    debug!(
        "Starting API transcription with model: {} at endpoint: {}",
        api_model, api_endpoint
    );

    // Convert audio to base64 WAV
    let base64_audio = samples_to_base64_wav(audio)?;

    // Build the transcription prompt based on language setting
    let prompt = match language {
        Some(lang) if lang != "auto" && !lang.is_empty() => {
            format!("Transcribe this audio in {}. Return only the transcribed text without any additional commentary.", lang)
        }
        _ => {
            "Transcribe this audio. Return only the transcribed text without any additional commentary.".to_string()
        }
    };

    debug!("Using transcription prompt: {}", prompt);

    // Prepare the request
    let request = ChatCompletionRequest {
        model: api_model.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: vec![
                MessageContent::Text {
                    text: prompt,
                },
                MessageContent::InputAudio {
                    input_audio: AudioData {
                        data: base64_audio,
                        format: "wav".to_string(),
                    },
                },
            ],
        }],
    };

    // Make the API call
    let client = reqwest::Client::new();
    let url = format!("{}chat/completions", api_endpoint);

    debug!("Making API request to: {}", url);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send API request: {}", e);
            anyhow!("Failed to send API request: {}", e)
        })?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        error!("API request failed with status {}: {}", status, response_text);
        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            response_text
        ));
    }

    debug!("API response: {}", response_text);

    // Parse the response
    let completion: ChatCompletionResponse = serde_json::from_str(&response_text)
        .map_err(|e| anyhow!("Failed to parse API response: {}", e))?;

    let transcription = completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .ok_or_else(|| anyhow!("No transcription in API response"))?;

    debug!("Transcription successful: {}", transcription);

    Ok(transcription.trim().to_string())
}
