//! Memory management commands for the frontend

use crate::memory::{EmbeddingModelInfo, MemoryManager, MemoryMessage, MemoryUserInfo};
use log::info;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::task::spawn_blocking;

/// Status information about the memory system
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MemoryStatus {
    pub is_running: bool,
    pub model_loaded: bool,
    pub total_memories: usize,
}

/// Get the current status of the memory system
#[tauri::command]
#[specta::specta]
pub fn get_memory_status(app: AppHandle) -> Result<MemoryStatus, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>();

    // Check if sidecar is running by attempting a status query
    let is_running = memory_manager.is_running();

    Ok(MemoryStatus {
        is_running,
        model_loaded: is_running, // Model is loaded if sidecar is running
        total_memories: 0,        // Will be updated when we have count_all
    })
}

/// Query memories semantically (for all users)
#[tauri::command]
#[specta::specta]
pub async fn query_all_memories(
    app: AppHandle,
    query: String,
    limit: usize,
) -> Result<Vec<MemoryMessage>, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Querying all memories with: {}", query);

    spawn_blocking(move || memory_manager.query_all(&query, limit))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Get total count of all memories
/// Note: Returns 0 if sidecar is not running to avoid spawning it just to check count
#[tauri::command]
#[specta::specta]
pub fn get_memory_count(app: AppHandle) -> Result<usize, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>();

    // Only get count if sidecar is already running to avoid spawning it
    // (which would load the embedding model and potentially cause memory pressure)
    if !memory_manager.is_running() {
        return Ok(0);
    }

    memory_manager.get_total_count()
}

/// Clear all memories
#[tauri::command]
#[specta::specta]
pub async fn clear_all_memories(app: AppHandle) -> Result<u32, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Clearing all memories");
    spawn_blocking(move || memory_manager.clear_all())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Cleanup old memories based on TTL
#[tauri::command]
#[specta::specta]
pub async fn cleanup_old_memories(app: AppHandle, ttl_days: u32) -> Result<u32, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Cleaning up memories older than {} days", ttl_days);
    spawn_blocking(move || memory_manager.cleanup_expired(ttl_days))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// List available embedding models
#[tauri::command]
#[specta::specta]
pub async fn list_embedding_models(app: AppHandle) -> Result<Vec<EmbeddingModelInfo>, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Listing available embedding models");
    spawn_blocking(move || memory_manager.list_models())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Load an embedding model by ID
#[tauri::command]
#[specta::specta]
pub async fn load_embedding_model(app: AppHandle, model_id: String) -> Result<String, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Loading embedding model: {}", model_id);
    spawn_blocking(move || memory_manager.load_model(&model_id))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Get the currently loaded embedding model
#[tauri::command]
#[specta::specta]
pub async fn get_current_embedding_model(
    app: AppHandle,
) -> Result<Option<EmbeddingModelInfo>, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    spawn_blocking(move || memory_manager.get_current_model())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Stop the memory sidecar
#[tauri::command]
#[specta::specta]
pub async fn stop_memory_sidecar(app: AppHandle) -> Result<(), String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Stopping memory sidecar");
    spawn_blocking(move || {
        memory_manager.shutdown();
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;
    Ok(())
}

/// Browse recent memories without semantic search
/// Supports filtering by user_id and is_bot
#[tauri::command]
#[specta::specta]
pub async fn browse_recent_memories(
    app: AppHandle,
    limit: usize,
    user_id: Option<String>,
    is_bot: Option<bool>,
) -> Result<Vec<MemoryMessage>, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!(
        "Browsing recent memories: limit={}, user={:?}, is_bot={:?}",
        limit, user_id, is_bot
    );

    spawn_blocking(move || memory_manager.browse_recent(limit, user_id, is_bot))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// List unique users with memory counts
#[tauri::command]
#[specta::specta]
pub async fn list_memory_users(app: AppHandle) -> Result<Vec<MemoryUserInfo>, String> {
    let memory_manager = app.state::<Arc<MemoryManager>>().inner().clone();

    info!("Listing memory users");

    spawn_blocking(move || memory_manager.list_users())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}
