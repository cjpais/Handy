//! DevOps-related Tauri commands.

use crate::devops::{
    check_all_dependencies,
    github::{
        self, GhAuthStatus, GitHubComment, GitHubIssue, GitHubPullRequest, IssueAgentMetadata,
        IssueWithAgent, PrStatus,
    },
    orchestrator::{self, AgentStatus, CompleteWorkResult, SpawnConfig, SpawnResult, WorkflowConfig},
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

// ============================================================================
// GitHub Issue Commands
// ============================================================================

/// Check GitHub CLI authentication status.
#[tauri::command]
#[specta::specta]
pub fn check_gh_auth() -> GhAuthStatus {
    github::check_auth_status()
}

/// List issues from a GitHub repository.
#[tauri::command]
#[specta::specta]
pub fn list_github_issues(
    repo: String,
    state: Option<String>,
    labels: Option<Vec<String>>,
    limit: Option<u32>,
) -> Result<Vec<GitHubIssue>, String> {
    let state_ref = state.as_deref();
    let labels_ref: Option<Vec<&str>> = labels.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());
    github::list_issues(&repo, state_ref, labels_ref, limit)
}

/// Get details of a specific GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn get_github_issue(repo: String, number: u64) -> Result<GitHubIssue, String> {
    github::get_issue(&repo, number)
}

/// Get issue with agent metadata.
#[tauri::command]
#[specta::specta]
pub fn get_github_issue_with_agent(repo: String, number: u64) -> Result<IssueWithAgent, String> {
    github::get_issue_with_agent(&repo, number)
}

/// Create a new GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn create_github_issue(
    repo: String,
    title: String,
    body: Option<String>,
    labels: Option<Vec<String>>,
) -> Result<GitHubIssue, String> {
    let body_ref = body.as_deref();
    let labels_ref: Option<Vec<&str>> = labels.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());
    github::create_issue(&repo, &title, body_ref, labels_ref)
}

/// Add a comment to a GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn comment_on_github_issue(repo: String, number: u64, body: String) -> Result<(), String> {
    github::add_comment(&repo, number, &body)
}

/// Assign an agent to a GitHub issue (adds metadata comment).
#[tauri::command]
#[specta::specta]
pub fn assign_agent_to_issue(
    repo: String,
    number: u64,
    session: String,
    agent_type: String,
    worktree: Option<String>,
) -> Result<(), String> {
    let metadata = IssueAgentMetadata {
        session,
        machine_id: hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        worktree,
        agent_type,
        started_at: chrono::Utc::now().to_rfc3339(),
        status: "working".to_string(),
    };
    github::add_agent_metadata_comment(&repo, number, &metadata)
}

/// List comments on a GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn list_github_issue_comments(repo: String, number: u64) -> Result<Vec<GitHubComment>, String> {
    github::list_comments(&repo, number)
}

/// Update labels on a GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn update_github_issue_labels(
    repo: String,
    number: u64,
    add_labels: Vec<String>,
    remove_labels: Vec<String>,
) -> Result<(), String> {
    let add_refs: Vec<&str> = add_labels.iter().map(|s| s.as_str()).collect();
    let remove_refs: Vec<&str> = remove_labels.iter().map(|s| s.as_str()).collect();
    github::update_labels(&repo, number, add_refs, remove_refs)
}

/// Close a GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn close_github_issue(repo: String, number: u64, comment: Option<String>) -> Result<(), String> {
    github::close_issue(&repo, number, comment.as_deref())
}

/// Reopen a closed GitHub issue.
#[tauri::command]
#[specta::specta]
pub fn reopen_github_issue(repo: String, number: u64) -> Result<(), String> {
    github::reopen_issue(&repo, number)
}

// ============================================================================
// GitHub Pull Request Commands
// ============================================================================

/// List pull requests from a GitHub repository.
#[tauri::command]
#[specta::specta]
pub fn list_github_prs(
    repo: String,
    state: Option<String>,
    base: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<GitHubPullRequest>, String> {
    let state_ref = state.as_deref();
    let base_ref = base.as_deref();
    github::list_prs(&repo, state_ref, base_ref, limit)
}

/// Get details of a specific GitHub pull request.
#[tauri::command]
#[specta::specta]
pub fn get_github_pr(repo: String, number: u64) -> Result<GitHubPullRequest, String> {
    github::get_pr(&repo, number)
}

/// Get full status of a pull request (PR + checks + reviews).
#[tauri::command]
#[specta::specta]
pub fn get_github_pr_status(repo: String, number: u64) -> Result<PrStatus, String> {
    github::get_pr_status(&repo, number)
}

/// Create a new GitHub pull request.
#[tauri::command]
#[specta::specta]
pub fn create_github_pr(
    repo: String,
    title: String,
    body: Option<String>,
    base: String,
    head: Option<String>,
    draft: bool,
) -> Result<GitHubPullRequest, String> {
    let body_ref = body.as_deref();
    let head_ref = head.as_deref();
    github::create_pr(&repo, &title, body_ref, &base, head_ref, draft)
}

/// Merge a GitHub pull request.
#[tauri::command]
#[specta::specta]
pub fn merge_github_pr(
    repo: String,
    number: u64,
    method: Option<String>,
    delete_branch: bool,
) -> Result<(), String> {
    github::merge_pr(&repo, number, method.as_deref(), delete_branch)
}

/// Close a GitHub pull request without merging.
#[tauri::command]
#[specta::specta]
pub fn close_github_pr(repo: String, number: u64, comment: Option<String>) -> Result<(), String> {
    github::close_pr(&repo, number, comment.as_deref())
}

// ============================================================================
// Agent Orchestration Commands
// ============================================================================

/// Spawn a new agent to work on an issue.
///
/// Creates a worktree, tmux session, and updates the issue with metadata.
#[tauri::command]
#[specta::specta]
pub fn spawn_agent(
    repo: String,
    issue_number: u64,
    agent_type: String,
    repo_path: String,
    session_name: Option<String>,
    worktree_prefix: Option<String>,
    working_labels: Option<Vec<String>>,
) -> Result<SpawnResult, String> {
    let config = SpawnConfig {
        repo,
        issue_number,
        agent_type,
        session_name,
        worktree_prefix,
        working_labels: working_labels.unwrap_or_default(),
    };
    orchestrator::spawn_agent(&config, &repo_path)
}

/// Get status of all active agents.
#[tauri::command]
#[specta::specta]
pub fn list_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    orchestrator::list_agent_statuses()
}

/// Clean up an agent's resources after work is complete.
#[tauri::command]
#[specta::specta]
pub fn cleanup_agent(
    session_name: String,
    repo_path: String,
    remove_worktree: bool,
    delete_branch: bool,
) -> Result<(), String> {
    orchestrator::cleanup_agent(&session_name, &repo_path, remove_worktree, delete_branch)
}

/// Create a PR from an agent's work.
#[tauri::command]
#[specta::specta]
pub fn create_pr_from_agent(
    session_name: String,
    title: String,
    body: Option<String>,
    draft: bool,
) -> Result<GitHubPullRequest, String> {
    orchestrator::create_pr_from_agent(&session_name, &title, body.as_deref(), draft)
}

/// Complete an agent's work with workflow automation.
///
/// Creates PR, updates issue with link, manages labels.
#[tauri::command]
#[specta::specta]
pub fn complete_agent_work(
    session_name: String,
    pr_title: String,
    pr_body: Option<String>,
    working_labels: Vec<String>,
    pr_labels: Vec<String>,
    draft_pr: bool,
) -> Result<CompleteWorkResult, String> {
    let config = WorkflowConfig {
        working_labels,
        pr_labels,
        draft_pr,
        close_on_merge: true,
    };
    orchestrator::complete_agent_work(&session_name, &pr_title, pr_body.as_deref(), &config)
}

/// Check if a PR has been merged and cleanup resources if so.
#[tauri::command]
#[specta::specta]
pub fn check_and_cleanup_merged_pr(
    session_name: String,
    repo_path: String,
    pr_number: u64,
) -> Result<bool, String> {
    orchestrator::check_and_cleanup_merged_pr(&session_name, &repo_path, pr_number)
}

/// Get current machine identifier.
#[tauri::command]
#[specta::specta]
pub fn get_current_machine_id() -> String {
    orchestrator::get_current_machine_id()
}

/// List only agents running on this machine.
#[tauri::command]
#[specta::specta]
pub fn list_local_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    orchestrator::list_local_agent_statuses()
}

/// List agents from other machines (potentially orphaned).
#[tauri::command]
#[specta::specta]
pub fn list_remote_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    orchestrator::list_remote_agent_statuses()
}
