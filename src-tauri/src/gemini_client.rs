use base64::Engine as _;
use log::debug;
use serde::{Deserialize, Serialize};

const GEMINI_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models";

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

/// Call Gemini generateContent API with audio bytes.
/// - `prompt`: if Some, used as system_instruction (transcribe_with_post_process mode)
/// - `prompt`: if None, plain transcription (basic transcribe mode)
pub async fn call_gemini_api(
    api_key: &str,
    model: &str,
    wav_bytes: Vec<u8>,
    prompt: Option<String>,
) -> anyhow::Result<String> {
    let url = format!("{}/{}:generateContent", GEMINI_BASE_URL, model);

    debug!("Calling Gemini API: model={}", model);

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
        .timeout(std::time::Duration::from_secs(60))
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
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
    }

    let parsed: GenerateContentResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Gemini response: {}", e))?;

    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .and_then(|p| p.text)
        .unwrap_or_default();

    debug!("Gemini API returned {} chars", text.len());
    Ok(text)
}

/// Send a minimal test request to verify API key + model.
/// Uses a tiny silent WAV (44-byte header, 0 samples).
pub async fn test_gemini_connection(api_key: &str, model: &str) -> anyhow::Result<()> {
    // Minimal valid 16kHz mono WAV with 0 samples (44-byte header only)
    let silent_wav: Vec<u8> = vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0x24, 0x00, 0x00, 0x00, // chunk size = 36
        0x57, 0x41, 0x56, 0x45, // "WAVE"
        0x66, 0x6D, 0x74, 0x20, // "fmt "
        0x10, 0x00, 0x00, 0x00, // subchunk1 size = 16
        0x01, 0x00,             // PCM
        0x01, 0x00,             // 1 channel
        0x80, 0x3E, 0x00, 0x00, // 16000 Hz
        0x00, 0x7D, 0x00, 0x00, // byte rate
        0x02, 0x00,             // block align
        0x10, 0x00,             // bits per sample = 16
        0x64, 0x61, 0x74, 0x61, // "data"
        0x00, 0x00, 0x00, 0x00, // data size = 0
    ];

    call_gemini_api(api_key, model, silent_wav, None).await?;
    Ok(())
}
