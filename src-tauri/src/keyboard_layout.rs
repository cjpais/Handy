//! "Follow keyboard layout": picks the transcription language from the OS
//! keyboard layout. Detection is adapted from @vladstudio's PR #559.

use crate::settings::AppSettings;
use tauri::AppHandle;

/// `selected_language` value that follows the keyboard layout.
pub const FOLLOW_KEYBOARD_LAYOUT: &str = "keyboard_layout";

/// Resolves the follow-layout intent on a settings snapshot (never written
/// back): the layout's language, or auto-detect when it can't be read.
pub fn apply(app: &AppHandle, settings: &mut AppSettings) {
    if settings.selected_language == FOLLOW_KEYBOARD_LAYOUT {
        settings.selected_language = current_language(app).unwrap_or_else(|| "auto".to_string());
    }
}

/// Language of the active keyboard layout (e.g. "ru", "en-US").
#[cfg(target_os = "macos")]
fn current_language(app: &AppHandle) -> Option<String> {
    use objc2_foundation::MainThreadMarker;
    use std::{sync::mpsc, time::Duration};

    // TIS APIs must run on the main thread.
    if MainThreadMarker::new().is_some() {
        return macos::current_language();
    }
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(macos::current_language());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(1)).ok().flatten()
}

#[cfg(target_os = "windows")]
fn current_language(_app: &AppHandle) -> Option<String> {
    use windows::Win32::Globalization::LCIDToLocaleName;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    // The layout is per thread, so read the foreground app's, not Handy's.
    // SAFETY: plain Win32 calls; the buffer outlives the call.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let lang_id = GetKeyboardLayout(thread).0 as usize as u32 & 0xFFFF;
        let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        let len = LCIDToLocaleName(lang_id, Some(&mut name), 0);
        (len > 1).then(|| String::from_utf16_lossy(&name[..len as usize - 1]))
    }
}

// ponytail: Linux has no single layout API (X11/Wayland/IBus differ); falls
// back to auto-detect until someone needs it.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn current_language(_app: &AppHandle) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void, CStr};

    type CfRef = *const c_void;
    const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentKeyboardInputSource() -> CfRef;
        fn TISGetInputSourceProperty(source: CfRef, key: CfRef) -> CfRef;
        static kTISPropertyInputSourceLanguages: CfRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: CfRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfRef, index: isize) -> CfRef;
        fn CFStringGetCString(s: CfRef, buf: *mut c_char, len: isize, encoding: u32) -> u8;
        fn CFRelease(value: CfRef);
    }

    /// First (primary) language the current input source is for. Unlike the
    /// layout, the input source also covers input methods such as Japanese.
    pub(super) fn current_language() -> Option<String> {
        // SAFETY: called on the main thread. The source follows the Create
        // Rule and is released below; the languages array and its strings are
        // owned by the source and only read while it is retained.
        unsafe {
            let source = TISCopyCurrentKeyboardInputSource();
            if source.is_null() {
                return None;
            }
            let langs = TISGetInputSourceProperty(source, kTISPropertyInputSourceLanguages);
            let mut buf = [0 as c_char; 32];
            let found = !langs.is_null()
                && CFArrayGetCount(langs) > 0
                && CFStringGetCString(
                    CFArrayGetValueAtIndex(langs, 0),
                    buf.as_mut_ptr(),
                    buf.len() as isize,
                    UTF8,
                ) != 0;
            CFRelease(source);
            found.then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
        }
    }
}
