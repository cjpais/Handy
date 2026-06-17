use crate::settings::PostProcessProvider;
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

fn parse_custom_body(provider: &PostProcessProvider) -> Result<Option<Map<String, Value>>, String> {
    let Some(custom_body) = provider.custom_body.as_deref() else {
        return Ok(None);
    };

    let trimmed = custom_body.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(trimmed).map_err(|e| {
        format!(
            "Invalid custom request body for provider '{}': {}",
            provider.id, e
        )
    })?;

    match value {
        Value::Object(object) => Ok(Some(object)),
        _ => Err(format!(
            "Invalid custom request body for provider '{}': expected a JSON object",
            provider.id
        )),
    }
}

fn build_chat_request_body(
    request_body: ChatCompletionRequest,
    provider: &PostProcessProvider,
) -> Result<Value, String> {
    let mut request_value = serde_json::to_value(request_body)
        .map_err(|e| format!("Failed to serialize chat request body: {}", e))?;

    if let Some(custom_body) = parse_custom_body(provider)? {
        let request_object = request_value
            .as_object_mut()
            .ok_or_else(|| "Chat request body did not serialize to a JSON object".to_string())?;

        for (key, value) in custom_body {
            request_object.insert(key, value);
        }
    }

    Ok(request_value)
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

    let request_body = build_chat_request_body(
        ChatCompletionRequest {
            model: model.to_string(),
            messages,
            response_format,
            reasoning_effort,
            reasoning,
        },
        provider,
    )?;

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
    let base_url = provider.base_url.trim_end_matches('/');
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

    fn test_provider(custom_body: Option<&str>) -> PostProcessProvider {
        PostProcessProvider {
            id: "custom".to_string(),
            label: "Custom".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            allow_base_url_edit: true,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: false,
            custom_body: custom_body.map(str::to_string),
        }
    }

    fn test_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gemma3:1b-it-qat".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "clean this".to_string(),
            }],
            response_format: None,
            reasoning_effort: Some("none".to_string()),
            reasoning: None,
        }
    }

    #[test]
    fn merges_custom_body_fields_into_chat_request() {
        let body = build_chat_request_body(
            test_request(),
            &test_provider(Some(r#"{"keep_alive":"1h","temperature":0}"#)),
        )
        .expect("custom body should merge");

        assert_eq!(body["model"], "gemma3:1b-it-qat");
        assert_eq!(body["keep_alive"], "1h");
        assert_eq!(body["temperature"], 0);
    }

    #[test]
    fn empty_custom_body_is_ignored() {
        let body = build_chat_request_body(test_request(), &test_provider(Some("   ")))
            .expect("empty custom body should be ignored");

        assert!(body.get("keep_alive").is_none());
        assert_eq!(body["reasoning_effort"], "none");
    }

    #[test]
    fn custom_body_must_be_a_json_object() {
        let result = build_chat_request_body(test_request(), &test_provider(Some(r#""1h""#)));

        assert!(result
            .expect_err("non-object custom body should fail")
            .contains("expected a JSON object"));
    }
}
