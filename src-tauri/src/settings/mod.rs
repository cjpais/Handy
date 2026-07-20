// settings — Application settings management.
// Splits into sub-modules by concern:
//   mod.rs       — Re-exports all public items
//   types.rs     — Struct/enum definitions, serde impls, AppSettings struct
//   defaults.rs  — get_default_settings() and default value functions
//   store.rs     — Load/save/flush, SettingsWriter, SettingsCache, migration, safe wrappers

pub mod defaults;
pub mod store;
pub mod types;

// Re-export everything from sub-modules so `crate::settings::X` still works
pub use defaults::get_default_settings;
pub use store::SettingsCache;
pub use store::*;
pub use types::*;