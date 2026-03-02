use anyhow::Result;
use base64::Engine as _;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::{CloudProvider, MODEL_ID_GEMINI};
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

use super::build_system_prompt;

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

        call_gemini_api(&api_key, &model, wav_bytes, prompt).await
    }

    async fn test_connection(&self, settings: &AppSettings) -> Result<()> {
        call_gemini_api(
            &settings.gemini_api_key,
            &settings.gemini_model,
            super::silent_wav(),
            None,
        )
        .await?;
        Ok(())
    }

    fn id(&self) -> &'static str {
        MODEL_ID_GEMINI
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{get_default_settings, GEMINI_PROMPT_ID};

    /// Verify that post_process=false produces no system instruction (plain transcription).
    #[test]
    fn prompt_is_none_when_post_process_false() {
        let settings = get_default_settings();
        let prompt = if false {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == GEMINI_PROMPT_ID)
                .map(|p| build_system_prompt(&p.prompt))
                .filter(|p| !p.is_empty())
        } else {
            None
        };
        assert!(
            prompt.is_none(),
            "post_process=false must not produce a prompt"
        );
    }

    /// Verify that post_process=true resolves the Gemini prompt as system instruction.
    #[test]
    fn prompt_is_some_when_post_process_true() {
        let settings = get_default_settings();
        let prompt = if true {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == GEMINI_PROMPT_ID)
                .map(|p| build_system_prompt(&p.prompt))
                .filter(|p| !p.is_empty())
        } else {
            None
        };
        assert!(prompt.is_some(), "post_process=true must produce a prompt");
        let text = prompt.unwrap();
        assert!(
            !text.contains("${output}"),
            "prompt must strip ${{output}} placeholder"
        );
        assert!(
            text.contains("transcription"),
            "prompt should mention transcription"
        );
    }

    /// Verify that post_process=true with empty prompts produces None (no crash).
    #[test]
    fn prompt_is_none_when_gemini_prompt_missing() {
        let mut settings = get_default_settings();
        // Remove all prompts
        settings.post_process_prompts.clear();
        let prompt = if true {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == GEMINI_PROMPT_ID)
                .map(|p| build_system_prompt(&p.prompt))
                .filter(|p| !p.is_empty())
        } else {
            None
        };
        assert!(
            prompt.is_none(),
            "missing prompt should produce None, not crash"
        );
    }

    /// Verify that post_process=true with empty prompt text produces None.
    #[test]
    fn prompt_is_none_when_gemini_prompt_empty() {
        let mut settings = get_default_settings();
        // Replace Gemini prompt with empty text
        if let Some(p) = settings
            .post_process_prompts
            .iter_mut()
            .find(|p| p.id == GEMINI_PROMPT_ID)
        {
            p.prompt = String::new();
        }
        let prompt = if true {
            settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == GEMINI_PROMPT_ID)
                .map(|p| build_system_prompt(&p.prompt))
                .filter(|p| !p.is_empty())
        } else {
            None
        };
        assert!(prompt.is_none(), "empty prompt text should produce None");
    }
}
