//! Restores focus to the application that was frontmost when a dictation
//! recording started, so the transcript lands in the field the user was
//! actually dictating into — not wherever focus happens to be when the
//! hotkey is released (#1995).
//!
//! Recording is driven by a global keyboard shortcut and keeps capturing
//! audio no matter which window has focus, so nothing about the audio
//! itself is lost when the user clicks around mid-dictation (into Handy's
//! own window, another app, the taskbar, ...). But the eventual paste is
//! just a simulated keystroke (or direct-typed text) sent to whichever
//! application happens to be frontmost at that moment — there was
//! previously no notion of a "target" application at all. Any focus change
//! mid-dictation therefore silently redirects the transcript, which reads
//! to the user as if the whole dictation buffer was discarded.
//!
//! [`capture`] records the frontmost application right when a recording
//! starts; [`restore`] reactivates it immediately before the paste is
//! injected, if focus has since moved elsewhere. Both are no-ops when
//! nothing was captured or focus never left the original application, so
//! this only ever changes behavior in exactly the broken case described in
//! #1995.
//!
//! macOS only for now — reactivating an application windowed elsewhere
//! needs platform-specific APIs (`NSWorkspace`/`NSRunningApplication`
//! here). Windows/Linux keep the pre-existing behavior (paste goes to
//! whatever has focus at release time) rather than ship unverified platform
//! code; see `paste_tx` for the same per-platform-module precedent this
//! follows.

/// Pure decision: given the pid captured at recording start and whichever
/// pid is frontmost right now, does the captured application need to be
/// reactivated? Kept outside the `cfg(target_os = "macos")` module (like
/// `paste_tx::evaluate`) so this logic is unit-tested on every platform,
/// independent of whether the AppKit-calling implementation below is wired
/// up for that platform yet.
fn needs_reactivation(original_pid: i32, current_pid: Option<i32>) -> bool {
    current_pid != Some(original_pid)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::needs_reactivation;
    use log::debug;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
    use std::sync::Mutex;

    /// The frontmost application's pid at the moment the current recording
    /// started. `None` means either no recording has started yet or there
    /// was no frontmost application to capture.
    ///
    /// A single slot is enough: only one recording is ever active at a
    /// time, and every `capture()` overwrites whatever a previous
    /// recording left behind before it could matter again.
    static ORIGINAL_FRONTMOST_PID: Mutex<Option<i32>> = Mutex::new(None);

    fn frontmost_pid() -> Option<i32> {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
    }

    /// Records the currently frontmost application so [`restore`] can
    /// reactivate it later. Call once, as close to recording start as
    /// possible. Must run on the main thread (AppKit requirement).
    pub fn capture() {
        let pid = frontmost_pid();
        match pid {
            Some(pid) => debug!("focus_restore: captured frontmost application (pid {pid})"),
            None => debug!("focus_restore: no frontmost application to capture"),
        }
        *ORIGINAL_FRONTMOST_PID.lock().unwrap() = pid;
    }

    /// Reactivates the application captured by [`capture`], if it is no
    /// longer frontmost. Must run on the main thread (AppKit requirement) —
    /// call this immediately before the paste keystroke is injected.
    ///
    /// Best-effort by design: any failure (the app already quit, activation
    /// was refused, ...) just leaves the paste going wherever focus
    /// currently is, which is the same behavior as before this module
    /// existed. Restoring focus must never block or fail the paste itself.
    pub fn restore() {
        let Some(original_pid) = *ORIGINAL_FRONTMOST_PID.lock().unwrap() else {
            return;
        };

        if !needs_reactivation(original_pid, frontmost_pid()) {
            // Focus never left (or has already returned to) the original
            // application — nothing to do.
            return;
        }

        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(original_pid)
        else {
            debug!(
                "focus_restore: originally frontmost application (pid {original_pid}) is no longer running"
            );
            return;
        };

        // No options: macOS 14+ made plain activation always take over from
        // whatever else is frontmost (the same effect `ActivateIgnoringOtherApps`
        // used to provide explicitly — that flag is now deprecated and a no-op).
        if app.activateWithOptions(NSApplicationActivationOptions::empty()) {
            debug!(
                "focus_restore: reactivated originally frontmost application (pid {original_pid})"
            );
            // activateWithOptions only requests activation — AppKit doesn't
            // guarantee (or even necessarily complete) the switch by the time
            // it returns. A short settle delay gives the window server a
            // chance to actually hand focus over before the paste keystroke
            // is injected right after this call returns.
            std::thread::sleep(std::time::Duration::from_millis(50));
        } else {
            debug!(
                "focus_restore: failed to reactivate originally frontmost application (pid {original_pid})"
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    // Not implemented on Windows/Linux yet — recording and pasting behave
    // exactly as they did before this module existed. See the module docs
    // above for why this is scoped to macOS for now.
    pub fn capture() {}
    pub fn restore() {}
}

/// Records the currently frontmost application so it can be restored before
/// the transcript is pasted. Call once, at the start of a recording.
pub fn capture() {
    imp::capture();
}

/// Reactivates the application captured by [`capture`], if focus has since
/// moved elsewhere. Call immediately before injecting the paste.
pub fn restore() {
    imp::restore();
}

#[cfg(test)]
mod tests {
    use super::needs_reactivation;

    #[test]
    fn no_reactivation_needed_when_still_frontmost() {
        assert!(!needs_reactivation(123, Some(123)));
    }

    #[test]
    fn reactivation_needed_when_a_different_app_is_frontmost() {
        assert!(needs_reactivation(123, Some(456)));
    }

    #[test]
    fn reactivation_needed_when_nothing_is_currently_frontmost() {
        assert!(needs_reactivation(123, None));
    }
}
