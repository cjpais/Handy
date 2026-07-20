//! macOS application focus management for paste reliability.
//!
//! When Handy shows the recording overlay, `orderFrontRegardless` can activate
//! the Handy application, stealing focus from the user's target app. When the
//! paste (Cmd+V) is then sent, it goes to Handy instead of the user's app.
//!
//! This module provides `save_frontmost_app` / `restore_frontmost_app` so that
//! the paste flow can re-activate the user's original application before
//! sending keystrokes.
//!
//! **Thread safety**: All Objective-C calls are wrapped in `autoreleasepool`
//! blocks to prevent heap corruption from autoreleased objects on non-main
//! threads (tokio workers). Without an autorelease pool, temporary ObjC objects
//! (like `NSString` from `bundleIdentifier()`) leak and corrupt the malloc
//! heap, causing `nanov2_guard_corruption_detected` / SIGABRT.

use log::{info, warn};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

/// A saved reference to the previously frontmost macOS application.
/// Wrapped in `Send + Sync` for safe storage in Tauri managed state.
pub struct SavedFrontmostApp(Mutex<Option<FrontmostApp>>);

/// Internal representation of a macOS `NSRunningApplication`.
///
/// We store the `bundleIdentifier` (e.g. "com.apple.Safari") and the
/// `processIdentifier` (PID) so we can look the app up later and activate it.
/// Only plain Rust types are stored — no ObjC references — so this is safe
/// to hold across threads and autorelease pool boundaries.
struct FrontmostApp {
    bundle_id: String,
    pid: i32,
}

impl SavedFrontmostApp {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

/// Save the bundle identifier and PID of the current frontmost application.
///
/// On non-macOS platforms this is a no-op (the overlay uses different windowing
/// primitives there and focus-stealing is not observed).
pub fn save_frontmost_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // All ObjC API calls MUST happen inside an autorelease pool to prevent
        // autoreleased temporary objects from corrupting the malloc heap on
        // background (tokio worker) threads.
        let saved = tauri_nspanel::objc2::rc::autoreleasepool(|_| get_frontmost_app_info());
        if let Some(ref info) = saved {
            info!(
                "Saving frontmost app: bundle_id={}, pid={}",
                info.bundle_id, info.pid
            );
        }
        if let Some(state) = app.try_state::<SavedFrontmostApp>() {
            let mut guard = state.0.lock();
            *guard = saved;
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

/// Check if the saved frontmost application is a desktop/file manager
/// (e.g., Finder on macOS) where pasting text is not meaningful.
///
/// On macOS, Finder interprets Cmd+V as "paste files" when a file reference
/// is on the clipboard, or does nothing when plain text is on the clipboard.
/// Neither of these is useful for the user — they want the text in a text
/// field. When the user is focused on Finder/Desktop, we should fall back
/// to clipboard-only mode (CopyToClipboard) and show a toast notification.
pub fn is_saved_app_desktop_like(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let state = app.try_state::<SavedFrontmostApp>();
        if let Some(s) = state {
            let guard = s.0.lock();
            if let Some(ref info) = *guard {
                // Finder is the macOS desktop/file manager app
                // Also detect other file managers that might be in the foreground
                let is_finder = info.bundle_id == "com.apple.finder";
                if is_finder {
                    info!(
                        "Saved frontmost app is Finder (bundle_id={}, pid={}) — treating as desktop",
                        info.bundle_id, info.pid
                    );
                }
                return is_finder;
            }
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

/// Restore the previously frontmost application by activating it.
///
/// Should be called **before** sending the Cmd+V paste keystroke so the
/// target application receives the paste instead of Handy.
///
/// Returns `true` if a previously-saved app was successfully re-activated,
/// `false` if there was no saved app or activation failed.
pub fn restore_frontmost_app(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let saved_info = {
            let state = app.try_state::<SavedFrontmostApp>();
            match state {
                Some(s) => s.0.lock().take(),
                None => None,
            }
        };

        let Some(info) = saved_info else {
            info!("No saved frontmost app to restore");
            return false;
        };

        info!(
            "Restoring frontmost app: bundle_id={}, pid={}",
            info.bundle_id, info.pid
        );
        // All ObjC API calls MUST happen inside an autorelease pool to prevent
        // autoreleased temporary objects from corrupting the malloc heap on
        // background (tokio worker) threads.
        let activated =
            tauri_nspanel::objc2::rc::autoreleasepool(|_| activate_app_by_pid(info.pid, &info.bundle_id));
        if activated {
            // Small delay to let the target app's run loop process the
            // activation before we send keystrokes. 50ms is generous —
            // the macOS window server typically processes activation within
            // a single run-loop iteration (~16ms at 60Hz).
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        activated
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

// ── macOS-specific implementation using objc2-app-kit safe API ───────

#[cfg(target_os = "macos")]
fn get_frontmost_app_info() -> Option<FrontmostApp> {
    use tauri_nspanel::objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;

    let bundle_id = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let pid = app.processIdentifier();

    Some(FrontmostApp { bundle_id, pid })
}

#[cfg(target_os = "macos")]
fn activate_app_by_pid(pid: i32, bundle_id: &str) -> bool {
    use tauri_nspanel::objc2_app_kit::{NSApplicationActivationOptions, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let running_apps = workspace.runningApplications();

    // Find the app by PID.
    let found = running_apps
        .iter()
        .find(|app| app.processIdentifier() == pid);

    let Some(app) = found else {
        warn!(
            "Could not find running app with pid={} (bundle_id={})",
            pid, bundle_id
        );
        return false;
    };

    // Already active — nothing to do.
    if app.isActive() {
        info!(
            "App {} (pid={}) is already active, no need to restore",
            bundle_id, pid
        );
        return true;
    }

    // Activate the app.  ActivateIgnoringOtherApps is deprecated on
    // macOS 14+ (it now has no effect since the system ignores it), but
    // on older macOS it is still needed to force the app to front even
    // when another app was recently activated (e.g. Handy).
    #[allow(deprecated)]
    let options = NSApplicationActivationOptions::ActivateIgnoringOtherApps;
    let result = app.activateWithOptions(options);

    if result {
        info!("Successfully activated app {} (pid={})", bundle_id, pid);
    } else {
        warn!("Failed to activate app {} (pid={})", bundle_id, pid);
    }
    result
}