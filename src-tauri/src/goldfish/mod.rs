//! Goldfish-only code. Anything in this module is not in upstream Handy
//! and should not be PR'd back. See docs/fork-strategy.md.

pub mod commands;

use tauri::AppHandle;

/// Called once from `initialize_core_logic` in `lib.rs` after upstream
/// managers are registered. Currently a no-op; future home for Goldfish
/// Tauri state, event listeners, and background tasks.
pub fn register_state(_app_handle: &AppHandle) {
    log::info!("goldfish: register_state (no-op)");
}
