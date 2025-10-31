use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Part {
    Text { text: String },
    InlineData { inline_data: InlineData },
}

#[derive(Debug, Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "promptFeedback")]
    prompt_feedback: Option<PromptFeedback>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: ResponseContent,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PromptFeedback {
    #[serde(rename = "blockReason")]
    block_reason: Option<String>,
}

pub struct GeminiClient {
    api_key: String,
    model: String,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    /// Transcribe audio samples (f32 PCM at 16kHz) to text using Gemini API
    pub async fn transcribe_audio(&self, audio_samples: &[f32]) -> Result<String> {
        // Convert f32 samples to i16 PCM (WAV format expects i16)
        let pcm_i16: Vec<i16> = audio_samples
            .iter()
            .map(|&sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();

        // Create WAV file in memory
        let wav_data = self.create_wav_bytes(&pcm_i16, 16000)?;

        // Encode WAV to base64
        let base64_audio = general_purpose::STANDARD.encode(&wav_data);

        // Create Gemini API request
        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::Text {
                        text: "Transcribe the following audio to text. Only return the transcription text without any additional commentary or formatting.".to_string(),
                    },
                    Part::InlineData {
                        inline_data: InlineData {
                            mime_type: "audio/wav".to_string(),
                            data: base64_audio,
                        },
                    },
                ],
            }],
            generation_config: Some(GenerationConfig {
                temperature: 0.0,
                max_output_tokens: Some(8192),
            }),
        };

        // Send request to Gemini API
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gemini API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        let gemini_response: GeminiResponse = response.json().await?;

        // Check for prompt feedback (blocked content)
        if let Some(feedback) = gemini_response.prompt_feedback {
            if let Some(block_reason) = feedback.block_reason {
                return Err(anyhow::anyhow!(
                    "Gemini API blocked the request: {}",
                    block_reason
                ));
            }
        }

        // Extract text from response
        let text = gemini_response
            .candidates
            .and_then(|candidates| candidates.into_iter().next())
            .and_then(|candidate| candidate.content.parts.into_iter().next())
            .map(|part| part.text)
            .ok_or_else(|| anyhow::anyhow!("No transcription text in Gemini response"))?;

        Ok(text.trim().to_string())
    }

    /// Create a WAV file in memory from PCM samples
    fn create_wav_bytes(&self, samples: &[i16], sample_rate: u32) -> Result<Vec<u8>> {
        let mut wav_data = Vec::new();

        // WAV header
        let num_channels: u16 = 1; // Mono
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = (samples.len() * 2) as u32; // 2 bytes per i16 sample
        let file_size = 36 + data_size;

        // RIFF header
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&file_size.to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");

        // fmt subchunk
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat (1 = PCM)
        wav_data.extend_from_slice(&num_channels.to_le_bytes());
        wav_data.extend_from_slice(&sample_rate.to_le_bytes());
        wav_data.extend_from_slice(&byte_rate.to_le_bytes());
        wav_data.extend_from_slice(&block_align.to_le_bytes());
        wav_data.extend_from_slice(&bits_per_sample.to_le_bytes());

        // data subchunk
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&data_size.to_le_bytes());

        // PCM data
        for &sample in samples {
            wav_data.extend_from_slice(&sample.to_le_bytes());
        }

        Ok(wav_data)
    }
}
