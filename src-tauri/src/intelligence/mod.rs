//! Shared plumbing for Handy's local intelligence features (voice edit
//! commands, vocabulary mining, voice-driven MCP tool calls). All of them
//! resolve one provider/model pair — separate from the post-processing
//! selection so intelligence can run locally (Ollama by default) while
//! post-processing uses any provider.

pub mod edit;
pub mod intent;
pub mod last_output;
pub mod vocab;

use crate::llm_client;
use crate::settings::{AppSettings, PostProcessProvider};
use log::debug;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum IntelligenceError {
    /// No usable provider/model configured (user action needed).
    NotConfigured(String),
    /// Provider configured but unreachable (e.g. Ollama not running).
    Unavailable(String),
    /// The request itself failed (HTTP error, unparsable response, ...).
    Request(String),
}

impl std::fmt::Display for IntelligenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntelligenceError::NotConfigured(msg) => write!(f, "not configured: {msg}"),
            IntelligenceError::Unavailable(msg) => write!(f, "unavailable: {msg}"),
            IntelligenceError::Request(msg) => write!(f, "request failed: {msg}"),
        }
    }
}

/// Everything needed to make an intelligence-layer LLM call.
#[derive(Debug, Clone)]
pub struct IntelligenceContext {
    pub provider: PostProcessProvider,
    pub api_key: String,
    pub model: String,
}

/// Resolve the intelligence provider/model from settings.
pub fn resolve_context(settings: &AppSettings) -> Result<IntelligenceContext, IntelligenceError> {
    let provider = settings
        .post_process_provider(&settings.intelligence_provider_id)
        .cloned()
        .ok_or_else(|| {
            IntelligenceError::NotConfigured(format!(
                "unknown intelligence provider '{}'",
                settings.intelligence_provider_id
            ))
        })?;

    let model = settings.intelligence_model.trim().to_string();
    if model.is_empty() {
        return Err(IntelligenceError::NotConfigured(
            "no intelligence model selected".to_string(),
        ));
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    Ok(IntelligenceContext {
        provider,
        api_key,
        model,
    })
}

/// Strip markdown code fences some models wrap around JSON output.
fn strip_code_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let without_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    without_open
        .strip_suffix("```")
        .unwrap_or(without_open)
        .trim()
}

/// Run a structured-output completion and parse the JSON result. Reasoning is
/// disabled — intelligence calls are latency-sensitive classification and
/// rewriting tasks, not deliberation.
pub async fn complete_structured(
    ctx: &IntelligenceContext,
    system_prompt: &str,
    user_content: String,
    schema: Value,
) -> Result<Value, IntelligenceError> {
    let response = llm_client::send_chat_completion_with_schema(
        &ctx.provider,
        ctx.api_key.clone(),
        &ctx.model,
        user_content,
        Some(system_prompt.to_string()),
        ctx.provider.supports_structured_output.then_some(schema),
        Some("none".to_string()),
        None,
    )
    .await
    .map_err(classify_request_error)?;

    let content = response.ok_or_else(|| {
        IntelligenceError::Request("model returned an empty response".to_string())
    })?;

    let cleaned = strip_code_fences(&content);
    serde_json::from_str(cleaned).map_err(|e| {
        debug!("Unparsable intelligence response: {content}");
        IntelligenceError::Request(format!("model returned invalid JSON: {e}"))
    })
}

/// Check the provider is reachable and list its models.
pub async fn health_check(ctx: &IntelligenceContext) -> Result<Vec<String>, IntelligenceError> {
    llm_client::fetch_models(&ctx.provider, ctx.api_key.clone())
        .await
        .map_err(classify_request_error)
}

/// Connection-level failures (Ollama not running) surface differently from
/// API-level errors so callers/UI can show "start Ollama" guidance.
fn classify_request_error(message: String) -> IntelligenceError {
    let lowered = message.to_lowercase();
    if lowered.contains("connection refused")
        || lowered.contains("connect error")
        || lowered.contains("error trying to connect")
        || lowered.contains("dns error")
        || lowered.contains("timed out")
    {
        IntelligenceError::Unavailable(message)
    } else {
        IntelligenceError::Request(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_json_code_fences() {
        assert_eq!(strip_code_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn classifies_connection_errors_as_unavailable() {
        assert!(matches!(
            classify_request_error("HTTP request failed: error trying to connect: tcp connect error: Connection refused".into()),
            IntelligenceError::Unavailable(_)
        ));
        assert!(matches!(
            classify_request_error("API request failed with status 404: model not found".into()),
            IntelligenceError::Request(_)
        ));
    }
}
