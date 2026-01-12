//! DevOps module for multi-agent coding assistant functionality.
//!
//! This module provides:
//! - Dependency detection (gh, tmux)
//! - tmux session management
//! - Git worktree management
//! - GitHub issue integration
//! - Agent orchestration

mod dependencies;
pub mod github;
pub mod orchestrator;
pub mod tmux;
pub mod worktree;

pub use dependencies::*;
