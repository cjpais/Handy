use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::TcpListener;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

// Placeholder Client ID - User should ideally provide this via settings or env
const CLIENT_ID: &str = "104169727409-77n7v7n7v7n7v7n7v7n7v7n7v7n7v7n7.apps.googleusercontent.com"; // Example placeholder
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub token_type: String,
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

    pub async fn start_oauth_flow() -> Result<TokenResponse> {
        let verifier = Self::generate_verifier();
        let challenge = Self::generate_challenge(&verifier);

        // Find an available port for the redirect
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{}", port);

        let mut auth_url = Url::parse(AUTH_URL)?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair(
                "scope",
                "https://www.googleapis.com/auth/gmail.send https://www.googleapis.com/auth/tasks",
            )
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        // Open browser
        log::info!("Opening browser for Google OAuth: {}", auth_url);
        opener::open(auth_url.as_str())?;

        // Wait for the callback
        let (mut stream, _) = listener.accept()?;
        let mut buffer = [0; 1024];
        let n = std::io::Read::read(&mut stream, &mut buffer)?;
        let request = String::from_utf8_lossy(&buffer[..n]);

        // Parse code from request (very basic parsing)
        let code = if let Some(code_idx) = request.find("code=") {
            let start = code_idx + 5;
            let end = request[start..].find(' ').unwrap_or(request[start..].len());
            request[start..start + end]
                .split('&')
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            return Err(anyhow!("Failed to find code in redirect request"));
        };

        // Send a simple success response to the browser
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authentication Successful!</h1><p>You can close this window now.</p></body></html>";
        std::io::Write::write_all(&mut stream, response.as_bytes())?;

        // Exchange code for tokens
        Self::exchange_code_for_token(&code, &verifier, &redirect_uri).await
    }

    pub async fn exchange_code_for_token(
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse> {
        let client = reqwest::Client::new();
        let params = [
            ("client_id", CLIENT_ID),
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

        let token_response = response.json::<TokenResponse>().await?;
        Ok(token_response)
    }

    pub async fn refresh_token(refresh_token: &str) -> Result<TokenResponse> {
        let client = reqwest::Client::new();
        let params = [
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let response = client.post(TOKEN_URL).form(&params).send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token refresh failed: {}", error_text));
        }

        let token_response = response.json::<TokenResponse>().await?;
        Ok(token_response)
    }
}
