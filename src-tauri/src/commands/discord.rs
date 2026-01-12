//! Discord bot commands for the frontend
//!
//! Security: The Discord bot token is stored in a separate store file and is NEVER
//! returned in full to the frontend. Only a masked version is shown for confirmation.

use crate::discord::{ChannelInfo, DiscordManager, DiscordState, GuildInfo};
use crate::discord_conversation::DiscordConversationManager;
use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

/// Separate store for sensitive credentials (not in main settings)
const CREDENTIALS_STORE_PATH: &str = "credentials.json";
const DISCORD_TOKEN_KEY: &str = "discord_bot_token";

/// Mask a token for display, showing only the last 4 characters
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        "*".repeat(token.len())
    } else {
        format!("{}...{}", "*".repeat(8), &token[token.len() - 4..])
    }
}

/// Save the Discord token securely to the credentials store
fn save_token_to_store(app: &AppHandle, token: &str) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.set(DISCORD_TOKEN_KEY, serde_json::json!(token));
    store
        .save()
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    info!("Discord token saved to secure store");
    Ok(())
}

/// Load the Discord token from the credentials store (internal use only)
fn load_token_from_store(app: &AppHandle) -> Option<String> {
    let store = app.store(CREDENTIALS_STORE_PATH).ok()?;
    store
        .get(DISCORD_TOKEN_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Delete the Discord token from the credentials store
fn delete_token_from_store(app: &AppHandle) -> Result<(), String> {
    let store = app
        .store(CREDENTIALS_STORE_PATH)
        .map_err(|e| format!("Failed to access credentials store: {}", e))?;

    store.delete(DISCORD_TOKEN_KEY);
    store
        .save()
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    info!("Discord token deleted from secure store");
    Ok(())
}

/// Check if a Discord bot token is configured (without returning the actual token)
#[tauri::command]
#[specta::specta]
pub fn discord_has_token(app: AppHandle) -> bool {
    load_token_from_store(&app).is_some()
}

/// Get a masked version of the Discord bot token for display purposes only
/// Returns None if no token is set, or a masked string like "********...abcd"
#[tauri::command]
#[specta::specta]
pub fn discord_get_token(app: AppHandle) -> Option<String> {
    load_token_from_store(&app).map(|t| mask_token(&t))
}

/// Set the Discord bot token (stores securely, never echoed back in full)
#[tauri::command]
#[specta::specta]
pub fn discord_set_token(app: AppHandle, token: String) -> Result<(), String> {
    // Validate token format (Discord tokens have a specific structure)
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Token cannot be empty".to_string());
    }

    // Discord bot tokens typically have 3 parts separated by dots
    // Format: base64.base64.base64 (total ~59-72 chars)
    if !token.contains('.') || token.len() < 50 {
        return Err("Invalid Discord bot token format".to_string());
    }

    // Save to secure store
    save_token_to_store(&app, &token)?;

    // Update the manager's in-memory token
    let manager = app.state::<Arc<DiscordManager>>();
    manager.set_token(token)?;

    Ok(())
}

/// Clear the stored Discord bot token
#[tauri::command]
#[specta::specta]
pub fn discord_clear_token(app: AppHandle) -> Result<(), String> {
    delete_token_from_store(&app)?;

    // Disconnect if connected
    let manager = app.state::<Arc<DiscordManager>>();
    let _ = manager.disconnect();

    Ok(())
}

/// Connect to Discord using the stored token
#[tauri::command]
#[specta::specta]
pub fn discord_connect_with_stored_token(app: AppHandle) -> Result<(), String> {
    let token = load_token_from_store(&app)
        .ok_or_else(|| "No Discord token configured. Please set a bot token first.".to_string())?;

    let manager = app.state::<Arc<DiscordManager>>();
    manager.set_token(token)?;
    manager.connect()
}

/// Get Discord connection status
#[tauri::command]
#[specta::specta]
pub fn discord_get_status(app: AppHandle) -> DiscordState {
    let manager = app.state::<Arc<DiscordManager>>();
    manager.status()
}

/// Get list of guilds the bot is in
#[tauri::command]
#[specta::specta]
pub fn discord_get_guilds(app: AppHandle) -> Result<Vec<GuildInfo>, String> {
    let manager = app.state::<Arc<DiscordManager>>();
    manager.get_guilds()
}

/// Get voice channels for a guild
#[tauri::command]
#[specta::specta]
pub fn discord_get_channels(app: AppHandle, guild_id: String) -> Result<Vec<ChannelInfo>, String> {
    let manager = app.state::<Arc<DiscordManager>>();
    manager.get_channels(&guild_id)
}

/// Connect to a Discord voice channel
#[tauri::command]
#[specta::specta]
pub fn discord_connect(app: AppHandle, guild_id: String, channel_id: String) -> Result<(), String> {
    use tauri::Emitter;
    info!(
        "discord_connect command called: guild={}, channel={}",
        guild_id, channel_id
    );

    let manager = app.state::<Arc<DiscordManager>>();
    let result = manager.join_voice(&guild_id, &channel_id);

    match &result {
        Ok(_) => info!("Successfully joined voice channel"),
        Err(e) => info!("Failed to join voice channel: {}", e),
    }

    // Emit state update to frontend
    let state = manager.status();
    let _ = app.emit("discord-state", state);

    result
}

/// Disconnect from Discord voice
#[tauri::command]
#[specta::specta]
pub fn discord_disconnect(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let manager = app.state::<Arc<DiscordManager>>();
    let result = manager.disconnect();

    // Emit state update to frontend
    let state = manager.status();
    let _ = app.emit("discord-state", state);

    result
}

/// Speak text in the voice channel
#[tauri::command]
#[specta::specta]
pub fn discord_speak(app: AppHandle, text: String) -> Result<(), String> {
    let manager = app.state::<Arc<DiscordManager>>();
    manager.speak(&text)
}

/// Start Discord conversation mode (listen and respond to voice in Discord)
#[tauri::command]
#[specta::specta]
pub fn discord_start_conversation(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<Arc<DiscordConversationManager>>();
    manager.start()
}

/// Stop Discord conversation mode
#[tauri::command]
#[specta::specta]
pub fn discord_stop_conversation(app: AppHandle) {
    let manager = app.state::<Arc<DiscordConversationManager>>();
    manager.stop()
}

/// Check if Discord conversation mode is running
#[tauri::command]
#[specta::specta]
pub fn discord_is_conversation_running(app: AppHandle) -> bool {
    let manager = app.state::<Arc<DiscordConversationManager>>();
    manager.is_running()
}
