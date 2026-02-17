//! Module for getting the frontmost/active application name.
//! This is platform-specific and returns the name of the application
//! that has keyboard focus when the user starts transcribing.

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
pub fn get_frontmost_app_name() -> Option<String> {
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CStr;

    unsafe {
        // Get NSWorkspace shared instance
        let workspace: *mut objc::runtime::Object =
            msg_send![objc::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }

        // Get frontmost application (NSRunningApplication)
        let frontmost_app: *mut objc::runtime::Object = msg_send![workspace, frontmostApplication];
        if frontmost_app.is_null() {
            return None;
        }

        // Get localized name of the application
        let name: *mut objc::runtime::Object = msg_send![frontmost_app, localizedName];
        if name.is_null() {
            return None;
        }

        // Convert NSString to Rust String
        let utf8_ptr: *const i8 = msg_send![name, UTF8String];
        if utf8_ptr.is_null() {
            return None;
        }

        let c_str = CStr::from_ptr(utf8_ptr);
        match c_str.to_str() {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }
}

#[cfg(target_os = "windows")]
pub fn get_frontmost_app_name() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let length = GetWindowTextLengthW(hwnd);
        if length == 0 {
            return None;
        }

        let mut buffer: Vec<u16> = vec![0; (length + 1) as usize];
        let chars_copied = GetWindowTextW(hwnd, &mut buffer);

        if chars_copied > 0 {
            buffer.truncate(chars_copied as usize);
            let title = OsString::from_wide(&buffer).to_string_lossy().into_owned();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
pub fn get_frontmost_app_name() -> Option<String> {
    use std::process::Command;

    // Try xdotool first (X11)
    if let Ok(output) = Command::new("xdotool")
        .args(["getactivewindow", "getwindowname"])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    // Fallback for Wayland - try to get from environment or use a generic name
    // Most Wayland compositors don't expose window info to external tools
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        // On Wayland, we can't easily get the active window name
        // Return None and let the caller handle it
        return None;
    }

    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn get_frontmost_app_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_frontmost_app_returns_something_or_none() {
        // This test just ensures the function doesn't panic
        let _result = get_frontmost_app_name();
    }
}
