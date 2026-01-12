//! Agent orchestration for multi-agent workflows.
//!
//! This module coordinates the spawning and management of coding agents,
//! tying together issues, worktrees, and tmux sessions.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::github::{self, GitHubIssue, IssueAgentMetadata};
use super::tmux::{self, AgentMetadata};
use super::worktree::{self, WorktreeConfig, WorktreeCreateResult};

/// Configuration for spawning an agent.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SpawnConfig {
    /// Repository in owner/repo format
    pub repo: String,
    /// Issue number to work on
    pub issue_number: u64,
    /// Agent type (e.g., "claude", "gpt", "codex")
    pub agent_type: String,
    /// Optional custom session name (auto-generated if not provided)
    pub session_name: Option<String>,
    /// Optional worktree prefix
    pub worktree_prefix: Option<String>,
    /// Labels to add when agent starts working
    pub working_labels: Vec<String>,
}

/// Result of spawning an agent.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SpawnResult {
    /// The issue being worked on
    pub issue: GitHubIssue,
    /// The created worktree
    pub worktree: WorktreeCreateResult,
    /// The tmux session name
    pub session_name: String,
    /// Machine ID where agent is running
    pub machine_id: String,
}

/// Status of an active agent.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AgentStatus {
    /// Session name
    pub session: String,
    /// Issue reference (owner/repo#number)
    pub issue_ref: Option<String>,
    /// Repository
    pub repo: Option<String>,
    /// Issue number
    pub issue_number: Option<u64>,
    /// Worktree path
    pub worktree: Option<String>,
    /// Agent type
    pub agent_type: String,
    /// Machine ID
    pub machine_id: String,
    /// Started timestamp
    pub started_at: String,
    /// Whether session is attached
    pub is_attached: bool,
}

/// Spawn a new agent to work on an issue.
///
/// This creates a worktree, tmux session, and updates the issue with metadata.
pub fn spawn_agent(config: &SpawnConfig, repo_path: &str) -> Result<SpawnResult, String> {
    // 1. Fetch the issue to ensure it exists
    let issue = github::get_issue(&config.repo, config.issue_number)?;

    // 2. Generate session name if not provided
    let session_name = config.session_name.clone().unwrap_or_else(|| {
        format!("handy-issue-{}-{}", config.issue_number, chrono::Utc::now().timestamp())
    });

    // 3. Create worktree for isolated work
    let worktree_name = format!("issue-{}", config.issue_number);
    let worktree_config = WorktreeConfig {
        prefix: config.worktree_prefix.clone().unwrap_or_default(),
        base_path: None,
        delete_branch_on_merge: true,
    };
    let worktree = worktree::create_worktree(repo_path, &worktree_name, &worktree_config, None)?;

    // 4. Get machine ID
    let machine_id = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // 5. Create tmux session in the worktree
    let metadata = AgentMetadata {
        session: session_name.clone(),
        issue_ref: Some(format!("{}#{}", config.repo, config.issue_number)),
        repo: Some(config.repo.clone()),
        worktree: Some(worktree.worktree_path.clone()),
        agent_type: config.agent_type.clone(),
        machine_id: machine_id.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    tmux::create_session(&session_name, Some(&worktree.worktree_path), &metadata)?;

    // 6. Add agent metadata comment to the issue
    let issue_metadata = IssueAgentMetadata {
        session: session_name.clone(),
        machine_id: machine_id.clone(),
        worktree: Some(worktree.worktree_path.clone()),
        agent_type: config.agent_type.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
        status: "working".to_string(),
    };
    github::add_agent_metadata_comment(&config.repo, config.issue_number, &issue_metadata)?;

    // 7. Add working labels to the issue
    if !config.working_labels.is_empty() {
        let labels_refs: Vec<&str> = config.working_labels.iter().map(|s| s.as_str()).collect();
        github::update_labels(&config.repo, config.issue_number, labels_refs, vec![])?;
    }

    Ok(SpawnResult {
        issue,
        worktree,
        session_name,
        machine_id,
    })
}

/// Get status of all active agents.
pub fn list_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    let sessions = tmux::list_sessions()?;
    let mut statuses = Vec::new();

    for session in sessions {
        // Try to get metadata for each session
        let metadata = tmux::get_session_metadata(&session.name).ok();

        let status = AgentStatus {
            session: session.name.clone(),
            issue_ref: metadata.as_ref().and_then(|m| m.issue_ref.clone()),
            repo: metadata.as_ref().and_then(|m| m.repo.clone()),
            issue_number: metadata.as_ref().and_then(|m| {
                m.issue_ref.as_ref().and_then(|r| {
                    r.split('#').last().and_then(|n| n.parse().ok())
                })
            }),
            worktree: metadata.as_ref().and_then(|m| m.worktree.clone()),
            agent_type: metadata.as_ref().map(|m| m.agent_type.clone()).unwrap_or_else(|| "unknown".to_string()),
            machine_id: metadata.as_ref().map(|m| m.machine_id.clone()).unwrap_or_else(|| "unknown".to_string()),
            started_at: metadata.as_ref().map(|m| m.started_at.clone()).unwrap_or_else(|| "unknown".to_string()),
            is_attached: session.attached,
        };

        statuses.push(status);
    }

    Ok(statuses)
}

/// Clean up an agent's resources after work is complete.
///
/// This kills the tmux session and optionally removes the worktree.
pub fn cleanup_agent(
    session_name: &str,
    repo_path: &str,
    remove_worktree: bool,
    delete_branch: bool,
) -> Result<(), String> {
    // Get session metadata to find the worktree
    let metadata = tmux::get_session_metadata(session_name).ok();

    // Kill the tmux session
    tmux::kill_session(session_name)?;

    // Remove worktree if requested
    if remove_worktree {
        if let Some(ref meta) = metadata {
            if let Some(ref worktree_path) = meta.worktree {
                worktree::remove_worktree(repo_path, worktree_path, true, delete_branch)?;
            }
        }
    }

    Ok(())
}

/// Create a PR from an agent's work.
pub fn create_pr_from_agent(
    session_name: &str,
    title: &str,
    body: Option<&str>,
    draft: bool,
) -> Result<github::GitHubPullRequest, String> {
    // Get session metadata
    let metadata = tmux::get_session_metadata(session_name)?;

    let repo = metadata.repo.ok_or("Session has no associated repository")?;
    let worktree_path = metadata.worktree.ok_or("Session has no associated worktree")?;

    // Get worktree info to find the branch
    let worktree_info = worktree::get_worktree_info(&worktree_path, &worktree_path)?;
    let branch = worktree_info.branch.ok_or("Worktree has no branch")?;

    // Get default branch for base
    let default_branch = worktree::get_default_branch(&worktree_path)?;

    // Create PR
    github::create_pr(&repo, title, body, &default_branch, Some(&branch), draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_config_default_session_name() {
        let config = SpawnConfig {
            repo: "KBVE/kbve".to_string(),
            issue_number: 123,
            agent_type: "claude".to_string(),
            session_name: None,
            worktree_prefix: None,
            working_labels: vec![],
        };
        assert!(config.session_name.is_none());
    }
}
