use crate::settings::GoogleFeature;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::TcpListener;
use url::Url;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.readonly";
const GMAIL_SEND_SCOPE: &str = "https://www.googleapis.com/auth/gmail.send";
const TASKS_SCOPE: &str = "https://www.googleapis.com/auth/tasks";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub token_type: String,
}

#[derive(Debug, Clone)]
pub struct OAuthRequest {
    pub client_id: String,
    pub scopes: Vec<String>,
}

pub struct GoogleOAuth;

impl GoogleOAuth {
    pub fn generate_verifier() -> String {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(128)
            .map(char::from)
            .collect()
    }

    pub fn generate_challenge(verifier: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let result = hasher.finalize();
        URL_SAFE_NO_PAD.encode(result)
    }

    pub fn scopes_for_features(features: &[GoogleFeature]) -> Vec<String> {
        let mut scopes = Vec::new();
        for feature in features {
            match feature {
                GoogleFeature::GmailTasks => {
                    if !scopes.iter().any(|s| s == GMAIL_SEND_SCOPE) {
                        scopes.push(GMAIL_SEND_SCOPE.to_string());
                    }
                    if !scopes.iter().any(|s| s == TASKS_SCOPE) {
                        scopes.push(TASKS_SCOPE.to_string());
                    }
                }
                GoogleFeature::Calendar => {
                    if !scopes.iter().any(|s| s == CALENDAR_SCOPE) {
                        scopes.push(CALENDAR_SCOPE.to_string());
                    }
                }
            }
        }
        scopes
    }

    pub fn desktop_client_id() -> Option<String> {
        let value = std::env::var("GOOGLE_DESKTOP_CLIENT_ID").ok()?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    pub fn is_client_id_configured() -> bool {
        Self::desktop_client_id().is_some()
    }

    pub fn build_oauth_request(features: &[GoogleFeature]) -> Result<OAuthRequest> {
        let client_id = Self::desktop_client_id()
            .ok_or_else(|| anyhow!("Google desktop OAuth client ID is not configured"))?;
        let scopes = Self::scopes_for_features(features);
        if scopes.is_empty() {
            return Err(anyhow!("No Google scopes requested"));
        }
        Ok(OAuthRequest { client_id, scopes })
    }

    pub async fn start_oauth_flow(features: &[GoogleFeature]) -> Result<TokenResponse> {
        let request = Self::build_oauth_request(features)?;
        let verifier = Self::generate_verifier();
        let challenge = Self::generate_challenge(&verifier);

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}");

        let mut auth_url = Url::parse(AUTH_URL)?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", &request.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &request.scopes.join(" "))
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        log::info!("Opening browser for Google OAuth: {}", auth_url);
        opener::open(auth_url.as_str())?;

        let (mut stream, _) = listener.accept()?;
        let mut buffer = [0; 2048];
        let n = std::io::Read::read(&mut stream, &mut buffer)?;
        let request_text = String::from_utf8_lossy(&buffer[..n]);

        let code = if let Some(code_idx) = request_text.find("code=") {
            let start = code_idx + 5;
            let end = request_text[start..]
                .find(' ')
                .unwrap_or(request_text[start..].len());
            request_text[start..start + end]
                .split('&')
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            return Err(anyhow!("Failed to find code in redirect request"));
        };

        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authentication Successful</h1><p>You can close this window now.</p></body></html>";
        std::io::Write::write_all(&mut stream, response.as_bytes())?;

        Self::exchange_code_for_token(&request.client_id, &code, &verifier, &redirect_uri).await
    }

    pub async fn exchange_code_for_token(
        client_id: &str,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse> {
        let client = reqwest::Client::new();
        let params = [
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ];

        let response = client.post(TOKEN_URL).form(&params).send().await?;
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        Ok(response.json::<TokenResponse>().await?)
    }

    pub async fn refresh_token(
        refresh_token: &str,
        feature: GoogleFeature,
    ) -> Result<TokenResponse> {
        let client_id = Self::desktop_client_id()
            .ok_or_else(|| anyhow!("Google desktop OAuth client ID is not configured"))?;
        let scopes = Self::scopes_for_features(&[feature]);
        let scope_string = scopes.join(" ");
        let client = reqwest::Client::new();
        let params = [
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", scope_string.as_str()),
        ];

        let response = client.post(TOKEN_URL).form(&params).send().await?;
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token refresh failed: {}", error_text));
        }

        Ok(response.json::<TokenResponse>().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_scopes_are_feature_specific() {
        let gmail = GoogleOAuth::scopes_for_features(&[GoogleFeature::GmailTasks]);
        assert!(gmail.iter().any(|s| s == GMAIL_SEND_SCOPE));
        assert!(gmail.iter().any(|s| s == TASKS_SCOPE));
        assert!(!gmail.iter().any(|s| s == CALENDAR_SCOPE));

        let combined =
            GoogleOAuth::scopes_for_features(&[GoogleFeature::Calendar, GoogleFeature::GmailTasks]);
        assert_eq!(combined.len(), 3);
        assert!(combined.iter().any(|s| s == CALENDAR_SCOPE));
    }
}
