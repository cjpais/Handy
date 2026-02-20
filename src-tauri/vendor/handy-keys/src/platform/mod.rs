//! Platform-specific keyboard utilities

pub(crate) mod state;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
