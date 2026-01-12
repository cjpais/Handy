//! Dependency detection for DevOps features.
//!
//! Checks for required CLI tools: gh (GitHub CLI) and tmux.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::process::Command;

/// Status of a single dependency
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DependencyStatus {
    /// Name of the dependency
    pub name: String,
    /// Whether the dependency is installed
    pub installed: bool,
    /// Version string if installed
    pub version: Option<String>,
    /// Path to the executable if installed
    pub path: Option<String>,
    /// Installation instructions if not installed
    pub install_hint: String,
}

/// Status of all DevOps dependencies
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DevOpsDependencies {
    /// GitHub CLI status
    pub gh: DependencyStatus,
    /// tmux status
    pub tmux: DependencyStatus,
    /// Whether all required dependencies are installed
    pub all_satisfied: bool,
}

/// Check if a command exists and get its version
fn check_command(name: &str, version_args: &[&str]) -> (bool, Option<String>, Option<String>) {
    // First check if command exists using `which`
    let which_result = Command::new("which").arg(name).output();

    let path = match which_result {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => return (false, None, None),
    };

    // Get version
    let version_result = Command::new(name).args(version_args).output();

    let version = match version_result {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Some tools output to stderr
            let output_str = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            // Extract first line and clean up
            output_str.lines().next().map(|s| s.trim().to_string())
        }
        _ => None,
    };

    (true, version, Some(path))
}

/// Check GitHub CLI (gh) status
fn check_gh() -> DependencyStatus {
    let (installed, version, path) = check_command("gh", &["--version"]);

    // Parse version from "gh version 2.40.0 (2024-01-01)" format
    let version = version.and_then(|v| {
        v.split_whitespace()
            .nth(2)
            .map(|s| s.trim_end_matches(',').to_string())
    });

    DependencyStatus {
        name: "gh".to_string(),
        installed,
        version,
        path,
        install_hint: "brew install gh".to_string(),
    }
}

/// Check tmux status
fn check_tmux() -> DependencyStatus {
    let (installed, version, path) = check_command("tmux", &["-V"]);

    // Parse version from "tmux 3.4" format
    let version = version.and_then(|v| v.split_whitespace().nth(1).map(|s| s.to_string()));

    DependencyStatus {
        name: "tmux".to_string(),
        installed,
        version,
        path,
        install_hint: "brew install tmux".to_string(),
    }
}

/// Check all DevOps dependencies
pub fn check_all_dependencies() -> DevOpsDependencies {
    let gh = check_gh();
    let tmux = check_tmux();
    let all_satisfied = gh.installed && tmux.installed;

    DevOpsDependencies {
        gh,
        tmux,
        all_satisfied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_dependencies() {
        let deps = check_all_dependencies();
        // Just verify it doesn't panic and returns valid structure
        assert!(!deps.gh.name.is_empty());
        assert!(!deps.tmux.name.is_empty());
    }
}
