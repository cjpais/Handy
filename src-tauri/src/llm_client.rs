use crate::settings::PostProcessProvider;
use log::{debug, error, info, warn};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    /// Plain string for most endpoints; OpenRouter gets a content-part array
    /// so the system prompt can carry a `cache_control` breakpoint.
    content: Value,
}

/// Build the system message. On routing gateways the prompt is wrapped as a
/// text part with an ephemeral `cache_control` breakpoint: the prompt is
/// identical across dictations and always sits at the top of the request, so
/// upstreams that need explicit breakpoints (Anthropic) cache it, while
/// implicit-caching upstreams (OpenAI, Gemini, DeepSeek) ignore the marker.
fn system_message(provider: &PostProcessProvider, system: String) -> ChatMessage {
    let content = if provider.supports_provider_routing {
        serde_json::json!([{
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" }
        }])
    } else {
        Value::String(system)
    };
    ChatMessage {
        role: "system".to_string(),
        content,
    }
}

/// Prefix for routing strategies stored in place of a provider slug
/// (`sort:throughput`, `sort:latency`, `sort:price`).
pub const SORT_PREFIX: &str = "sort:";
const SORT_STRATEGIES: [&str; 3] = ["throughput", "latency", "price"];

/// OpenRouter-style provider-routing object
/// (https://openrouter.ai/docs/features/provider-routing).
///
/// * `sort:<strategy>` routes each request to whichever upstream currently
///   leads on throughput (tokens/s), latency (time to first token) or price.
///   `require_parameters` keeps the router on upstreams that accept the fields
///   we send (notably `response_format`).
/// * Any other value is a provider slug pinned with `allow_fallbacks: false`,
///   so the request never silently lands on a different upstream (which would
///   also miss that upstream's prompt cache).
fn provider_routing(provider: &PostProcessProvider, pinned: Option<&str>) -> Option<Value> {
    let value = pinned.map(str::trim).filter(|s| !s.is_empty())?;
    if !provider.supports_provider_routing {
        return None;
    }
    if let Some(strategy) = value.strip_prefix(SORT_PREFIX) {
        if !SORT_STRATEGIES.contains(&strategy) {
            warn!("Ignoring unknown upstream routing strategy '{}'", value);
            return None;
        }
        return Some(serde_json::json!({
            "sort": strategy,
            "require_parameters": true
        }));
    }
    Some(serde_json::json!({
        "order": [value],
        "allow_fallbacks": false
    }))
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

#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

/// Request fields used to ask an endpoint to skip reasoning/thinking.
/// Providers disagree on the field name and accepted values, so at most one of
/// these is set per request (see `reasoning_disable_params`).
#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

impl ReasoningParams {
    fn is_empty(&self) -> bool {
        self.reasoning_effort.is_none() && self.reasoning.is_none() && self.thinking.is_none()
    }
}

/// Pick the reasoning-disable request fields an endpoint understands.
/// Unknown endpoints get the common OpenAI-style field; if they reject it,
/// the request is retried without it (see `send_chat_completion_with_schema`).
fn reasoning_disable_params(provider: &PostProcessProvider) -> ReasoningParams {
    let base_url = provider.base_url.to_lowercase();
    if base_url.contains("api.deepseek.com") {
        // DeepSeek rejects reasoning_effort "none" and uses its own field:
        // https://api-docs.deepseek.com/guides/thinking_mode
        ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
    } else if provider.id == "openrouter" {
        // OpenRouter nested object; exclude:true also keeps reasoning text out
        // of the response so it can't pollute structured-output JSON parsing
        ReasoningParams {
            reasoning: Some(ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
            ..Default::default()
        }
    } else {
        ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
    }
}

/// Endpoints (base_url|model) that rejected the reasoning-disable fields with a
/// 4xx. Remembered for the lifetime of the process so every dictation after the
/// first skips the doomed attempt and goes straight to a plain request.
fn reasoning_rejections() -> &'static Mutex<HashSet<String>> {
    static REJECTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REJECTED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn endpoint_key(provider: &PostProcessProvider, model: &str) -> String {
    format!("{}|{}", provider.base_url.trim_end_matches('/'), model)
}

fn is_known_rejected(key: &str) -> bool {
    reasoning_rejections()
        .lock()
        .map(|set| set.contains(key))
        .unwrap_or(false)
}

fn remember_rejection(key: String) {
    if let Ok(mut set) = reasoning_rejections().lock() {
        set.insert(key);
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(flatten)]
    reasoning: ReasoningParams,
    /// OpenRouter provider routing (https://openrouter.ai/docs/features/provider-routing)
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<Value>,
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
        .map_err(|e| report_reqwest_error("Failed to build HTTP client", &e))
}

/// Format a bounded error source chain.
///
/// `reqwest::Error`'s Display implementation intentionally gives only a short
/// summary. Nested causes contain the useful transport details, such as a
/// certificate validation failure, an HTTP/2 error, or a connection reset.
/// Callers must skip source types whose Display text can quote payload data.
fn error_source_chain(error: &(dyn StdError + 'static)) -> Vec<String> {
    let mut causes = Vec::new();
    let mut source = error.source();

    // Defensive cap in case a third-party error exposes a cyclic source chain.
    for _ in 0..16 {
        let Some(cause) = source else {
            break;
        };
        causes.push(cause.to_string());
        source = cause.source();
    }

    causes
}

fn reqwest_error_kinds(error: &reqwest::Error) -> String {
    let mut kinds = Vec::new();

    if error.is_builder() {
        kinds.push("builder");
    }
    if error.is_connect() {
        kinds.push("connect");
    }
    if error.is_request() {
        kinds.push("request");
    }
    if error.is_redirect() {
        kinds.push("redirect");
    }
    if error.is_timeout() {
        kinds.push("timeout");
    }
    if error.is_status() {
        kinds.push("status");
    }
    if error.is_body() {
        kinds.push("body");
    }
    if error.is_decode() {
        kinds.push("decode");
    }
    if error.is_upgrade() {
        kinds.push("upgrade");
    }

    if kinds.is_empty() {
        "unknown".to_string()
    } else {
        kinds.join(", ")
    }
}

fn sanitized_url(url: &reqwest::Url) -> String {
    let mut url = url.clone();

    // Custom endpoints should not contain credentials or query-string tokens,
    // but omit them from diagnostics in case one does.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    url.to_string()
}

fn sanitized_url_for_log(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|url| sanitized_url(&url))
        // Do not echo an invalid URL: the parse failure might have been caused
        // by sensitive data entered in the custom endpoint field.
        .unwrap_or_else(|_| "<invalid URL>".to_string())
}

fn report_reqwest_error(context: &str, error: &reqwest::Error) -> String {
    let kinds = reqwest_error_kinds(error);
    let url = error
        .url()
        .map(sanitized_url)
        .map(|url| format!(", url: {url}"))
        .unwrap_or_default();

    // serde_json's error text can quote values from a malformed response. That
    // response may contain transcription content, so retain the useful decode
    // classification but never put its nested source in logs or UI errors.
    let causes = if error.is_decode() {
        Vec::new()
    } else {
        error_source_chain(error)
    };
    let cause_details = if !causes.is_empty() {
        format!(": caused by: {}", causes.join(" -> "))
    } else if error.url().is_none() {
        // Reqwest's short Display text is safe when it cannot append a raw URL.
        format!(": {error}")
    } else {
        // The sanitized URL is already included above. Avoid formatting the
        // original error because its Display implementation includes the raw URL.
        String::new()
    };

    let details = format!("{context} (kind: {kinds}{url}){cause_details}");
    error!("{details}");
    details
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    disable_reasoning: bool,
    pinned_provider: Option<&str>,
) -> Result<Option<String>, String> {
    send_chat_completion_with_schema(
        provider,
        api_key,
        model,
        prompt,
        None,
        None,
        disable_reasoning,
        pinned_provider,
    )
    .await
}

/// Send a chat completion request with structured output support.
/// When json_schema is provided, uses structured outputs mode.
/// system_prompt is used as the system message when provided.
///
/// When disable_reasoning is set, the request carries the reasoning-disable
/// fields the endpoint is expected to understand. Not every OpenAI-compatible
/// endpoint accepts them (DeepSeek, Gemini's compat layer, and some OpenRouter
/// upstreams reject with 400), so a 400/422 answer to such a request triggers
/// one retry without the fields, and the rejection is remembered per
/// (base_url, model) so later requests skip the failing attempt entirely.
///
/// `pinned_provider` is an upstream slug (e.g. "google-ai-studio") for
/// providers with `supports_provider_routing`; ignored for every other provider.
#[allow(clippy::too_many_arguments)]
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
    disable_reasoning: bool,
    pinned_provider: Option<&str>,
) -> Result<Option<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base_url);

    debug!(
        "Sending chat completion request to: {}",
        sanitized_url_for_log(&url)
    );

    let client = create_client(provider, &api_key)?;

    // Build messages vector
    let mut messages = Vec::new();

    // Add system prompt if provided
    // The system prompt goes first so the stable prefix is cacheable.
    if let Some(system) = system_prompt {
        messages.push(system_message(provider, system));
    }

    // Add user message
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: Value::String(user_content),
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

    let key = endpoint_key(provider, model);
    let reasoning = if disable_reasoning && !is_known_rejected(&key) {
        reasoning_disable_params(provider)
    } else {
        ReasoningParams::default()
    };

    let mut request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        stream: false,
        response_format,
        reasoning,
        provider: provider_routing(provider, pinned_provider),
    };

    let mut response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;
    let mut status = response.status();
    debug!(
        "Chat completion response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );

    // A 400/422 on a request carrying reasoning-disable fields is almost always
    // the endpoint rejecting those fields — retry once without them.
    if !status.is_success()
        && matches!(status.as_u16(), 400 | 422)
        && !request_body.reasoning.is_empty()
    {
        let error_text = response.text().await.unwrap_or_else(|e| {
            report_reqwest_error("Failed to read reasoning rejection response", &e)
        });
        info!(
            "Endpoint rejected request with reasoning disabled (status {}): {}. Retrying without reasoning fields",
            status, error_text
        );

        request_body.reasoning = ReasoningParams::default();
        response = client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| report_reqwest_error("HTTP retry failed", &e))?;
        status = response.status();
        debug!(
            "Chat completion retry response received with status {} over {:?} from {}",
            status,
            response.version(),
            sanitized_url(response.url())
        );

        if status.is_success() {
            info!(
                "Retry without reasoning fields succeeded; '{}' (model '{}') will skip them from now on",
                sanitized_url_for_log(base_url), model
            );
            remember_rejection(key);
        }
    }

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(format!(
            "API request failed with status {}: {}",
            status, error_text
        ));
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    Ok(completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone()))
}

/// GET `url` with the provider's auth headers and decode the JSON body.
/// `what` names the resource for log and error messages.
async fn get_json(
    provider: &PostProcessProvider,
    api_key: &str,
    url: &str,
    what: &str,
) -> Result<Value, String> {
    debug!("Fetching {} from: {}", what, sanitized_url_for_log(url));

    let client = create_client(provider, api_key)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| report_reqwest_error(&format!("Failed to fetch {what}"), &e))?;

    let status = response.status();
    debug!(
        "{} response received with status {} over {:?} from {}",
        what,
        status,
        response.version(),
        sanitized_url(response.url())
    );
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error(&format!("Failed to read {what} error"), &e));
        return Err(format!(
            "{} request failed ({}): {}",
            what, status, error_text
        ));
    }

    response
        .json()
        .await
        .map_err(|e| report_reqwest_error(&format!("Failed to parse {what} response"), &e))
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models", base_url);
    let parsed = get_json(provider, &api_key, &url, "model list").await?;

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

/// One upstream provider serving a model on OpenRouter.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OpenRouterEndpoint {
    /// Slug accepted by the `provider.order` routing field (e.g. "google-ai-studio").
    pub slug: String,
    /// Human readable provider name (e.g. "Google AI Studio").
    pub name: String,
    pub context_length: Option<f64>,
    /// USD per input token, as reported by OpenRouter.
    pub prompt_price: Option<f64>,
    /// USD per output token.
    pub completion_price: Option<f64>,
    /// USD per cached input token; present when the upstream supports prompt caching.
    pub cache_read_price: Option<f64>,
    /// Upstream caches the prompt prefix without explicit `cache_control`.
    pub supports_implicit_caching: bool,
    /// Accepts `response_format` (structured outputs); without it Handy falls
    /// back to legacy plain-text mode on every request.
    pub supports_structured_output: bool,
    /// Uptime over the last 30 minutes, in percent.
    pub uptime_pct: Option<f64>,
    /// OpenRouter health status; negative means degraded/deranked.
    pub status: Option<f64>,
    /// Mean time to first token over the last 30 minutes, in ms (when reported).
    pub latency_ms: Option<f64>,
    /// Mean generation speed over the last 30 minutes, tokens/s (when reported).
    pub throughput_tps: Option<f64>,
}

fn parse_price(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Fetch the upstream providers that serve `model` on OpenRouter.
/// Uses `GET {base_url}/models/{author}/{slug}/endpoints`.
pub async fn fetch_openrouter_endpoints(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
) -> Result<Vec<OpenRouterEndpoint>, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Select a model first".to_string());
    }
    // Variant suffixes (":free", ":nitro") are routing hints, not part of the
    // model path the endpoints API expects.
    let model_path = model.split(':').next().unwrap_or(model);
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models/{}/endpoints", base_url, model_path);
    let parsed = get_json(provider, &api_key, &url, "endpoint list").await?;

    Ok(parse_openrouter_endpoints(&parsed))
}

fn parse_openrouter_endpoints(parsed: &Value) -> Vec<OpenRouterEndpoint> {
    let Some(entries) = parsed
        .get("data")
        .and_then(|d| d.get("endpoints"))
        .and_then(|e| e.as_array())
    else {
        return Vec::new();
    };

    let mut seen: HashSet<&str> = HashSet::new();
    entries
        .iter()
        .filter_map(|entry| {
            // `tag` is the routing slug; `provider_name` is the display name.
            let slug = entry
                .get("tag")
                .and_then(|t| t.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())?;
            if !seen.insert(slug) {
                return None;
            }
            let name = entry
                .get("provider_name")
                .and_then(|n| n.as_str())
                .unwrap_or(slug)
                .to_string();
            let pricing = entry.get("pricing");
            let number = |key: &str| entry.get(key).and_then(|v| v.as_f64());
            let flag = |key: &str| entry.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
            let supports_structured_output = entry
                .get("supported_parameters")
                .and_then(|p| p.as_array())
                .map(|params| {
                    params.iter().any(|p| {
                        p.as_str() == Some("structured_outputs")
                            || p.as_str() == Some("response_format")
                    })
                })
                .unwrap_or(false);
            Some(OpenRouterEndpoint {
                slug: slug.to_string(),
                name,
                context_length: number("context_length"),
                prompt_price: parse_price(pricing.and_then(|p| p.get("prompt"))),
                completion_price: parse_price(pricing.and_then(|p| p.get("completion"))),
                cache_read_price: parse_price(pricing.and_then(|p| p.get("input_cache_read"))),
                supports_implicit_caching: flag("supports_implicit_caching"),
                supports_structured_output,
                uptime_pct: number("uptime_last_30m"),
                status: number("status"),
                latency_ms: number("latency_last_30m"),
                throughput_tps: number("throughput_last_30m"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl StdError for TestError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn StdError + 'static))
        }
    }

    fn provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            supports_provider_routing: false,
        }
    }

    fn request_json(reasoning: ReasoningParams) -> Value {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Value::String("hi".to_string()),
            }],
            stream: false,
            response_format: None,
            reasoning,
            provider: None,
        };
        serde_json::to_value(&request).unwrap()
    }

    async fn serve_one_response(status: &str, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        format!("http://{address}")
    }

    #[test]
    fn error_source_chain_includes_all_nested_causes() {
        let error = TestError {
            message: "request failed",
            source: Some(Box::new(TestError {
                message: "TLS handshake failed",
                source: Some(Box::new(TestError {
                    message: "unknown certificate authority",
                    source: None,
                })),
            })),
        };

        assert_eq!(
            error_source_chain(&error),
            vec!["TLS handshake failed", "unknown certificate authority"]
        );
    }

    #[test]
    fn log_url_sanitization_removes_credentials_and_tokens() {
        let url = "https://user:password@example.com/v1/models?api_key=secret#private";
        assert_eq!(sanitized_url_for_log(url), "https://example.com/v1/models");
    }

    #[test]
    fn invalid_log_urls_are_not_echoed() {
        assert_eq!(
            sanitized_url_for_log("not a URL containing secret"),
            "<invalid URL>"
        );
    }

    #[tokio::test]
    async fn decode_error_does_not_echo_response_values() {
        let base_url =
            serve_one_response("200 OK", r#"{"choices":"PRIVATE TRANSCRIPTION CONTENT"}"#).await;
        let error = reqwest::get(base_url)
            .await
            .unwrap()
            .json::<ChatCompletionResponse>()
            .await
            .unwrap_err();

        let details = report_reqwest_error("Failed to parse API response", &error);
        assert!(details.contains("kind: decode"));
        assert!(!details.contains("PRIVATE TRANSCRIPTION CONTENT"));
    }

    #[tokio::test]
    async fn raw_error_url_is_not_reintroduced_without_a_source() {
        let base_url = serve_one_response("400 Bad Request", "bad request").await;
        let error = reqwest::get(format!(
            "{base_url}/private?api_key=SECRET_QUERY_TOKEN#private"
        ))
        .await
        .unwrap()
        .error_for_status()
        .unwrap_err();

        let details = report_reqwest_error("Request failed", &error);
        assert!(details.contains(&format!("url: {base_url}/private")));
        assert!(!details.contains("SECRET_QUERY_TOKEN"));
        assert!(!details.contains("#private"));
    }

    #[test]
    fn requests_explicitly_disable_streaming() {
        let json = request_json(ReasoningParams::default());
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn default_reasoning_params_serialize_to_no_fields() {
        let json = request_json(ReasoningParams::default());
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn custom_provider_uses_top_level_reasoning_effort() {
        let params = reasoning_disable_params(&provider("custom", "http://localhost:11434/v1"));
        let json = request_json(params);
        assert_eq!(json["reasoning_effort"], "none");
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    fn routing_provider() -> PostProcessProvider {
        PostProcessProvider {
            supports_provider_routing: true,
            ..provider("openrouter", "https://openrouter.ai/api/v1")
        }
    }

    #[test]
    fn routing_gateway_system_prompt_carries_cache_breakpoint() {
        let msg = system_message(&routing_provider(), "rules".to_string());
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "system");
        assert_eq!(json["content"][0]["text"], "rules");
        assert_eq!(json["content"][0]["cache_control"]["type"], "ephemeral");

        let plain = system_message(
            &provider("openai", "https://api.openai.com/v1"),
            "rules".into(),
        );
        assert_eq!(serde_json::to_value(&plain).unwrap()["content"], "rules");
    }

    #[test]
    fn provider_routing_only_applies_to_routing_gateways_with_a_slug() {
        let or = routing_provider();
        let routing = provider_routing(&or, Some("google-ai-studio")).unwrap();
        assert_eq!(routing["order"][0], "google-ai-studio");
        assert_eq!(routing["allow_fallbacks"], false);
        assert!(provider_routing(&or, Some("  ")).is_none());
        let sorted = provider_routing(&or, Some("sort:throughput")).unwrap();
        assert_eq!(sorted["sort"], "throughput");
        assert_eq!(sorted["require_parameters"], true);
        assert!(sorted.get("order").is_none());
        assert!(provider_routing(&or, Some("sort:bogus")).is_none());
        assert!(provider_routing(&or, None).is_none());
        assert!(provider_routing(&provider("openai", "x"), Some("google-ai-studio")).is_none());
    }

    #[test]
    fn endpoints_response_is_parsed_and_deduped() {
        let body = serde_json::json!({ "data": { "endpoints": [
            { "tag": "google-ai-studio", "provider_name": "Google AI Studio",
              "context_length": 1000000, "uptime_last_30m": 99.5, "status": 0,
              "supports_implicit_caching": true,
              "supported_parameters": ["response_format", "structured_outputs"],
              "pricing": { "prompt": "0.0000003", "completion": "0.0000025", "input_cache_read": "0.000000075" } },
            { "tag": "google-vertex", "provider_name": "Google Vertex", "pricing": { "prompt": "0.0000003" } },
            { "tag": "google-vertex", "provider_name": "dup" },
            { "provider_name": "no tag" }
        ]}});
        let endpoints = parse_openrouter_endpoints(&body);
        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0].slug, "google-ai-studio");
        assert_eq!(endpoints[0].name, "Google AI Studio");
        assert_eq!(endpoints[0].cache_read_price, Some(0.000000075));
        assert_eq!(endpoints[0].context_length, Some(1000000.0));
        assert_eq!(endpoints[1].cache_read_price, None);
        assert!(endpoints[0].supports_structured_output);
        assert!(endpoints[0].supports_implicit_caching);
        assert_eq!(endpoints[0].uptime_pct, Some(99.5));
        assert!(!endpoints[1].supports_structured_output);
    }

    #[test]
    fn openrouter_uses_nested_reasoning_object() {
        let params =
            reasoning_disable_params(&provider("openrouter", "https://openrouter.ai/api/v1"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert_eq!(json["reasoning"]["effort"], "none");
        assert_eq!(json["reasoning"]["exclude"], true);
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn deepseek_base_url_uses_thinking_disabled() {
        let params = reasoning_disable_params(&provider("custom", "https://api.deepseek.com"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert_eq!(json["thinking"]["type"], "disabled");
    }

    #[test]
    fn reasoning_params_is_empty_tracks_all_fields() {
        assert!(ReasoningParams::default().is_empty());
        assert!(!ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
        .is_empty());
        assert!(!ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn rejection_memo_is_keyed_by_base_url_and_model() {
        let deepseek = provider("custom", "https://api.deepseek.com/");
        let key = endpoint_key(&deepseek, "deepseek-chat");
        assert_eq!(key, "https://api.deepseek.com|deepseek-chat");
        assert!(!is_known_rejected(&key));
        remember_rejection(key.clone());
        assert!(is_known_rejected(&key));
        // A different model on the same endpoint is tracked separately
        assert!(!is_known_rejected(&endpoint_key(&deepseek, "other-model")));
    }
}
