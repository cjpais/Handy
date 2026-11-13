//! Placeholder backend for platforms without window redirect yet. Its
//! `SUPPORTED` is false, so the tray hides the feature.

use super::{DeliverError, ScreenWindow, WindowInfo};
use crate::settings::{AutoSubmitKey, PasteMethod};
use tauri::AppHandle;

pub const SUPPORTED: bool = false;

/// Nothing identifies a window here yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowHandle;

pub fn remember_front() {}

pub fn restore_front() {}

pub fn windows_at(_x: f64, _y: f64) -> Vec<ScreenWindow> {
    Vec::new()
}

pub fn pick(_x: f64, _y: f64, _depth: usize) -> Option<WindowInfo> {
    None
}

pub fn is_focused(_window: &WindowHandle) -> bool {
    false
}

fn unsupported() -> DeliverError {
    DeliverError::Failed("window redirect is not supported on this platform".to_string())
}

/// Nothing to restore.
pub struct Previous;

pub fn prepare(_window: &WindowHandle) -> Result<Previous, DeliverError> {
    Err(unsupported())
}

pub fn restore(_previous: Previous) {}

pub fn hand_text(
    _window: &WindowHandle,
    _text: &str,
    _submit: bool,
) -> Result<Option<&'static str>, DeliverError> {
    Ok(None)
}

pub fn post_text(_window: &WindowHandle, _text: &str) -> Result<(), DeliverError> {
    Err(unsupported())
}

pub fn post_chord(
    _app: &AppHandle,
    _window: &WindowHandle,
    _method: PasteMethod,
) -> Result<(), DeliverError> {
    Err(unsupported())
}

pub fn post_submit(_window: &WindowHandle, _key: AutoSubmitKey) -> Result<(), DeliverError> {
    Err(unsupported())
}

#[cfg(test)]
pub(super) fn test_handle() -> WindowHandle {
    WindowHandle
}
