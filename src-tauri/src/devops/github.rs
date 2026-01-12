//! GitHub CLI integration for issue management.
//!
//! Uses the `gh` CLI to interact with GitHub issues, providing
//! task queue functionality for coding agents.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::process::Command;

/// GitHub authentication status.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GhAuthStatus {
    /// Whether the user is authenticated
    pub authenticated: bool,
    /// Username if authenticated
    pub username: Option<String>,
    /// Scopes available
    pub scopes: Vec<String>,
    /// Error message if not authenticated
    pub error: Option<String>,
}

/// A GitHub issue.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GitHubIssue {
    /// Issue number
    pub number: u64,
    /// Issue title
    pub title: String,
    /// Issue body/description
    pub body: Option<String>,
    /// Issue state (open, closed)
    pub state: String,
    /// Issue URL
    pub url: String,
    /// Labels on the issue
    pub labels: Vec<String>,
    /// Assignees
    pub assignees: Vec<String>,
    /// Author username
    pub author: String,
    /// Created timestamp
    pub created_at: String,
    /// Updated timestamp
    pub updated_at: String,
    /// Repository in owner/repo format
    pub repo: String,
}

/// A GitHub issue comment.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GitHubComment {
    /// Comment ID
    pub id: u64,
    /// Comment body
    pub body: String,
    /// Author username
    pub author: String,
    /// Created timestamp
    pub created_at: String,
}

/// Agent metadata stored in issue comments.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IssueAgentMetadata {
    /// Session name
    pub session: String,
    /// Machine ID
    pub machine_id: String,
    /// Worktree path
    pub worktree: Option<String>,
    /// Agent type
    pub agent_type: String,
    /// Started timestamp
    pub started_at: String,
    /// Current status
    pub status: String,
}

/// Parsed issue with agent metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IssueWithAgent {
    /// The issue
    pub issue: GitHubIssue,
    /// Agent metadata if assigned
    pub agent: Option<IssueAgentMetadata>,
}

const METADATA_START: &str = "<!-- HANDY_AGENT_METADATA";
const METADATA_END: &str = "-->";

/// Check GitHub CLI authentication status.
pub fn check_auth_status() -> GhAuthStatus {
    let output = Command::new("gh")
        .args(["auth", "status", "--show-token"])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);

            if output.status.success() || combined.contains("Logged in to") {
                // Parse username from output
                let username = combined
                    .lines()
                    .find(|line| line.contains("Logged in to"))
                    .and_then(|line| {
                        // Format: "✓ Logged in to github.com account username (keyring)"
                        line.split("account ")
                            .nth(1)
                            .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                    });

                // Parse scopes
                let scopes = combined
                    .lines()
                    .find(|line| line.contains("Token scopes:"))
                    .map(|line| {
                        line.split("Token scopes:")
                            .nth(1)
                            .unwrap_or("")
                            .split(',')
                            .map(|s| s.trim().trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();

                GhAuthStatus {
                    authenticated: true,
                    username,
                    scopes,
                    error: None,
                }
            } else {
                GhAuthStatus {
                    authenticated: false,
                    username: None,
                    scopes: vec![],
                    error: Some(combined.trim().to_string()),
                }
            }
        }
        Err(e) => GhAuthStatus {
            authenticated: false,
            username: None,
            scopes: vec![],
            error: Some(format!("Failed to run gh: {}", e)),
        },
    }
}

/// List issues from a repository.
pub fn list_issues(
    repo: &str,
    state: Option<&str>,
    labels: Option<Vec<&str>>,
    limit: Option<u32>,
) -> Result<Vec<GitHubIssue>, String> {
    let mut args = vec![
        "issue",
        "list",
        "--repo",
        repo,
        "--json",
        "number,title,body,state,url,labels,assignees,author,createdAt,updatedAt",
    ];

    let state_str;
    if let Some(s) = state {
        state_str = s.to_string();
        args.push("--state");
        args.push(&state_str);
    }

    let labels_str;
    if let Some(l) = labels {
        labels_str = l.join(",");
        args.push("--label");
        args.push(&labels_str);
    }

    let limit_str;
    if let Some(l) = limit {
        limit_str = l.to_string();
        args.push("--limit");
        args.push(&limit_str);
    }

    let output = Command::new("gh")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON response
    #[derive(Deserialize)]
    struct GhIssue {
        number: u64,
        title: String,
        body: Option<String>,
        state: String,
        url: String,
        labels: Vec<GhLabel>,
        assignees: Vec<GhUser>,
        author: GhUser,
        #[serde(rename = "createdAt")]
        created_at: String,
        #[serde(rename = "updatedAt")]
        updated_at: String,
    }

    #[derive(Deserialize)]
    struct GhLabel {
        name: String,
    }

    #[derive(Deserialize)]
    struct GhUser {
        login: String,
    }

    let gh_issues: Vec<GhIssue> =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(gh_issues
        .into_iter()
        .map(|i| GitHubIssue {
            number: i.number,
            title: i.title,
            body: i.body,
            state: i.state,
            url: i.url,
            labels: i.labels.into_iter().map(|l| l.name).collect(),
            assignees: i.assignees.into_iter().map(|a| a.login).collect(),
            author: i.author.login,
            created_at: i.created_at,
            updated_at: i.updated_at,
            repo: repo.to_string(),
        })
        .collect())
}

/// Get details of a specific issue.
pub fn get_issue(repo: &str, number: u64) -> Result<GitHubIssue, String> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &number.to_string(),
            "--repo",
            repo,
            "--json",
            "number,title,body,state,url,labels,assignees,author,createdAt,updatedAt",
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue view failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);

    #[derive(Deserialize)]
    struct GhIssue {
        number: u64,
        title: String,
        body: Option<String>,
        state: String,
        url: String,
        labels: Vec<GhLabel>,
        assignees: Vec<GhUser>,
        author: GhUser,
        #[serde(rename = "createdAt")]
        created_at: String,
        #[serde(rename = "updatedAt")]
        updated_at: String,
    }

    #[derive(Deserialize)]
    struct GhLabel {
        name: String,
    }

    #[derive(Deserialize)]
    struct GhUser {
        login: String,
    }

    let gh_issue: GhIssue =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(GitHubIssue {
        number: gh_issue.number,
        title: gh_issue.title,
        body: gh_issue.body,
        state: gh_issue.state,
        url: gh_issue.url,
        labels: gh_issue.labels.into_iter().map(|l| l.name).collect(),
        assignees: gh_issue.assignees.into_iter().map(|a| a.login).collect(),
        author: gh_issue.author.login,
        created_at: gh_issue.created_at,
        updated_at: gh_issue.updated_at,
        repo: repo.to_string(),
    })
}

/// Create a new issue.
pub fn create_issue(
    repo: &str,
    title: &str,
    body: Option<&str>,
    labels: Option<Vec<&str>>,
) -> Result<GitHubIssue, String> {
    let mut args = vec!["issue", "create", "--repo", repo, "--title", title];

    let body_str;
    if let Some(b) = body {
        body_str = b.to_string();
        args.push("--body");
        args.push(&body_str);
    }

    let labels_str;
    if let Some(l) = labels {
        labels_str = l.join(",");
        args.push("--label");
        args.push(&labels_str);
    }

    let output = Command::new("gh")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Output is the issue URL, parse the number from it
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let number = url
        .split('/')
        .last()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| "Failed to parse issue number from URL".to_string())?;

    // Fetch the full issue details
    get_issue(repo, number)
}

/// Add a comment to an issue.
pub fn add_comment(repo: &str, number: u64, body: &str) -> Result<(), String> {
    let output = Command::new("gh")
        .args([
            "issue",
            "comment",
            &number.to_string(),
            "--repo",
            repo,
            "--body",
            body,
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue comment failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Add agent metadata comment to an issue.
pub fn add_agent_metadata_comment(
    repo: &str,
    number: u64,
    metadata: &IssueAgentMetadata,
) -> Result<(), String> {
    let metadata_json =
        serde_json::to_string(metadata).map_err(|e| format!("Failed to serialize metadata: {}", e))?;

    let body = format!(
        r#"{}
{}
{}
🤖 **Agent Assigned**
- Session: `{}`
- Type: {}
- Machine: {}
- Started: {}"#,
        METADATA_START,
        metadata_json,
        METADATA_END,
        metadata.session,
        metadata.agent_type,
        metadata.machine_id,
        metadata.started_at
    );

    add_comment(repo, number, &body)
}

/// List comments on an issue.
pub fn list_comments(repo: &str, number: u64) -> Result<Vec<GitHubComment>, String> {
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &number.to_string(),
            "--repo",
            repo,
            "--json",
            "comments",
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue view failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);

    #[derive(Deserialize)]
    struct GhComments {
        comments: Vec<GhComment>,
    }

    #[derive(Deserialize)]
    struct GhComment {
        id: String,
        body: String,
        author: GhUser,
        #[serde(rename = "createdAt")]
        created_at: String,
    }

    #[derive(Deserialize)]
    struct GhUser {
        login: String,
    }

    let gh_comments: GhComments =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(gh_comments
        .comments
        .into_iter()
        .map(|c| GitHubComment {
            id: c.id.parse().unwrap_or(0),
            body: c.body,
            author: c.author.login,
            created_at: c.created_at,
        })
        .collect())
}

/// Parse agent metadata from issue comments.
pub fn parse_agent_metadata(comments: &[GitHubComment]) -> Option<IssueAgentMetadata> {
    for comment in comments.iter().rev() {
        if let Some(metadata) = extract_metadata_from_comment(&comment.body) {
            return Some(metadata);
        }
    }
    None
}

/// Extract metadata from a comment body.
fn extract_metadata_from_comment(body: &str) -> Option<IssueAgentMetadata> {
    let start_idx = body.find(METADATA_START)?;
    let end_idx = body[start_idx..].find(METADATA_END)?;

    let json_start = start_idx + METADATA_START.len();
    let json_end = start_idx + end_idx;
    let json_str = body[json_start..json_end].trim();

    serde_json::from_str(json_str).ok()
}

/// Get issue with agent metadata.
pub fn get_issue_with_agent(repo: &str, number: u64) -> Result<IssueWithAgent, String> {
    let issue = get_issue(repo, number)?;
    let comments = list_comments(repo, number)?;
    let agent = parse_agent_metadata(&comments);

    Ok(IssueWithAgent { issue, agent })
}

/// Update issue labels.
pub fn update_labels(repo: &str, number: u64, add: Vec<&str>, remove: Vec<&str>) -> Result<(), String> {
    // Add labels
    if !add.is_empty() {
        let add_str = add.join(",");
        let output = Command::new("gh")
            .args([
                "issue",
                "edit",
                &number.to_string(),
                "--repo",
                repo,
                "--add-label",
                &add_str,
            ])
            .output()
            .map_err(|e| format!("Failed to execute gh: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "gh issue edit (add labels) failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    // Remove labels
    if !remove.is_empty() {
        let remove_str = remove.join(",");
        let output = Command::new("gh")
            .args([
                "issue",
                "edit",
                &number.to_string(),
                "--repo",
                repo,
                "--remove-label",
                &remove_str,
            ])
            .output()
            .map_err(|e| format!("Failed to execute gh: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "gh issue edit (remove labels) failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    Ok(())
}

/// Close an issue with an optional comment.
pub fn close_issue(repo: &str, number: u64, comment: Option<&str>) -> Result<(), String> {
    // Add closing comment if provided
    if let Some(c) = comment {
        add_comment(repo, number, c)?;
    }

    let output = Command::new("gh")
        .args([
            "issue",
            "close",
            &number.to_string(),
            "--repo",
            repo,
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue close failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Reopen a closed issue.
pub fn reopen_issue(repo: &str, number: u64) -> Result<(), String> {
    let output = Command::new("gh")
        .args([
            "issue",
            "reopen",
            &number.to_string(),
            "--repo",
            repo,
        ])
        .output()
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "gh issue reopen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_metadata() {
        let comment = r#"<!-- HANDY_AGENT_METADATA
{"session":"handy-agent-42","machine_id":"test-mac","worktree":null,"agent_type":"claude","started_at":"2024-01-15T10:30:00Z","status":"working"}
-->
🤖 **Agent Assigned**"#;

        let metadata = extract_metadata_from_comment(comment);
        assert!(metadata.is_some());
        let m = metadata.unwrap();
        assert_eq!(m.session, "handy-agent-42");
        assert_eq!(m.machine_id, "test-mac");
    }

    #[test]
    fn test_no_metadata() {
        let comment = "Just a regular comment without metadata";
        let metadata = extract_metadata_from_comment(comment);
        assert!(metadata.is_none());
    }
}
