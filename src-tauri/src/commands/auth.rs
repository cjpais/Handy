//! Supabase authentication commands
//!
//! Security: Auth tokens stored in credentials.json, never returned in full.
//! Session refresh handled automatically by frontend Supabase client.

use crate::auth_server::AuthServer;
use log::info;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

const CREDENTIALS_STORE_PATH: &str = "credentials.json";
const SUPABASE_SESSION_KEY: &str = "supabase_session";

/// Manages the temporary auth server
pub struct AuthManager {
    server: Mutex<Option<AuthServer>>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            server: Mutex::new(None),
        }
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SupabaseSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
    /// User's display name from OAuth provider
    pub name: Option<String>,
    /// User's avatar URL from OAuth provider
    pub avatar_url: Option<String>,
    /// OAuth provider used (github, discord, twitch)
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub provider: Option<String>,
    pub is_authenticated: bool,
}

/// Start the OAuth callback server and return the callback URL
/// Call this before opening the OAuth URL in the browser
#[tauri::command]
#[specta::specta]
pub fn auth_start_server(
    app: AppHandle,
    auth_manager: State<'_, Arc<AuthManager>>,
) -> Result<String, String> {
    let mut server_guard = auth_manager.server.lock().unwrap();

    // Shutdown any existing server
    if let Some(mut old_server) = server_guard.take() {
        old_server.shutdown();
    }

    // Start new server
    let server = AuthServer::start(app)?;
    let callback_url = server.callback_url();

    info!("Auth server started, callback URL: {}", callback_url);

    *server_guard = Some(server);
    Ok(callback_url)
}

/// Stop the OAuth callback server (called after auth completes or on cancel)
#[tauri::command]
#[specta::specta]
pub fn auth_stop_server(auth_manager: State<'_, Arc<AuthManager>>) {
    let mut server_guard = auth_manager.server.lock().unwrap();
    if let Some(mut server) = server_guard.take() {
        server.shutdown();
        info!("Auth server stopped");
    }
}

/// Save Supabase session to secure credentials store
#[tauri::command]
#[specta::specta]
pub fn auth_save_session(app: AppHandle, session: SupabaseSession) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.set(
        SUPABASE_SESSION_KEY,
        serde_json::to_value(&session).unwrap(),
    );
    store
        .save()
        .map_err(|e| format!("Failed to save session: {}", e))?;

    info!("Supabase session saved for user: {}", session.user_id);
    Ok(())
}

/// Load Supabase session from credentials store
#[tauri::command]
#[specta::specta]
pub fn auth_get_session(app: AppHandle) -> Option<SupabaseSession> {
    let store = app.store(CREDENTIALS_STORE_PATH).ok()?;
    store
        .get(SUPABASE_SESSION_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Get current authenticated user info (safe to expose)
#[tauri::command]
#[specta::specta]
pub fn auth_get_user(app: AppHandle) -> AuthUser {
    match auth_get_session(app) {
        Some(session) => AuthUser {
            id: session.user_id,
            email: session.email,
            name: session.name,
            avatar_url: session.avatar_url,
            provider: session.provider,
            is_authenticated: true,
        },
        None => AuthUser {
            id: String::new(),
            email: None,
            name: None,
            avatar_url: None,
            provider: None,
            is_authenticated: false,
        },
    }
}

/// Clear auth session (logout)
#[tauri::command]
#[specta::specta]
pub fn auth_logout(app: AppHandle) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.delete(SUPABASE_SESSION_KEY);
    store.save().map_err(|e| format!("Failed to save: {}", e))?;

    info!("User logged out, session cleared");
    Ok(())
}

/// Check if user is authenticated
#[tauri::command]
#[specta::specta]
pub fn auth_is_authenticated(app: AppHandle) -> bool {
    auth_get_session(app).is_some()
}

/// Get the access token for API requests
/// Returns None if not authenticated or token is expired
#[tauri::command]
#[specta::specta]
pub fn auth_get_access_token(app: AppHandle) -> Option<String> {
    let session = auth_get_session(app)?;

    // Check if token is expired (with 60 second buffer)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    if session.expires_at > 0 && session.expires_at < now + 60 {
        info!("Access token expired");
        return None;
    }

    Some(session.access_token)
}
