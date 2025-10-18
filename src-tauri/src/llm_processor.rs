use log::{debug, error};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct OpenRouterMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Debug)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
}

#[derive(Deserialize, Debug)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
}

#[derive(Deserialize, Debug)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
}

/// Post-processes text using an LLM via OpenRouter API
/// 
/// # Arguments
/// * `text` - The transcribed text to process
/// * `prompt` - The prompt template (can contain ${output} variable)
/// * `api_key` - OpenRouter API key
/// * `model` - The model to use (e.g., "openai/gpt-4")
/// 
/// # Returns
/// * `Ok(String)` - The processed text
/// * `Err(String)` - Error message, will fallback to original text
pub async fn post_process_with_llm(
    text: String,
    prompt: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    debug!("Starting LLM post-processing with model: {}", model);
    
    // Replace ${output} variable in the prompt with the actual text
    let processed_prompt = prompt.replace("${output}", &text);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    // Build the request
    let request_body = OpenRouterRequest {
        model: model.clone(),
        messages: vec![OpenRouterMessage {
            role: "user".to_string(),
            content: processed_prompt,
        }],
    };

    // Create HTTP client
    let client = reqwest::Client::new();
    
    // Make the API call
    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            let error_msg = format!("Failed to send request to OpenRouter: {}", e);
            error!("{}", error_msg);
            error_msg
        })?;

    // Check response status
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        let error_msg = format!("OpenRouter API error ({}): {}", status, error_text);
        error!("{}", error_msg);
        return Err(error_msg);
    }

    // Parse response
    let response_body: OpenRouterResponse = response.json().await.map_err(|e| {
        let error_msg = format!("Failed to parse OpenRouter response: {}", e);
        error!("{}", error_msg);
        error_msg
    })?;

    // Extract the processed text
    if let Some(choice) = response_body.choices.first() {
        let processed_text = choice.message.content.clone();
        debug!("LLM post-processing completed successfully. Output length: {} chars", processed_text.len());
        Ok(processed_text)
    } else {
        let error_msg = "OpenRouter response has no choices".to_string();
        error!("{}", error_msg);
        Err(error_msg)
    }
}
