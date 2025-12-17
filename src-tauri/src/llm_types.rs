use serde::Deserialize;

/// Custom response types for OpenAI-compatible APIs that may have
/// non-standard fields (like Groq's `service_tier: "on_demand"`)

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(skip)]
    pub usage: Option<serde_json::Value>,
    #[serde(skip)]
    pub service_tier: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
    #[serde(skip)]
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
}
