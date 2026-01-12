//! Local OAuth callback server
//!
//! Spawns a temporary HTTP server to receive OAuth callbacks from Supabase.
//! More reliable than deep links, especially on Linux.

use log::{error, info};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const AUTH_CALLBACK_EVENT: &str = "auth://callback";
const AUTH_SERVER_PORT: u16 = 4321;

/// HTML page shown to user after successful OAuth redirect
const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authentication Successful</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
        }
        .container {
            text-align: center;
            padding: 40px;
            background: rgba(255,255,255,0.1);
            border-radius: 16px;
            backdrop-filter: blur(10px);
        }
        h1 { margin-bottom: 16px; }
        p { opacity: 0.9; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Authentication Successful</h1>
        <p>You can close this window and return to the app.</p>
    </div>
</body>
</html>"#;

/// HTML page that extracts tokens from URL fragment and sends to server
/// This handles Supabase's implicit flow which returns tokens in fragment (#)
const FRAGMENT_HANDLER_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Processing Authentication...</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
        }
        .container {
            text-align: center;
            padding: 40px;
            background: rgba(255,255,255,0.1);
            border-radius: 16px;
            backdrop-filter: blur(10px);
        }
        h1 { margin-bottom: 16px; }
        p { opacity: 0.9; }
        .spinner {
            border: 3px solid rgba(255,255,255,0.3);
            border-top: 3px solid white;
            border-radius: 50%;
            width: 30px;
            height: 30px;
            animation: spin 1s linear infinite;
            margin: 20px auto;
        }
        @keyframes spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Processing Authentication...</h1>
        <div class="spinner"></div>
        <p id="status">Please wait...</p>
    </div>
    <script>
        function sendToken() {
            // Check for tokens in URL fragment (implicit flow)
            const hash = window.location.hash.substring(1);
            console.log('Hash fragment:', hash ? 'present' : 'empty');

            if (hash) {
                const params = new URLSearchParams(hash);
                const accessToken = params.get('access_token');
                const refreshToken = params.get('refresh_token');

                console.log('Access token found:', !!accessToken);

                if (accessToken) {
                    const tokenData = {
                        access_token: accessToken,
                        refresh_token: refreshToken || '',
                        expires_in: params.get('expires_in') || '3600',
                        token_type: params.get('token_type') || 'bearer'
                    };

                    console.log('Sending token to server...');

                    // Send tokens to server via POST with retry
                    const sendRequest = (attempt) => {
                        fetch('http://127.0.0.1:4321/auth/token', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(tokenData)
                        }).then(response => {
                            console.log('Response status:', response.status);
                            if (response.ok) {
                                document.getElementById('status').textContent = 'Success! You can close this window.';
                                document.querySelector('h1').textContent = 'Authentication Successful';
                                document.querySelector('.spinner').style.display = 'none';
                            } else {
                                throw new Error('Server returned ' + response.status);
                            }
                        }).catch(err => {
                            console.error('Fetch error:', err);
                            if (attempt < 3) {
                                console.log('Retrying in 500ms...');
                                setTimeout(() => sendRequest(attempt + 1), 500);
                            } else {
                                document.getElementById('status').textContent = 'Error: ' + err.message;
                            }
                        });
                    };

                    // Small delay to ensure server is ready for the POST
                    setTimeout(() => sendRequest(1), 100);
                } else {
                    document.getElementById('status').textContent = 'No access token found in URL.';
                }
            } else {
                document.getElementById('status').textContent = 'No authentication data found. Please try again.';
            }
        }

        // Run when DOM is ready
        if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', sendToken);
        } else {
            sendToken();
        }
    </script>
</body>
</html>"#;

/// HTML page shown on error
const ERROR_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authentication Failed</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #f44336;
            color: white;
        }
        .container { text-align: center; padding: 40px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Authentication Failed</h1>
        <p>Please try again from the app.</p>
    </div>
</body>
</html>"#;

pub struct AuthServer {
    port: u16,
    shutdown_tx: Option<Sender<()>>,
}

impl AuthServer {
    /// Start the auth server on the fixed port 4321
    pub fn start(app_handle: AppHandle) -> Result<Self, String> {
        // Bind to fixed port 4321 (whitelisted in Supabase)
        let listener =
            TcpListener::bind(format!("127.0.0.1:{}", AUTH_SERVER_PORT)).map_err(|e| {
                format!(
                    "Failed to bind auth server on port {}: {}",
                    AUTH_SERVER_PORT, e
                )
            })?;

        let port = AUTH_SERVER_PORT;

        // Set non-blocking so we can check for shutdown
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        info!("Auth server started on port {}", port);

        // Spawn listener thread
        thread::spawn(move || {
            Self::run_server(listener, app_handle, shutdown_rx);
        });

        Ok(Self {
            port,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Get the callback URL for this server
    pub fn callback_url(&self) -> String {
        format!("http://127.0.0.1:{}/auth/callback", self.port)
    }

    /// Get the port
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Shutdown the server
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    fn run_server(listener: TcpListener, app_handle: AppHandle, shutdown_rx: Receiver<()>) {
        loop {
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                info!("Auth server shutting down");
                break;
            }

            // Try to accept a connection (non-blocking)
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if Self::handle_connection(stream, &app_handle) {
                        // Successfully handled auth callback, shutdown
                        info!("Auth callback received, server shutting down");
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection yet, sleep briefly and try again
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Auth server accept error: {}", e);
                }
            }
        }
    }

    fn handle_connection(mut stream: TcpStream, app_handle: &AppHandle) -> bool {
        let mut buffer = [0; 8192];

        if let Err(e) = stream.read(&mut buffer) {
            error!("Failed to read request: {}", e);
            return false;
        }

        let request = String::from_utf8_lossy(&buffer);

        // Parse the request line
        let first_line = request.lines().next().unwrap_or("");
        info!("Auth server received request: {}", first_line);

        // Handle OPTIONS preflight for CORS
        if first_line.starts_with("OPTIONS") {
            Self::handle_options(&mut stream);
            return false;
        }

        // Handle POST /auth/token (from fragment handler JavaScript)
        if first_line.starts_with("POST") && first_line.contains("/auth/token") {
            return Self::handle_token_post(&request, &mut stream, app_handle);
        }

        // Check if this is the callback path
        if !first_line.contains("/auth/callback") {
            // Not the callback, return 404
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
            return false;
        }

        // Extract query string from GET /auth/callback?code=XXX HTTP/1.1
        let query_start = first_line.find('?');
        let query_end = first_line.rfind(" HTTP");

        let (code, error_msg) = if let (Some(start), Some(end)) = (query_start, query_end) {
            let query = &first_line[start + 1..end];
            Self::parse_query(query)
        } else {
            (None, None) // No query params - might have fragment instead
        };

        // If we have a code in query params, use PKCE flow
        if let Some(auth_code) = code {
            info!("Auth code received (PKCE flow), emitting to frontend");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                SUCCESS_HTML.len(),
                SUCCESS_HTML
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = app_handle.emit(AUTH_CALLBACK_EVENT, auth_code);
            return true;
        }

        // If we have an error in query params
        if let Some(err) = error_msg {
            error!("Auth error in query: {}", err);
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                ERROR_HTML.len(),
                ERROR_HTML
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = app_handle.emit("auth://error", err);
            return false;
        }

        // No code and no error - serve the fragment handler HTML
        // This handles implicit flow where tokens are in the URL fragment (#)
        // The JavaScript will extract them and POST to /auth/token
        info!("No query params, serving fragment handler for implicit flow");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            FRAGMENT_HANDLER_HTML.len(),
            FRAGMENT_HANDLER_HTML
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        false // Don't shutdown yet, wait for the POST from JavaScript
    }

    /// Handle POST /auth/token from the fragment handler JavaScript
    fn handle_token_post(request: &str, stream: &mut TcpStream, app_handle: &AppHandle) -> bool {
        // Find the JSON body (after the empty line)
        let body_start = request
            .find("\r\n\r\n")
            .map(|i| i + 4)
            .or_else(|| request.find("\n\n").map(|i| i + 2));

        if let Some(start) = body_start {
            let body = request[start..].trim_end_matches('\0');
            info!("Received token POST body: {} bytes", body.len());

            // Parse the JSON to extract access_token
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(_access_token) = json.get("access_token").and_then(|v| v.as_str()) {
                    info!("Access token received (implicit flow), emitting to frontend");

                    // Emit the token data as JSON so frontend can handle it
                    let _ = app_handle.emit("auth://token", body.to_string());

                    // Include CORS headers for same-origin requests
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"success\":true}";
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    return true;
                }
            }
        }

        error!("Failed to parse token from POST body");
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"success\":false}";
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        false
    }

    /// Handle OPTIONS preflight request for CORS
    fn handle_options(stream: &mut TcpStream) {
        let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }

    fn parse_query(query: &str) -> (Option<String>, Option<String>) {
        let mut code = None;
        let mut error_msg = None;

        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");

            match key {
                "code" => code = Some(Self::url_decode(value)),
                "error" => error_msg = Some(Self::url_decode(value)),
                "error_description" => {
                    if error_msg.is_none() {
                        error_msg = Some(Self::url_decode(value));
                    }
                }
                _ => {}
            }
        }

        (code, error_msg)
    }

    /// Simple URL decoding (handles common cases)
    fn url_decode(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                } else {
                    result.push('%');
                    result.push_str(&hex);
                }
            } else if c == '+' {
                result.push(' ');
            } else {
                result.push(c);
            }
        }

        result
    }
}

impl Drop for AuthServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
