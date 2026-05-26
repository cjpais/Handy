//! Shared model metadata types used across host platforms.
//!
//! The actual `ModelManager` (download, extract, file enumeration) lives in
//! host crates because it touches host-specific resource resolution
//! (`tauri::path::BaseDirectory::Resource` on desktop, asset bundles on mobile)
//! and settings read/write paths.
//!
//! Host managers exchange progress and lifecycle events through
//! `handy_platform::EventSink` using these payload structs.

pub mod engine;
pub mod info;
pub mod progress;

pub use engine::EngineType;
pub use info::ModelInfo;
pub use progress::DownloadProgress;
