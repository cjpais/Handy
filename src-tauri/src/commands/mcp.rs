use crate::managers::mcp::{sync_in_background, McpManager, McpToolInfo};
use crate::settings::{get_settings, write_settings, McpServerConfig};
use crate::shortcut::{register_shortcut, unregister_shortcut};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub fn change_mcp_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.mcp_enabled = enabled;
    write_settings(&app, settings.clone());

    // Register or unregister the voice-command shortcut (mirrors the
    // post-processing binding gating).
    if let Some(binding) = settings.bindings.get("voice_command").cloned() {
        if enabled {
            let _ = register_shortcut(&app, binding);
        } else {
            let _ = unregister_shortcut(&app, binding);
        }
    }

    sync_in_background(&app.state::<Arc<McpManager>>());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_mcp_servers(app: AppHandle) -> Vec<McpServerConfig> {
    get_settings(&app).mcp_servers
}

/// Add or update a server config (matched by id) and re-sync connections.
#[tauri::command]
#[specta::specta]
pub fn upsert_mcp_server(app: AppHandle, config: McpServerConfig) -> Result<(), String> {
    if config.id.trim().is_empty() || config.command.trim().is_empty() {
        return Err("Server id and command are required".to_string());
    }
    let mut settings = get_settings(&app);
    let server_id = config.id.clone();
    let is_update = match settings.mcp_servers.iter_mut().find(|s| s.id == config.id) {
        Some(existing) => {
            *existing = config;
            true
        }
        None => {
            settings.mcp_servers.push(config);
            false
        }
    };
    write_settings(&app, settings);
    let manager = Arc::clone(&app.state::<Arc<McpManager>>());
    tauri::async_runtime::spawn(async move {
        if is_update {
            // An edited config must respawn the process, not keep the old one.
            manager.restart_server(&server_id).await;
        } else {
            manager.sync_with_settings().await;
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn remove_mcp_server(app: AppHandle, server_id: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.mcp_servers.retain(|s| s.id != server_id);
    // Drop any allowlist entries for that server too.
    settings
        .mcp_auto_approved_tools
        .retain(|t| !t.starts_with(&format!("{server_id}/")));
    write_settings(&app, settings);
    sync_in_background(&app.state::<Arc<McpManager>>());
    Ok(())
}

/// Spawn a (possibly unsaved) server config and return its tools — the
/// settings UI "Test" button.
#[tauri::command]
#[specta::specta]
pub async fn test_mcp_server(config: McpServerConfig) -> Result<Vec<McpToolInfo>, String> {
    McpManager::test_server(config).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_mcp_tool_catalog(app: AppHandle) -> Vec<McpToolInfo> {
    app.state::<Arc<McpManager>>().tool_catalog().await
}

/// Toggle a tool ("server_id/tool_name") on the auto-approve allowlist.
#[tauri::command]
#[specta::specta]
pub fn set_mcp_tool_auto_approved(
    app: AppHandle,
    tool: String,
    approved: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    if approved {
        if !settings.mcp_auto_approved_tools.contains(&tool) {
            settings.mcp_auto_approved_tools.push(tool);
        }
    } else {
        settings.mcp_auto_approved_tools.retain(|t| t != &tool);
    }
    write_settings(&app, settings);
    Ok(())
}
