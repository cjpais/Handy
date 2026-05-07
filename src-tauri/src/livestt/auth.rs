use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::settings;

use super::types::LiveSttAuthStatus;

const LIVESTT_REFRESH_PROACTIVE_WINDOW_SECONDS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSttTokens {
    pub access_token: String,
    pub refresh_token: String,
}

pub struct LiveSttAuthState {
    tokens: Mutex<Option<LiveSttTokens>>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl Default for LiveSttAuthState {
    fn default() -> Self {
        Self {
            tokens: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }
}

impl LiveSttAuthState {
    pub fn set_tokens(&self, access_token: String, refresh_token: String) {
        *self.tokens.lock().expect("LiveSTT auth state poisoned") = Some(LiveSttTokens {
            access_token,
            refresh_token,
        });
    }

    pub fn clear_tokens(&self) {
        *self.tokens.lock().expect("LiveSTT auth state poisoned") = None;
    }

    pub fn tokens(&self) -> Option<LiveSttTokens> {
        self.tokens
            .lock()
            .expect("LiveSTT auth state poisoned")
            .clone()
    }

    #[cfg(test)]
    fn access_token(&self) -> Option<String> {
        self.tokens().map(|tokens| tokens.access_token)
    }

    #[cfg(test)]
    fn refresh_token(&self) -> Option<String> {
        self.tokens().map(|tokens| tokens.refresh_token)
    }

    pub fn is_authenticated(&self) -> bool {
        self.tokens()
            .map(|tokens| {
                !tokens.access_token.trim().is_empty() && !tokens.refresh_token.trim().is_empty()
            })
            .unwrap_or(false)
    }
}

pub(super) fn parse_token_pair_response(body: &str) -> Result<LiveSttTokens, String> {
    let response: Value = serde_json::from_str(body)
        .map_err(|_| "LiveSTT auth response is not valid JSON".to_string())?;

    let access_token = response
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| "LiveSTT auth response missing access_token".to_string())?;

    let refresh_token = response
        .get("refresh_token")
        .and_then(Value::as_str)
        .ok_or_else(|| "LiveSTT auth response missing refresh_token".to_string())?;

    if access_token.trim().is_empty() {
        return Err("LiveSTT auth response missing access_token".to_string());
    }

    if refresh_token.trim().is_empty() {
        return Err("LiveSTT auth response missing refresh_token".to_string());
    }

    Ok(LiveSttTokens {
        access_token: access_token.trim().to_string(),
        refresh_token: refresh_token.trim().to_string(),
    })
}

pub(super) fn jwt_exp_unverified(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;

    value.get("exp")?.as_i64()
}

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn access_token_needs_refresh(access_token: &str) -> bool {
    jwt_exp_unverified(access_token)
        .map(|exp| exp <= current_unix_timestamp() + LIVESTT_REFRESH_PROACTIVE_WINDOW_SECONDS)
        .unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub async fn livestt_login(
    app: AppHandle,
    server_url: String,
    username: String,
    password: String,
) -> Result<(), String> {
    let base_url = settings::validate_livestt_server_url_required(&server_url)?;

    let login_url = format!("{}/auth/login", base_url);

    let username = username.trim().to_string();

    log::debug!("LiveSTT login request url={}", login_url);
    log::debug!("LiveSTT login username_present={}", !username.is_empty());

    if username.is_empty() {
        return Err("LiveSTT username is empty".to_string());
    }

    if password.is_empty() {
        return Err("LiveSTT password is empty".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build LiveSTT login client: {}", e))?;

    let form = [
        ("username", username.as_str()),
        ("password", password.as_str()),
    ];

    let response = client
        .post(&login_url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("LiveSTT login request failed: {}", e.without_url()))?;

    let status = response.status();

    let body = response
        .text()
        .await
        .map_err(|_| "Failed to read LiveSTT login response".to_string())?;

    if !status.is_success() {
        log::warn!(
            "LiveSTT login failed: status={}, response_body_len={}",
            status,
            body.len()
        );

        return Err(format!("LiveSTT login failed with status {}", status));
    }

    log::debug!(
        "LiveSTT login succeeded: status={}, response_body_len={}",
        status,
        body.len()
    );

    let tokens = parse_token_pair_response(&body)?;

    log::debug!(
        "LiveSTT login response parsed: access_token_present={}, refresh_token_present={}",
        !tokens.access_token.trim().is_empty(),
        !tokens.refresh_token.trim().is_empty()
    );

    let auth_state = app.state::<LiveSttAuthState>();
    auth_state.set_tokens(tokens.access_token, tokens.refresh_token);

    Ok(())
}

pub async fn ensure_fresh_livestt_access_token(app_handle: &AppHandle) -> Result<String, String> {
    let auth_state = app_handle.state::<LiveSttAuthState>();
    let tokens = auth_state
        .tokens()
        .ok_or_else(|| "LiveSTT login is required before starting transcription".to_string())?;

    if tokens.access_token.trim().is_empty() || tokens.refresh_token.trim().is_empty() {
        return Err("LiveSTT login is required before starting transcription".to_string());
    }

    if !access_token_needs_refresh(&tokens.access_token) {
        return Ok(tokens.access_token);
    }

    refresh_livestt_access_token_locked(app_handle, false).await
}

pub async fn force_refresh_livestt_access_token(app_handle: &AppHandle) -> Result<String, String> {
    refresh_livestt_access_token_locked(app_handle, true).await
}

async fn refresh_livestt_access_token_locked(
    app_handle: &AppHandle,
    force: bool,
) -> Result<String, String> {
    let auth_state = app_handle.state::<LiveSttAuthState>();
    let _refresh_guard = auth_state.refresh_lock.lock().await;

    let tokens = auth_state
        .tokens()
        .ok_or_else(|| "LiveSTT login is required before starting transcription".to_string())?;

    if tokens.refresh_token.trim().is_empty() {
        return Err("LiveSTT login expired; please log in again".to_string());
    }

    if !force && !access_token_needs_refresh(&tokens.access_token) {
        return Ok(tokens.access_token);
    }

    let app_settings = settings::get_settings(app_handle);
    let base_url =
        settings::validate_livestt_server_url_required(&app_settings.livestt_server_url)?;
    let refresh_url = format!("{}/auth/refresh", base_url);

    log::debug!(
        "LiveSTT token refresh request: refresh_token_present={}",
        !tokens.refresh_token.trim().is_empty()
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build LiveSTT refresh client: {}", e))?;

    let response = client
        .post(&refresh_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&serde_json::json!({ "refresh_token": tokens.refresh_token }))
        .send()
        .await
        .map_err(|e| format!("LiveSTT token refresh request failed: {}", e.without_url()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|_| "Failed to read LiveSTT token refresh response".to_string())?;

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        log::warn!(
            "LiveSTT token refresh rejected: status={}, response_body_len={}",
            status,
            body.len()
        );
        auth_state.clear_tokens();
        return Err("LiveSTT login expired; please log in again".to_string());
    }

    if !status.is_success() {
        log::warn!(
            "LiveSTT token refresh failed: status={}, response_body_len={}",
            status,
            body.len()
        );
        return Err(format!(
            "LiveSTT token refresh failed with status {}",
            status
        ));
    }

    log::debug!(
        "LiveSTT token refresh succeeded: status={}, response_body_len={}",
        status,
        body.len()
    );

    let new_tokens = parse_token_pair_response(&body)?;
    log::debug!(
        "LiveSTT token refresh parsed: access_token_present={}, refresh_token_present={}",
        !new_tokens.access_token.trim().is_empty(),
        !new_tokens.refresh_token.trim().is_empty()
    );

    let access_token = new_tokens.access_token.clone();
    auth_state.set_tokens(new_tokens.access_token, new_tokens.refresh_token);

    Ok(access_token)
}

#[tauri::command]
#[specta::specta]
pub fn livestt_logout(app: AppHandle) -> Result<(), String> {
    let cancel_result = if let Some(session_manager) =
        app.try_state::<std::sync::Arc<crate::livestt::session::LiveSttSessionManager>>()
    {
        session_manager.cancel_session()
    } else {
        Ok(())
    };

    let auth_state = app.state::<LiveSttAuthState>();
    auth_state.clear_tokens();

    cancel_result
}

#[tauri::command]
#[specta::specta]
pub fn livestt_auth_status(app: AppHandle) -> Result<LiveSttAuthStatus, String> {
    let auth_state = app.state::<LiveSttAuthState>();

    Ok(LiveSttAuthStatus {
        is_authenticated: auth_state.is_authenticated(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_jwt_with_payload(payload: &str) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("{}.{}.signature", header, payload)
    }

    #[test]
    fn auth_state_sets_and_clears_tokens() {
        let state = LiveSttAuthState::default();
        assert!(!state.is_authenticated());
        assert_eq!(state.tokens(), None);

        state.set_tokens("access-token".to_string(), "refresh-token".to_string());
        assert!(state.is_authenticated());
        assert_eq!(state.access_token(), Some("access-token".to_string()));
        assert_eq!(state.refresh_token(), Some("refresh-token".to_string()));

        state.clear_tokens();
        assert!(!state.is_authenticated());
        assert_eq!(state.tokens(), None);
    }

    #[test]
    fn parse_token_pair_response_accepts_access_and_refresh() {
        let tokens =
            parse_token_pair_response(r#"{"access_token":"a","refresh_token":"r"}"#).unwrap();

        assert_eq!(tokens.access_token, "a");
        assert_eq!(tokens.refresh_token, "r");
    }

    #[test]
    fn parse_token_pair_response_trims_tokens() {
        let tokens =
            parse_token_pair_response(r#"{"access_token":" a ","refresh_token":" r "}"#).unwrap();

        assert_eq!(tokens.access_token, "a");
        assert_eq!(tokens.refresh_token, "r");
    }

    #[test]
    fn parse_token_pair_response_rejects_missing_access() {
        let error = parse_token_pair_response(r#"{"refresh_token":"r"}"#).unwrap_err();
        assert_eq!(error, "LiveSTT auth response missing access_token");
    }

    #[test]
    fn parse_token_pair_response_rejects_missing_refresh() {
        let error = parse_token_pair_response(r#"{"access_token":"a"}"#).unwrap_err();
        assert_eq!(error, "LiveSTT auth response missing refresh_token");
    }

    #[test]
    fn parse_token_pair_response_rejects_empty_tokens() {
        let error =
            parse_token_pair_response(r#"{"access_token":"","refresh_token":"r"}"#).unwrap_err();
        assert_eq!(error, "LiveSTT auth response missing access_token");

        let error =
            parse_token_pair_response(r#"{"access_token":"a","refresh_token":" "}"#).unwrap_err();
        assert_eq!(error, "LiveSTT auth response missing refresh_token");
    }

    #[test]
    fn jwt_exp_parsing_returns_timestamp() {
        let token = test_jwt_with_payload(r#"{"exp":4102444800}"#);

        assert_eq!(jwt_exp_unverified(&token), Some(4_102_444_800));
    }

    #[test]
    fn jwt_exp_parsing_ignores_malformed_token() {
        assert_eq!(jwt_exp_unverified("not-a-jwt"), None);
    }

    #[test]
    fn jwt_exp_parsing_ignores_jwt_without_exp() {
        let token = test_jwt_with_payload(r#"{"sub":"user"}"#);

        assert_eq!(jwt_exp_unverified(&token), None);
    }
}
