use crate::llm_types::ChatCompletionResponse;
use crate::settings::PostProcessProvider;
use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// LLM client for making chat completion requests to OpenAI-compatible APIs
pub struct LlmClient {
    http_client: Client,
    base_url: String,
    api_key: String,
}

impl LlmClient {
    /// Send a chat completion request and return the response content
    pub async fn chat_completion(
        &self,
        model: &str,
        user_message: &str,
    ) -> Result<String, String> {
        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_message.to_string(),
            }],
        };

        let url = format!("{}/chat/completions", self.base_url);
        
        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API request failed with status {}: {}", status, body));
        }

        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        let parsed: ChatCompletionResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse response: {} - body: {}", e, body))?;

        parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| "No content in response".to_string())
    }
}

/// Create an LLM client configured for the given provider
pub fn create_client(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<LlmClient, String> {
    let base_url = provider.base_url.trim_end_matches('/').to_string();

    let mut headers = reqwest::header::HeaderMap::new();
    
    // Add provider-specific headers
    if provider.id == "anthropic" {
        headers.insert(
            "anthropic-version",
            reqwest::header::HeaderValue::from_static("2023-06-01"),
        );
    }

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    Ok(LlmClient {
        http_client,
        base_url,
        api_key,
    })
}
