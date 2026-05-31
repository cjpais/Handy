use crate::settings::PostProcessProvider;
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

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

#[derive(Debug, Serialize, Clone, Default)]
pub struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
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

/// Create an HTTP client with a custom connection timeout.
/// Used for fallback requests so the user doesn't wait 30+ seconds
/// before trying the additional URL.
fn create_client_with_timeout(
    provider: &PostProcessProvider,
    api_key: &str,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let headers = build_headers(provider, api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(timeout_secs))
        .timeout(Duration::from_secs(timeout_secs.saturating_mul(2)))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// Returns true when the provider has a non-empty additional_url configured,
/// meaning fallback and short timeout behaviour should be used.
fn has_fallback(provider: &PostProcessProvider) -> bool {
    provider
        .additional_url
        .as_ref()
        .map(|url| !url.trim().is_empty())
        .unwrap_or(false)
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    reasoning_effort: Option<String>,
    reasoning: Option<ReasoningConfig>,
) -> Result<Option<String>, String> {
    send_chat_completion_with_schema(
        provider,
        api_key,
        model,
        prompt,
        None,
        None,
        reasoning_effort,
        reasoning,
    )
    .await
}

/// Try sending a chat completion request to a single URL.
/// Returns Ok(Some(content)) on success, Ok(None) on empty response,
/// or Err on connection/timeout errors that may be retried.
async fn try_chat_completion(
    client: &reqwest::Client,
    url: &str,
    request_body: &ChatCompletionRequest,
) -> Result<Option<String>, String> {
    let response = client
        .post(url)
        .json(request_body)
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

/// Send a chat completion request with structured output support
/// When json_schema is provided, uses structured outputs mode
/// system_prompt is used as the system message when provided
/// reasoning_effort sets the OpenAI-style top-level field (e.g., "none", "low", "medium", "high")
/// reasoning sets the OpenRouter-style nested object (effort + exclude)
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
    reasoning_effort: Option<String>,
    reasoning: Option<ReasoningConfig>,
) -> Result<Option<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let primary_url = format!("{}/chat/completions", base_url);

    debug!("Sending chat completion request to: {}", primary_url);

    let fallback_url = if has_fallback(provider) {
        let trimmed = provider
            .additional_url
            .as_ref()
            .unwrap()
            .trim_end_matches('/');
        Some(format!("{}/chat/completions", trimmed))
    } else {
        None
    };

    let client = if fallback_url.is_some() {
        create_client_with_timeout(provider, &api_key, 5)?
    } else {
        create_client(provider, &api_key)?
    };

    let mut messages = Vec::new();
    if let Some(system) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

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
        reasoning_effort,
        reasoning,
    };

    match try_chat_completion(&client, &primary_url, &request_body).await {
        Ok(result) => return Ok(result),
        Err(err) => {
            let is_connection_error = err.starts_with("HTTP request failed:");
            if is_connection_error {
                if let Some(ref url) = fallback_url {
                    let fallback_client =
                        create_client_with_timeout(provider, &api_key, 5)?;
                    debug!("Primary URL unreachable, falling back to: {}", url);
                    return try_chat_completion(&fallback_client, url, &request_body).await;
                }
            }
            Err(err)
        }
    }
}

/// Fetch models from a given URL using the provider's auth headers.
/// Unlike fetch_models(), this does NOT do fallback logic — it hits the given URL directly.
pub async fn fetch_models_from_url(
    provider: &PostProcessProvider,
    api_key: String,
    url: &str,
) -> Result<Vec<String>, String> {
    let trimmed = url.trim_end_matches('/');
    let full_url = format!("{}/models", trimmed);
    debug!("Fetching models from explicit URL: {}", full_url);
    let client = create_client_with_timeout(provider, &api_key, 10)?;
    try_fetch_models(&client, &full_url).await
}

/// Try fetching models from a single URL.
async fn try_fetch_models(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
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
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    } else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let primary_url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", primary_url);

    let fallback_url = if has_fallback(provider) {
        let trimmed = provider
            .additional_url
            .as_ref()
            .unwrap()
            .trim_end_matches('/');
        Some(format!("{}/models", trimmed))
    } else {
        None
    };

    let client = if fallback_url.is_some() {
        create_client_with_timeout(provider, &api_key, 5)?
    } else {
        create_client(provider, &api_key)?
    };

    match try_fetch_models(&client, &primary_url).await {
        Ok(models) => Ok(models),
        Err(err) => {
            let is_connection_error = err.starts_with("Failed to fetch models:");
            if is_connection_error {
                if let Some(ref url) = fallback_url {
                    let fallback_client =
                        create_client_with_timeout(provider, &api_key, 5)?;
                    debug!("Primary models URL unreachable, falling back to: {}", url);
                    return try_fetch_models(&fallback_client, url).await;
                }
            }
            Err(err)
        }
    }
}
