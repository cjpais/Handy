use crate::settings::{PostProcessProvider, PostProcessProviderKind};
use log::{debug, error};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Debug)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Serialize, Debug)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Serialize, Debug)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContent>,
}

#[derive(Serialize, Debug)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponseContent {
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicResponseContent>,
}

/// Post-processes text using an LLM via OpenAI-compatible API
///
/// Docs: https://platform.openai.com/docs/api-reference/chat/create
///
/// # Arguments
/// * `text` - The transcribed text to process
/// * `prompt` - The prompt template (can contain ${output} variable)
/// * `base_url` - Base URL for the API (e.g., "https://api.openai.com/v1")
/// * `api_key` - API key for authentication
/// * `model` - The model to use (e.g., "gpt-4", "gpt-3.5-turbo")
///
/// # Returns
/// * `Ok(String)` - The processed text
/// * `Err(String)` - Error message, will fallback to original text
pub async fn post_process_with_llm(
    text: String,
    prompt: String,
    provider: &PostProcessProvider,
    api_key: Option<String>,
    model: String,
) -> Result<String, String> {
    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    // Replace ${output} variable in the prompt with the actual text
    let processed_prompt = prompt.replace("${output}", &text);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    // Create HTTP client
    let client = reqwest::Client::new();
    let base_url = provider.base_url.trim_end_matches('/');

    match provider.kind {
        PostProcessProviderKind::OpenAiCompatible => {
            let request_body = ChatRequest {
                model: model.clone(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: processed_prompt,
                }],
            };

            let endpoint = format!("{}/chat/completions", base_url);
            debug!("Using OpenAI-compatible endpoint: {}", endpoint);

            let mut request = client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://github.com/cjpais/Handy")
                .header("X-Title", "Handy")
                .json(&request_body);

            if let Some(key) = api_key.as_ref().filter(|key| !key.is_empty()) {
                request = request.header("Authorization", format!("Bearer {}", key));
            }

            let response = request.send().await.map_err(|e| {
                let error_msg = format!("Failed to send request to LLM API: {}", e);
                error!("{}", error_msg);
                error_msg
            })?;

            handle_openai_like_response(response).await
        }
        PostProcessProviderKind::Anthropic => {
            let api_key = api_key
                .filter(|key| !key.is_empty())
                .ok_or_else(|| "Anthropic requires an API key".to_string())?;

            let request_body = AnthropicRequest {
                model: model.clone(),
                messages: vec![AnthropicMessage {
                    role: "user".to_string(),
                    content: vec![AnthropicContent {
                        content_type: "text".to_string(),
                        text: processed_prompt,
                    }],
                }],
                max_tokens: 1024,
            };

            let endpoint = format!("{}/messages", base_url);
            debug!("Using Anthropic endpoint: {}", endpoint);

            let response = client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request_body)
                .send()
                .await
                .map_err(|e| {
                    let error_msg = format!("Failed to send request to Anthropic API: {}", e);
                    error!("{}", error_msg);
                    error_msg
                })?;

            let status = response.status();
            if !status.is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                let error_msg = format!("Anthropic API error ({}): {}", status, error_text);
                error!("{}", error_msg);
                return Err(error_msg);
            }

            let response_body: AnthropicResponse = response.json().await.map_err(|e| {
                let error_msg = format!("Failed to parse Anthropic response: {}", e);
                error!("{}", error_msg);
                error_msg
            })?;

            let segments: Vec<String> = response_body
                .content
                .into_iter()
                .filter_map(|item| item.text)
                .collect();

            if segments.is_empty() {
                let error_msg = "Anthropic response did not contain any text segments".to_string();
                error!("{}", error_msg);
                return Err(error_msg);
            }

            let processed_text = segments.join("\n");
            debug!(
                "Anthropic post-processing completed successfully. Output length: {} chars",
                processed_text.len()
            );
            Ok(processed_text)
        }
    }
}

async fn handle_openai_like_response(response: reqwest::Response) -> Result<String, String> {
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        let error_msg = format!("LLM API error ({}): {}", status, error_text);
        error!("{}", error_msg);
        return Err(error_msg);
    }

    let response_body: ChatResponse = response.json().await.map_err(|e| {
        let error_msg = format!("Failed to parse LLM API response: {}", e);
        error!("{}", error_msg);
        error_msg
    })?;

    if let Some(choice) = response_body.choices.first() {
        let processed_text = choice.message.content.clone();
        debug!(
            "LLM post-processing completed successfully. Output length: {} chars",
            processed_text.len()
        );
        Ok(processed_text)
    } else {
        let error_msg = "LLM API response has no choices".to_string();
        error!("{}", error_msg);
        Err(error_msg)
    }
}
