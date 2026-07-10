//! MCP client manager: spawns user-configured stdio MCP servers as child
//! processes, keeps a catalog of their tools, and executes tool calls for
//! the voice-command pipeline.

use crate::settings::{get_settings, McpServerConfig};
use anyhow::{anyhow, Result};
use log::{info, warn};
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

/// A tool discovered on a connected server, flattened for the intent
/// pipeline and the settings UI.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct McpToolInfo {
    pub server_id: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    /// MCP `readOnlyHint` annotation: `Some(true)` means the tool does not
    /// modify its environment (safe to auto-run).
    pub read_only_hint: Option<bool>,
}

type Connection = RunningService<RoleClient, ()>;

pub struct McpManager {
    app: AppHandle,
    connections: Mutex<HashMap<String, Connection>>,
    catalog: RwLock<Vec<McpToolInfo>>,
}

fn build_command(config: &McpServerConfig) -> Command {
    Command::new(&config.command).configure(|cmd| {
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
    })
}

async fn connect(config: &McpServerConfig) -> Result<(Connection, Vec<McpToolInfo>)> {
    let transport = TokioChildProcess::new(build_command(config))
        .map_err(|e| anyhow!("failed to spawn '{}': {e}", config.command))?;
    let client = ()
        .serve(transport)
        .await
        .map_err(|e| anyhow!("MCP handshake with '{}' failed: {e}", config.name))?;

    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| anyhow!("listing tools on '{}' failed: {e}", config.name))?
        .into_iter()
        .map(|tool| McpToolInfo {
            server_id: config.id.clone(),
            name: tool.name.to_string(),
            description: tool.description.as_ref().map(|d| d.to_string()),
            input_schema: Value::Object((*tool.input_schema).clone()),
            read_only_hint: tool.annotations.as_ref().and_then(|a| a.read_only_hint),
        })
        .collect();

    Ok((client, tools))
}

impl McpManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            connections: Mutex::new(HashMap::new()),
            catalog: RwLock::new(Vec::new()),
        }
    }

    /// Current flattened tool catalog across all connected servers.
    pub async fn tool_catalog(&self) -> Vec<McpToolInfo> {
        self.catalog.read().await.clone()
    }

    /// Reconcile running connections with the settings: spawn newly enabled
    /// servers, kill removed/disabled ones, and rebuild the tool catalog.
    /// Config changes restart the affected server (kill + respawn).
    pub async fn sync_with_settings(&self) {
        let settings = get_settings(&self.app);
        let desired: Vec<McpServerConfig> = if settings.mcp_enabled {
            settings
                .mcp_servers
                .iter()
                .filter(|s| s.enabled)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let mut connections = self.connections.lock().await;

        // Kill connections that are no longer desired.
        let desired_ids: Vec<&str> = desired.iter().map(|s| s.id.as_str()).collect();
        let stale: Vec<String> = connections
            .keys()
            .filter(|id| !desired_ids.contains(&id.as_str()))
            .cloned()
            .collect();
        for id in stale {
            if let Some(conn) = connections.remove(&id) {
                info!("Stopping MCP server '{id}'");
                let _ = conn.cancel().await;
            }
        }

        // Spawn missing ones and rebuild the catalog.
        let mut catalog = Vec::new();
        for config in &desired {
            if !connections.contains_key(&config.id) {
                match connect(config).await {
                    Ok((conn, tools)) => {
                        info!(
                            "Connected MCP server '{}' ({} tool(s))",
                            config.name,
                            tools.len()
                        );
                        connections.insert(config.id.clone(), conn);
                        catalog.extend(tools);
                    }
                    Err(e) => warn!("MCP server '{}' unavailable: {e:#}", config.name),
                }
            } else {
                // Already connected: keep its existing catalog entries.
                let existing = self.catalog.read().await;
                catalog.extend(
                    existing
                        .iter()
                        .filter(|t| t.server_id == config.id)
                        .cloned(),
                );
            }
        }

        *self.catalog.write().await = catalog;
    }

    /// Restart one server (used after its config is edited).
    pub async fn restart_server(&self, server_id: &str) {
        if let Some(conn) = self.connections.lock().await.remove(server_id) {
            let _ = conn.cancel().await;
        }
        self.sync_with_settings().await;
    }

    /// Spawn a server from an unsaved config, list its tools, and shut it
    /// down again — the settings UI's "Test" button.
    pub async fn test_server(config: McpServerConfig) -> Result<Vec<McpToolInfo>, String> {
        match connect(&config).await {
            Ok((conn, tools)) => {
                let _ = conn.cancel().await;
                Ok(tools)
            }
            Err(e) => Err(format!("{e:#}")),
        }
    }

    /// Execute a tool call. Returns a human-readable summary of the result.
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String, String> {
        let connections = self.connections.lock().await;
        let conn = connections
            .get(server_id)
            .ok_or_else(|| format!("MCP server '{server_id}' is not connected"))?;

        let args_object = match arguments {
            Value::Object(map) => Some(map),
            Value::Null => None,
            other => {
                return Err(format!(
                    "tool arguments must be a JSON object, got: {other}"
                ))
            }
        };
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = args_object {
            params = params.with_arguments(args);
        }

        let result = conn
            .call_tool(params)
            .await
            .map_err(|e| format!("tool call failed: {e}"))?;

        let summary = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error == Some(true) {
            return Err(if summary.is_empty() {
                "tool reported an error".to_string()
            } else {
                summary
            });
        }
        Ok(summary)
    }

    /// Cancel all connections (app shutdown). Not wired to an exit hook yet —
    /// stdio children die with the app — but kept for a clean-exit follow-up.
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;
        for (id, conn) in connections.drain() {
            info!("Stopping MCP server '{id}'");
            let _ = conn.cancel().await;
        }
        self.catalog.write().await.clear();
    }
}

/// Helper for the settings-change commands: re-sync in the background.
pub fn sync_in_background(manager: &Arc<McpManager>) {
    let manager = Arc::clone(manager);
    tauri::async_runtime::spawn(async move {
        manager.sync_with_settings().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end smoke test against a real MCP server. Ignored by default:
    /// needs npx + network. Run with:
    ///   cargo test mcp_filesystem_server_end_to_end -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn mcp_filesystem_server_end_to_end() {
        let config = McpServerConfig {
            id: "fs-test".to_string(),
            name: "Filesystem".to_string(),
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                "/tmp".to_string(),
            ],
            env: HashMap::new(),
            enabled: true,
        };

        let tools = McpManager::test_server(config.clone())
            .await
            .expect("filesystem server should connect and list tools");
        assert!(
            tools.iter().any(|t| t.name == "list_directory"),
            "expected list_directory in {:?}",
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        // Full call round-trip through a live connection.
        let (conn, _) = connect(&config).await.expect("connect");
        let mut params = CallToolRequestParams::new("list_directory".to_string());
        let mut args = serde_json::Map::new();
        args.insert("path".to_string(), serde_json::json!("/tmp"));
        params = params.with_arguments(args);
        let result = conn.call_tool(params).await.expect("call_tool");
        assert_ne!(result.is_error, Some(true));
        let _ = conn.cancel().await;
    }
}
