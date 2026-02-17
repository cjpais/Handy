use crate::settings::{PostProcessProvider, CUSTOM_LLM_BASE_URL_ENV};
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;

/// Get the effective base URL for a provider.
/// For the "custom" provider, checks the environment variable first.
/// This is called fresh on each invocation to pick up runtime changes.
fn get_effective_base_url(provider: &PostProcessProvider) -> String {
    if provider.id == "custom" {
        // Check environment variable for custom provider override
        if let Ok(env_url) = env::var(CUSTOM_LLM_BASE_URL_ENV) {
            let trimmed = env_url.trim();
            if !trimmed.is_empty() {
                debug!(
                    "Using base URL from environment variable {}: {}",
                    CUSTOM_LLM_BASE_URL_ENV, trimmed
                );
                return trimmed.trim_end_matches('/').to_string();
            }
        }
    }
    provider.base_url.trim_end_matches('/').to_string()
}
use serde_json::Value;

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct JsonSchema {
    name: String,
    strict: bool,
    schema: Value,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    json_schema: JsonSchema,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}

/// Build headers for API requests based on provider type
fn build_headers(provider: &PostProcessProvider, api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    // Common headers
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://github.com/cjpais/Handy"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Handy/1.0 (+https://github.com/cjpais/Handy)"),
    );
    headers.insert("X-Title", HeaderValue::from_static("Handy"));

    // Provider-specific auth headers
    if !api_key.is_empty() {
        if provider.id == "anthropic" {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key)
                    .map_err(|e| format!("Invalid API key header value: {}", e))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| format!("Invalid authorization header value: {}", e))?,
            );
        }
    }

    Ok(headers)
}

/// Create an HTTP client with provider-specific headers
fn create_client(provider: &PostProcessProvider, api_key: &str) -> Result<reqwest::Client, String> {
    let headers = build_headers(provider, api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
) -> Result<Option<String>, String> {
    // Get effective base URL (checks env var for custom provider on each call)
    let base_url = get_effective_base_url(provider);
    send_chat_completion_with_schema(provider, api_key, model, prompt, None, None).await
}

/// Send a chat completion request with structured output support
/// When json_schema is provided, uses structured outputs mode
/// system_prompt is used as the system message when provided
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
) -> Result<Option<String>, String> {
    let base_url = get_effective_base_url(provider);
    let url = format!("{}/chat/completions", base_url);

    debug!("Sending chat completion request to: {}", url);

    let client = create_client(provider, &api_key)?;

    // Build messages vector
    let mut messages = Vec::new();

    // Add system prompt if provided
    if let Some(system) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }

    // Add user message
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    // Build response_format if schema is provided
    let response_format = json_schema.map(|schema| ResponseFormat {
        format_type: "json_schema".to_string(),
        json_schema: JsonSchema {
            name: "transcription_output".to_string(),
            strict: true,
            schema,
        },
    });

    let request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        response_format,
    };

    let response = client
        .post(&url)
        .json(&request_body)
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
            "API request failed with status {}: {}",
            status, error_text
        ));
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse API response: {}", e))?;

    Ok(completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone()))
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    // Get effective base URL (checks env var for custom provider on each call)
    let base_url = get_effective_base_url(provider);
    let url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", url);

    let client = create_client(provider, &api_key)?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch models: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!(
            "Model list request failed ({}): {}",
            status, error_text
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let mut models = Vec::new();

    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PostProcessProvider;

    #[test]
    fn test_get_effective_base_url_custom_provider_with_env() {
        // Set environment variable
        std::env::set_var(CUSTOM_LLM_BASE_URL_ENV, "http://custom-server:8080/v1");

        let provider = PostProcessProvider {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
        };

        let result = get_effective_base_url(&provider);
        assert_eq!(result, "http://custom-server:8080/v1");

        // Clean up
        std::env::remove_var(CUSTOM_LLM_BASE_URL_ENV);
    }

    #[test]
    fn test_get_effective_base_url_custom_provider_without_env() {
        // Ensure env var is not set
        std::env::remove_var(CUSTOM_LLM_BASE_URL_ENV);

        let provider = PostProcessProvider {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
        };

        let result = get_effective_base_url(&provider);
        assert_eq!(result, "http://localhost:11434/v1");
    }

    #[test]
    fn test_get_effective_base_url_custom_provider_with_empty_env() {
        // Set empty environment variable
        std::env::set_var(CUSTOM_LLM_BASE_URL_ENV, "  ");

        let provider = PostProcessProvider {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
        };

        let result = get_effective_base_url(&provider);
        assert_eq!(result, "http://localhost:11434/v1");

        // Clean up
        std::env::remove_var(CUSTOM_LLM_BASE_URL_ENV);
    }

    #[test]
    fn test_get_effective_base_url_non_custom_provider() {
        // Set environment variable (should be ignored for non-custom provider)
        std::env::set_var(CUSTOM_LLM_BASE_URL_ENV, "http://custom-server:8080/v1");

        let provider = PostProcessProvider {
            id: "openai".to_string(),
            label: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
        };

        let result = get_effective_base_url(&provider);
        assert_eq!(result, "https://api.openai.com/v1");

        // Clean up
        std::env::remove_var(CUSTOM_LLM_BASE_URL_ENV);
    }

    #[test]
    fn test_get_effective_base_url_strips_trailing_slash() {
        // Set environment variable with trailing slash
        std::env::set_var(CUSTOM_LLM_BASE_URL_ENV, "http://custom-server:8080/v1/");

        let provider = PostProcessProvider {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            base_url: "http://localhost:11434/v1/".to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
        };

        let result = get_effective_base_url(&provider);
        assert_eq!(result, "http://custom-server:8080/v1");

        // Clean up
        std::env::remove_var(CUSTOM_LLM_BASE_URL_ENV);
    }
}
