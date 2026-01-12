//! tmux session management for DevOps agent sessions.
//!
//! Sessions persist independently in the tmux server, surviving app restarts.
//! Metadata is stored in tmux environment variables for recovery.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::process::Command;

/// Session naming prefix for all Handy agent sessions
const SESSION_PREFIX: &str = "handy-agent-";

/// Environment variable keys stored in tmux sessions
const ENV_ISSUE_REF: &str = "HANDY_ISSUE_REF";
const ENV_REPO: &str = "HANDY_REPO";
const ENV_WORKTREE: &str = "HANDY_WORKTREE";
const ENV_AGENT_TYPE: &str = "HANDY_AGENT_TYPE";
const ENV_MACHINE_ID: &str = "HANDY_MACHINE_ID";
const ENV_STARTED_AT: &str = "HANDY_STARTED_AT";

/// Status of an agent session
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub enum AgentStatus {
    /// Session is running and agent is active
    Running,
    /// Session exists but agent process has exited
    Stopped,
    /// Session was recovered from metadata (tmux or GitHub)
    Recovered,
}

/// Metadata stored with each agent session
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AgentMetadata {
    /// Session name (e.g., "handy-agent-42")
    pub session: String,
    /// GitHub issue reference (e.g., "org/repo#42")
    pub issue_ref: Option<String>,
    /// Repository being worked on
    pub repo: Option<String>,
    /// Path to the worktree
    pub worktree: Option<String>,
    /// Type of agent (e.g., "claude", "aider")
    pub agent_type: String,
    /// Machine identifier for multi-machine disambiguation
    pub machine_id: String,
    /// ISO timestamp when session started
    pub started_at: String,
}

/// Information about a tmux session
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TmuxSession {
    /// Session name
    pub name: String,
    /// Whether the session is attached
    pub attached: bool,
    /// Number of windows in the session
    pub windows: u32,
    /// Session creation time (Unix timestamp)
    pub created: u64,
    /// Agent metadata if this is a Handy session
    pub metadata: Option<AgentMetadata>,
    /// Current status
    pub status: AgentStatus,
}

/// Source of recovered session information
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum RecoverySource {
    /// Found in tmux, normal operation
    Tmux,
    /// Recovered from GitHub issue comment
    GitHubIssue,
    /// Confirmed by both sources
    Both,
}

/// Recommended action for a recovered session
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum RecoveryAction {
    /// tmux alive, continue monitoring
    Resume,
    /// tmux dead but work incomplete, offer restart
    Restart,
    /// orphan session, offer to kill/remove
    Cleanup,
    /// completed normally, nothing to do
    None,
}

/// A session recovered during startup
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RecoveredSession {
    pub metadata: AgentMetadata,
    pub source: RecoverySource,
    pub tmux_alive: bool,
    pub worktree_exists: bool,
    pub recommended_action: RecoveryAction,
}

/// Check if tmux server is running
pub fn is_tmux_running() -> bool {
    Command::new("tmux")
        .args(["list-sessions"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current machine's hostname for identification
fn get_machine_id() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// List all tmux sessions, filtering for Handy agent sessions
pub fn list_sessions() -> Result<Vec<TmuxSession>, String> {
    // Format: session_name, attached, windows, created
    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}\t#{session_windows}\t#{session_created}",
        ])
        .output()
        .map_err(|e| format!("Failed to list tmux sessions: {}", e))?;

    if !output.status.success() {
        // No sessions or tmux not running
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("no sessions") {
            return Ok(vec![]);
        }
        return Err(format!("tmux error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 4 {
            let name = parts[0].to_string();
            let attached = parts[1] == "1";
            let windows = parts[2].parse().unwrap_or(1);
            let created = parts[3].parse().unwrap_or(0);

            // Only include Handy agent sessions
            if name.starts_with(SESSION_PREFIX) {
                let metadata = get_session_metadata(&name).ok();
                let status = if check_session_has_active_process(&name) {
                    AgentStatus::Running
                } else {
                    AgentStatus::Stopped
                };

                sessions.push(TmuxSession {
                    name,
                    attached,
                    windows,
                    created,
                    metadata,
                    status,
                });
            }
        }
    }

    Ok(sessions)
}

/// Check if a session has an active process running in its pane
fn check_session_has_active_process(session_name: &str) -> bool {
    // Get the command running in the session's active pane
    Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            session_name,
            "-F",
            "#{pane_current_command}",
        ])
        .output()
        .map(|o| {
            if o.status.success() {
                let cmd = String::from_utf8_lossy(&o.stdout).trim().to_string();
                // Check if it's not just a shell prompt
                !cmd.is_empty() && cmd != "bash" && cmd != "zsh" && cmd != "sh" && cmd != "fish"
            } else {
                false
            }
        })
        .unwrap_or(false)
}

/// Get metadata for a specific session from its environment variables
pub fn get_session_metadata(session_name: &str) -> Result<AgentMetadata, String> {
    let output = Command::new("tmux")
        .args(["show-environment", "-t", session_name])
        .output()
        .map_err(|e| format!("Failed to get session environment: {}", e))?;

    if !output.status.success() {
        return Err("Session not found or no environment set".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut env_vars: HashMap<String, String> = HashMap::new();

    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key.starts_with("HANDY_") {
                env_vars.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(AgentMetadata {
        session: session_name.to_string(),
        issue_ref: env_vars.get(ENV_ISSUE_REF).cloned(),
        repo: env_vars.get(ENV_REPO).cloned(),
        worktree: env_vars.get(ENV_WORKTREE).cloned(),
        agent_type: env_vars
            .get(ENV_AGENT_TYPE)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        machine_id: env_vars
            .get(ENV_MACHINE_ID)
            .cloned()
            .unwrap_or_else(get_machine_id),
        started_at: env_vars
            .get(ENV_STARTED_AT)
            .cloned()
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
    })
}

/// Create a new tmux session with metadata
pub fn create_session(
    session_name: &str,
    working_dir: Option<&str>,
    metadata: &AgentMetadata,
) -> Result<(), String> {
    // Validate session name
    if !session_name.starts_with(SESSION_PREFIX) {
        return Err(format!("Session name must start with '{}'", SESSION_PREFIX));
    }

    // Check if session already exists
    let existing = list_sessions()?;
    if existing.iter().any(|s| s.name == session_name) {
        return Err(format!("Session '{}' already exists", session_name));
    }

    // Build the create command
    let mut args = vec!["new-session", "-d", "-s", session_name];

    if let Some(dir) = working_dir {
        args.push("-c");
        args.push(dir);
    }

    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to create session: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tmux error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Set environment variables for metadata
    set_session_env(session_name, ENV_AGENT_TYPE, &metadata.agent_type)?;
    set_session_env(session_name, ENV_MACHINE_ID, &metadata.machine_id)?;
    set_session_env(session_name, ENV_STARTED_AT, &metadata.started_at)?;

    if let Some(ref issue_ref) = metadata.issue_ref {
        set_session_env(session_name, ENV_ISSUE_REF, issue_ref)?;
    }
    if let Some(ref repo) = metadata.repo {
        set_session_env(session_name, ENV_REPO, repo)?;
    }
    if let Some(ref worktree) = metadata.worktree {
        set_session_env(session_name, ENV_WORKTREE, worktree)?;
    }

    Ok(())
}

/// Set an environment variable in a tmux session
fn set_session_env(session_name: &str, key: &str, value: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["set-environment", "-t", session_name, key, value])
        .output()
        .map_err(|e| format!("Failed to set environment: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to set {}: {}",
            key,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Kill a tmux session
pub fn kill_session(session_name: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .map_err(|e| format!("Failed to kill session: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tmux error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Get recent output from a session's pane
pub fn get_session_output(session_name: &str, lines: Option<u32>) -> Result<String, String> {
    let line_count = lines.unwrap_or(100).to_string();

    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-t",
            session_name,
            "-p",
            "-S",
            &format!("-{}", line_count),
        ])
        .output()
        .map_err(|e| format!("Failed to capture pane: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tmux error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Send a command to a session
pub fn send_command(session_name: &str, command: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["send-keys", "-t", session_name, command, "Enter"])
        .output()
        .map_err(|e| format!("Failed to send command: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tmux error: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Recover agent sessions on startup
pub fn recover_sessions() -> Result<Vec<RecoveredSession>, String> {
    let current_machine = get_machine_id();
    let sessions = list_sessions()?;
    let mut recovered = Vec::new();

    for session in sessions {
        if let Some(metadata) = session.metadata {
            // Only recover sessions from this machine
            if metadata.machine_id != current_machine {
                continue;
            }

            let worktree_exists = metadata
                .worktree
                .as_ref()
                .map(|p| std::path::Path::new(p).exists())
                .unwrap_or(false);

            let tmux_alive = session.status == AgentStatus::Running;

            let recommended_action = match (tmux_alive, worktree_exists) {
                (true, _) => RecoveryAction::Resume,
                (false, true) => RecoveryAction::Restart,
                (false, false) => RecoveryAction::Cleanup,
            };

            recovered.push(RecoveredSession {
                metadata,
                source: RecoverySource::Tmux,
                tmux_alive,
                worktree_exists,
                recommended_action,
            });
        }
    }

    Ok(recovered)
}

/// Generate a session name for an issue
pub fn session_name_for_issue(issue_number: u32) -> String {
    format!("{}{}", SESSION_PREFIX, issue_number)
}

/// Generate a session name for a manual (non-issue) session
pub fn session_name_manual(suffix: &str) -> String {
    format!("{}manual-{}", SESSION_PREFIX, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_generation() {
        assert_eq!(session_name_for_issue(42), "handy-agent-42");
        assert_eq!(session_name_manual("test"), "handy-agent-manual-test");
    }

    #[test]
    fn test_is_tmux_running() {
        // Just ensure it doesn't panic
        let _ = is_tmux_running();
    }
}
