use anyhow::Result;
use base64::Engine as _;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::{with_retry, CloudProvider, MODEL_ID_GEMINI};
use crate::settings::{AppSettings, GEMINI_PROMPT_ID};

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ---- Request types ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    contents: Vec<Content>,
}

// ---- Response types ----

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: ContentResponse,
}

#[derive(Debug, Deserialize)]
struct ContentResponse {
    parts: Vec<PartResponse>,
}

#[derive(Debug, Deserialize)]
struct PartResponse {
    text: Option<String>,
}

pub struct GeminiProvider;

/// Strip the `${output}` placeholder from a prompt template to produce a system instruction.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template
        .replace("${output}", "")
        .trim()
        .to_string()
}

/// Call Gemini generateContent API with audio bytes.
async fn call_gemini_api(
    api_key: &str,
    model: &str,
    wav_bytes: Vec<u8>,
    prompt: Option<String>,
) -> Result<String> {
    let url = format!("{}/{}:generateContent", GEMINI_BASE_URL, model);

    info!(
        "Gemini API call: model={}, audio_bytes={}, has_prompt={}",
        model,
        wav_bytes.len(),
        prompt.is_some()
    );

    let audio_data = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

    let system_instruction = prompt.map(|p| SystemInstruction {
        parts: vec![Part {
            text: Some(p),
            inline_data: None,
        }],
    });

    let request = GenerateContentRequest {
        system_instruction,
        contents: vec![Content {
            parts: vec![
                Part {
                    text: Some(
                        "Please transcribe this audio file. Provide only the transcribed text, with no introductory phrases, labels, or formatting.".to_string(),
                    ),
                    inline_data: None,
                },
                Part {
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "audio/wav".to_string(),
                        data: audio_data,
                    }),
                },
            ],
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-goog-api-key", api_key)
        .json(&request)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Gemini API request failed: {}", e))?;

    let status = response.status();
    info!("Gemini API response status: {}", status);
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        warn!("Gemini API error body: {}", &body[..body.len().min(500)]);
        return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
    }

    let raw_body = response
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read Gemini response: {}", e))?;

    let parsed: GenerateContentResponse = serde_json::from_str(&raw_body).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse Gemini response: {}. Body: {}",
            e,
            &raw_body[..raw_body.len().min(300)]
        )
    })?;

    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .and_then(|p| p.text)
        .unwrap_or_default();

    info!("Gemini API returned {} chars", text.len());
    Ok(text)
}

#[async_trait::async_trait]
impl CloudProvider for GeminiProvider {
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        post_process: bool,
        settings: &AppSettings,
    ) -> Result<String> {
        let api_key = settings.gemini_api_key.clone();
        let model = settings.gemini_model.clone();

        // When post_process=true, use the Gemini prompt as system_instruction
        let prompt = if post_process {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == GEMINI_PROMPT_ID)
                .map(|p| build_system_prompt(&p.prompt))
                .filter(|p| !p.is_empty())
        } else {
            None
        };

        with_retry("Gemini", || {
            let api_key = api_key.clone();
            let model = model.clone();
            let wav = wav_bytes.clone();
            let prompt = prompt.clone();
            async move { call_gemini_api(&api_key, &model, wav, prompt).await }
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

        call_gemini_api(&settings.gemini_api_key, &settings.gemini_model, silent_wav, None).await?;
        Ok(())
    }

    fn id(&self) -> &'static str {
        MODEL_ID_GEMINI
    }
}
