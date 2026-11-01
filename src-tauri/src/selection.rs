//! Read the text selected in the focused control for context scribe.
//!
//! Windows tries UI Automation first, then a clipboard copy. macOS tries
//! Accessibility (`AXSelectedText`), then the same clipboard copy. Linux has
//! no capture: Wayland and X11 selections are intentionally unsupported.

use log::debug;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// Selected text, or an empty string when there is nothing to read.
/// Never fails the recording: a miss becomes an empty selection.
pub fn capture_focused_selection(app: &AppHandle) -> String {
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        debug!("Context scribe does not read the OS selection on this platform");
        return String::new();
    }

    #[cfg(target_os = "windows")]
    {
        match windows_uia::selected_text() {
            Ok(text) => {
                debug!(
                    "UI Automation selection is {} characters",
                    text.chars().count()
                );
                return text;
            }
            Err(error) => debug!("UI Automation selection unavailable: {error}"),
        }
    }

    #[cfg(target_os = "macos")]
    {
        match macos_ax::selected_text() {
            Ok(text) => {
                debug!(
                    "Accessibility selection is {} characters",
                    text.chars().count()
                );
                return text;
            }
            Err(error) => debug!("Accessibility selection unavailable: {error}"),
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        if !shortcut_modifiers_released(Duration::from_millis(200)) {
            debug!("Shortcut modifiers are still held; skipping synthetic copy");
            return String::new();
        }
        match crate::clipboard::copy_selection_from_focused_app(app) {
            Ok(text) => {
                debug!(
                    "Clipboard selection copy is {} characters",
                    text.chars().count()
                );
                text
            }
            Err(error) => {
                debug!("Clipboard selection copy failed: {error}");
                String::new()
            }
        }
    }
}

/// Wait until Ctrl/Shift/Alt/Win (and Command on macOS) are up, so a synthetic
/// Ctrl/Cmd+C is not combined with the hotkey chord.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn shortcut_modifiers_released(budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if !shortcut_modifiers_held() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "windows")]
fn shortcut_modifiers_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    // VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN.
    const MODIFIERS: [i32; 5] = [0x10, 0x11, 0x12, 0x5B, 0x5C];
    MODIFIERS
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(*vk) } < 0)
}

#[cfg(target_os = "macos")]
fn shortcut_modifiers_held() -> bool {
    // kCGEventSourceStateHIDSystemState
    const HID_SYSTEM_STATE: i32 = 1;
    const SHIFT: u64 = 1 << 17;
    const CONTROL: u64 = 1 << 18;
    const OPTION: u64 = 1 << 19;
    const COMMAND: u64 = 1 << 20;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    flags & (SHIFT | CONTROL | OPTION | COMMAND) != 0
}

#[cfg(target_os = "windows")]
mod windows_uia {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
    };

    pub fn selected_text() -> Result<String, String> {
        ensure_com()?;
        let automation: IUIAutomation = unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("UI Automation is unavailable: {e}"))?
        };
        let focused = unsafe {
            automation
                .GetFocusedElement()
                .map_err(|e| format!("no focused UI Automation element: {e}"))?
        };
        let pattern: IUIAutomationTextPattern = unsafe {
            focused
                .GetCurrentPatternAs(UIA_TextPatternId)
                .map_err(|e| format!("focused control has no text pattern: {e}"))?
        };
        let ranges = unsafe {
            pattern
                .GetSelection()
                .map_err(|e| format!("text pattern has no selection: {e}"))?
        };
        let count = unsafe {
            ranges
                .Length()
                .map_err(|e| format!("selection length: {e}"))?
        };
        if count <= 0 {
            return Ok(String::new());
        }

        let mut parts = Vec::with_capacity(count as usize);
        for index in 0..count {
            let range = unsafe {
                ranges
                    .GetElement(index)
                    .map_err(|e| format!("selection range {index}: {e}"))?
            };
            // -1 asks for the whole range.
            let text = unsafe {
                range
                    .GetText(-1)
                    .map_err(|e| format!("selection text: {e}"))?
            };
            parts.push(text.to_string());
        }
        Ok(parts.join("\n"))
    }

    fn ensure_com() -> Result<(), String> {
        // The coordinator thread is long-lived. Initializing COM once and
        // leaving it is enough; S_FALSE (already initialized) is success.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            return Ok(());
        }
        // RPC_E_CHANGED_MODE: this thread is already in another apartment.
        // UI Automation can still be used.
        const RPC_E_CHANGED_MODE: i32 = 0x80010106_u32 as i32;
        if hr.0 == RPC_E_CHANGED_MODE {
            return Ok(());
        }
        Err(format!("COM initialization failed: {hr}"))
    }
}

#[cfg(target_os = "macos")]
mod macos_ax {
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use std::ffi::c_void;

    // kAXErrorNoValue — the attribute exists and there is nothing selected.
    const AX_ERROR_NO_VALUE: i32 = -25212;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFRelease(cf: *const c_void);
    }

    pub fn selected_text() -> Result<String, String> {
        // SAFETY: AXUIElementCreateSystemWide returns a +1 CF reference, released
        // below. Attribute names are toll-free bridged NSString/CFString values
        // kept alive for the copy call. Copied values are released once read.
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return Err("AXUIElementCreateSystemWide failed".into());
            }

            let focused_name = NSString::from_str("AXFocusedUIElement");
            let mut focused: *const c_void = std::ptr::null();
            let focused_status = AXUIElementCopyAttributeValue(
                system,
                Retained::as_ptr(&focused_name).cast::<c_void>(),
                &mut focused,
            );
            CFRelease(system);
            if focused_status == AX_ERROR_NO_VALUE {
                if !focused.is_null() {
                    CFRelease(focused);
                }
                return Ok(String::new());
            }
            if focused_status != 0 || focused.is_null() {
                if !focused.is_null() {
                    CFRelease(focused);
                }
                return Err(format!(
                    "no focused accessibility element ({focused_status})"
                ));
            }

            let selected_name = NSString::from_str("AXSelectedText");
            let mut value: *const c_void = std::ptr::null();
            let selected_status = AXUIElementCopyAttributeValue(
                focused as *mut c_void,
                Retained::as_ptr(&selected_name).cast::<c_void>(),
                &mut value,
            );
            CFRelease(focused);
            if selected_status == AX_ERROR_NO_VALUE {
                if !value.is_null() {
                    CFRelease(value);
                }
                return Ok(String::new());
            }
            if selected_status != 0 || value.is_null() {
                if !value.is_null() {
                    CFRelease(value);
                }
                return Err(format!("selected text is unavailable ({selected_status})"));
            }
            if CFGetTypeID(value) != CFStringGetTypeID() {
                CFRelease(value);
                return Err("selected text is not a string".into());
            }

            // The copy gave us +1. from_raw takes that retain; dropping the
            // Retained releases it.
            let ns = Retained::from_raw(value as *mut NSString)
                .ok_or_else(|| "selected text pointer was null".to_string())?;
            Ok(ns.to_string())
        }
    }
}
