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
use std::time::{Duration, Instant};

use log::info;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSInteger;
use tauri::AppHandle;

use crate::clipboard::SavedClipboard;

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

/// Restores `saved` `clipboard_restore_delay_ms` after `since`, unless the
/// clipboard changed from `change_count` by then (the user copied something)
/// or a newer paste took the restore over (`take_saved`).
pub(crate) fn schedule(
    app: &AppHandle,
    saved: SavedClipboard,
    change_count: NSInteger,
    since: Instant,
) {
    let delay_ms = crate::settings::get_settings(app).clipboard_restore_delay_ms;
    let delay = Duration::from_millis(delay_ms).saturating_sub(since.elapsed());
    info!("[paste] restoring previous clipboard in {delay:?}");
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
