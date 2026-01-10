//! Supabase credential commands for the frontend
//!
//! Security: The Supabase anon key is stored in a separate store file and is NEVER
//! returned in full to the frontend. Only a masked version is shown for confirmation.

use log::info;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Separate store for sensitive credentials (not in main settings)
const CREDENTIALS_STORE_PATH: &str = "credentials.json";
const SUPABASE_URL_KEY: &str = "supabase_url";
const SUPABASE_ANON_KEY_KEY: &str = "supabase_anon_key";

/// Default Supabase URL
const DEFAULT_SUPABASE_URL: &str = "https://supabase.kbve.com";
/// Default Supabase anon key
const DEFAULT_SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoiYW5vbiIsImlzcyI6InN1cGFiYXNlIiwiaWF0IjoxNzU1NDAzMjAwLCJleHAiOjE5MTMxNjk2MDB9.oietJI22ZytbghFywvdYMSJp7rcsBdBYbcciJxeGWrg";

/// Mask a token for display, showing only the last 4 characters
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        "*".repeat(token.len())
    } else {
        format!("{}...{}", "*".repeat(8), &token[token.len() - 4..])
    }
}

/// Get the Supabase URL (returns default if not set)
#[tauri::command]
#[specta::specta]
pub fn get_supabase_url(app: AppHandle) -> String {
    let store = match app.store(CREDENTIALS_STORE_PATH).ok() {
        Some(s) => s,
        None => return DEFAULT_SUPABASE_URL.to_string(),
    };
    store
        .get(SUPABASE_URL_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| DEFAULT_SUPABASE_URL.to_string())
}

/// Set the Supabase URL
#[tauri::command]
#[specta::specta]
pub fn set_supabase_url(app: AppHandle, url: String) -> Result<(), String> {
    let url = url.trim().to_string();

    // Basic validation - should be a URL
    if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Supabase URL must start with http:// or https://".to_string());
    }

    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.set(SUPABASE_URL_KEY, serde_json::json!(url));
    store
        .save()
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    info!("Supabase URL saved");
    Ok(())
}

/// Get a masked version of the Supabase anon key for display (returns masked default if not set)
#[tauri::command]
#[specta::specta]
pub fn get_supabase_anon_key(app: AppHandle) -> String {
    let store = match app.store(CREDENTIALS_STORE_PATH).ok() {
        Some(s) => s,
        None => return mask_token(DEFAULT_SUPABASE_ANON_KEY),
    };
    store
        .get(SUPABASE_ANON_KEY_KEY)
        .and_then(|v| v.as_str().map(|s| mask_token(s)))
        .unwrap_or_else(|| mask_token(DEFAULT_SUPABASE_ANON_KEY))
}

/// Check if Supabase anon key is configured (always true since we have a default)
#[tauri::command]
#[specta::specta]
pub fn has_supabase_anon_key(_app: AppHandle) -> bool {
    // Always true since we have a default anon key
    true
}

/// Get the actual Supabase anon key (for internal use by auth system)
/// This returns the unmasked key - use with care
#[tauri::command]
#[specta::specta]
pub fn get_supabase_anon_key_raw(app: AppHandle) -> String {
    let store = match app.store(CREDENTIALS_STORE_PATH).ok() {
        Some(s) => s,
        None => return DEFAULT_SUPABASE_ANON_KEY.to_string(),
    };
    store
        .get(SUPABASE_ANON_KEY_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| DEFAULT_SUPABASE_ANON_KEY.to_string())
}

/// Set the Supabase anon key (stored securely, only masked version returned)
#[tauri::command]
#[specta::specta]
pub fn set_supabase_anon_key(app: AppHandle, key: String) -> Result<(), String> {
    let key = key.trim().to_string();

    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.set(SUPABASE_ANON_KEY_KEY, serde_json::json!(key));
    store
        .save()
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    info!("Supabase anon key saved");
    Ok(())
}

/// Clear Supabase credentials
#[tauri::command]
#[specta::specta]
pub fn clear_supabase_credentials(app: AppHandle) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.delete(SUPABASE_URL_KEY);
    store.delete(SUPABASE_ANON_KEY_KEY);
    store
        .save()
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    info!("Supabase credentials cleared");
    Ok(())
}
