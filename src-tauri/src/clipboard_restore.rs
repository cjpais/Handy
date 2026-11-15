//! macOS: puts the user's clipboard back in the background after a paste.
//!
//! A paste only queues the keystroke; the target reads the clipboard whenever
//! it gets to it, and nothing says when that was. Restoring too early makes it
//! paste the old clipboard (#502), so the restore waits
//! `clipboard_restore_delay_ms` on a background thread and the paste returns
//! at once. Both clipboard paste paths finish here: the plain one counting the
//! delay from the keystroke, reliable paste from the transcript's read.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use log::info;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSInteger;
use tauri::image::Image;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// The clipboard as the user had it before a paste.
pub(crate) struct SavedClipboard {
    text: Option<String>,
    image: Option<Image<'static>>,
}

impl SavedClipboard {
    fn read(app: &AppHandle) -> Self {
        let clipboard = app.clipboard();
        let text = clipboard.read_text().ok().filter(|t| !t.is_empty());
        // Only probe for an image when there is no text: reading one decodes
        // the full bitmap.
        let image = if text.is_none() {
            clipboard.read_image().ok().map(|image| image.to_owned())
        } else {
            None
        };
        Self { text, image }
    }

    /// Puts it back, or clears the clipboard when it held nothing.
    pub fn restore(&self, app: &AppHandle) {
        let clipboard = app.clipboard();
        if let Some(text) = &self.text {
            let _ = clipboard.write_text(text);
        } else if let Some(image) = &self.image {
            let _ = clipboard.write_image(image);
        } else {
            let _ = clipboard.clear();
        }
    }
}

struct Pending {
    id: u64,
    saved: SavedClipboard,
    /// The pasteboard's changeCount with the transcript on it.
    change_count: NSInteger,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn change_count() -> NSInteger {
    NSPasteboard::generalPasteboard().changeCount()
}

/// The clipboard to restore after a new paste, and cancels the pending
/// restore. While one is pending and the previous transcript is still on the
/// clipboard, that is the clipboard it would have brought back.
pub(crate) fn take_saved(app: &AppHandle) -> SavedClipboard {
    let pending = PENDING.lock().ok().and_then(|mut slot| slot.take());
    match pending {
        Some(p) if p.change_count == change_count() => p.saved,
        _ => SavedClipboard::read(app),
    }
}

/// Restores `saved` after `delay`, unless the clipboard changed from
/// `change_count` by then (the user copied something) or a newer paste took
/// the restore over (`take_saved`).
pub(crate) fn schedule(
    app: &AppHandle,
    saved: SavedClipboard,
    change_count: NSInteger,
    delay: Duration,
) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let Ok(mut slot) = PENDING.lock() else { return };
    *slot = Some(Pending {
        id,
        saved,
        change_count,
    });
    drop(slot);

    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(delay);
        // Held while restoring, so a paste starting now waits for it.
        let Ok(mut slot) = PENDING.lock() else { return };
        let Some(p) = slot.take_if(|p| p.id == id) else {
            return;
        };
        if self::change_count() == p.change_count {
            p.saved.restore(&app);
            info!("[paste] restored previous clipboard");
        } else {
            info!("[paste] clipboard changed since the paste; leaving it untouched");
        }
    });
}
