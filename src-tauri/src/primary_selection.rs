//! The Linux PRIMARY selection (the text last highlighted, pasted with
//! middle-click). Shift+Insert reads it instead of the clipboard in many
//! terminals (kitty, Ghostty, Alacritty, foot, xterm, VTE/GNOME Terminal),
//! while GUI apps read the clipboard. Writing the transcript to both lets one
//! Shift+Insert paste it everywhere.
//!
//! Wayland uses wl-clipboard when it is installed (the same tool Handy uses for
//! the clipboard there); everything else goes through arboard.

use log::{debug, warn};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::utils::is_wayland;
use arboard::{ClearExtLinux, Clipboard, GetExtLinux, LinuxClipboardKind, SetExtLinux};

/// A frozen selection owner must not hang the paste: wl-paste waits for it.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// arboard serves X11 selections from a background context that lives as
/// long as a `Clipboard` does, so keep one for the life of the process.
static ARBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

fn with_arboard<T>(
    f: impl FnOnce(&mut Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, String> {
    let mut guard = ARBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(|e| format!("Failed to open clipboard: {e}"))?);
    }
    f(guard.as_mut().expect("initialized above")).map_err(|e| e.to_string())
}

fn use_wl_clipboard() -> bool {
    is_wayland() && which("wl-copy") && which("wl-paste")
}

fn which(tool: &str) -> bool {
    Command::new("which")
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The current PRIMARY text, or `None` when it is empty, not text, or unreadable.
pub fn read() -> Option<String> {
    let text = if use_wl_clipboard() {
        read_via_wl_paste()
    } else {
        with_arboard(|c| c.get().clipboard(LinuxClipboardKind::Primary).text())
            .map_err(|e| debug!("No primary selection to save: {e}"))
            .ok()
    };
    text.filter(|t| !t.is_empty())
}

fn read_via_wl_paste() -> Option<String> {
    let mut child = Command::new("wl-paste")
        .args(["--primary", "--no-newline", "--type", "text"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| warn!("Failed to run wl-paste: {e}"))
        .ok()?;
    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    // Exits non-zero when nothing is selected.
                    return None;
                }
                let mut out = Vec::new();
                child.stdout.take()?.read_to_end(&mut out).ok()?;
                return String::from_utf8(out).ok();
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                warn!("wl-paste --primary did not answer in time; not saving the selection");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Puts `text` on PRIMARY.
pub fn write(text: &str) -> Result<(), String> {
    if use_wl_clipboard() {
        // Stdio::null: wl-copy forks a server that would keep piped fds open.
        let status = Command::new("wl-copy")
            .args(["--primary", "--"])
            .arg(text)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Failed to execute wl-copy: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err(
                "wl-copy --primary failed (does the compositor support the primary selection?)"
                    .into(),
            )
        };
    }
    with_arboard(|c| c.set().clipboard(LinuxClipboardKind::Primary).text(text))
}

/// Puts back what [`read`] saved, or empties PRIMARY if there was nothing.
pub fn restore(saved: Option<String>) {
    let result = match saved {
        Some(text) => write(&text),
        None => clear(),
    };
    if let Err(e) = result {
        warn!("Failed to restore the primary selection: {e}");
    }
}

fn clear() -> Result<(), String> {
    if use_wl_clipboard() {
        let status = Command::new("wl-copy")
            .args(["--primary", "--clear"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Failed to execute wl-copy: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err("wl-copy --primary --clear failed".into())
        };
    }
    with_arboard(|c| c.clear_with().clipboard(LinuxClipboardKind::Primary))
}
