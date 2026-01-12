//! DevOps-related Tauri commands.

use crate::devops::{check_all_dependencies, DevOpsDependencies};

/// Check if required DevOps dependencies (gh, tmux) are installed.
#[tauri::command]
#[specta::specta]
pub fn check_devops_dependencies() -> DevOpsDependencies {
    check_all_dependencies()
}
