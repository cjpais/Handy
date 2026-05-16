use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration as ChronoDuration, Local, NaiveDate, NaiveDateTime, TimeZone};
use rand::RngCore;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use specta::Type;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_store::StoreExt;

const STORE_PATH: &str = "agent_connections_store.json";
const USER_AGENT: &str = "unburdn-agent/0.1";
const TOOL_HTTP_TIMEOUT_SECS: u64 = 20;
const NOTION_API_VERSION: &str = "2026-03-11";

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentConnectionKind {
    RemoteMcp,
    GoogleApi,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectionStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: AgentConnectionKind,
    pub connected: bool,
    pub requires_env: Vec<String>,
    pub missing_env: Vec<String>,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotionTableValidation {
    pub data_source_id: String,
}

#[derive(Clone, Debug)]
struct ProviderConfig {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    kind: AgentConnectionKind,
    mcp_url: Option<&'static str>,
    scopes: &'static [&'static str],
    required_env: &'static [&'static str],
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct ConnectionStore {
    connections: HashMap<String, StoredConnection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredConnection {
    provider_id: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    token_endpoint: String,
    client_id: String,
    client_secret: Option<String>,
    mcp_url: Option<String>,
    scopes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ProtectedResourceMetadata {
    authorization_servers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct OAuthMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ClientRegistrationResponse {
    client_id: String,
    client_secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            id: "notion",
            name: "Notion",
            description: "Read and write workspace content through Notion MCP.",
            kind: AgentConnectionKind::RemoteMcp,
            mcp_url: Some("https://mcp.notion.com/mcp"),
            scopes: &[],
            required_env: &[],
        },
        ProviderConfig {
            id: "granola",
            name: "Granola",
            description: "Search meeting notes and transcripts through Granola MCP.",
            kind: AgentConnectionKind::RemoteMcp,
            mcp_url: Some("https://mcp.granola.ai/mcp"),
            scopes: &[],
            required_env: &[],
        },
        ProviderConfig {
            id: "gmail",
            name: "Gmail",
            description: "Search messages and create drafts using the Gmail API.",
            kind: AgentConnectionKind::GoogleApi,
            mcp_url: None,
            scopes: &[
                "https://www.googleapis.com/auth/gmail.readonly",
                "https://www.googleapis.com/auth/gmail.compose",
            ],
            required_env: &["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET"],
        },
        ProviderConfig {
            id: "google_calendar",
            name: "Google Calendar",
            description: "Check availability and create events using Google Calendar.",
            kind: AgentConnectionKind::GoogleApi,
            mcp_url: None,
            scopes: &["https://www.googleapis.com/auth/calendar"],
            required_env: &["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET"],
        },
    ]
}

fn provider_by_id(id: &str) -> Result<ProviderConfig, String> {
    providers()
        .into_iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| format!("Unknown agent connection provider: {}", id))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random_urlsafe(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

fn connection_store(app: &AppHandle) -> Result<ConnectionStore, String> {
    let store = app
        .store(crate::portable::store_path(STORE_PATH))
        .map_err(|error| format!("Failed to open agent connection store: {}", error))?;

    Ok(store
        .get("connections")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default())
}

fn write_connection_store(app: &AppHandle, connections: ConnectionStore) -> Result<(), String> {
    let store = app
        .store(crate::portable::store_path(STORE_PATH))
        .map_err(|error| format!("Failed to open agent connection store: {}", error))?;

    store.set(
        "connections",
        serde_json::to_value(connections)
            .map_err(|error| format!("Failed to serialize agent connections: {}", error))?,
    );
    Ok(())
}

fn wait_for_callback(listener: TcpListener, expected_state: String) -> Result<String, String> {
    let (mut stream, _) = listener
        .accept()
        .map_err(|error| format!("OAuth callback was not received: {}", error))?;
    let mut buffer = [0_u8; 8192];
    let bytes_read = stream
        .read(&mut buffer)
        .map_err(|error| format!("Failed to read OAuth callback: {}", error))?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| "OAuth callback request was empty".to_string())?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "OAuth callback request was malformed".to_string())?;
    let callback_url = format!("http://127.0.0.1{}", path);
    let parsed = Url::parse(&callback_url)
        .map_err(|error| format!("Failed to parse OAuth callback URL: {}", error))?;
    let callback: OAuthCallback = serde_urlencoded::from_str(parsed.query().unwrap_or_default())
        .map_err(|error| format!("Failed to parse OAuth callback parameters: {}", error))?;

    let body = if callback.error.is_some() {
        "unburdn. could not connect this account. You can close this tab."
    } else {
        "unburdn. connected this account. You can close this tab."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());

    if let Some(error) = callback.error {
        return Err(format!(
            "OAuth authorization failed: {} {}",
            error,
            callback.error_description.unwrap_or_default()
        ));
    }

    if callback.state.as_deref() != Some(expected_state.as_str()) {
        return Err("OAuth callback state did not match".to_string());
    }

    callback
        .code
        .ok_or_else(|| "OAuth callback did not include an authorization code".to_string())
}

fn callback_listener() -> Result<(TcpListener, String), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("Failed to start OAuth callback listener: {}", error))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("Failed to read OAuth callback listener address: {}", error))?
        .port();
    Ok((
        listener,
        format!("http://127.0.0.1:{}/oauth/callback", port),
    ))
}

async fn discover_oauth_metadata(mcp_url: &str) -> Result<OAuthMetadata, String> {
    let client = reqwest::Client::new();
    let parsed = Url::parse(mcp_url).map_err(|error| format!("Invalid MCP URL: {}", error))?;
    let origin = parsed
        .origin()
        .ascii_serialization()
        .trim_end_matches('/')
        .to_string();
    let path_base = mcp_url.trim_end_matches('/');
    let candidates = [
        format!("{}/.well-known/oauth-protected-resource", path_base),
        format!("{}/.well-known/oauth-protected-resource", origin),
    ];

    let mut last_error = String::new();
    for candidate in candidates {
        match client
            .get(&candidate)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                let protected_resource: ProtectedResourceMetadata = response
                    .json()
                    .await
                    .map_err(|error| format!("Invalid protected resource metadata: {}", error))?;
                let auth_server = protected_resource
                    .authorization_servers
                    .first()
                    .ok_or_else(|| {
                        "MCP server did not advertise an authorization server".to_string()
                    })?;
                let metadata_url = format!(
                    "{}/.well-known/oauth-authorization-server",
                    auth_server.trim_end_matches('/')
                );
                let metadata = client
                    .get(metadata_url)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await
                    .map_err(|error| format!("Failed to fetch OAuth metadata: {}", error))?;
                if !metadata.status().is_success() {
                    return Err(format!(
                        "OAuth metadata request failed: {}",
                        metadata.status()
                    ));
                }
                return metadata
                    .json()
                    .await
                    .map_err(|error| format!("Invalid OAuth metadata: {}", error));
            }
            Ok(response) => {
                last_error = format!("{} returned {}", candidate, response.status());
            }
            Err(error) => {
                last_error = format!("{} failed: {}", candidate, error);
            }
        }
    }

    Err(format!(
        "Failed to discover OAuth metadata for MCP server: {}",
        last_error
    ))
}

async fn register_mcp_client(
    metadata: &OAuthMetadata,
    redirect_uri: &str,
) -> Result<ClientRegistrationResponse, String> {
    let endpoint = metadata.registration_endpoint.as_ref().ok_or_else(|| {
        "MCP authorization server does not support dynamic client registration".to_string()
    })?;
    let response = reqwest::Client::new()
        .post(endpoint)
        .header("User-Agent", USER_AGENT)
        .json(&json!({
            "client_name": "unburdn. Voice Agent",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await
        .map_err(|error| format!("Dynamic client registration failed: {}", error))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Dynamic client registration failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }

    response
        .json()
        .await
        .map_err(|error| format!("Invalid client registration response: {}", error))
}

fn google_client_credentials(app: &AppHandle) -> Result<(String, String), String> {
    let client_id = crate::agent_config::get_config_value(app, "GOOGLE_OAUTH_CLIENT_ID")
        .ok_or_else(|| "GOOGLE_OAUTH_CLIENT_ID is required for Google connections".to_string())?;
    let client_secret = crate::agent_config::get_config_value(app, "GOOGLE_OAUTH_CLIENT_SECRET")
        .ok_or_else(|| {
            "GOOGLE_OAUTH_CLIENT_SECRET is required for Google connections".to_string()
        })?;
    Ok((client_id, client_secret))
}

fn tool_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(TOOL_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|error| format!("Failed to build tool HTTP client: {}", error))
}

fn build_auth_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
    scopes: &[&str],
    resource: Option<&str>,
) -> Result<String, String> {
    let mut url = Url::parse(authorization_endpoint)
        .map_err(|error| format!("Invalid authorization endpoint: {}", error))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", &code_challenge(verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("prompt", "consent");

        if !scopes.is_empty() {
            pairs.append_pair("scope", &scopes.join(" "));
        }

        if let Some(resource) = resource {
            pairs.append_pair("resource", resource);
        }
    }
    Ok(url.to_string())
}

async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code: &str,
    verifier: &str,
    resource: Option<&str>,
) -> Result<TokenResponse, String> {
    let mut params = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("client_id", client_id.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", verifier.to_string()),
    ];

    if let Some(secret) = client_secret {
        params.push(("client_secret", secret.to_string()));
    }

    if let Some(resource) = resource {
        params.push(("resource", resource.to_string()));
    }

    let response = reqwest::Client::new()
        .post(token_endpoint)
        .header("User-Agent", USER_AGENT)
        .form(&params)
        .send()
        .await
        .map_err(|error| format!("Token exchange failed: {}", error))?;
    let status = response.status();

    if !status.is_success() {
        return Err(format!(
            "Token exchange failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }

    response
        .json()
        .await
        .map_err(|error| format!("Invalid token response: {}", error))
}

async fn refresh_connection(
    provider_id: &str,
    connection: &mut StoredConnection,
) -> Result<(), String> {
    if connection
        .expires_at
        .map(|expires_at| expires_at > now_secs() + 60)
        .unwrap_or(true)
    {
        return Ok(());
    }

    let Some(refresh_token) = connection.refresh_token.clone() else {
        return Err(format!("{} needs to be reconnected", provider_id));
    };

    let mut params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token),
        ("client_id", connection.client_id.clone()),
    ];

    if let Some(secret) = &connection.client_secret {
        params.push(("client_secret", secret.clone()));
    }

    if let Some(resource) = &connection.mcp_url {
        params.push(("resource", resource.clone()));
    }

    let response = reqwest::Client::new()
        .post(&connection.token_endpoint)
        .header("User-Agent", USER_AGENT)
        .form(&params)
        .send()
        .await
        .map_err(|error| format!("Failed to refresh {} token: {}", provider_id, error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Failed to refresh {} token with {}: {}",
            provider_id,
            status,
            response.text().await.unwrap_or_default()
        ));
    }

    let token: TokenResponse = response
        .json()
        .await
        .map_err(|error| format!("Invalid refresh token response: {}", error))?;
    connection.access_token = token.access_token;
    if token.refresh_token.is_some() {
        connection.refresh_token = token.refresh_token;
    }
    connection.expires_at = token.expires_in.map(|expires_in| now_secs() + expires_in);
    Ok(())
}

async fn connect_remote_mcp(app: &AppHandle, provider: &ProviderConfig) -> Result<(), String> {
    let mcp_url = provider.mcp_url.expect("remote MCP provider missing URL");
    let metadata = discover_oauth_metadata(mcp_url).await?;
    let (listener, redirect_uri) = callback_listener()?;
    let registration = register_mcp_client(&metadata, &redirect_uri).await?;
    let verifier = random_urlsafe(32);
    let state = random_urlsafe(32);
    let auth_url = build_auth_url(
        &metadata.authorization_endpoint,
        &registration.client_id,
        &redirect_uri,
        &state,
        &verifier,
        provider.scopes,
        Some(mcp_url),
    )?;
    let state_for_callback = state.clone();
    let callback_task = tauri::async_runtime::spawn_blocking(move || {
        wait_for_callback(listener, state_for_callback)
    });

    app.opener()
        .open_url(auth_url, None::<String>)
        .map_err(|error| format!("Failed to open OAuth URL: {}", error))?;

    let code = callback_task
        .await
        .map_err(|error| format!("OAuth callback task failed: {}", error))??;
    let token = exchange_code(
        &metadata.token_endpoint,
        &registration.client_id,
        registration.client_secret.as_deref(),
        &redirect_uri,
        &code,
        &verifier,
        Some(mcp_url),
    )
    .await?;

    let mut store = connection_store(app)?;
    store.connections.insert(
        provider.id.to_string(),
        StoredConnection {
            provider_id: provider.id.to_string(),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_in.map(|expires_in| now_secs() + expires_in),
            token_endpoint: metadata.token_endpoint,
            client_id: registration.client_id,
            client_secret: registration.client_secret,
            mcp_url: Some(mcp_url.to_string()),
            scopes: provider
                .scopes
                .iter()
                .map(|scope| scope.to_string())
                .collect(),
        },
    );
    write_connection_store(app, store)
}

async fn connect_google(app: &AppHandle, provider: &ProviderConfig) -> Result<(), String> {
    let (client_id, client_secret) = google_client_credentials(app)?;
    let (listener, redirect_uri) = callback_listener()?;
    let verifier = random_urlsafe(32);
    let state = random_urlsafe(32);
    let auth_url = build_auth_url(
        "https://accounts.google.com/o/oauth2/v2/auth",
        &client_id,
        &redirect_uri,
        &state,
        &verifier,
        provider.scopes,
        None,
    )?;
    let mut auth_url =
        Url::parse(&auth_url).map_err(|error| format!("Invalid Google auth URL: {}", error))?;
    auth_url
        .query_pairs_mut()
        .append_pair("access_type", "offline");

    let state_for_callback = state.clone();
    let callback_task = tauri::async_runtime::spawn_blocking(move || {
        wait_for_callback(listener, state_for_callback)
    });

    app.opener()
        .open_url(auth_url.to_string(), None::<String>)
        .map_err(|error| format!("Failed to open OAuth URL: {}", error))?;

    let code = callback_task
        .await
        .map_err(|error| format!("OAuth callback task failed: {}", error))??;
    let token = exchange_code(
        "https://oauth2.googleapis.com/token",
        &client_id,
        Some(&client_secret),
        &redirect_uri,
        &code,
        &verifier,
        None,
    )
    .await?;

    let mut store = connection_store(app)?;
    store.connections.insert(
        provider.id.to_string(),
        StoredConnection {
            provider_id: provider.id.to_string(),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_in.map(|expires_in| now_secs() + expires_in),
            token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
            client_id,
            client_secret: Some(client_secret),
            mcp_url: None,
            scopes: provider
                .scopes
                .iter()
                .map(|scope| scope.to_string())
                .collect(),
        },
    );
    write_connection_store(app, store)
}

async fn stored_connection(app: &AppHandle, provider_id: &str) -> Result<StoredConnection, String> {
    let mut store = connection_store(app)?;
    let mut connection = store
        .connections
        .get(provider_id)
        .cloned()
        .ok_or_else(|| format!("{} is not connected", provider_id))?;
    refresh_connection(provider_id, &mut connection).await?;
    store
        .connections
        .insert(provider_id.to_string(), connection.clone());
    write_connection_store(app, store)?;
    Ok(connection)
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_connections(app: AppHandle) -> Result<Vec<AgentConnectionStatus>, String> {
    let store = connection_store(&app)?;
    Ok(providers()
        .into_iter()
        .map(|provider| {
            let missing_env = provider
                .required_env
                .iter()
                .filter(|name| crate::agent_config::get_config_value(&app, name).is_none())
                .map(|name| name.to_string())
                .collect::<Vec<_>>();

            AgentConnectionStatus {
                id: provider.id.to_string(),
                name: provider.name.to_string(),
                description: provider.description.to_string(),
                kind: provider.kind.clone(),
                connected: store.connections.contains_key(provider.id),
                requires_env: provider
                    .required_env
                    .iter()
                    .map(|name| name.to_string())
                    .collect(),
                missing_env,
                scopes: provider
                    .scopes
                    .iter()
                    .map(|scope| scope.to_string())
                    .collect(),
            }
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn connect_agent_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<AgentConnectionStatus>, String> {
    let provider = provider_by_id(&provider_id)?;
    match provider.kind {
        AgentConnectionKind::RemoteMcp => connect_remote_mcp(&app, &provider).await?,
        AgentConnectionKind::GoogleApi => connect_google(&app, &provider).await?,
    }
    get_agent_connections(app)
}

#[tauri::command]
#[specta::specta]
pub fn disconnect_agent_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<AgentConnectionStatus>, String> {
    let mut store = connection_store(&app)?;
    store.connections.remove(&provider_id);
    write_connection_store(&app, store)?;
    get_agent_connections(app)
}

async fn gmail_search(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    let connection = stored_connection(app, "gmail").await?;
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("in:inbox newer_than:7d");
    let max_results = arguments
        .get("maxResults")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .min(10);

    let client = reqwest::Client::new();
    let list_response = client
        .get("https://gmail.googleapis.com/gmail/v1/users/me/messages")
        .bearer_auth(&connection.access_token)
        .query(&[("q", query), ("maxResults", &max_results.to_string())])
        .send()
        .await
        .map_err(|error| format!("Gmail search failed: {}", error))?;
    let status = list_response.status();
    if !status.is_success() {
        return Err(format!(
            "Gmail search failed with {}: {}",
            status,
            list_response.text().await.unwrap_or_default()
        ));
    }
    let list_json: Value = list_response
        .json()
        .await
        .map_err(|error| format!("Invalid Gmail search response: {}", error))?;
    let messages = list_json
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for message in messages {
        let Some(id) = message.get("id").and_then(Value::as_str) else {
            continue;
        };
        let message_response = client
            .get(format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}",
                id
            ))
            .bearer_auth(&connection.access_token)
            .query(&[("format", "metadata"), ("metadataHeaders", "Subject")])
            .send()
            .await
            .map_err(|error| format!("Failed to fetch Gmail message metadata: {}", error))?;
        if !message_response.status().is_success() {
            continue;
        }
        let message_json: Value = message_response.json().await.unwrap_or_else(|_| json!({}));
        let subject = message_json
            .pointer("/payload/headers")
            .and_then(Value::as_array)
            .and_then(|headers| {
                headers.iter().find_map(|header| {
                    if header.get("name").and_then(Value::as_str) == Some("Subject") {
                        header.get("value").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or("(No subject)");
        results.push(json!({
            "id": id,
            "subject": subject,
            "snippet": message_json.get("snippet").and_then(Value::as_str).unwrap_or("")
        }));
    }

    Ok(json!({ "query": query, "messages": results }))
}

fn header_value(arguments: &Value, key: &str) -> String {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string()
}

async fn gmail_create_draft(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    let connection = stored_connection(app, "gmail").await?;
    let to = header_value(arguments, "to");
    let cc = header_value(arguments, "cc");
    let subject = header_value(arguments, "subject");
    let body = arguments
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();

    if to.is_empty() {
        return Err("Draft recipient is required".to_string());
    }
    if subject.is_empty() {
        return Err("Draft subject is required".to_string());
    }
    if body.is_empty() {
        return Err("Draft body is required".to_string());
    }

    let mut message = format!(
        "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nMIME-Version: 1.0\r\n",
        to, subject
    );
    if !cc.is_empty() {
        message.push_str(&format!("Cc: {}\r\n", cc));
    }
    message.push_str("\r\n");
    message.push_str(body);

    let raw = URL_SAFE_NO_PAD.encode(message.as_bytes());
    let response = reqwest::Client::new()
        .post("https://gmail.googleapis.com/gmail/v1/users/me/drafts")
        .bearer_auth(&connection.access_token)
        .json(&json!({
            "message": {
                "raw": raw
            }
        }))
        .send()
        .await
        .map_err(|error| format!("Gmail draft creation failed: {}", error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Gmail draft creation failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }

    let draft: Value = response
        .json()
        .await
        .map_err(|error| format!("Invalid Gmail draft response: {}", error))?;

    Ok(json!({
        "created": true,
        "draftId": draft.get("id").and_then(Value::as_str).unwrap_or_default(),
        "messageId": draft.pointer("/message/id").and_then(Value::as_str).unwrap_or_default(),
        "to": to,
        "cc": cc,
        "subject": subject,
        "sent": false,
        "source": "gmail"
    }))
}

async fn calendar_check_availability(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    log::info!(
        "Agent tool calendar_check_availability started: {}",
        arguments
    );
    let connection = stored_connection(app, "google_calendar").await?;
    let date = arguments
        .get("date")
        .and_then(Value::as_str)
        .ok_or_else(|| "date is required".to_string())?;
    let duration_minutes = arguments
        .get("durationMinutes")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    let start_time = arguments.get("time").and_then(Value::as_str);
    let (time_min, time_max) = if let Some(start_time) = start_time {
        let start =
            NaiveDateTime::parse_from_str(&format!("{} {}", date, start_time), "%Y-%m-%d %H:%M")
                .map_err(|error| format!("Expected date YYYY-MM-DD and time HH:MM: {}", error))?;
        let start = Local
            .from_local_datetime(&start)
            .single()
            .ok_or_else(|| "Could not resolve requested local calendar time".to_string())?;
        let end = start + ChronoDuration::minutes(duration_minutes as i64);
        (start.to_rfc3339(), end.to_rfc3339())
    } else {
        local_day_bounds(date)?
    };

    let response = tool_http_client()?
        .post("https://www.googleapis.com/calendar/v3/freeBusy")
        .bearer_auth(&connection.access_token)
        .json(&json!({
            "timeMin": time_min,
            "timeMax": time_max,
            "items": [{ "id": "primary" }]
        }))
        .send()
        .await
        .map_err(|error| format!("Calendar availability check failed: {}", error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Calendar availability check failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("Invalid Calendar response: {}", error))?;
    let busy = body
        .pointer("/calendars/primary/busy")
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    log::info!(
        "Agent tool calendar_check_availability finished: date={}, busy={}",
        date,
        busy
    );

    Ok(json!({
        "date": date,
        "time": start_time,
        "durationMinutes": duration_minutes,
        "available": !busy,
        "busy": body.pointer("/calendars/primary/busy").cloned().unwrap_or_else(|| json!([])),
        "source": "google_calendar"
    }))
}

fn local_day_bounds(date: &str) -> Result<(String, String), String> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| format!("Expected date YYYY-MM-DD: {}", error))?;
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "Could not build calendar day start".to_string())?;
    let end = (date + ChronoDuration::days(1))
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "Could not build calendar day end".to_string())?;
    let start = Local
        .from_local_datetime(&start)
        .single()
        .ok_or_else(|| "Could not resolve calendar day start".to_string())?;
    let end = Local
        .from_local_datetime(&end)
        .single()
        .ok_or_else(|| "Could not resolve calendar day end".to_string())?;
    Ok((start.to_rfc3339(), end.to_rfc3339()))
}

async fn calendar_list_events(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    log::info!("Agent tool calendar_list_events started: {}", arguments);
    let connection = stored_connection(app, "google_calendar").await?;
    let date = arguments
        .get("date")
        .and_then(Value::as_str)
        .ok_or_else(|| "date is required".to_string())?;
    let (time_min, time_max) = local_day_bounds(date)?;

    let response = tool_http_client()?
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(&connection.access_token)
        .query(&[
            ("timeMin", time_min.as_str()),
            ("timeMax", time_max.as_str()),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
        ])
        .send()
        .await
        .map_err(|error| format!("Calendar event list failed: {}", error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Calendar event list failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("Invalid Calendar events response: {}", error))?;
    let events = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|event| {
            json!({
                "id": event.get("id").and_then(Value::as_str).unwrap_or_default(),
                "summary": event.get("summary").and_then(Value::as_str).unwrap_or("(No title)"),
                "start": event.pointer("/start/dateTime").or_else(|| event.pointer("/start/date")).cloned().unwrap_or(Value::Null),
                "end": event.pointer("/end/dateTime").or_else(|| event.pointer("/end/date")).cloned().unwrap_or(Value::Null),
                "status": event.get("status").and_then(Value::as_str).unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();

    let event_count = events.len();
    log::info!(
        "Agent tool calendar_list_events finished: date={}, count={}",
        date,
        event_count
    );

    Ok(json!({
        "date": date,
        "events": events,
        "count": event_count,
        "source": "google_calendar"
    }))
}

fn json_rpc(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

fn parse_mcp_response(text: &str) -> Result<Value, String> {
    if let Ok(json) = serde_json::from_str::<Value>(text) {
        return Ok(json);
    }

    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<Value>(data) {
            if json.get("id").is_some() {
                return Ok(json);
            }
        }
    }

    Err("MCP server returned an unrecognized response".to_string())
}

async fn mcp_post(
    client: &reqwest::Client,
    connection: &StoredConnection,
    body: Value,
    session_id: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let mcp_url = connection
        .mcp_url
        .as_ref()
        .ok_or_else(|| "Connection is not a remote MCP provider".to_string())?;
    let mut request = client
        .post(mcp_url)
        .bearer_auth(&connection.access_token)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&body);

    if let Some(session_id) = session_id {
        request = request.header("MCP-Session-Id", session_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("MCP request failed: {}", error))?;
    let status = response.status();
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("MCP request failed with {}: {}", status, text));
    }

    Ok((parse_mcp_response(&text)?, session))
}

async fn initialize_mcp_session(
    client: &reqwest::Client,
    connection: &StoredConnection,
) -> Result<Option<String>, String> {
    let (_, session_id) = mcp_post(
        client,
        connection,
        json_rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {
                    "name": "unburdn. Voice Agent",
                    "version": "0.1.0"
                }
            }),
        ),
        None,
    )
    .await?;

    let _ = mcp_post(
        client,
        connection,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        session_id.as_deref(),
    )
    .await;

    Ok(session_id)
}

async fn call_mcp_tool(
    app: &AppHandle,
    provider_id: &str,
    tool_name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let connection = stored_connection(app, provider_id).await?;
    let client = reqwest::Client::new();
    let session_id = initialize_mcp_session(&client, &connection).await?;
    let (response, _) = mcp_post(
        &client,
        &connection,
        json_rpc(
            3,
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": arguments
            }),
        ),
        session_id.as_deref(),
    )
    .await?;

    if let Some(error) = response.get("error") {
        return Err(format!("MCP tool call failed: {}", error));
    }

    let result = response.get("result").cloned().unwrap_or(response);
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let message = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("MCP tool returned an error");
        return Err(message.to_string());
    }

    Ok(result)
}

async fn notion_search(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    call_mcp_tool(app, "notion", "notion-search", arguments.clone()).await
}

fn is_broad_task_query(query: &str) -> bool {
    let normalized = query
        .trim()
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "all"
                | "all tasks"
                | "all task"
                | "my tasks"
                | "my task"
                | "tasks"
                | "task"
                | "task list"
                | "todo"
                | "todos"
                | "to dos"
                | "to do"
                | "follow ups"
                | "followups"
                | "open tasks"
                | "open task"
                | "all my tasks"
                | "all of my tasks"
                | "done tasks"
                | "done task"
                | "completed tasks"
                | "complete tasks"
                | "tasks with stage to do"
                | "tasks with status to do"
                | "tasks with stage todo"
                | "tasks with status todo"
                | "tasks with stage done"
                | "tasks with status done"
                | "tasks with stage in progress"
                | "tasks with status in progress"
        )
}

fn task_search_queries(query: &str, owner_name: &str, skip_owner_filter: bool) -> Vec<String> {
    let query = query.trim();
    if !is_broad_task_query(query) {
        return vec![query.to_string()];
    }

    let mut queries = Vec::new();
    queries.push("task".to_string());

    if skip_owner_filter {
    } else {
        queries.push(owner_name.to_string());
        for term in owner_name
            .split_whitespace()
            .map(str::trim)
            .filter(|term| term.len() >= 3)
        {
            queries.push(term.to_string());
        }
        queries.push(format!("{} task", owner_name));
    }

    let mut deduped = Vec::new();
    for query in queries {
        if !deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&query))
        {
            deduped.push(query);
        }
    }
    deduped
}

#[derive(Clone, Debug)]
enum TaskStageFilter {
    Active,
    Any,
    Exact(String),
}

impl TaskStageFilter {
    fn label(&self) -> String {
        match self {
            TaskStageFilter::Active => "active".to_string(),
            TaskStageFilter::Any => "all".to_string(),
            TaskStageFilter::Exact(stage) => stage.clone(),
        }
    }
}

fn normalize_task_stage(value: &str) -> String {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_task_stage(value: &str) -> String {
    match normalize_task_stage(value).as_str() {
        "todo" | "to do" => "to do".to_string(),
        "complete" | "completed" => "done".to_string(),
        "cancelled" => "canceled".to_string(),
        stage => stage.to_string(),
    }
}

fn task_stage_filter_from_value(value: &str) -> TaskStageFilter {
    match canonical_task_stage(value).as_str() {
        "all" | "any" | "all stages" | "all statuses" | "include done" | "including done" => {
            TaskStageFilter::Any
        }
        "active" | "open" | "not done" | "incomplete" => TaskStageFilter::Active,
        "to do" => TaskStageFilter::Exact("To Do".to_string()),
        "in progress" => TaskStageFilter::Exact("In Progress".to_string()),
        "done" => TaskStageFilter::Exact("Done".to_string()),
        "canceled" => TaskStageFilter::Exact("Canceled".to_string()),
        stage => TaskStageFilter::Exact(stage.to_string()),
    }
}

fn task_stage_filter(arguments: &Value, query: &str) -> TaskStageFilter {
    if let Some(stage) = arguments
        .get("stage")
        .or_else(|| arguments.get("status"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return task_stage_filter_from_value(stage);
    }

    let normalized_query = normalize_task_stage(query);
    if normalized_query.contains("all stages")
        || normalized_query.contains("all statuses")
        || normalized_query.contains("include done")
        || normalized_query.contains("including done")
    {
        return TaskStageFilter::Any;
    }
    if normalized_query.contains("done")
        || normalized_query.contains("completed")
        || normalized_query.contains("complete tasks")
    {
        return TaskStageFilter::Exact("Done".to_string());
    }
    if normalized_query.contains("stage to do")
        || normalized_query.contains("status to do")
        || normalized_query.contains("stage todo")
        || normalized_query.contains("status todo")
    {
        return TaskStageFilter::Exact("To Do".to_string());
    }
    if normalized_query.contains("in progress") {
        return TaskStageFilter::Exact("In Progress".to_string());
    }
    if normalized_query.contains("blocked") {
        return TaskStageFilter::Exact("Blocked".to_string());
    }

    TaskStageFilter::Active
}

fn notion_properties_text(text: &str) -> Option<&str> {
    let start = text.find("<properties>")? + "<properties>".len();
    let after_start = &text[start..];
    let end = after_start.find("</properties>")?;
    Some(after_start[..end].trim())
}

fn notion_property_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty()
                || text == "<omitted />"
                || text.starts_with("formulaResult://")
                || text.starts_with("<mention-user ")
            {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Array(items) => {
            let values = items
                .iter()
                .filter_map(notion_property_string)
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        }
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn notion_page_property(text: &str, names: &[&str]) -> Option<String> {
    let properties = notion_properties_text(text)?;
    let value = serde_json::from_str::<Value>(properties).ok()?;
    let object = value.as_object()?;
    names
        .iter()
        .find_map(|name| object.get(*name).and_then(notion_property_string))
}

fn task_stage_from_page(text: &str) -> Option<String> {
    notion_page_property(text, &["Stage", "Status"])
}

fn notion_property_raw_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .map(notion_property_raw_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(object) => object
            .values()
            .map(notion_property_raw_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

fn notion_page_property_raw_text(text: &str, names: &[&str]) -> Option<String> {
    let properties = notion_properties_text(text)?;
    let value = serde_json::from_str::<Value>(properties).ok()?;
    let object = value.as_object()?;
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .map(notion_property_raw_text)
        .map(|text| text.trim().to_string())
        .find(|text| !text.is_empty())
}

fn task_owner_from_page(text: &str) -> Option<String> {
    notion_page_property_raw_text(text, &["Owner", "Assignee", "Assigned To"])
}

fn is_terminal_task_stage(stage: &str) -> bool {
    matches!(
        canonical_task_stage(stage).as_str(),
        "done" | "canceled" | "cancelled" | "closed" | "archived" | "abandoned" | "missed"
    )
}

fn task_matches_stage_filter(stage: Option<&str>, filter: &TaskStageFilter) -> bool {
    match filter {
        TaskStageFilter::Any => true,
        TaskStageFilter::Active => stage
            .map(|stage| !is_terminal_task_stage(stage))
            .unwrap_or(true),
        TaskStageFilter::Exact(expected) => stage
            .map(|stage| canonical_task_stage(stage) == canonical_task_stage(expected))
            .unwrap_or(false),
    }
}

fn collect_notion_candidates(
    text: &str,
    candidates: &mut Vec<crate::agent_review::AgentRelationCandidate>,
) {
    for line in text.lines() {
        for url in notion_urls_in_text(line) {
            if candidates
                .iter()
                .any(|candidate: &crate::agent_review::AgentRelationCandidate| candidate.url == url)
            {
                continue;
            }
            candidates.push(crate::agent_review::AgentRelationCandidate {
                title: title_for_url_line(line, &url)
                    .chars()
                    .take(90)
                    .collect::<String>(),
                url,
            });
        }
    }
}

fn notion_api_uuid(id: &str) -> Option<String> {
    let id = id
        .strip_prefix("collection://")
        .or_else(|| id.strip_prefix("user://"))
        .unwrap_or(id);
    hyphenate_notion_id(id)
}

fn notion_api_property<'a>(page: &'a Value, name: &str) -> Option<&'a Value> {
    page.get("properties")?.get(name)
}

fn notion_api_plain_text(items: Option<&Value>) -> Option<String> {
    let text = items?
        .as_array()?
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn notion_api_property_text(property: &Value) -> Option<String> {
    let property_type = property.get("type").and_then(Value::as_str);
    let text = match property_type {
        Some("title") => notion_api_plain_text(property.get("title")),
        Some("rich_text") => notion_api_plain_text(property.get("rich_text")),
        Some("select") => property
            .pointer("/select/name")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("status") => property
            .pointer("/status/name")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("people") => {
            let names = property
                .get("people")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(|person| person.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>();
            if names.is_empty() {
                None
            } else {
                Some(names.join(", "))
            }
        }
        Some("date") => property
            .pointer("/date/start")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("checkbox") => property
            .get("checkbox")
            .and_then(Value::as_bool)
            .map(|value| value.to_string()),
        Some("number") => property.get("number").map(|value| value.to_string()),
        Some("url") => property
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("email") => property
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("phone_number") => property
            .get("phone_number")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }?;

    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn notion_api_page_title(page: &Value) -> String {
    if let Some(title) = notion_api_property(page, "Name").and_then(notion_api_property_text) {
        return title;
    }

    if let Some(properties) = page.get("properties").and_then(Value::as_object) {
        for property in properties.values() {
            if property.get("type").and_then(Value::as_str) == Some("title") {
                if let Some(title) = notion_api_property_text(property) {
                    return title;
                }
            }
        }
    }

    page.get("id")
        .and_then(Value::as_str)
        .unwrap_or("(Untitled)")
        .to_string()
}

fn notion_api_task_stage(page: &Value) -> Option<String> {
    notion_api_property(page, "Stage")
        .or_else(|| notion_api_property(page, "Status"))
        .and_then(notion_api_property_text)
}

fn notion_api_task_owner(page: &Value) -> Option<String> {
    notion_api_property(page, "Owner")
        .or_else(|| notion_api_property(page, "Assignee"))
        .or_else(|| notion_api_property(page, "Assigned To"))
        .and_then(notion_api_property_text)
}

fn notion_api_task_matches_owner(
    page: &Value,
    owner_name: &str,
    owner_user_id: Option<&str>,
) -> bool {
    let Some(owner_property) = notion_api_property(page, "Owner")
        .or_else(|| notion_api_property(page, "Assignee"))
        .or_else(|| notion_api_property(page, "Assigned To"))
    else {
        return false;
    };

    if let Some(owner_user_id) = owner_user_id {
        let owner_user_id = owner_user_id.replace('-', "");
        if owner_property
            .get("people")
            .and_then(Value::as_array)
            .map(|people| {
                people.iter().any(|person| {
                    person
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|id| id.replace('-', "") == owner_user_id)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
        {
            return true;
        }
    }

    let Some(owner_text) = notion_api_property_text(owner_property) else {
        return false;
    };
    let lower_text = owner_text.to_ascii_lowercase();
    let lower_owner = owner_name.trim().to_ascii_lowercase();
    if lower_owner.is_empty() {
        return false;
    }
    if lower_text.contains(&lower_owner) {
        return true;
    }
    let terms = lower_owner
        .split_whitespace()
        .filter(|term| term.len() >= 2)
        .collect::<Vec<_>>();
    !terms.is_empty() && terms.iter().all(|term| lower_text.contains(term))
}

fn notion_api_task_filter(
    query: &str,
    owner_user_id: Option<&str>,
    skip_owner_filter: bool,
) -> Option<Value> {
    let mut filters = Vec::new();

    if !skip_owner_filter {
        if let Some(owner_user_id) = owner_user_id {
            filters.push(json!({
                "property": "Owner",
                "people": { "contains": owner_user_id }
            }));
        }
    }

    if !is_broad_task_query(query) {
        filters.push(json!({
            "property": "Name",
            "title": { "contains": query.trim() }
        }));
    }

    match filters.len() {
        0 => None,
        1 => filters.into_iter().next(),
        _ => Some(json!({ "and": filters })),
    }
}

async fn notion_api_resolve_data_source_id(
    app: &AppHandle,
    target: &str,
) -> Result<String, String> {
    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let target = raw_notion_table_url(target);
    let id = notion_api_uuid(&extract_notion_id(&target))
        .ok_or_else(|| format!("Invalid Notion target ID: {}", target))?;
    let client = tool_http_client()?;

    let data_source_response = client
        .get(format!("https://api.notion.com/v1/data_sources/{}", id))
        .bearer_auth(&api_key)
        .header("Notion-Version", NOTION_API_VERSION)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("Notion data source lookup failed: {}", error))?;
    if data_source_response.status().is_success() {
        return Ok(id);
    }
    let data_source_status = data_source_response.status();
    let data_source_error = data_source_response.text().await.unwrap_or_default();

    let database_response = client
        .get(format!("https://api.notion.com/v1/databases/{}", id))
        .bearer_auth(&api_key)
        .header("Notion-Version", NOTION_API_VERSION)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("Notion database lookup failed: {}", error))?;
    let status = database_response.status();
    if !status.is_success() {
        return Err(format!(
            "Notion target lookup failed. data source lookup: {} {}; database lookup: {} {}",
            data_source_status,
            data_source_error,
            status,
            database_response.text().await.unwrap_or_default()
        ));
    }
    let body: Value = database_response
        .json()
        .await
        .map_err(|error| format!("Invalid Notion database lookup response: {}", error))?;
    body.get("data_sources")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .and_then(notion_api_uuid)
        .ok_or_else(|| "Notion database lookup did not include a data source ID".to_string())
}

async fn notion_api_search_tasks(
    app: &AppHandle,
    data_source_id: &str,
    query: &str,
    owner_name: &str,
    owner_user_url: Option<&str>,
    skip_owner_filter: bool,
    stage_filter: &TaskStageFilter,
) -> Result<Value, String> {
    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let data_source_id = notion_api_uuid(data_source_id)
        .ok_or_else(|| format!("Invalid Notion data source ID: {}", data_source_id))?;
    let owner_user_id = if skip_owner_filter {
        None
    } else {
        owner_user_url.and_then(notion_api_uuid)
    };
    let broad_query = is_broad_task_query(query);
    let result_limit = if broad_query { 25 } else { 12 };
    let max_scanned = if broad_query { 500 } else { 150 };
    let filter = notion_api_task_filter(query, owner_user_id.as_deref(), skip_owner_filter);
    let client = tool_http_client()?;
    let mut start_cursor: Option<String> = None;
    let mut scanned_count = 0usize;
    let mut tasks = Vec::new();
    let mut stopped_early = false;

    loop {
        let mut body = json!({
            "page_size": 100
        });
        if let Some(filter) = filter.clone() {
            body["filter"] = filter;
        }
        if let Some(cursor) = &start_cursor {
            body["start_cursor"] = Value::String(cursor.clone());
        }

        let response = client
            .post(format!(
                "https://api.notion.com/v1/data_sources/{}/query",
                data_source_id
            ))
            .bearer_auth(&api_key)
            .header("Notion-Version", NOTION_API_VERSION)
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("Notion data source query failed: {}", error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Notion data source query failed with {}: {}",
                status,
                response.text().await.unwrap_or_default()
            ));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("Invalid Notion data source query response: {}", error))?;
        let results = body
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        scanned_count += results.len();

        for page in results {
            if !skip_owner_filter
                && !notion_api_task_matches_owner(&page, owner_name, owner_user_id.as_deref())
            {
                continue;
            }

            let stage = notion_api_task_stage(&page);
            if !task_matches_stage_filter(stage.as_deref(), stage_filter) {
                continue;
            }

            let mut detail_parts = Vec::new();
            if skip_owner_filter {
                detail_parts.push("Owner filter: all owners".to_string());
            } else if let Some(owner) = notion_api_task_owner(&page) {
                detail_parts.push(format!("Owner: {}", owner));
            } else {
                detail_parts.push(format!("Owner: {}", owner_name));
            }
            if let Some(stage) = &stage {
                detail_parts.push(format!("Stage: {}", stage));
            }

            tasks.push(json!({
                "title": notion_api_page_title(&page),
                "url": page.get("url").and_then(Value::as_str).unwrap_or_default(),
                "stage": stage,
                "detail": detail_parts.join(" | ")
            }));

            if tasks.len() >= result_limit {
                stopped_early = true;
                break;
            }
        }

        if stopped_early || scanned_count >= max_scanned {
            stopped_early = true;
            break;
        }

        if body
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            start_cursor = body
                .get("next_cursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if start_cursor.is_none() {
                break;
            }
        } else {
            break;
        }
    }

    Ok(json!({
        "source": "notion_tasks_api",
        "query": if query.is_empty() { "all tasks" } else { query },
        "ownerName": if skip_owner_filter { Value::Null } else { Value::String(owner_name.to_string()) },
        "ownerUserId": owner_user_id,
        "ownerFilter": if skip_owner_filter { "all" } else { "specific" },
        "stageFilter": stage_filter.label(),
        "searchedTaskCount": scanned_count,
        "count": tasks.len(),
        "moreAvailable": stopped_early,
        "tasks": tasks,
        "message": if tasks.is_empty() {
            if skip_owner_filter {
                format!(
                    "No task results matched {}.",
                    if query.is_empty() { "all tasks" } else { query }
                )
            } else {
                format!(
                    "No task results matched {} for {}.",
                    if query.is_empty() { "all tasks" } else { query },
                    owner_name
                )
            }
        } else {
            String::new()
        }
    }))
}

async fn notion_search_tasks(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let explicit_owner = arguments
        .get("ownerName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let owner_name = explicit_owner
        .map(str::to_string)
        .or_else(|| {
            crate::agent_config::get_config_value(app, crate::agent_config::AGENT_OWNER_NAME)
        })
        .unwrap_or_else(|| "Jason Walkow".to_string());
    let skip_owner_filter = explicit_owner
        .map(|owner| {
            matches!(
                owner.to_ascii_lowercase().as_str(),
                "all" | "all owners" | "anyone" | "any owner"
            )
        })
        .unwrap_or(false);
    let table_target =
        crate::agent_config::get_config_value(app, crate::agent_config::NOTION_TASKS_TABLE_TARGET)
            .ok_or_else(|| "Allowed Notion table is not configured: Tasks".to_string())?;
    let stage_filter = task_stage_filter(arguments, query);
    let configured_owner_url = if skip_owner_filter || explicit_owner.is_some() {
        None
    } else {
        crate::agent_config::get_config_value(app, crate::agent_config::AGENT_OWNER_USER_ID)
            .map(|id| notion_user_url(&id))
    };

    if crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY).is_some() {
        let data_source_id = notion_api_resolve_data_source_id(app, &table_target).await?;
        return notion_api_search_tasks(
            app,
            &data_source_id,
            query,
            &owner_name,
            configured_owner_url.as_deref(),
            skip_owner_filter,
            &stage_filter,
        )
        .await;
    }

    let connection = stored_connection(app, "notion").await?;
    let client = reqwest::Client::new();
    let session_id = initialize_mcp_session(&client, &connection).await?;
    let data_source_id =
        resolve_notion_data_source_id(&client, &connection, session_id.as_deref(), &table_target)
            .await?;
    let task_data_source_url = notion_collection_url(&data_source_id);

    let mut candidates = Vec::new();
    let mut search_errors = Vec::new();
    let search_queries = task_search_queries(query, &owner_name, skip_owner_filter);
    let broad_query = is_broad_task_query(query);
    for (index, search_query) in search_queries.iter().enumerate() {
        let result = call_mcp_tool_on_session(
            &client,
            &connection,
            session_id.as_deref(),
            10 + index as u64,
            "notion-search",
            json!({
                "query": search_query,
                "query_type": "internal",
                "data_source_url": task_data_source_url.clone(),
                "page_size": 25,
                "max_highlight_length": 0
            }),
        )
        .await;
        match result {
            Ok(result) => {
                let text = notion_payload_text(&result);
                collect_notion_candidates(&text, &mut candidates);
            }
            Err(error) => {
                search_errors.push(format!("{}: {}", search_query, error));
            }
        }
    }

    if candidates.is_empty() && !search_errors.is_empty() {
        log::warn!(
            "Notion task search returned no candidates. Search errors: {}",
            search_errors.join(" | ")
        );
    }

    let owner_url = if skip_owner_filter {
        None
    } else if configured_owner_url.is_some() {
        configured_owner_url
    } else {
        notion_search_user_url_on_session(&client, &connection, session_id.as_deref(), &owner_name)
            .await
            .ok()
            .flatten()
    };

    let mut tasks = Vec::new();
    let fetch_limit = if broad_query { 60 } else { 24 };
    let result_limit = if broad_query { 12 } else { 8 };
    for (index, candidate) in candidates.iter().take(fetch_limit).enumerate() {
        let Ok(page_result) = call_mcp_tool_on_session(
            &client,
            &connection,
            session_id.as_deref(),
            30 + index as u64,
            "notion-fetch",
            json!({ "id": candidate.url }),
        )
        .await
        else {
            continue;
        };
        let page_text = notion_payload_text(&page_result);
        if !page_has_parent_data_source(&page_text, &task_data_source_url) {
            continue;
        }

        if !skip_owner_filter {
            if !notion_page_matches_owner(&page_text, &owner_name, owner_url.as_deref()) {
                continue;
            }
        }

        let stage = task_stage_from_page(&page_text);
        if !task_matches_stage_filter(stage.as_deref(), &stage_filter) {
            continue;
        }

        let mut detail_parts = Vec::new();
        if skip_owner_filter {
            detail_parts.push("Owner filter: all owners".to_string());
        } else {
            detail_parts.push(format!("Owner: {}", owner_name));
        }
        if let Some(stage) = &stage {
            detail_parts.push(format!("Stage: {}", stage));
        }

        tasks.push(json!({
            "title": candidate.title,
            "url": candidate.url,
            "stage": stage,
            "detail": detail_parts.join(" | ")
        }));
        if tasks.len() >= result_limit {
            break;
        }
    }

    Ok(json!({
        "source": "notion_tasks",
        "query": if query.is_empty() { "all tasks" } else { query },
        "searchQueries": search_queries,
        "ownerName": if skip_owner_filter { Value::Null } else { Value::String(owner_name.clone()) },
        "ownerFilter": if skip_owner_filter { "all" } else { "specific" },
        "stageFilter": stage_filter.label(),
        "searchedTaskCount": candidates.len(),
        "count": tasks.len(),
        "tasks": tasks,
        "message": if tasks.is_empty() {
            if skip_owner_filter {
                format!(
                    "No task results matched {}.",
                    if query.is_empty() { "all tasks" } else { query }
                )
            } else {
                format!(
                    "No task results matched {} for {}.",
                    if query.is_empty() { "all tasks" } else { query },
                    owner_name
                )
            }
        } else {
            String::new()
        }
    }))
}

fn extract_notion_id(input: &str) -> String {
    input
        .split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        .filter_map(|segment| {
            let compact = segment.replace('-', "");
            if compact.len() == 32
                && compact
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            {
                Some(compact)
            } else {
                None
            }
        })
        .last()
        .unwrap_or_else(|| input.trim().to_string())
}

fn extract_notion_view_id(input: &str) -> Option<String> {
    Url::parse(input)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.to_string())
        })
        .map(|view_id| extract_notion_id(&view_id))
        .filter(|view_id| !view_id.is_empty())
}

fn hyphenate_notion_id(id: &str) -> Option<String> {
    let compact = id.replace('-', "");
    if compact.len() != 32
        || !compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }

    Some(format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    ))
}

async fn resolve_notion_data_source_id(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    target: &str,
) -> Result<String, String> {
    let requested_target = target.trim();
    let target = raw_notion_table_url(requested_target);
    let target = target.trim();
    if target.is_empty() {
        return Err("Notion data source or database ID is empty".to_string());
    }

    let fetch_result_value = call_mcp_tool_on_session(
        client,
        connection,
        session_id,
        4,
        "notion-fetch",
        json!({ "id": target }),
    )
    .await?;
    let fetch_result = notion_payload_text(&fetch_result_value);

    if let Some(view_id) = extract_notion_view_id(requested_target) {
        if let Some(collection_id) = collection_id_for_view(&fetch_result, &view_id) {
            return Ok(collection_id);
        }
    }

    if let Some(collection_id) = collection_id_from_fetch_text(&fetch_result) {
        return Ok(collection_id);
    }

    let id = extract_notion_id(target);
    if !id.is_empty() && fetch_result.contains("\"type\":\"data_source\"") {
        return Ok(id);
    }

    Err(format!(
        "Could not resolve Notion target {} to a data source. Fetch the database in Notion MCP and copy the collection://... data source ID from its response.",
        requested_target
    ))
}

fn collection_id_from_fetch_text(text: &str) -> Option<String> {
    if let Some(data_sources_index) = text.find("<data-sources>") {
        if let Some(collection_id) =
            collection_id_after_data_source_marker(&text[data_sources_index..])
        {
            return Some(collection_id);
        }
    }

    if let Some(collection_id) = collection_id_after_data_source_marker(text) {
        return Some(collection_id);
    }

    text.split("collection://").nth(1).and_then(first_uuid)
}

fn collection_id_after_data_source_marker(text: &str) -> Option<String> {
    for marker in [
        "<data-source url=\"{{collection://",
        "<data-source url=\"collection://",
        "<data-source url=\\\"{{collection://",
        "<data-source url=\\\"collection://",
        "\"url\":\"collection://",
        "\\\"url\\\":\\\"collection://",
    ] {
        if let Some(collection_id) = text.split(marker).nth(1).and_then(first_uuid) {
            return Some(collection_id);
        }
    }
    None
}

fn collection_id_for_view(text: &str, view_id: &str) -> Option<String> {
    let compact_view_id = view_id.replace('-', "");
    let hyphenated_view_id = hyphenate_notion_id(view_id);
    let markers = [
        format!("view://{}", compact_view_id),
        hyphenated_view_id
            .map(|id| format!("view://{}", id))
            .unwrap_or_default(),
    ];
    let (view_start, marker_len) = markers
        .iter()
        .filter(|marker| !marker.is_empty())
        .filter_map(|marker| text.find(marker).map(|index| (index, marker.len())))
        .min_by_key(|(index, _)| *index)?;
    let after_view = &text[view_start..];
    let next_view = after_view
        .get(marker_len..)
        .and_then(|remaining| remaining.find("view://"))
        .map(|index| marker_len + index)
        .unwrap_or(after_view.len());
    collection_id_from_fetch_text(&after_view[..next_view])
}

fn mcp_text_content(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .filter(|text| !text.trim().is_empty())
}

fn notion_payload_text(value: &Value) -> String {
    let text = mcp_text_content(value).unwrap_or_else(|| value.to_string());
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|payload| {
            payload
                .get("text")
                .and_then(Value::as_str)
                .map(|inner| inner.to_string())
        })
        .unwrap_or(text)
}

#[tauri::command]
#[specta::specta]
pub async fn validate_agent_notion_table_target(
    app: AppHandle,
    target: String,
) -> Result<NotionTableValidation, String> {
    if crate::agent_config::get_config_value(&app, crate::agent_config::NOTION_API_KEY).is_some() {
        let data_source_id = notion_api_resolve_data_source_id(&app, &target).await?;
        return Ok(NotionTableValidation { data_source_id });
    }

    let connection = stored_connection(&app, "notion").await?;
    let client = reqwest::Client::new();
    let session_id = initialize_mcp_session(&client, &connection).await?;
    let data_source_id =
        resolve_notion_data_source_id(&client, &connection, session_id.as_deref(), &target).await?;

    Ok(NotionTableValidation { data_source_id })
}

fn property_has_type(schema_text: &str, property_name: &str, property_type: &str) -> bool {
    let property_marker = format!("\"{}\"", property_name);
    let Some(property_index) = schema_text.find(&property_marker) else {
        return false;
    };
    let after_property = &schema_text[property_index..schema_text.len().min(property_index + 220)];
    after_property
        .to_ascii_lowercase()
        .contains(&property_type.to_ascii_lowercase())
}

fn relation_data_source_from_schema(schema_text: &str, property_name: &str) -> Option<String> {
    let start = schema_text.find("<data-source-state>")? + "<data-source-state>".len();
    let after_start = &schema_text[start..];
    let end = after_start.find("</data-source-state>")?;
    let state = serde_json::from_str::<Value>(after_start[..end].trim()).ok()?;
    state
        .pointer(&format!("/schema/{}/dataSourceUrl", property_name))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

fn normalize_checkbox_properties(
    schema_text: &str,
    properties: &mut serde_json::Map<String, Value>,
) {
    let checkbox_updates = properties
        .iter()
        .filter_map(|(property_name, value)| {
            value
                .as_bool()
                .filter(|_| property_has_type(schema_text, property_name, "checkbox"))
                .map(|checked| {
                    (
                        property_name.clone(),
                        Value::String(if checked { "__YES__" } else { "__NO__" }.to_string()),
                    )
                })
        })
        .collect::<Vec<_>>();

    for (property_name, value) in checkbox_updates {
        properties.insert(property_name, value);
    }
}

async fn notion_api_get_data_source(
    app: &AppHandle,
    data_source_id: &str,
) -> Result<Value, String> {
    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let data_source_id = notion_api_uuid(data_source_id)
        .ok_or_else(|| format!("Invalid Notion data source ID: {}", data_source_id))?;
    let response = tool_http_client()?
        .get(format!(
            "https://api.notion.com/v1/data_sources/{}",
            data_source_id
        ))
        .bearer_auth(&api_key)
        .header("Notion-Version", NOTION_API_VERSION)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("Notion data source lookup failed: {}", error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Notion data source lookup failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }
    response
        .json()
        .await
        .map_err(|error| format!("Invalid Notion data source response: {}", error))
}

fn notion_api_schema_properties(data_source: &Value) -> Option<&serde_json::Map<String, Value>> {
    data_source.get("properties").and_then(Value::as_object)
}

fn notion_api_schema_property_type(schema_property: &Value) -> Option<&str> {
    schema_property.get("type").and_then(Value::as_str)
}

fn notion_api_title_property_name(data_source: &Value) -> Option<String> {
    notion_api_schema_properties(data_source)?
        .iter()
        .find_map(|(name, property)| {
            (notion_api_schema_property_type(property) == Some("title")).then(|| name.clone())
        })
}

fn notion_api_text_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn notion_api_checkbox_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "__yes__" | "yes" | "true" | "1" | "checked" => Some(true),
            "__no__" | "no" | "false" | "0" | "unchecked" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn notion_api_number_value(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return number.is_finite().then_some(number);
    }
    let text = value.as_str()?;
    let numeric = text
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.' || *character == '-')
        .collect::<String>();
    numeric
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

fn notion_api_text_object(text: &str) -> Value {
    let content = text.chars().take(1900).collect::<String>();
    json!({
        "type": "text",
        "text": { "content": content }
    })
}

fn notion_api_page_id_from_value(value: &Value) -> Option<String> {
    let text = notion_api_text_value(value)?;
    notion_api_uuid(&extract_notion_id(&text))
}

fn notion_api_user_id_from_literal(value: &Value) -> Option<String> {
    let text = notion_api_text_value(value)?;
    notion_api_uuid(&text)
}

fn notion_api_relation_target_from_schema(schema_property: &Value) -> Option<String> {
    schema_property
        .pointer("/relation/data_source_id")
        .or_else(|| schema_property.pointer("/relation/database_id"))
        .and_then(Value::as_str)
        .and_then(notion_api_uuid)
}

async fn notion_api_find_user_id(app: &AppHandle, query: &str) -> Result<Option<String>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    if let Some(id) = notion_api_uuid(query) {
        return Ok(Some(id));
    }

    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let client = tool_http_client()?;
    let mut start_cursor: Option<String> = None;
    let query_lower = query.to_ascii_lowercase();
    let query_terms = query_lower
        .split_whitespace()
        .filter(|term| term.len() >= 2)
        .collect::<Vec<_>>();

    loop {
        let mut request = client
            .get("https://api.notion.com/v1/users")
            .bearer_auth(&api_key)
            .header("Notion-Version", NOTION_API_VERSION)
            .header("User-Agent", USER_AGENT)
            .query(&[("page_size", "100")]);
        if let Some(cursor) = &start_cursor {
            request = request.query(&[("start_cursor", cursor.as_str())]);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Notion user lookup failed: {}", error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Notion user lookup failed with {}: {}",
                status,
                response.text().await.unwrap_or_default()
            ));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("Invalid Notion user lookup response: {}", error))?;
        for user in body
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = user
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let email = user
                .pointer("/person/email")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let haystack = format!("{} {}", name, email);
            if haystack.contains(&query_lower)
                || (!query_terms.is_empty()
                    && query_terms.iter().all(|term| haystack.contains(term)))
            {
                return Ok(user
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(notion_api_uuid));
            }
        }

        if body
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            start_cursor = body
                .get("next_cursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if start_cursor.is_none() {
                break;
            }
        } else {
            break;
        }
    }

    Ok(None)
}

async fn notion_api_relation_target_id(
    app: &AppHandle,
    schema_property: &Value,
    property_name: &str,
) -> Result<String, String> {
    let configured_target = match property_name {
        "Client" => crate::agent_config::get_config_value(
            app,
            crate::agent_config::NOTION_COMPANIES_TABLE_TARGET,
        ),
        "Contacts" => crate::agent_config::get_config_value(
            app,
            crate::agent_config::NOTION_CONTACTS_TABLE_TARGET,
        ),
        _ => None,
    };

    if let Some(target) = configured_target.filter(|target| !target.trim().is_empty()) {
        return notion_api_resolve_data_source_id(app, &target).await;
    }

    notion_api_relation_target_from_schema(schema_property).ok_or_else(|| {
        format!(
            "Could not determine the allowed relation target for {}.",
            property_name
        )
    })
}

async fn notion_api_relation_candidates(
    app: &AppHandle,
    data_source_id: &str,
    query: &str,
) -> Result<Vec<crate::agent_review::AgentRelationCandidate>, String> {
    let data_source = notion_api_get_data_source(app, data_source_id).await?;
    let title_property =
        notion_api_title_property_name(&data_source).unwrap_or_else(|| "Name".to_string());
    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let data_source_id = notion_api_uuid(data_source_id)
        .ok_or_else(|| format!("Invalid Notion data source ID: {}", data_source_id))?;

    let mut search_queries = vec![query.trim().to_string()];
    for term in query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| term.len() >= 3)
    {
        if !search_queries
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(term))
        {
            search_queries.push(term.to_string());
        }
    }

    let client = tool_http_client()?;
    let mut candidates = Vec::new();
    for search_query in search_queries {
        let mut body = json!({ "page_size": 5 });
        if !search_query.trim().is_empty() {
            body["filter"] = json!({
                "property": title_property,
                "title": { "contains": search_query.trim() }
            });
        }

        let response = client
            .post(format!(
                "https://api.notion.com/v1/data_sources/{}/query",
                data_source_id
            ))
            .bearer_auth(&api_key)
            .header("Notion-Version", NOTION_API_VERSION)
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("Notion relation lookup failed: {}", error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Notion relation lookup failed with {}: {}",
                status,
                response.text().await.unwrap_or_default()
            ));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("Invalid Notion relation lookup response: {}", error))?;
        for page in body
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(url) = page.get("url").and_then(Value::as_str) else {
                continue;
            };
            if candidates
                .iter()
                .any(|candidate: &crate::agent_review::AgentRelationCandidate| candidate.url == url)
            {
                continue;
            }
            candidates.push(crate::agent_review::AgentRelationCandidate {
                title: notion_api_page_title(page),
                url: url.to_string(),
            });
            if candidates.len() >= 5 {
                return Ok(candidates);
            }
        }
    }

    Ok(candidates)
}

async fn notion_api_request_relation_selection(
    app: &AppHandle,
    schema_property: &Value,
    property_name: &str,
    query: &str,
) -> Result<(), String> {
    let data_source_id = notion_api_relation_target_id(app, schema_property, property_name).await?;
    let candidates = notion_api_relation_candidates(app, &data_source_id, query).await?;
    let record_type = match property_name {
        "Client" => "company",
        "Contacts" => "contact",
        "Deal" => "deal",
        "Engagement" => "engagement",
        "Projects" => "project",
        "Thread" => "thread",
        _ => "record",
    };
    let can_create = matches!(property_name, "Client" | "Contacts");
    Err(crate::agent_review::relation_selection_error(
        crate::agent_review::AgentRelationSelection {
            property_name: property_name.to_string(),
            record_type: record_type.to_string(),
            query: query.to_string(),
            message: if candidates.is_empty() {
                format!(
                    "No existing {} matched {}. Create a new one, or paste a known Notion page URL.",
                    record_type, query
                )
            } else {
                format!(
                    "Choose the {} for {}, or create a new one.",
                    record_type, query
                )
            },
            candidates,
            can_create,
        },
    ))
}

async fn notion_api_property_value(
    app: &AppHandle,
    data_source: &Value,
    schema_property: &Value,
    property_name: &str,
    raw_value: &Value,
) -> Result<Option<Value>, String> {
    let Some(property_type) = notion_api_schema_property_type(schema_property) else {
        return Ok(None);
    };

    if property_type == "multi_select" {
        let names = match raw_value {
            Value::Array(items) => items
                .iter()
                .filter_map(notion_api_text_value)
                .collect::<Vec<_>>(),
            _ => notion_api_text_value(raw_value)
                .map(|text| {
                    text.split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        };
        if names.is_empty() {
            return Ok(None);
        }
        return Ok(Some(json!({
            "multi_select": names
                .into_iter()
                .map(|name| json!({ "name": name }))
                .collect::<Vec<_>>()
        })));
    }

    let Some(text) = notion_api_text_value(raw_value) else {
        if property_type == "checkbox" {
            return Ok(
                notion_api_checkbox_value(raw_value).map(|value| json!({ "checkbox": value }))
            );
        }
        return Ok(None);
    };

    let property_value = match property_type {
        "title" => json!({ "title": [notion_api_text_object(&text)] }),
        "rich_text" => json!({ "rich_text": [notion_api_text_object(&text)] }),
        "number" => {
            let Some(number) = notion_api_number_value(raw_value) else {
                return Ok(None);
            };
            json!({ "number": number })
        }
        "select" => json!({ "select": { "name": text } }),
        "status" => json!({ "status": { "name": text } }),
        "date" => json!({ "date": { "start": text } }),
        "checkbox" => {
            let Some(checked) = notion_api_checkbox_value(raw_value) else {
                return Ok(None);
            };
            json!({ "checkbox": checked })
        }
        "people" => {
            let user_id = notion_api_user_id_from_literal(raw_value).or_else(|| {
                if text.starts_with("user://") {
                    notion_api_uuid(&text)
                } else {
                    None
                }
            });
            let user_id = match user_id {
                Some(user_id) => user_id,
                None => notion_api_find_user_id(app, &text)
                    .await?
                    .ok_or_else(|| format!("Could not find Notion user for {}.", text))?,
            };
            json!({ "people": [{ "id": user_id }] })
        }
        "relation" => {
            if let Some(page_id) = notion_api_page_id_from_value(raw_value) {
                json!({ "relation": [{ "id": page_id }] })
            } else {
                notion_api_request_relation_selection(app, schema_property, property_name, &text)
                    .await?;
                return Ok(None);
            }
        }
        "url" => json!({ "url": text }),
        "email" => json!({ "email": text }),
        "phone_number" => json!({ "phone_number": text }),
        _ => {
            let _ = data_source;
            return Ok(None);
        }
    };

    Ok(Some(property_value))
}

async fn notion_api_page_properties(
    app: &AppHandle,
    data_source: &Value,
    raw_properties: &serde_json::Map<String, Value>,
) -> Result<serde_json::Map<String, Value>, String> {
    let schema = notion_api_schema_properties(data_source)
        .ok_or_else(|| "Notion data source response did not include properties".to_string())?;
    let mut properties = serde_json::Map::new();
    let title_property_name = notion_api_title_property_name(data_source);

    for (property_name, schema_property) in schema {
        let raw_value = raw_properties.get(property_name).or_else(|| {
            if title_property_name.as_deref() == Some(property_name.as_str()) {
                raw_properties
                    .get("title")
                    .or_else(|| raw_properties.get("Name"))
                    .or_else(|| raw_properties.get("name"))
            } else {
                None
            }
        });
        let Some(raw_value) = raw_value else {
            continue;
        };
        if let Some(value) =
            notion_api_property_value(app, data_source, schema_property, property_name, raw_value)
                .await?
        {
            properties.insert(property_name.clone(), value);
        }
    }

    if !properties
        .values()
        .any(|value| value.get("title").is_some())
    {
        if let Some(title_property_name) = title_property_name {
            if let Some(title) = raw_properties
                .get("title")
                .or_else(|| raw_properties.get("Name"))
                .or_else(|| raw_properties.get("name"))
                .and_then(notion_api_text_value)
            {
                properties.insert(
                    title_property_name,
                    json!({ "title": [notion_api_text_object(&title)] }),
                );
            }
        }
    }

    Ok(properties)
}

fn notion_api_children_from_content(content: Option<&str>) -> Option<Vec<Value>> {
    let content = content?.trim();
    if content.is_empty() {
        return None;
    }
    Some(vec![json!({
        "object": "block",
        "type": "paragraph",
        "paragraph": {
            "rich_text": [notion_api_text_object(content)]
        }
    })])
}

async fn notion_create_page_via_api(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    let target = arguments
        .pointer("/parent/data_source_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "Notion API create requires parent.data_source_id".to_string())?;
    let data_source_id = notion_api_resolve_data_source_id(app, target).await?;
    let data_source = notion_api_get_data_source(app, &data_source_id).await?;
    let page = arguments
        .get("pages")
        .and_then(Value::as_array)
        .and_then(|pages| pages.first())
        .ok_or_else(|| "Notion create requires at least one page".to_string())?;
    let raw_properties = page
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| "Notion create page requires properties".to_string())?;
    let properties = notion_api_page_properties(app, &data_source, raw_properties).await?;
    let api_key = crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY)
        .ok_or_else(|| "NOTION_API_KEY is not configured".to_string())?;
    let mut body = json!({
        "parent": {
            "type": "data_source_id",
            "data_source_id": data_source_id
        },
        "properties": properties
    });
    if let Some(children) =
        notion_api_children_from_content(page.get("content").and_then(Value::as_str))
    {
        body["children"] = Value::Array(children);
    }

    let response = tool_http_client()?
        .post("https://api.notion.com/v1/pages")
        .bearer_auth(&api_key)
        .header("Notion-Version", NOTION_API_VERSION)
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Notion page create failed: {}", error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Notion page create failed with {}: {}",
            status,
            response.text().await.unwrap_or_default()
        ));
    }
    let page: Value = response
        .json()
        .await
        .map_err(|error| format!("Invalid Notion page create response: {}", error))?;
    Ok(json!({
        "source": "notion_api",
        "pages": [{
            "id": page.get("id").cloned().unwrap_or(Value::Null),
            "url": page.get("url").cloned().unwrap_or(Value::Null),
            "properties": page.get("properties").cloned().unwrap_or(Value::Null)
        }]
    }))
}

fn notion_urls_in_text(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("https://www.notion.so/") {
        let after_start = &remaining[start..];
        let end = after_start
            .find(|character: char| {
                character.is_whitespace()
                    || character == '"'
                    || character == '<'
                    || character == '>'
                    || character == '}'
                    || character == ')'
            })
            .unwrap_or(after_start.len());
        let url = after_start[..end].trim_matches('}').to_string();
        if !urls.contains(&url) {
            urls.push(url);
        }
        remaining = &after_start[end..];
    }
    urls
}

fn title_for_url_line(line: &str, url: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(line) {
        if let Some(title) = json_title_near_url(&value, url) {
            return title;
        }
    }

    let without_url = line
        .replace(url, "")
        .replace(['[', ']', '(', ')', '*'], "")
        .trim()
        .trim_matches('-')
        .trim()
        .to_string();
    let title = without_url
        .lines()
        .next()
        .unwrap_or_default()
        .split(['|', '{', '}'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .trim()
        .to_string();
    if title.is_empty() {
        url.to_string()
    } else {
        title
    }
}

fn json_title_near_url(value: &Value, url: &str) -> Option<String> {
    match value {
        Value::Array(items) => items.iter().find_map(|item| json_title_near_url(item, url)),
        Value::Object(object) => {
            let has_url = object.values().any(|value| value.as_str() == Some(url));
            if has_url {
                for key in ["title", "name", "Name", "plain_text"] {
                    if let Some(title) = object
                        .get(key)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|title| !title.is_empty())
                    {
                        return Some(title.to_string());
                    }
                }
            }

            object
                .values()
                .find_map(|value| json_title_near_url(value, url))
        }
        _ => None,
    }
}

fn raw_notion_table_url(target: &str) -> String {
    let trimmed = target.trim();
    if let Ok(mut url) = Url::parse(trimmed) {
        url.set_query(None);
        url.set_fragment(None);
        return url.to_string();
    }
    trimmed.split('?').next().unwrap_or(trimmed).to_string()
}

fn notion_collection_url(data_source_id: &str) -> String {
    let id = data_source_id
        .strip_prefix("collection://")
        .unwrap_or(data_source_id);
    let hyphenated = hyphenate_notion_id(id).unwrap_or_else(|| id.to_string());
    format!("collection://{}", hyphenated)
}

fn page_has_parent_data_source(text: &str, data_source_url: &str) -> bool {
    let id = data_source_url
        .strip_prefix("collection://")
        .unwrap_or(data_source_url);
    let collection_url = notion_collection_url(id);
    [
        format!("<parent-data-source url=\"{}\"", collection_url),
        format!("<parent-data-source url=\\\"{}\\\"", collection_url),
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn notion_page_matches_owner(text: &str, owner_name: &str, owner_url: Option<&str>) -> bool {
    let Some(owner_text) = task_owner_from_page(text) else {
        return false;
    };

    if let Some(owner_url) = owner_url.filter(|url| !url.is_empty()) {
        return owner_text.contains(owner_url);
    }

    let lower_text = owner_text.to_ascii_lowercase();
    let lower_owner = owner_name.trim().to_ascii_lowercase();
    if lower_owner.is_empty() {
        return false;
    }
    if lower_text.contains(&lower_owner) {
        return true;
    }

    let terms = lower_owner
        .split_whitespace()
        .filter(|term| term.len() >= 2)
        .collect::<Vec<_>>();
    !terms.is_empty() && terms.iter().all(|term| lower_text.contains(term))
}

fn notion_user_url(id: &str) -> String {
    let id = id.strip_prefix("user://").unwrap_or(id);
    let hyphenated = hyphenate_notion_id(id).unwrap_or_else(|| id.to_string());
    format!("user://{}", hyphenated)
}

fn first_notion_user_url(text: &str) -> Option<String> {
    text.find("user://")
        .and_then(|index| first_uuid(&text[index + "user://".len()..]))
        .map(|id| notion_user_url(&id))
}

fn first_uuid(text: &str) -> Option<String> {
    text.split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        .find_map(|segment| {
            let compact = segment.replace('-', "");
            if compact.len() == 32
                && compact
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            {
                Some(compact)
            } else {
                None
            }
        })
}

async fn notion_fetch_text_on_session(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    target: &str,
) -> Result<String, String> {
    let result = call_mcp_tool_on_session(
        client,
        connection,
        session_id,
        6,
        "notion-fetch",
        json!({ "id": target }),
    )
    .await?;
    Ok(notion_payload_text(&result))
}

async fn notion_relation_candidates_on_session(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    query: &str,
    data_source_url: &str,
) -> Result<Vec<crate::agent_review::AgentRelationCandidate>, String> {
    let mut arguments = json!({
        "query": query,
        "query_type": "internal"
    });
    arguments["data_source_url"] = Value::String(data_source_url.to_string());

    let result = call_mcp_tool_on_session(
        client,
        connection,
        session_id,
        7,
        "notion-search",
        arguments,
    )
    .await?;
    let text = notion_payload_text(&result);
    let mut candidates = Vec::new();
    for line in text.lines() {
        for url in notion_urls_in_text(line) {
            if candidates
                .iter()
                .any(|candidate: &crate::agent_review::AgentRelationCandidate| candidate.url == url)
            {
                continue;
            }
            candidates.push(crate::agent_review::AgentRelationCandidate {
                title: title_for_url_line(line, &url)
                    .chars()
                    .take(90)
                    .collect::<String>(),
                url,
            });
        }
    }

    let mut filtered_candidates = Vec::new();
    for candidate in candidates.into_iter().take(12) {
        let result = call_mcp_tool_on_session(
            client,
            connection,
            session_id,
            9,
            "notion-fetch",
            json!({ "id": candidate.url }),
        )
        .await;
        let Ok(result) = result else {
            continue;
        };
        let page_text = notion_payload_text(&result);
        if page_has_parent_data_source(&page_text, data_source_url) {
            filtered_candidates.push(candidate);
        }
        if filtered_candidates.len() >= 5 {
            break;
        }
    }

    Ok(filtered_candidates)
}

async fn notion_search_user_url_on_session(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    query: &str,
) -> Result<Option<String>, String> {
    let result = call_mcp_tool_on_session(
        client,
        connection,
        session_id,
        8,
        "notion-search",
        json!({
            "query": query,
            "query_type": "user"
        }),
    )
    .await?;
    let text = notion_payload_text(&result);
    Ok(first_notion_user_url(&text).or_else(|| first_uuid(&text).map(|id| notion_user_url(&id))))
}

async fn request_relation_selection_if_needed(
    app: &AppHandle,
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    schema_text: &str,
    properties: &serde_json::Map<String, Value>,
    property_name: &str,
) -> Result<(), String> {
    let Some(query) = properties.get(property_name).and_then(Value::as_str) else {
        return Ok(());
    };
    if query.starts_with("https://www.notion.so/") {
        return Ok(());
    }

    let schema_relation_target = relation_data_source_from_schema(schema_text, property_name);
    if !property_has_type(schema_text, property_name, "relation")
        && schema_relation_target.is_none()
    {
        return Ok(());
    }

    let configured_target = match property_name {
        "Client" => crate::agent_config::get_config_value(
            app,
            crate::agent_config::NOTION_COMPANIES_TABLE_TARGET,
        ),
        "Contacts" => crate::agent_config::get_config_value(
            app,
            crate::agent_config::NOTION_CONTACTS_TABLE_TARGET,
        ),
        _ => None,
    };
    let relation_target = configured_target
        .or(schema_relation_target)
        .ok_or_else(|| {
            format!(
                "Could not determine the allowed relation target for {}.",
                property_name
            )
        })?;
    let data_source_id =
        resolve_notion_data_source_id(client, connection, session_id, &relation_target).await?;
    let data_source_url = notion_collection_url(&data_source_id);
    let candidates = notion_relation_candidates_on_session(
        client,
        connection,
        session_id,
        query,
        &data_source_url,
    )
    .await?;
    let record_type = match property_name {
        "Client" => "company",
        "Contacts" => "contact",
        "Deal" => "deal",
        "Engagement" => "engagement",
        "Projects" => "project",
        "Thread" => "thread",
        _ => "record",
    };
    let can_create = matches!(property_name, "Client" | "Contacts");

    Err(crate::agent_review::relation_selection_error(
        crate::agent_review::AgentRelationSelection {
            property_name: property_name.to_string(),
            record_type: record_type.to_string(),
            query: query.to_string(),
            message: if candidates.is_empty() {
                format!(
                    "No existing {} matched {}. Create a new one, or paste a known Notion page URL.",
                    record_type, query
                )
            } else {
                format!(
                    "Choose the {} for {}, or create a new one.",
                    record_type, query
                )
            },
            candidates,
            can_create,
        },
    ))
}

async fn replace_people_with_user_id(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    properties: &mut serde_json::Map<String, Value>,
    schema_text: &str,
    property_name: &str,
) -> Result<(), String> {
    if !property_has_type(schema_text, property_name, "person")
        && !property_has_type(schema_text, property_name, "people")
    {
        return Ok(());
    }
    let Some(query) = properties.get(property_name).and_then(Value::as_str) else {
        return Ok(());
    };
    if query.starts_with("user://") {
        return Ok(());
    }
    let user_url = notion_search_user_url_on_session(client, connection, session_id, query)
        .await?
        .ok_or_else(|| format!("Could not find Notion user for {}.", query))?;
    properties.insert(property_name.to_string(), Value::String(user_url));
    Ok(())
}

async fn call_mcp_tool_on_session(
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    id: u64,
    tool_name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let (response, _) = mcp_post(
        client,
        connection,
        json_rpc(
            id,
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": arguments
            }),
        ),
        session_id,
    )
    .await?;

    if let Some(error) = response.get("error") {
        return Err(format!("MCP tool call failed: {}", error));
    }

    let result = response.get("result").cloned().unwrap_or(response);
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let message = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("MCP tool returned an error");
        return Err(message.to_string());
    }

    Ok(result)
}

async fn normalize_notion_parent(
    app: &AppHandle,
    client: &reqwest::Client,
    connection: &StoredConnection,
    session_id: Option<&str>,
    arguments: &Value,
) -> Result<Value, String> {
    let mut normalized = arguments.clone();
    let Some(target) = normalized
        .pointer("/parent/data_source_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
    else {
        return Ok(normalized);
    };

    let data_source_id =
        resolve_notion_data_source_id(client, connection, session_id, &target).await?;
    normalized["parent"] = json!({
        "type": "data_source_id",
        "data_source_id": data_source_id.clone()
    });

    if let Some(page) = normalized
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .and_then(|pages| pages.first_mut())
    {
        if let Some(properties) = page.get_mut("properties").and_then(Value::as_object_mut) {
            let schema_text = notion_fetch_text_on_session(
                client,
                connection,
                session_id,
                &notion_collection_url(&data_source_id),
            )
            .await?;
            normalize_checkbox_properties(&schema_text, properties);
            for property_name in [
                "Client",
                "Contacts",
                "Deal",
                "Engagement",
                "Projects",
                "Thread",
            ] {
                request_relation_selection_if_needed(
                    app,
                    client,
                    connection,
                    session_id,
                    &schema_text,
                    properties,
                    property_name,
                )
                .await?;
            }
            for property_name in [
                "Relationship Owner",
                "Owner",
                "Contributor(s)",
                "Reviewer(s)",
            ] {
                replace_people_with_user_id(
                    client,
                    connection,
                    session_id,
                    properties,
                    &schema_text,
                    property_name,
                )
                .await?;
            }
        }
    }
    Ok(normalized)
}

async fn notion_create_page(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    if crate::agent_config::get_config_value(app, crate::agent_config::NOTION_API_KEY).is_some()
        && arguments.pointer("/parent/data_source_id").is_some()
    {
        return notion_create_page_via_api(app, arguments).await;
    }

    let connection = stored_connection(app, "notion").await?;
    let client = reqwest::Client::new();
    let session_id = initialize_mcp_session(&client, &connection).await?;
    let normalized_arguments =
        normalize_notion_parent(app, &client, &connection, session_id.as_deref(), arguments)
            .await?;
    call_mcp_tool_on_session(
        &client,
        &connection,
        session_id.as_deref(),
        5,
        "notion-create-pages",
        normalized_arguments,
    )
    .await
}

fn granola_text_content(value: &Value) -> String {
    mcp_text_content(value).unwrap_or_else(|| value.to_string())
}

fn granola_meeting_blocks(text: &str) -> Vec<String> {
    text.split("</meeting>")
        .filter_map(|block| {
            let start = block.find("<meeting")?;
            Some(format!("{}</meeting>", block[start..].trim()))
        })
        .collect()
}

fn granola_meeting_id(block: &str) -> Option<String> {
    block
        .split("<meeting id=\"")
        .nth(1)
        .and_then(|remaining| remaining.split('"').next())
        .map(|id| id.to_string())
}

fn granola_query_terms(query: &str) -> Vec<String> {
    let stop_words = [
        "the", "and", "for", "with", "from", "about", "that", "this", "what", "when", "where",
        "who", "why", "how", "meeting", "meetings", "recent", "most", "latest", "find", "search",
        "notes", "note", "call", "calls", "sync",
    ];
    let mut terms = Vec::new();
    for term in query
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| term.len() >= 3 && !stop_words.contains(term))
    {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_string());
        }
        let abbreviation = match term {
            "january" => Some("jan"),
            "february" => Some("feb"),
            "march" => Some("mar"),
            "april" => Some("apr"),
            "june" => Some("jun"),
            "july" => Some("jul"),
            "august" => Some("aug"),
            "september" => Some("sep"),
            "october" => Some("oct"),
            "november" => Some("nov"),
            "december" => Some("dec"),
            _ => None,
        };
        if let Some(abbreviation) = abbreviation {
            if !terms.iter().any(|existing| existing == abbreviation) {
                terms.push(abbreviation.to_string());
            }
        }
    }
    terms
}

fn granola_score_block(block: &str, terms: &[String]) -> usize {
    let lower = block.to_ascii_lowercase();
    terms
        .iter()
        .map(|term| usize::from(lower.contains(term)))
        .sum()
}

fn granola_bool_arg(arguments: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_bool))
        .unwrap_or(false)
}

fn granola_usize_arg(arguments: &Value, keys: &[&str], default: usize) -> usize {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_u64))
        .map(|value| value as usize)
        .unwrap_or(default)
}

async fn granola_search_notes(app: &AppHandle, arguments: &Value) -> Result<Value, String> {
    let connection = stored_connection(app, "granola").await?;
    let client = reqwest::Client::new();
    let session_id = initialize_mcp_session(&client, &connection).await?;
    let query = arguments
        .get("query")
        .or_else(|| arguments.get("q"))
        .or_else(|| arguments.get("search"))
        .or_else(|| arguments.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "Granola search requires a query.".to_string())?;
    let terms = granola_query_terms(query);
    if terms.is_empty() {
        return Err("Granola search needs at least one specific search term.".to_string());
    }

    let end = Local::now().date_naive();
    let start = end - ChronoDuration::days(180);
    let list_result = call_mcp_tool_on_session(
        &client,
        &connection,
        session_id.as_deref(),
        4,
        "list_meetings",
        json!({
            "time_range": "custom",
            "custom_start": start.format("%Y-%m-%d").to_string(),
            "custom_end": end.format("%Y-%m-%d").to_string()
        }),
    )
    .await?;
    let list_text = granola_text_content(&list_result);
    let mut scored_blocks = granola_meeting_blocks(&list_text)
        .into_iter()
        .filter_map(|block| {
            let score = granola_score_block(&block, &terms);
            (score > 0).then_some((score, block))
        })
        .collect::<Vec<_>>();
    scored_blocks.sort_by(|(left_score, left_block), (right_score, right_block)| {
        right_score
            .cmp(left_score)
            .then_with(|| right_block.cmp(left_block))
    });

    let meeting_ids = scored_blocks
        .iter()
        .filter_map(|(_, block)| granola_meeting_id(block))
        .take(5)
        .collect::<Vec<_>>();

    if meeting_ids.is_empty() {
        return Ok(json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "No Granola meetings matched \"{}\" in the last 180 days. Searched terms: {}.",
                    query,
                    terms.join(", ")
                )
            }],
            "query": query,
            "matchedMeetingCount": 0,
            "source": "granola_list_meetings"
        }));
    }

    let include_transcript = granola_bool_arg(
        arguments,
        &[
            "includeTranscript",
            "include_transcript",
            "fullTranscript",
            "full_transcript",
        ],
    );
    let max_transcripts =
        granola_usize_arg(arguments, &["maxTranscripts", "max_transcripts"], 1).clamp(1, 3);
    let detail_result = call_mcp_tool_on_session(
        &client,
        &connection,
        session_id.as_deref(),
        5,
        "get_meetings",
        json!({ "meeting_ids": meeting_ids }),
    )
    .await?;
    let details = granola_text_content(&detail_result);
    let matched_list = scored_blocks
        .iter()
        .take(5)
        .map(|(_, block)| block.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let transcripts = if include_transcript {
        let mut transcript_sections = Vec::new();
        for meeting_id in meeting_ids.iter().take(max_transcripts) {
            let transcript_result = call_mcp_tool_on_session(
                &client,
                &connection,
                session_id.as_deref(),
                6,
                "get_meeting_transcript",
                json!({ "meeting_id": meeting_id }),
            )
            .await?;
            transcript_sections.push(format!(
                "Transcript for meeting {}:\n{}",
                meeting_id,
                granola_text_content(&transcript_result)
            ));
        }
        transcript_sections.join("\n\n")
    } else {
        String::new()
    };
    let transcript_text = if transcripts.is_empty() {
        String::new()
    } else {
        format!("\n\nTranscripts:\n{}", transcripts)
    };

    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "Found {} Granola meeting(s) matching \"{}\".\n\nMatched meetings:\n{}\n\nDetails:\n{}{}",
                scored_blocks.len().min(5),
                query,
                matched_list,
                details,
                transcript_text
            )
        }],
        "query": query,
        "matchedMeetingCount": scored_blocks.len(),
        "transcriptIncluded": include_transcript,
        "source": "granola_list_meetings_get_meetings"
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn run_agent_connection_tool(
    app: AppHandle,
    name: String,
    arguments_json: String,
) -> Result<String, String> {
    log::info!(
        "Agent tool broker received: name={}, arguments={}",
        name,
        arguments_json
    );
    let arguments: Value = serde_json::from_str(&arguments_json)
        .map_err(|error| format!("Invalid tool arguments JSON: {}", error))?;
    let result = match name.as_str() {
        "gmail_search" => gmail_search(&app, &arguments).await,
        "gmail_create_draft" => gmail_create_draft(&app, &arguments).await,
        "calendar_check_availability" => calendar_check_availability(&app, &arguments).await,
        "calendar_list_events" => calendar_list_events(&app, &arguments).await,
        "notion_search" => notion_search(&app, &arguments).await,
        "notion_search_tasks" => notion_search_tasks(&app, &arguments).await,
        "notion_create_page" => notion_create_page(&app, &arguments).await,
        "granola_search_notes" => granola_search_notes(&app, &arguments).await,
        other => Err(format!("Unknown agent connection tool: {}", other)),
    };

    match &result {
        Ok(value) => log::info!(
            "Agent tool broker succeeded: name={}, result={}",
            name,
            value
        ),
        Err(error) => log::error!("Agent tool broker failed: name={}, error={}", name, error),
    }

    let result = result?;
    crate::agent_review::show_agent_tool_overlay(app, &name, result.to_string());
    Ok(result.to_string())
}
