//! DevOps-related Tauri commands.

use crate::devops::{
    check_all_dependencies,
    tmux::{
        self, AgentMetadata, RecoveredSession, TmuxSession,
    },
    DevOpsDependencies,
};

/// Check if required DevOps dependencies (gh, tmux) are installed.
#[tauri::command]
#[specta::specta]
pub fn check_devops_dependencies() -> DevOpsDependencies {
    check_all_dependencies()
}

/// List all Handy agent tmux sessions.
#[tauri::command]
#[specta::specta]
pub fn list_tmux_sessions() -> Result<Vec<TmuxSession>, String> {
    tmux::list_sessions()
}

/// Get metadata for a specific tmux session.
#[tauri::command]
#[specta::specta]
pub fn get_tmux_session_metadata(session_name: String) -> Result<AgentMetadata, String> {
    tmux::get_session_metadata(&session_name)
}

/// Create a new tmux session with metadata.
#[tauri::command]
#[specta::specta]
pub fn create_tmux_session(
    session_name: String,
    working_dir: Option<String>,
    issue_ref: Option<String>,
    repo: Option<String>,
    agent_type: String,
) -> Result<(), String> {
    let metadata = AgentMetadata {
        session: session_name.clone(),
        issue_ref,
        repo,
        worktree: working_dir.clone(),
        agent_type,
        machine_id: hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        started_at: chrono::Utc::now().to_rfc3339(),
    };

    tmux::create_session(&session_name, working_dir.as_deref(), &metadata)
}

/// Kill a tmux session.
#[tauri::command]
#[specta::specta]
pub fn kill_tmux_session(session_name: String) -> Result<(), String> {
    tmux::kill_session(&session_name)
}

/// Get recent output from a tmux session.
#[tauri::command]
#[specta::specta]
pub fn get_tmux_session_output(session_name: String, lines: Option<u32>) -> Result<String, String> {
    tmux::get_session_output(&session_name, lines)
}

/// Send a command to a tmux session.
#[tauri::command]
#[specta::specta]
pub fn send_tmux_command(session_name: String, command: String) -> Result<(), String> {
    tmux::send_command(&session_name, &command)
}

/// Recover agent sessions on startup.
#[tauri::command]
#[specta::specta]
pub fn recover_tmux_sessions() -> Result<Vec<RecoveredSession>, String> {
    tmux::recover_sessions()
}

/// Check if tmux server is running.
#[tauri::command]
#[specta::specta]
pub fn is_tmux_running() -> bool {
    tmux::is_tmux_running()
}
