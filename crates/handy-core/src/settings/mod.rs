//! Shared, platform-independent settings used by `handy-core` services.
//!
//! ## Policy
//!
//! Fields belong here only when a `handy-core` service actually consumes them.
//! Desktop-only or platform-specific fields (global shortcuts, tray, overlay,
//! Vulkan/Metal accelerator preferences, Tauri-specific UI flags) MUST stay in
//! the host app's settings struct (e.g. `src-tauri/src/settings.rs`).
//!
//! Host apps compose `CoreSettings` into their own `Settings` struct via
//! `#[serde(flatten)]` or by holding it as a named field. They are also
//! responsible for persisting and loading.
//!
//! See `docs/superpowers/specs/2026-05-26-handy-mobile-design.md` §3.1 for
//! the boundary rules between handy-core and host crates.

use serde::{Deserialize, Serialize};

/// Settings consumed by `handy-core` services across all hosts.
///
/// Currently empty — fields are added on demand as services are migrated
/// (see Tasks 10 and 11 of the mobile phase 0+1 plan).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreSettings {}

impl CoreSettings {
    pub fn defaults() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let s = CoreSettings::defaults();
        let json = serde_json::to_string(&s).expect("serialize");
        let back: CoreSettings = serde_json::from_str(&json).expect("deserialize");
        // No fields yet — both should serialize to "{}".
        assert_eq!(json, "{}");
        let _ = back;
    }
}
