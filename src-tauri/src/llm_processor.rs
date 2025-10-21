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
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    debug!("Starting LLM post-processing with model: {}", model);

    // Replace ${output} variable in the prompt with the actual text
    let processed_prompt = prompt.replace("${output}", &text);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    // Build the request
    let request_body = ChatRequest {
        model: model.clone(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: processed_prompt,
        }],
    };

    // Create HTTP client
    let client = reqwest::Client::new();

    // Build the endpoint URL
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    debug!("Using API endpoint: {}", endpoint);

    // Make the API call
    let response = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/cjpais/Handy")
        .header("X-Title", "Handy")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            let error_msg = format!("Failed to send request to LLM API: {}", e);
            error!("{}", error_msg);
            error_msg
        })?;

    // Check response status
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

    // Parse response
    let response_body: ChatResponse = response.json().await.map_err(|e| {
        let error_msg = format!("Failed to parse LLM API response: {}", e);
        error!("{}", error_msg);
        error_msg
    })?;

    // Extract the processed text
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
