//! DevOps-related Tauri commands.

use crate::devops::{
    check_all_dependencies,
    tmux::{self, AgentMetadata, RecoveredSession, TmuxSession},
    worktree::{self, CollisionCheck, WorktreeConfig, WorktreeCreateResult, WorktreeInfo},
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

// ============================================================================
// Git Worktree Commands
// ============================================================================

/// List all git worktrees in a repository.
#[tauri::command]
#[specta::specta]
pub fn list_git_worktrees(repo_path: String) -> Result<Vec<WorktreeInfo>, String> {
    worktree::list_worktrees(&repo_path)
}

/// Get information about a specific worktree.
#[tauri::command]
#[specta::specta]
pub fn get_git_worktree_info(
    repo_path: String,
    worktree_path: String,
) -> Result<WorktreeInfo, String> {
    worktree::get_worktree_info(&repo_path, &worktree_path)
}

/// Check for collisions before creating a worktree.
#[tauri::command]
#[specta::specta]
pub fn check_worktree_collision(
    repo_path: String,
    worktree_path: String,
    branch_name: String,
) -> Result<CollisionCheck, String> {
    worktree::check_collision(&repo_path, &worktree_path, &branch_name)
}

/// Create a new git worktree with a new branch.
#[tauri::command]
#[specta::specta]
pub fn create_git_worktree(
    repo_path: String,
    name: String,
    prefix: Option<String>,
    base_path: Option<String>,
    base_branch: Option<String>,
) -> Result<WorktreeCreateResult, String> {
    let config = WorktreeConfig {
        prefix: prefix.unwrap_or_default(),
        base_path,
        delete_branch_on_merge: true,
    };
    worktree::create_worktree(&repo_path, &name, &config, base_branch.as_deref())
}

/// Create a worktree using an existing branch.
#[tauri::command]
#[specta::specta]
pub fn create_git_worktree_existing_branch(
    repo_path: String,
    branch_name: String,
    prefix: Option<String>,
    base_path: Option<String>,
) -> Result<WorktreeCreateResult, String> {
    let config = WorktreeConfig {
        prefix: prefix.unwrap_or_default(),
        base_path,
        delete_branch_on_merge: true,
    };
    worktree::create_worktree_existing_branch(&repo_path, &branch_name, &config)
}

/// Remove a git worktree.
#[tauri::command]
#[specta::specta]
pub fn remove_git_worktree(
    repo_path: String,
    worktree_path: String,
    force: bool,
    delete_branch: bool,
) -> Result<(), String> {
    worktree::remove_worktree(&repo_path, &worktree_path, force, delete_branch)
}

/// Prune stale worktree entries.
#[tauri::command]
#[specta::specta]
pub fn prune_git_worktrees(repo_path: String) -> Result<(), String> {
    worktree::prune_worktrees(&repo_path)
}

/// Get the root directory of a git repository.
#[tauri::command]
#[specta::specta]
pub fn get_git_repo_root(path: String) -> Result<String, String> {
    worktree::get_repo_root(&path)
}

/// Get the default branch of a repository.
#[tauri::command]
#[specta::specta]
pub fn get_git_default_branch(repo_path: String) -> Result<String, String> {
    worktree::get_default_branch(&repo_path)
}
