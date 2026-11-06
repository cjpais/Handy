//! Windows: is the focused window's process running at a higher integrity
//! level than Handy (for example an app started "as administrator")?
//!
//! User Interface Privilege Isolation (UIPI) silently drops input that a
//! lower-integrity process injects into a higher-integrity window, so a paste
//! into such a window does nothing while `SendInput` still reports success.
//! See issues #434 and #2319.

use log::debug;
use std::sync::OnceLock;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE, HWND};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
};

/// The focused window's process, as far as Handy could tell.
#[derive(Debug)]
pub struct ForegroundTarget {
    pub pid: u32,
    pub class: String,
    pub elevated: bool,
}

/// What reading a process's integrity level produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Integrity {
    Rid(u32),
    /// Windows refused to let us look. Our own user's processes at our level
    /// are readable, so a refusal almost always means an elevated app (or one
    /// of another user's, which UIPI does not block; the text still lands on
    /// the clipboard, so that rare miss costs a Ctrl+V).
    AccessDenied,
    Unknown,
}

/// A mandatory-label SID's last sub-authority is its integrity level
/// (low 0x1000, medium 0x2000, high 0x3000, system 0x4000).
fn rid_from_sub_authorities(sub_authorities: &[u32]) -> Option<u32> {
    sub_authorities.last().copied()
}

fn target_is_higher(target_rid: u32, own_rid: u32) -> bool {
    target_rid > own_rid
}

/// `None` when either level is unknown: the caller pastes as before.
fn decide(target: Integrity, own: Option<u32>) -> Option<bool> {
    match (target, own) {
        (Integrity::AccessDenied, _) => Some(true),
        (Integrity::Rid(target), Some(own)) => Some(target_is_higher(target, own)),
        _ => None,
    }
}

/// Closes the handle when dropped.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess/OpenProcessToken and is
        // closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn is_access_denied(error: &windows::core::Error) -> bool {
    error.code() == ERROR_ACCESS_DENIED.to_hresult()
}

/// Reads the integrity RID from a process handle opened with at least
/// PROCESS_QUERY_LIMITED_INFORMATION (or the current-process pseudo-handle).
fn process_integrity(process: HANDLE) -> Integrity {
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out pointer; the handle is closed by OwnedHandle.
    if let Err(e) = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } {
        if is_access_denied(&e) {
            return Integrity::AccessDenied;
        }
        debug!("Elevation check: OpenProcessToken failed: {e}");
        return Integrity::Unknown;
    }
    let token = OwnedHandle(token);

    // TOKEN_MANDATORY_LABEL followed by its SID; 64 bytes of u64 is plenty
    // (the label is 16 bytes, the SID at most 68) and keeps the buffer aligned.
    let mut buffer = [0u64; 16];
    let mut needed = 0u32;
    // SAFETY: the buffer and its length match; `needed` is a valid out pointer.
    if let Err(e) = unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            std::mem::size_of_val(&buffer) as u32,
            &mut needed,
        )
    } {
        debug!("Elevation check: GetTokenInformation failed ({needed} bytes needed): {e}");
        return Integrity::Unknown;
    }

    // SAFETY: GetTokenInformation succeeded, so the buffer starts with a
    // TOKEN_MANDATORY_LABEL whose SID points into the same buffer.
    let rid = unsafe {
        let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        if sid.0.is_null() {
            None
        } else {
            let count = *GetSidSubAuthorityCount(sid) as u32;
            let sub_authorities: Vec<u32> =
                (0..count).map(|i| *GetSidSubAuthority(sid, i)).collect();
            rid_from_sub_authorities(&sub_authorities)
        }
    };
    match rid {
        Some(rid) => Integrity::Rid(rid),
        None => Integrity::Unknown,
    }
}

fn own_integrity_rid() -> Option<u32> {
    static OWN: OnceLock<Option<u32>> = OnceLock::new();
    *OWN.get_or_init(|| {
        // SAFETY: the current-process pseudo-handle needs no closing.
        match process_integrity(unsafe { GetCurrentProcess() }) {
            Integrity::Rid(rid) => Some(rid),
            other => {
                debug!("Elevation check: own integrity level unreadable ({other:?})");
                None
            }
        }
    })
}

fn window_class(hwnd: HWND) -> String {
    let mut name = [0u16; 128];
    // SAFETY: the buffer is valid for its length.
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// The focused window and whether it runs above Handy. `None` when there is
/// no focused window or its level could not be read; never panics.
pub fn foreground_target() -> Option<ForegroundTarget> {
    // SAFETY: plain Win32 queries with valid arguments.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        debug!("Elevation check: no foreground window");
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        debug!("Elevation check: foreground window has no process id");
        return None;
    }

    // SAFETY: the handle is closed by OwnedHandle.
    let target = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(process) => {
            let process = OwnedHandle(process);
            process_integrity(process.0)
        }
        Err(e) if is_access_denied(&e) => Integrity::AccessDenied,
        Err(e) => {
            debug!("Elevation check: OpenProcess({pid}) failed: {e}");
            Integrity::Unknown
        }
    };

    let elevated = decide(target, own_integrity_rid())?;
    Some(ForegroundTarget {
        pid,
        class: window_class(hwnd),
        elevated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDIUM: u32 = 0x2000;
    const HIGH: u32 = 0x3000;
    const SYSTEM: u32 = 0x4000;

    #[test]
    fn only_a_higher_level_counts_as_elevated() {
        assert!(target_is_higher(HIGH, MEDIUM));
        assert!(target_is_higher(SYSTEM, HIGH));
        assert!(!target_is_higher(MEDIUM, MEDIUM));
        assert!(!target_is_higher(HIGH, HIGH));
        assert!(!target_is_higher(MEDIUM, HIGH));
    }

    #[test]
    fn the_rid_is_the_last_sub_authority() {
        assert_eq!(rid_from_sub_authorities(&[HIGH]), Some(HIGH));
        assert_eq!(rid_from_sub_authorities(&[5, 21, MEDIUM]), Some(MEDIUM));
        assert_eq!(rid_from_sub_authorities(&[]), None);
    }

    #[test]
    fn access_denied_is_elevated_and_unknown_pastes_as_before() {
        assert_eq!(decide(Integrity::AccessDenied, Some(MEDIUM)), Some(true));
        assert_eq!(decide(Integrity::AccessDenied, None), Some(true));
        assert_eq!(decide(Integrity::Rid(HIGH), Some(MEDIUM)), Some(true));
        assert_eq!(decide(Integrity::Rid(MEDIUM), Some(MEDIUM)), Some(false));
        assert_eq!(decide(Integrity::Rid(HIGH), None), None);
        assert_eq!(decide(Integrity::Unknown, Some(MEDIUM)), None);
    }

    #[test]
    fn this_process_has_a_readable_level() {
        assert!(own_integrity_rid().is_some());
    }
}
