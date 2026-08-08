//! Windows reliable paste.
//!
//! Publishes the transcript as a *delayed-render* clipboard format
//! (`SetClipboardData(CF_UNICODETEXT, NULL)`) owned by a hidden message-only
//! window. Windows sends the owner `WM_RENDERFORMAT` when a consumer actually
//! requests the data — that message is the read receipt. The previous
//! clipboard contents (snapshotted with full format fidelity) are restored
//! once receipts go quiet (see `paste_tx::evaluate`), guarded by the clipboard
//! sequence number so we never clobber a newer user copy.
//!
//! A read receipt alone is not proof the *paste target* took the transcript:
//! clipboard monitors (history/cloud services, managers, antivirus, IMEs)
//! read eagerly on every clipboard change, milliseconds after the chord, and
//! field logs from #502 show such a reader consuming the one-shot promise on
//! every paste. Two mechanisms make the receipt attributable:
//!
//! - While a requester is blocked inside `GetClipboardData`, it still has the
//!   clipboard open, so `GetOpenClipboardWindow` identifies its process. Only
//!   a read by the process the chord was addressed to (foreground at
//!   injection, or foreground right now) counts as a *trusted* receipt and
//!   triggers the early restore + auto-submit; anything else is logged and
//!   ignored.
//! - Rendering is one-shot: the first read consumes the promise and every
//!   later read is invisible. After an untrusted read we therefore *re-arm*
//!   (re-publish the promise, bounded by `MAX_REARMS`) so the target's own
//!   read remains observable. Without a trusted receipt the transcript simply
//!   stays on the clipboard until the `RESTORE_TIMEOUT` backstop — the
//!   failure mode is a late restore, never a stale paste.
//!
//! Threading: clipboard ownership and delayed rendering are per-thread and
//! need a message pump, so the whole transaction lives on a dedicated worker
//! thread. The calling thread only sends the paste chord once the worker
//! signals the transcript is published, then returns; the wait, guarded
//! restore and auto-submit all finish on the worker.

use std::sync::{mpsc::Sender, Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use log::{error, info, warn};
use tauri::Manager;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT,
    WPARAM,
};

use super::{evaluate, send_chord, TxState, WaitDecision};
use crate::clipboard::send_return_key;
use crate::input::EnigoState;
use crate::settings::{AutoSubmitKey, ClipboardHandling, PasteMethod};
use windows::Win32::Foundation::GlobalFree;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, GetOpenClipboardWindow, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{
    CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_DSPTEXT, CF_ENHMETAFILE,
    CF_OWNERDISPLAY, CF_PALETTE, CF_UNICODETEXT,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyImage, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId, KillTimer,
    PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, SetWindowLongPtrW, GDI_IMAGE_TYPE,
    GWLP_USERDATA, HWND_MESSAGE, IMAGE_FLAGS, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DESTROYCLIPBOARD, WM_RENDERALLFORMATS, WM_RENDERFORMAT, WM_TIMER, WNDCLASSW,
};

const CLASS_NAME: PCWSTR = w!("HandyPasteTxWindow");
const TIMER_ID: usize = 1;
const TIMER_INTERVAL_MS: u32 = 25;
/// Posted to the transaction window after a non-target reader consumed the
/// delayed-render promise, asking the pump thread to re-publish it.
const WM_APP_REARM: u32 = WM_APP + 1;
/// Upper bound on promise re-publications per transaction. Each re-arm bumps
/// the clipboard sequence and re-notifies clipboard listeners, so an
/// aggressive monitor could otherwise ping-pong with us indefinitely. Once
/// exhausted the transcript stays (readable) on the clipboard and the
/// `RESTORE_TIMEOUT` backstop settles the transaction.
const MAX_REARMS: u32 = 10;
/// Skip clipboard formats larger than this when snapshotting.
const MAX_FORMAT_BYTES: usize = 64 * 1024 * 1024;

const IMAGE_BITMAP_TYPE: GDI_IMAGE_TYPE = GDI_IMAGE_TYPE(0);
const LR_CREATEDIBSECTION_FLAG: IMAGE_FLAGS = IMAGE_FLAGS(0x2000);

struct SavedFormat {
    format: u32,
    data: Vec<u8>,
}

pub(super) struct WinTxShared {
    state: Mutex<TxState>,
    text: String,
    snapshot: Mutex<Vec<SavedFormat>>,
    /// Copied HBITMAP (as raw usize), restored via SetClipboardData.
    saved_bitmap: Mutex<Option<usize>>,
    sequence: Mutex<u32>,
    app_handle: tauri::AppHandle,
    auto_submit: bool,
    auto_submit_key: AutoSubmitKey,
    /// ClipboardHandling::CopyToClipboard — settle by leaving the transcript
    /// on the clipboard as plain text instead of restoring the snapshot.
    preserve_transcript: bool,
    /// PID of the foreground process at chord injection — the process the
    /// chord was addressed to. Only its clipboard reads (or the current
    /// foreground process's) count as paste receipts.
    target_pid: Mutex<Option<u32>>,
    /// How many times the delayed-render promise has been re-published after
    /// a non-target reader consumed it (see `MAX_REARMS`).
    rearm_count: Mutex<u32>,
}

/// The transaction currently holding the clipboard, if any. A new
/// transaction settles it before snapshotting (see `flush_pending`).
static PENDING: Mutex<Option<Arc<WinTxShared>>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn shared_ptr(hwnd: HWND) -> *const WinTxShared {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WinTxShared
}

/// Full image path of a process, e.g. `C:\...\chrome.exe`.
unsafe fn process_image_name(pid: u32) -> Option<String> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let queried = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buf.as_mut_ptr()),
        &mut len,
    );
    let _ = CloseHandle(handle);
    queried.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// PID of the process that currently has the clipboard open. Only meaningful
/// while handling a clipboard message (WM_RENDERFORMAT / WM_DESTROYCLIPBOARD):
/// the requester still holds the clipboard open at that point, so
/// GetOpenClipboardWindow points at their window. Returns None when the reader
/// opened the clipboard with a NULL hwnd and cannot be identified.
unsafe fn clipboard_opener_pid() -> Option<u32> {
    let hwnd = GetOpenClipboardWindow().ok()?;
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    (pid != 0).then_some(pid)
}

/// PID of the process owning the current foreground window, if any.
unsafe fn foreground_pid() -> Option<u32> {
    let fg_hwnd = GetForegroundWindow();
    if fg_hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(fg_hwnd, Some(&mut pid));
    (pid != 0).then_some(pid)
}

fn describe_pid(pid: Option<u32>) -> String {
    match pid {
        Some(pid) => format!(
            "{:?} (pid {pid})",
            unsafe { process_image_name(pid) }.unwrap_or_else(|| "<unknown>".to_string())
        ),
        None => "<unidentified>".to_string(),
    }
}

/// The clipboard access timing relative to the paste chord, for the logs.
fn timing_relative_to_chord(shared: &WinTxShared, now: Instant) -> String {
    match shared.state.lock() {
        Ok(st) => match st.injected_at {
            Some(injected) if now >= injected => {
                format!("{}ms after chord", now.duration_since(injected).as_millis())
            }
            Some(injected) => {
                format!(
                    "{}ms BEFORE chord",
                    injected.duration_since(now).as_millis()
                )
            }
            None => "before chord injection".to_string(),
        },
        Err(_) => "<state poisoned>".to_string(),
    }
}

/// Diagnostic for #502: name the process taking clipboard ownership away from
/// this transaction (the user copying elsewhere, or a clipboard tool).
fn log_ownership_taken(shared: &WinTxShared, now: Instant) {
    info!(
        "[reliable-paste] ownership taken: accessor={} foreground={} at {}",
        describe_pid(unsafe { clipboard_opener_pid() }),
        describe_pid(unsafe { foreground_pid() }),
        timing_relative_to_chord(shared, now)
    );
}

/// Handles WM_RENDERFORMAT: identify the reader, decide whether this is the
/// paste target taking the transcript (a *trusted* receipt) or a third-party
/// monitor, render the promised text either way, and after an untrusted read
/// ask the pump to re-arm the promise so the target's read stays observable.
unsafe fn handle_render_request(hwnd: HWND, shared: &WinTxShared, format: u32, now: Instant) {
    let reader_pid = clipboard_opener_pid();
    let fg_pid = foreground_pid();
    let target_pid = shared.target_pid.lock().ok().and_then(|slot| *slot);
    // The chord is addressed to the process that was foreground at injection;
    // also accept the process that is foreground *now* to cover a focus
    // change between injection and delivery. An unidentifiable reader
    // (clipboard opened with a NULL window) is never trusted — the safe
    // failure direction is a late restore, not a stale paste.
    let trusted = matches!(
        reader_pid,
        Some(pid) if Some(pid) == target_pid || Some(pid) == fg_pid
    );
    info!(
        "[reliable-paste] render request (format {format}): accessor={} trusted={trusted} \
         target={} foreground={} at {}",
        describe_pid(reader_pid),
        describe_pid(target_pid),
        describe_pid(fg_pid),
        timing_relative_to_chord(shared, now)
    );
    if trusted {
        if let Ok(mut st) = shared.state.lock() {
            st.record_receipt(now);
        }
    }
    if format == CF_UNICODETEXT.0 as u32 {
        render_text(shared);
        if !trusted {
            // The one-shot promise is now consumed by a non-target reader.
            // Re-arm once the reader releases the clipboard (we cannot open
            // it here — the requester still holds it).
            let _ = PostMessageW(Some(hwnd), WM_APP_REARM, WPARAM(0), LPARAM(0));
        }
    }
}

/// Re-publishes the delayed-render promise after a non-target reader consumed
/// it. Runs on the pump thread via WM_APP_REARM, i.e. after the reader's
/// GetClipboardData returned.
unsafe fn rearm_promise(hwnd: HWND, shared: &WinTxShared) {
    {
        let st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        // A trusted receipt or a finished/cancelled transaction needs no
        // tripwire anymore.
        if st.cancelled || st.ownership_lost || st.any_receipt_after_injection() {
            return;
        }
    }
    let mut count = match shared.rearm_count.lock() {
        Ok(count) => count,
        Err(_) => return,
    };
    if *count >= MAX_REARMS {
        if *count == MAX_REARMS {
            *count += 1;
            info!(
                "[reliable-paste] re-arm budget exhausted; further reads are undetectable, \
                 transcript stays on clipboard until timeout"
            );
        }
        return;
    }
    if !GetClipboardOwner()
        .map(|owner| owner == hwnd)
        .unwrap_or(false)
    {
        return;
    }
    // The reader may not have closed the clipboard yet; retry briefly.
    let mut opened = false;
    for _ in 0..5 {
        if OpenClipboard(Some(hwnd)).is_ok() {
            opened = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    if !opened {
        warn!(
            "[reliable-paste] could not reopen clipboard to re-arm; transcript stays as plain text"
        );
        return;
    }
    let armed = set_text_promise();
    // Read the sequence before releasing the clipboard: nobody else can bump
    // it while we hold the clipboard open, so this cannot race a user copy.
    let sequence = GetClipboardSequenceNumber();
    let _ = CloseClipboard();
    match armed {
        Ok(()) => {
            if let Ok(mut slot) = shared.sequence.lock() {
                *slot = sequence;
            }
            *count += 1;
            info!(
                "[reliable-paste] re-armed delayed-render promise ({}/{})",
                *count, MAX_REARMS
            );
        }
        Err(e) => warn!("[reliable-paste] re-arm failed: {e}; transcript stays as plain text"),
    }
}

/// Sends the auto-submit Enter. Uses `try_lock` because the paste caller may
/// currently hold the enigo lock while waiting for this worker.
fn send_auto_submit(shared: &WinTxShared) {
    {
        let mut st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        if st.auto_submit_sent {
            return;
        }
        st.auto_submit_sent = true;
    }
    if let Some(enigo_state) = shared.app_handle.try_state::<EnigoState>() {
        match enigo_state.0.try_lock() {
            Ok(mut enigo) => {
                let _ = send_return_key(&mut enigo, shared.auto_submit_key);
            }
            Err(_) => warn!("[reliable-paste] skipping auto-submit: input state busy"),
        }
    }
}

/// Renders the promised transcript into the clipboard, which must already be
/// open: the system opens it on our behalf for WM_RENDERFORMAT; every other
/// caller has to wrap this in OpenClipboard/CloseClipboard itself.
unsafe fn render_text(shared: &WinTxShared) {
    let wide_text: Vec<u16> = shared
        .text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, wide_text.len() * 2) else {
        return;
    };
    let ptr = GlobalLock(hg) as *mut u16;
    if ptr.is_null() {
        let _ = GlobalFree(Some(hg));
        return;
    }
    std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
    let _ = GlobalUnlock(hg);
    if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hg.0))).is_err() {
        let _ = GlobalFree(Some(hg));
    }
}

unsafe extern "system" fn paste_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shared = shared_ptr(hwnd);
    match msg {
        WM_RENDERFORMAT => {
            if !shared.is_null() {
                handle_render_request(hwnd, &*shared, wparam.0 as u32, Instant::now());
            }
            LRESULT(0)
        }
        WM_APP_REARM => {
            if !shared.is_null() {
                rearm_promise(hwnd, &*shared);
            }
            LRESULT(0)
        }
        WM_RENDERALLFORMATS => {
            // Sent when the window is destroyed while an unrendered promise is
            // still on the clipboard — not a consumer read, so no receipt.
            // Unlike WM_RENDERFORMAT the system does not open the clipboard on
            // our behalf here: open it and confirm we still own it first.
            if !shared.is_null() {
                let shared = &*shared;
                if OpenClipboard(Some(hwnd)).is_ok() {
                    if GetClipboardOwner()
                        .map(|owner| owner == hwnd)
                        .unwrap_or(false)
                    {
                        render_text(shared);
                    }
                    let _ = CloseClipboard();
                }
            }
            LRESULT(0)
        }
        WM_DESTROYCLIPBOARD => {
            if !shared.is_null() {
                let shared = &*shared;
                // Our own settle empties the clipboard too (cancelled is set
                // before settling); only log genuine third-party takeovers.
                let already_settling = shared.state.lock().map(|st| st.cancelled).unwrap_or(false);
                if !already_settling {
                    log_ownership_taken(shared, Instant::now());
                }
                if let Ok(mut st) = shared.state.lock() {
                    st.ownership_lost = true;
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if !shared.is_null() {
                on_timer(hwnd, &*shared);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn ensure_window_class(hinstance: HINSTANCE) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(paste_wnd_proc),
            hInstance: hinstance,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&wc);
        }
    });
}

/// If a previous transaction is still holding the clipboard, settle it now so
/// the snapshot below captures the user's original clipboard content. The
/// previous worker observes `cancelled` on its next timer tick and tears down
/// without restoring.
fn flush_pending() {
    let previous = match PENDING.lock() {
        Ok(mut slot) => slot.take(),
        Err(_) => None,
    };
    let Some(previous) = previous else {
        return;
    };
    let receipt = {
        let mut st = match previous.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        st.cancelled = true;
        st.any_receipt_after_injection()
    };
    if previous.auto_submit && receipt {
        send_auto_submit(&previous);
    }
    let sequence = *previous.sequence.lock().unwrap();
    let still_ours = unsafe { GetClipboardSequenceNumber() } == sequence;
    if still_ours {
        unsafe { settle_clipboard(&previous) };
    }
}

/// Settle-time clipboard handling once we know we still own the clipboard:
/// restore the snapshot, or — for ClipboardHandling::CopyToClipboard — replace
/// the concealed promise with plain transcript text, so clipboard history and
/// managers record it and it survives this transaction's window going away.
unsafe fn settle_clipboard(shared: &WinTxShared) {
    if !shared.preserve_transcript {
        restore_snapshot(shared);
        return;
    }
    if OpenClipboard(None).is_err() {
        warn!("[reliable-paste] could not open clipboard to leave transcript");
        return;
    }
    let _ = EmptyClipboard();
    render_text(shared);
    let _ = CloseClipboard();
    info!("[reliable-paste] left transcript on clipboard as plain text");
}

/// Restores the snapshotted clipboard contents. Safe to call from any thread.
unsafe fn restore_snapshot(shared: &WinTxShared) {
    if OpenClipboard(None).is_err() {
        warn!("[reliable-paste] could not open clipboard to restore");
        return;
    }
    let _ = EmptyClipboard();
    if let Ok(formats) = shared.snapshot.lock() {
        for saved in formats.iter() {
            if saved.data.is_empty() {
                continue;
            }
            let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, saved.data.len()) else {
                continue;
            };
            let ptr = GlobalLock(hg) as *mut u8;
            if ptr.is_null() {
                let _ = GlobalFree(Some(hg));
                continue;
            }
            std::ptr::copy_nonoverlapping(saved.data.as_ptr(), ptr, saved.data.len());
            let _ = GlobalUnlock(hg);
            // SetClipboardData takes ownership of the handle on success.
            if SetClipboardData(saved.format, Some(HANDLE(hg.0))).is_err() {
                let _ = GlobalFree(Some(hg));
            }
        }
    }
    if let Ok(mut bitmap) = shared.saved_bitmap.lock() {
        if let Some(raw) = bitmap.take() {
            let _ = SetClipboardData(CF_BITMAP.0 as u32, Some(HANDLE(raw as *mut _)));
        }
    }
    let _ = CloseClipboard();
    info!("[reliable-paste] restored previous clipboard");
}

unsafe fn snapshot_clipboard(hwnd: HWND, shared: &WinTxShared) -> Result<(), String> {
    OpenClipboard(Some(hwnd)).map_err(|e| format!("OpenClipboard failed: {e}"))?;
    let mut formats = Vec::new();
    let mut format = 0u32;
    loop {
        format = EnumClipboardFormats(format);
        if format == 0 {
            break;
        }
        if format == CF_BITMAP.0 as u32 {
            // GDI object, not global memory: duplicate the handle instead.
            if let Ok(handle) = GetClipboardData(CF_BITMAP.0 as u32) {
                if let Ok(copy) =
                    CopyImage(handle, IMAGE_BITMAP_TYPE, 0, 0, LR_CREATEDIBSECTION_FLAG)
                {
                    if let Ok(mut slot) = shared.saved_bitmap.lock() {
                        *slot = Some(copy.0 as usize);
                    }
                }
            }
            continue;
        }
        // Formats whose handles are not plain global memory cannot be
        // byte-copied; skipping them matches what the legacy path restored.
        if format == CF_ENHMETAFILE.0 as u32
            || format == CF_DSPENHMETAFILE.0 as u32
            || format == CF_DSPBITMAP.0 as u32
            || format == CF_DSPMETAFILEPICT.0 as u32
            || format == CF_DSPTEXT.0 as u32
            || format == CF_OWNERDISPLAY.0 as u32
            || format == CF_PALETTE.0 as u32
        {
            continue;
        }
        if let Ok(handle) = GetClipboardData(format) {
            let hg = HGLOBAL(handle.0);
            let size = GlobalSize(hg);
            if size == 0 || size > MAX_FORMAT_BYTES {
                continue;
            }
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() {
                continue;
            }
            let data = std::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(hg);
            formats.push(SavedFormat { format, data });
        }
    }
    let _ = CloseClipboard();
    if let Ok(mut slot) = shared.snapshot.lock() {
        *slot = formats;
    }
    Ok(())
}

/// Publishes the transcript as a delayed-render promise plus clipboard
/// history / cloud / monitoring opt-out markers (the same formats Chrome uses
/// for Incognito copies). Returns the new clipboard sequence number.
unsafe fn publish(hwnd: HWND) -> Result<u32, String> {
    OpenClipboard(Some(hwnd)).map_err(|e| format!("OpenClipboard failed: {e}"))?;
    let published = publish_formats();
    let closed = CloseClipboard();
    published?;
    closed.map_err(|e| format!("CloseClipboard failed: {e}"))?;
    Ok(GetClipboardSequenceNumber())
}

/// Everything `publish` does while the clipboard is open, split out so
/// `publish` closes the clipboard on every path — bailing out while holding it
/// open (and possibly already emptied) would strand the clipboard and leave
/// the legacy fallback snapshotting nothing.
unsafe fn publish_formats() -> Result<(), String> {
    EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;

    for (name, value) in [
        ("ExcludeClipboardContentFromMonitorProcessing", 1u32),
        ("CanIncludeInClipboardHistory", 0u32),
        ("CanUploadToCloudClipboard", 0u32),
    ] {
        let name_wide = wide(name);
        let format = RegisterClipboardFormatW(PCWSTR(name_wide.as_ptr()));
        if format == 0 {
            continue;
        }
        if let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>()) {
            let ptr = GlobalLock(hg) as *mut u32;
            if !ptr.is_null() {
                *ptr = value;
                let _ = GlobalUnlock(hg);
                if SetClipboardData(format, Some(HANDLE(hg.0))).is_err() {
                    let _ = GlobalFree(Some(hg));
                }
            } else {
                let _ = GlobalFree(Some(hg));
            }
        }
    }

    set_text_promise()
}

/// Puts the delayed-render CF_UNICODETEXT promise on the (already open)
/// clipboard: a NULL handle means we are only asked for the data (via
/// WM_RENDERFORMAT) when a consumer actually reads it. SetClipboardData
/// returns the handle it was given, so for delayed rendering success is also
/// NULL and the windows crate reports it as an Err carrying GetLastError().
/// Only a nonzero thread error is a real failure, and the thread error must
/// be cleared first so a stale value from an earlier call can't masquerade as
/// one.
unsafe fn set_text_promise() -> Result<(), String> {
    SetLastError(ERROR_SUCCESS);
    if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, None) {
        if e.code().is_err() {
            return Err(format!("SetClipboardData failed: {e}"));
        }
    }
    Ok(())
}

fn on_timer(_hwnd: HWND, shared: &WinTxShared) {
    let now = Instant::now();
    let finish = {
        let mut st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        if st.cancelled {
            true
        } else {
            match evaluate(&st, now) {
                WaitDecision::KeepWaiting => false,
                WaitDecision::Finish => {
                    st.cancelled = true;
                    true
                }
            }
        }
    };
    if !finish {
        return;
    }

    let (receipt, ownership_lost, injection_failed) = {
        let st = match shared.state.lock() {
            Ok(st) => st,
            Err(_) => return,
        };
        (
            st.any_receipt_after_injection(),
            st.ownership_lost,
            st.injection_failed,
        )
    };
    if ownership_lost {
        info!("[reliable-paste] settling: clipboard ownership lost");
    } else if receipt {
        info!("[reliable-paste] settling: reads went quiet");
    } else if injection_failed {
        info!("[reliable-paste] settling: chord injection failed, restoring quickly");
    } else {
        info!("[reliable-paste] settling: no read within timeout, restoring anyway");
    }

    // Auto-submit only once the target demonstrably read the transcript;
    // pressing Enter after an unconfirmed paste could submit stale content.
    if shared.auto_submit && receipt {
        send_auto_submit(shared);
    }

    let sequence = *shared.sequence.lock().unwrap();
    let still_ours = !ownership_lost && unsafe { GetClipboardSequenceNumber() } == sequence;
    if still_ours {
        unsafe { settle_clipboard(shared) };
    } else {
        info!("[reliable-paste] clipboard changed externally; leaving it untouched");
    }

    if let Ok(mut slot) = PENDING.lock() {
        let is_us = slot
            .as_ref()
            .map(|pending| Arc::as_ptr(pending) as *const WinTxShared == shared as *const _)
            .unwrap_or(false);
        if is_us {
            *slot = None;
        }
    }

    unsafe {
        PostQuitMessage(0);
    }
}

unsafe fn destroy_window_and_shared(hwnd: HWND) {
    let ptr = shared_ptr(hwnd);
    let _ = DestroyWindow(hwnd);
    if !ptr.is_null() {
        drop(Arc::from_raw(ptr));
    }
}

fn pump_thread(shared: Arc<WinTxShared>, ready: Sender<Result<(), String>>) {
    unsafe {
        // Settle any previous transaction first so the snapshot captures the
        // user's original clipboard, not the previous transcript.
        flush_pending();

        let hinstance = match GetModuleHandleW(PCWSTR::null()) {
            Ok(hmodule) => HINSTANCE(hmodule.0),
            Err(e) => {
                let _ = ready.send(Err(format!("GetModuleHandle failed: {e}")));
                return;
            }
        };
        ensure_window_class(hinstance);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("HandyPasteTx"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                let _ = ready.send(Err(format!("CreateWindowEx failed: {e}")));
                return;
            }
        };
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            Arc::into_raw(shared.clone()) as *const _ as isize,
        );

        let published = match snapshot_clipboard(hwnd, &shared) {
            Ok(()) => match publish(hwnd) {
                Ok(sequence) => Ok(sequence),
                Err(e) => {
                    // publish may have emptied the clipboard before failing;
                    // put the snapshot back so the legacy fallback's own
                    // snapshot captures the user's clipboard, not an empty one.
                    restore_snapshot(&shared);
                    Err(e)
                }
            },
            Err(e) => Err(e),
        };
        let sequence = match published {
            Ok(sequence) => sequence,
            Err(e) => {
                destroy_window_and_shared(hwnd);
                let _ = ready.send(Err(e));
                return;
            }
        };
        *shared.sequence.lock().unwrap() = sequence;
        shared.state.lock().unwrap().published_at = Instant::now();
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(shared.clone());
        }
        let _ = SetTimer(Some(hwnd), TIMER_ID, TIMER_INTERVAL_MS, None);
        let _ = ready.send(Ok(()));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = DispatchMessageW(&msg);
        }

        let _ = KillTimer(Some(hwnd), TIMER_ID);
        destroy_window_and_shared(hwnd);
    }
}

pub(super) fn run(
    text: &str,
    app_handle: &tauri::AppHandle,
    paste_method: &PasteMethod,
    enigo: &mut enigo::Enigo,
    auto_submit: bool,
    auto_submit_key: AutoSubmitKey,
    clipboard_handling: ClipboardHandling,
) -> Result<(), String> {
    let shared = Arc::new(WinTxShared {
        state: Mutex::new(TxState::new()),
        text: text.to_string(),
        snapshot: Mutex::new(Vec::new()),
        saved_bitmap: Mutex::new(None),
        sequence: Mutex::new(0),
        app_handle: app_handle.clone(),
        auto_submit,
        auto_submit_key,
        preserve_transcript: clipboard_handling == ClipboardHandling::CopyToClipboard,
        target_pid: Mutex::new(None),
        rearm_count: Mutex::new(0),
    });

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let shared_for_pump = shared.clone();
    thread::spawn(move || pump_thread(shared_for_pump, ready_tx));

    // Wait until the transcript is actually published (or the worker reports
    // why it could not) before injecting the chord.
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("reliable paste worker died before publishing".to_string()),
    }
    info!("[reliable-paste] published transcript (delayed render)");

    // The chord goes to whichever process is foreground right now; remember
    // it so only its clipboard reads count as paste receipts.
    let target_pid = unsafe { foreground_pid() };
    if let Ok(mut slot) = shared.target_pid.lock() {
        *slot = target_pid;
    }
    info!(
        "[reliable-paste] paste target: {}",
        describe_pid(target_pid)
    );

    // Mark injection *before* sending: enigo holds the chord for ~100ms and a
    // fast target may legitimately read while the chord is still held.
    shared.state.lock().unwrap().injected_at = Some(Instant::now());
    match send_chord(enigo, paste_method) {
        Ok(()) => {
            info!("[reliable-paste] paste chord sent ({paste_method:?})");
        }
        Err(e) => {
            // Keep the transaction alive: the worker restores the clipboard
            // after the short failed-injection timeout.
            shared.state.lock().unwrap().injection_failed = true;
            error!("[reliable-paste] failed to send paste chord: {e}");
        }
    }

    Ok(())
}
