//! macOS platform support

pub(crate) mod keycode;
pub(crate) mod listener;
mod permissions;

pub use permissions::{check_accessibility, open_accessibility_settings};
