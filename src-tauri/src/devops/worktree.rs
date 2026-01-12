//! Git worktree management for isolated agent development environments.
//!
//! Enables creating, listing, and removing git worktrees with collision detection.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for worktree creation.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorktreeConfig {
    /// Prefix for worktree directories (e.g., "Handy-" -> "Handy-feature-1")
    pub prefix: String,
    /// Base directory for worktrees (default: parent of repo)
    pub base_path: Option<String>,
    /// Auto-delete branch after merge
    pub delete_branch_on_merge: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            base_path: None,
            delete_branch_on_merge: true,
        }
    }
}

/// Information about a git worktree.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree
    pub path: String,
    /// Branch name checked out in this worktree
    pub branch: Option<String>,
    /// Commit SHA of HEAD
    pub head: String,
    /// Whether this is the main worktree
    pub is_main: bool,
    /// Whether the worktree is locked
    pub is_locked: bool,
    /// Whether the worktree is prunable (missing)
    pub is_prunable: bool,
}

/// Result of a worktree creation attempt.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorktreeCreateResult {
    /// Path to the created worktree
    pub path: String,
    /// Branch name
    pub branch: String,
    /// Whether a new branch was created
    pub branch_created: bool,
}

/// Collision check result.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CollisionCheck {
    /// Whether any collision was detected
    pub has_collision: bool,
    /// Path collision: directory already exists
    pub path_exists: bool,
    /// Branch collision: branch already exists
    pub branch_exists: bool,
    /// Worktree collision: worktree at path already exists
    pub worktree_exists: bool,
    /// Details about the collision
    pub details: Option<String>,
}

/// Get the root directory of the git repository.
pub fn get_repo_root(repo_path: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Not a git repository: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the project name from the repository root.
pub fn get_project_name(repo_path: &str) -> Result<String, String> {
    let root = get_repo_root(repo_path)?;
    let path = Path::new(&root);
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Could not determine project name".to_string())
}

/// Get the default branch (main or master).
pub fn get_default_branch(repo_path: &str) -> Result<String, String> {
    // Try to get the default branch from remote
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(repo_path)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Remove "origin/" prefix
            if let Some(name) = branch.strip_prefix("origin/") {
                return Ok(name.to_string());
            }
            return Ok(branch);
        }
    }

    // Fallback: check if main or master exists
    for branch in &["main", "master"] {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(repo_path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                return Ok(branch.to_string());
            }
        }
    }

    Err("Could not determine default branch".to_string())
}

/// List all git worktrees in a repository.
pub fn list_worktrees(repo_path: &str) -> Result<Vec<WorktreeInfo>, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git worktree list: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current = WorktreeInfo {
        path: String::new(),
        branch: None,
        head: String::new(),
        is_main: false,
        is_locked: false,
        is_prunable: false,
    };
    let mut is_first = true;

    for line in stdout.lines() {
        if line.starts_with("worktree ") {
            if !current.path.is_empty() {
                worktrees.push(current.clone());
            }
            current = WorktreeInfo {
                path: line.strip_prefix("worktree ").unwrap_or("").to_string(),
                branch: None,
                head: String::new(),
                is_main: is_first,
                is_locked: false,
                is_prunable: false,
            };
            is_first = false;
        } else if line.starts_with("HEAD ") {
            current.head = line.strip_prefix("HEAD ").unwrap_or("").to_string();
        } else if line.starts_with("branch ") {
            let branch = line.strip_prefix("branch ").unwrap_or("");
            // Remove refs/heads/ prefix
            current.branch = Some(
                branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string(),
            );
        } else if line == "locked" {
            current.is_locked = true;
        } else if line == "prunable" {
            current.is_prunable = true;
        } else if line == "detached" {
            // Detached HEAD, no branch
            current.branch = None;
        }
    }

    // Don't forget the last worktree
    if !current.path.is_empty() {
        worktrees.push(current);
    }

    Ok(worktrees)
}

/// Check for collisions before creating a worktree.
pub fn check_collision(
    repo_path: &str,
    worktree_path: &str,
    branch_name: &str,
) -> Result<CollisionCheck, String> {
    let mut result = CollisionCheck {
        has_collision: false,
        path_exists: false,
        branch_exists: false,
        worktree_exists: false,
        details: None,
    };

    // Check if path exists
    if Path::new(worktree_path).exists() {
        result.path_exists = true;
        result.has_collision = true;
        result.details = Some(format!("Directory already exists: {}", worktree_path));
    }

    // Check if branch exists
    let branch_check = Command::new("git")
        .args(["rev-parse", "--verify", branch_name])
        .current_dir(repo_path)
        .output();

    if let Ok(output) = branch_check {
        if output.status.success() {
            result.branch_exists = true;
            result.has_collision = true;
            let msg = format!("Branch already exists: {}", branch_name);
            result.details = Some(
                result
                    .details
                    .map_or(msg.clone(), |d| format!("{}; {}", d, msg)),
            );
        }
    }

    // Check if worktree already exists at path
    let worktrees = list_worktrees(repo_path)?;
    for wt in worktrees {
        if wt.path == worktree_path {
            result.worktree_exists = true;
            result.has_collision = true;
            let msg = format!("Worktree already exists at: {}", worktree_path);
            result.details = Some(
                result
                    .details
                    .map_or(msg.clone(), |d| format!("{}; {}", d, msg)),
            );
        }
    }

    Ok(result)
}

/// Create a new git worktree with a new branch.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
/// * `name` - Name for the worktree (will be used in path and branch name)
/// * `config` - Worktree configuration (prefix, base path)
/// * `base_branch` - Branch to create from (default: main/master)
///
/// # Returns
/// Result with the created worktree info or an error
pub fn create_worktree(
    repo_path: &str,
    name: &str,
    config: &WorktreeConfig,
    base_branch: Option<&str>,
) -> Result<WorktreeCreateResult, String> {
    let repo_root = get_repo_root(repo_path)?;
    let project_name = get_project_name(repo_path)?;

    // Determine base branch
    let base = match base_branch {
        Some(b) => b.to_string(),
        None => get_default_branch(repo_path)?,
    };

    // Build the worktree path and branch name
    let prefix = if config.prefix.is_empty() {
        format!("{}-", project_name)
    } else {
        config.prefix.clone()
    };

    let worktree_name = format!("{}{}", prefix, name);
    let branch_name = worktree_name.clone();

    // Determine worktree directory
    let base_path = config.base_path.clone().unwrap_or_else(|| {
        Path::new(&repo_root)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_root.clone())
    });

    let worktree_path = PathBuf::from(&base_path).join(&worktree_name);
    let worktree_path_str = worktree_path.to_string_lossy().to_string();

    // Check for collisions
    let collision = check_collision(repo_path, &worktree_path_str, &branch_name)?;
    if collision.has_collision {
        return Err(format!(
            "Cannot create worktree: {}",
            collision
                .details
                .unwrap_or_else(|| "collision detected".to_string())
        ));
    }

    // Create the worktree with a new branch
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch_name,
            &worktree_path_str,
            &base,
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git worktree add: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(WorktreeCreateResult {
        path: worktree_path_str,
        branch: branch_name,
        branch_created: true,
    })
}

/// Create a worktree using an existing branch.
pub fn create_worktree_existing_branch(
    repo_path: &str,
    branch_name: &str,
    config: &WorktreeConfig,
) -> Result<WorktreeCreateResult, String> {
    let repo_root = get_repo_root(repo_path)?;
    let project_name = get_project_name(repo_path)?;

    // Build the worktree path
    let prefix = if config.prefix.is_empty() {
        format!("{}-", project_name)
    } else {
        config.prefix.clone()
    };

    let worktree_name = format!("{}{}", prefix, branch_name);

    // Determine worktree directory
    let base_path = config.base_path.clone().unwrap_or_else(|| {
        Path::new(&repo_root)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_root.clone())
    });

    let worktree_path = PathBuf::from(&base_path).join(&worktree_name);
    let worktree_path_str = worktree_path.to_string_lossy().to_string();

    // Check if path exists
    if Path::new(&worktree_path_str).exists() {
        return Err(format!("Directory already exists: {}", worktree_path_str));
    }

    // Create the worktree using existing branch
    let output = Command::new("git")
        .args(["worktree", "add", &worktree_path_str, branch_name])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git worktree add: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(WorktreeCreateResult {
        path: worktree_path_str,
        branch: branch_name.to_string(),
        branch_created: false,
    })
}

/// Remove a git worktree.
///
/// # Arguments
/// * `repo_path` - Path to the main repository
/// * `worktree_path` - Path to the worktree to remove
/// * `force` - Force removal even if there are uncommitted changes
/// * `delete_branch` - Also delete the associated branch
pub fn remove_worktree(
    repo_path: &str,
    worktree_path: &str,
    force: bool,
    delete_branch: bool,
) -> Result<(), String> {
    // Get branch name before removing worktree (if we need to delete it)
    let branch_to_delete = if delete_branch {
        let worktrees = list_worktrees(repo_path)?;
        worktrees
            .iter()
            .find(|wt| wt.path == worktree_path)
            .and_then(|wt| wt.branch.clone())
    } else {
        None
    };

    // Remove the worktree
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_path);

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git worktree remove: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Delete the branch if requested
    if let Some(branch) = branch_to_delete {
        let delete_args = if force {
            vec!["branch", "-D", &branch]
        } else {
            vec!["branch", "-d", &branch]
        };

        let output = Command::new("git")
            .args(&delete_args)
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to delete branch: {}", e))?;

        if !output.status.success() {
            // Branch deletion failure is not critical, just log it
            eprintln!(
                "Warning: Could not delete branch {}: {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    Ok(())
}

/// Get information about a specific worktree.
pub fn get_worktree_info(repo_path: &str, worktree_path: &str) -> Result<WorktreeInfo, String> {
    let worktrees = list_worktrees(repo_path)?;
    worktrees
        .into_iter()
        .find(|wt| wt.path == worktree_path)
        .ok_or_else(|| format!("Worktree not found: {}", worktree_path))
}

/// Prune stale worktree entries.
pub fn prune_worktrees(repo_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git worktree prune: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree prune failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Check if a path is inside a git worktree or repository.
pub fn is_inside_worktree(path: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(result == "true")
            } else {
                Ok(false)
            }
        }
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collision_check_structure() {
        let check = CollisionCheck {
            has_collision: false,
            path_exists: false,
            branch_exists: false,
            worktree_exists: false,
            details: None,
        };
        assert!(!check.has_collision);
    }

    #[test]
    fn test_worktree_config_default() {
        let config = WorktreeConfig::default();
        assert!(config.prefix.is_empty());
        assert!(config.base_path.is_none());
        assert!(config.delete_branch_on_merge);
    }
}
