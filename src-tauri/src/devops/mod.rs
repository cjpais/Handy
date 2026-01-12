//! DevOps module for multi-agent coding assistant functionality.
//!
//! This module provides:
//! - Dependency detection (gh, tmux)
//! - tmux session management
//! - Git worktree management
//! - GitHub issue integration

mod dependencies;
pub mod tmux;

pub use dependencies::*;
