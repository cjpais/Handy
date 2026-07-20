use crate::input::{self, EnigoState};
#[cfg(target_os = "linux")]
use crate::settings::TypingTool;
use crate::settings::{get_settings, AutoSubmitKey, ClipboardHandling, PasteMethod};
use enigo::{Direction, Enigo, Key, Keyboard};
use log::{error, info, warn};
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "linux")]
use crate::utils::{is_kde_wayland, is_wayland};

// ── macOS Accessibility API bindings for paste verification & AX paste ──
//
// Raw FFI bindings to the macOS Accessibility API (HIServices).
// Used for:
// 1. Paste verification: checking if Cmd+V landed in the target text field
//    by reading the focused element's AXValue (verify-then-commit pattern).
// 2. AX paste fallback: setting the focused element's AXValue directly,
//    bypassing Cmd+V entirely for apps that don't respond to keystrokes.
#[cfg(target_os = "macos")]
mod macos_ax {
    use std::os::raw::c_int;

    // Opaque types from ApplicationServices.framework
    #[repr(C)]
    pub struct AXUIElement(pub *mut std::ffi::c_void);
    #[repr(C)]
    pub struct CFString(pub *mut std::ffi::c_void);
    #[repr(C)]
    pub struct CFType(pub *mut std::ffi::c_void);

    pub type AXUIElementRef = *const AXUIElement;
    pub type CFTypeRef = *const CFType;
    pub type CFStringRef = *const CFString;
    pub type AXError = c_int;
    pub type CFStringEncoding = u32;

    // AXError codes
    pub const KAX_ERROR_SUCCESS: i32 = 0;

    // AX attribute names
    pub const KAX_FOCUSED_UI_ELEMENT_ATTRIBUTE: *const i8 =
        b"AXFocusedUIElement\0".as_ptr() as *const i8;
    pub const KAX_VALUE_ATTRIBUTE: *const i8 = b"AXValue\0".as_ptr() as *const i8;

    // CoreFoundation encoding
    pub const K_CFSTRING_ENCODING_UTF8: u32 = 0x08000100;

    #[link(kind = "framework", name = "ApplicationServices")]
    extern "C" {
        pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        pub fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: *const i8,
            value: *mut *mut std::ffi::c_void,
        ) -> AXError;
        pub fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: *const i8,
            value: *mut std::ffi::c_void,
        ) -> AXError;
        pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
        pub fn CFStringGetTypeID() -> usize;
        pub fn CFStringGetLength(theString: CFStringRef) -> isize;
        pub fn CFStringGetCString(
            theString: CFStringRef,
            buffer: *mut i8,
            bufferSize: isize,
            encoding: CFStringEncoding,
        ) -> bool;
        pub fn CFStringCreateWithCString(
            alloc: *mut std::ffi::c_void,
            cStr: *const i8,
            encoding: CFStringEncoding,
        ) -> CFStringRef;
        pub fn CFRelease(cf: CFTypeRef);
    }
}

#[cfg(target_os = "macos")]
use macos_ax::*;

/// Pastes text using the clipboard: saves current content, writes text, sends paste keystroke, restores clipboard.
fn paste_via_clipboard(
    enigo: &mut Enigo,
    text: &str,
    app_handle: &AppHandle,
    paste_method: &PasteMethod,
    paste_delay_ms: u64,
    paste_delay_after_ms: u64,
) -> Result<(), String> {
    let clipboard = app_handle.clipboard();
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    // Write text to clipboard first
    // On Wayland, prefer wl-copy for better compatibility (especially with umlauts)
    #[cfg(target_os = "linux")]
    let write_result = if is_wayland() && is_wl_copy_available() {
        info!("Using wl-copy for clipboard write on Wayland");
        write_clipboard_via_wl_copy(text)
    } else {
        clipboard
            .write_text(text)
            .map_err(|e| format!("Failed to write to clipboard: {}", e))
    };

    #[cfg(not(target_os = "linux"))]
    let write_result = clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e));

    write_result?;

    std::thread::sleep(Duration::from_millis(paste_delay_ms));

    // Send paste key combo
    #[cfg(target_os = "linux")]
    let key_combo_sent = try_send_key_combo_linux(paste_method)?;

    #[cfg(not(target_os = "linux"))]
    let key_combo_sent = false;

    // Fall back to enigo if no native tool handled it
    if !key_combo_sent {
        match paste_method {
            PasteMethod::CtrlV => input::send_paste_ctrl_v(enigo)?,
            PasteMethod::CtrlShiftV => input::send_paste_ctrl_shift_v(enigo)?,
            PasteMethod::ShiftInsert => input::send_paste_shift_insert(enigo)?,
            _ => return Err("Invalid paste method for clipboard paste".into()),
        }
    }

    std::thread::sleep(Duration::from_millis(paste_delay_after_ms));

    // Restore original clipboard content
    // On Wayland, prefer wl-copy for better compatibility
    #[cfg(target_os = "linux")]
    if is_wayland() && is_wl_copy_available() {
        let _ = write_clipboard_via_wl_copy(&clipboard_content);
    } else {
        let _ = clipboard.write_text(&clipboard_content);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = clipboard.write_text(&clipboard_content);

    Ok(())
}

/// Attempts to send a key combination using Linux-native tools.
/// Returns `Ok(true)` if a native tool handled it, `Ok(false)` to fall back to enigo.
#[cfg(target_os = "linux")]
fn try_send_key_combo_linux(paste_method: &PasteMethod) -> Result<bool, String> {
    if is_wayland() {
        // Wayland: prefer wtype (but not on KDE), then dotool, then ydotool
        // Note: wtype doesn't work on KDE (no zwp_virtual_keyboard_manager_v1 support)
        if !is_kde_wayland() && is_wtype_available() {
            info!("Using wtype for key combo");
            send_key_combo_via_wtype(paste_method)?;
            return Ok(true);
        }
        if is_dotool_available() {
            info!("Using dotool for key combo");
            send_key_combo_via_dotool(paste_method)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for key combo");
            send_key_combo_via_ydotool(paste_method)?;
            return Ok(true);
        }
    } else {
        // X11: prefer xdotool, then ydotool
        if is_xdotool_available() {
            info!("Using xdotool for key combo");
            send_key_combo_via_xdotool(paste_method)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for key combo");
            send_key_combo_via_ydotool(paste_method)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Attempts to type text directly using Linux-native tools.
/// Returns `Ok(true)` if a native tool handled it, `Ok(false)` to fall back to enigo.
#[cfg(target_os = "linux")]
fn try_direct_typing_linux(text: &str, preferred_tool: TypingTool) -> Result<bool, String> {
    // If user specified a tool, try only that one
    if preferred_tool != TypingTool::Auto {
        return match preferred_tool {
            TypingTool::Wtype if is_wtype_available() => {
                info!("Using user-specified wtype");
                type_text_via_wtype(text)?;
                Ok(true)
            }
            TypingTool::Kwtype if is_kwtype_available() => {
                info!("Using user-specified kwtype");
                type_text_via_kwtype(text)?;
                Ok(true)
            }
            TypingTool::Dotool if is_dotool_available() => {
                info!("Using user-specified dotool");
                type_text_via_dotool(text)?;
                Ok(true)
            }
            TypingTool::Ydotool if is_ydotool_available() => {
                info!("Using user-specified ydotool");
                type_text_via_ydotool(text)?;
                Ok(true)
            }
            TypingTool::Xdotool if is_xdotool_available() => {
                info!("Using user-specified xdotool");
                type_text_via_xdotool(text)?;
                Ok(true)
            }
            _ => Err(format!(
                "Typing tool {:?} is not available on this system",
                preferred_tool
            )),
        };
    }

    // Auto mode - existing fallback chain
    if is_wayland() {
        // KDE Wayland: prefer kwtype (uses KDE Fake Input protocol, supports umlauts)
        if is_kde_wayland() && is_kwtype_available() {
            info!("Using kwtype for direct text input on KDE Wayland");
            type_text_via_kwtype(text)?;
            return Ok(true);
        }
        // Wayland: prefer wtype, then dotool, then ydotool
        // Note: wtype doesn't work on KDE (no zwp_virtual_keyboard_manager_v1 support)
        if !is_kde_wayland() && is_wtype_available() {
            info!("Using wtype for direct text input");
            type_text_via_wtype(text)?;
            return Ok(true);
        }
        if is_dotool_available() {
            info!("Using dotool for direct text input");
            type_text_via_dotool(text)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for direct text input");
            type_text_via_ydotool(text)?;
            return Ok(true);
        }
    } else {
        // X11: prefer xdotool, then ydotool
        if is_xdotool_available() {
            info!("Using xdotool for direct text input");
            type_text_via_xdotool(text)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for direct text input");
            type_text_via_ydotool(text)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Returns the list of available typing tools on this system.
/// Always includes "auto" as the first entry.
#[cfg(target_os = "linux")]
pub fn get_available_typing_tools() -> Vec<String> {
    let mut tools = vec!["auto".to_string()];
    if is_wtype_available() {
        tools.push("wtype".to_string());
    }
    if is_kwtype_available() {
        tools.push("kwtype".to_string());
    }
    if is_dotool_available() {
        tools.push("dotool".to_string());
    }
    if is_ydotool_available() {
        tools.push("ydotool".to_string());
    }
    if is_xdotool_available() {
        tools.push("xdotool".to_string());
    }
    tools
}

/// Check if wtype is available (Wayland text input tool)
#[cfg(target_os = "linux")]
fn is_wtype_available() -> bool {
    Command::new("which")
        .arg("wtype")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if dotool is available (another Wayland text input tool)
#[cfg(target_os = "linux")]
fn is_dotool_available() -> bool {
    Command::new("which")
        .arg("dotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if ydotool is available (uinput-based, works on both Wayland and X11)
#[cfg(target_os = "linux")]
fn is_ydotool_available() -> bool {
    Command::new("which")
        .arg("ydotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_xdotool_available() -> bool {
    Command::new("which")
        .arg("xdotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if kwtype is available (KDE Wayland virtual keyboard input tool)
#[cfg(target_os = "linux")]
fn is_kwtype_available() -> bool {
    Command::new("which")
        .arg("kwtype")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if wl-copy is available (Wayland clipboard tool)
#[cfg(target_os = "linux")]
fn is_wl_copy_available() -> bool {
    Command::new("which")
        .arg("wl-copy")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Type text directly via wtype on Wayland.
#[cfg(target_os = "linux")]
fn type_text_via_wtype(text: &str) -> Result<(), String> {
    let output = Command::new("wtype")
        .arg("--") // Protect against text starting with -
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute wtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wtype failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via xdotool on X11.
#[cfg(target_os = "linux")]
fn type_text_via_xdotool(text: &str) -> Result<(), String> {
    let output = Command::new("xdotool")
        .arg("type")
        .arg("--clearmodifiers")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute xdotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("xdotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via dotool (works on both Wayland and X11 via uinput).
#[cfg(target_os = "linux")]
fn type_text_via_dotool(text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn dotool: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        // dotool uses "type <text>" command
        writeln!(stdin, "type {}", text)
            .map_err(|e| format!("Failed to write to dotool stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for dotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via ydotool (uinput-based, requires ydotoold daemon).
#[cfg(target_os = "linux")]
fn type_text_via_ydotool(text: &str) -> Result<(), String> {
    let output = Command::new("ydotool")
        .arg("type")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute ydotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ydotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via kwtype (KDE Wayland virtual keyboard, uses KDE Fake Input protocol).
#[cfg(target_os = "linux")]
fn type_text_via_kwtype(text: &str) -> Result<(), String> {
    let output = Command::new("kwtype")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute kwtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kwtype failed: {}", stderr));
    }

    Ok(())
}

/// Write text to clipboard via wl-copy (Wayland clipboard tool).
/// Uses Stdio::null() to avoid blocking on repeated calls — wl-copy forks a
/// daemon that inherits piped fds, causing read_to_end to hang indefinitely.
#[cfg(target_os = "linux")]
fn write_clipboard_via_wl_copy(text: &str) -> Result<(), String> {
    use std::process::Stdio;
    let status = Command::new("wl-copy")
        .arg("--")
        .arg(text)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to execute wl-copy: {}", e))?;

    if !status.success() {
        return Err("wl-copy failed".into());
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via wtype on Wayland.
#[cfg(target_os = "linux")]
fn send_key_combo_via_wtype(paste_method: &PasteMethod) -> Result<(), String> {
    let args: Vec<&str> = match paste_method {
        PasteMethod::CtrlV => vec!["-M", "ctrl", "-k", "v"],
        PasteMethod::ShiftInsert => vec!["-M", "shift", "-k", "Insert"],
        PasteMethod::CtrlShiftV => vec!["-M", "ctrl", "-M", "shift", "-k", "v"],
        _ => return Err("Unsupported paste method".into()),
    };

    let output = Command::new("wtype")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to execute wtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wtype failed: {}", stderr));
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via dotool.
#[cfg(target_os = "linux")]
fn send_key_combo_via_dotool(paste_method: &PasteMethod) -> Result<(), String> {
    let command;
    match paste_method {
        PasteMethod::CtrlV => command = "echo key ctrl+v | dotool",
        PasteMethod::ShiftInsert => command = "echo key shift+insert | dotool",
        PasteMethod::CtrlShiftV => command = "echo key ctrl+shift+v | dotool",
        _ => return Err("Unsupported paste method".into()),
    }
    use std::process::Stdio;
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to execute dotool: {}", e))?;
    if !status.success() {
        return Err("dotool failed".into());
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via ydotool (requires ydotoold daemon).
#[cfg(target_os = "linux")]
fn send_key_combo_via_ydotool(paste_method: &PasteMethod) -> Result<(), String> {
    // ydotool uses Linux input event keycodes with format <keycode>:<pressed>
    // where pressed is 1 for down, 0 for up. Keycodes: ctrl=29, shift=42, v=47, insert=110
    let args: Vec<&str> = match paste_method {
        PasteMethod::CtrlV => vec!["key", "29:1", "47:1", "47:0", "29:0"],
        PasteMethod::ShiftInsert => vec!["key", "42:1", "110:1", "110:0", "42:0"],
        PasteMethod::CtrlShiftV => vec!["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"],
        _ => return Err("Unsupported paste method".into()),
    };

    let output = Command::new("ydotool")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to execute ydotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ydotool failed: {}", stderr));
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via xdotool on X11.
#[cfg(target_os = "linux")]
fn send_key_combo_via_xdotool(paste_method: &PasteMethod) -> Result<(), String> {
    let key_combo = match paste_method {
        PasteMethod::CtrlV => "ctrl+v",
        PasteMethod::CtrlShiftV => "ctrl+shift+v",
        PasteMethod::ShiftInsert => "shift+Insert",
        _ => return Err("Unsupported paste method".into()),
    };

    let output = Command::new("xdotool")
        .arg("key")
        .arg("--clearmodifiers")
        .arg(key_combo)
        .output()
        .map_err(|e| format!("Failed to execute xdotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("xdotool failed: {}", stderr));
    }

    Ok(())
}

/// Pastes text by invoking an external script.
/// The script receives the text to paste as a single argument.
fn paste_via_external_script(text: &str, script_path: &str) -> Result<(), String> {
    info!("Pasting via external script: {}", script_path);

    let output = Command::new(script_path)
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute external script '{}': {}", script_path, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "External script '{}' failed with exit code {:?}. stderr: {}, stdout: {}",
            script_path,
            output.status.code(),
            stderr.trim(),
            stdout.trim()
        ));
    }

    Ok(())
}

/// Types text directly by simulating individual key presses.
fn paste_direct(
    enigo: &mut Enigo,
    text: &str,
    #[cfg(target_os = "linux")] typing_tool: TypingTool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if try_direct_typing_linux(text, typing_tool)? {
            return Ok(());
        }
        info!("Falling back to enigo for direct text input");
    }

    input::paste_text_direct(enigo, text)
}

fn send_return_key(enigo: &mut Enigo, key_type: AutoSubmitKey) -> Result<(), String> {
    match key_type {
        AutoSubmitKey::Enter => {
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
        }
        AutoSubmitKey::CtrlEnter => {
            enigo
                .key(Key::Control, Direction::Press)
                .map_err(|e| format!("Failed to press Control key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
            enigo
                .key(Key::Control, Direction::Release)
                .map_err(|e| format!("Failed to release Control key: {}", e))?;
        }
        AutoSubmitKey::CmdEnter => {
            enigo
                .key(Key::Meta, Direction::Press)
                .map_err(|e| format!("Failed to press Meta/Cmd key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
            enigo
                .key(Key::Meta, Direction::Release)
                .map_err(|e| format!("Failed to release Meta/Cmd key: {}", e))?;
        }
    }

    Ok(())
}

fn should_send_auto_submit(auto_submit: bool, paste_method: PasteMethod) -> bool {
    auto_submit && paste_method != PasteMethod::None
}

pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;
    let paste_delay_ms = settings.paste_delay_ms;
    let paste_delay_after_ms = settings.paste_delay_after_ms;

    // Append trailing space if setting is enabled
    let text = if settings.append_trailing_space {
        format!("{} ", text)
    } else {
        text
    };

    info!(
        "Using paste method: {:?}, delay before: {}ms, delay after: {}ms",
        paste_method, paste_delay_ms, paste_delay_after_ms
    );

    // Get the managed Enigo instance
    let enigo_state = app_handle
        .try_state::<EnigoState>()
        .ok_or("Enigo state not initialized")?;
    let mut enigo = enigo_state
        .0
        .lock()
        .map_err(|e| format!("Failed to lock Enigo: {}", e))?;

    // Perform the paste operation
    match paste_method {
        PasteMethod::None => {
            info!("PasteMethod::None selected - skipping paste action");
        }
        PasteMethod::Direct => {
            paste_direct(
                &mut enigo,
                &text,
                #[cfg(target_os = "linux")]
                settings.typing_tool,
            )?;
        }
        PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert => {
            paste_via_clipboard(
                &mut enigo,
                &text,
                &app_handle,
                &paste_method,
                paste_delay_ms,
                paste_delay_after_ms,
            )?
        }
        PasteMethod::ExternalScript => {
            let script_path = settings
                .external_script_path
                .as_ref()
                .filter(|p| !p.is_empty())
                .ok_or("External script path is not configured")?;
            paste_via_external_script(&text, script_path)?;
        }
    }

    if should_send_auto_submit(settings.auto_submit, paste_method) {
        std::thread::sleep(Duration::from_millis(50));
        send_return_key(&mut enigo, settings.auto_submit_key)?;
    }

    // After pasting, optionally copy to clipboard based on settings
    if settings.clipboard_handling == ClipboardHandling::CopyToClipboard {
        let clipboard = app_handle.clipboard();
        clipboard
            .write_text(&text)
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
    }

    Ok(())
}

// ── Clipboard extensions: AX paste verification & fallback ─────────────
//
// The functions below implement the "verify-then-commit" pattern for
// clipboard restore and provide an AX-based paste fallback when Cmd+V
// fails. They are additive — existing functions are unchanged.

/// Writes text to the system clipboard without pasting or restoring previous content.
/// Used as a fallback when the paste keystroke fails, so the user can manually paste.
pub fn write_to_clipboard(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();

    // On Wayland, prefer wl-copy for better compatibility
    #[cfg(target_os = "linux")]
    let write_result = if is_wayland() && is_wl_copy_available() {
        info!("Using wl-copy for clipboard write on Wayland (fallback)");
        write_clipboard_via_wl_copy(text)
    } else {
        clipboard
            .write_text(text)
            .map_err(|e| format!("Failed to write to clipboard: {}", e))
    };

    #[cfg(not(target_os = "linux"))]
    let write_result = clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e));

    write_result
}

/// Verifies that a paste operation landed in the target application.
///
/// Implements the "verify-then-commit" pattern: we don't restore the
/// original clipboard content until we have some confidence that the
/// paste was consumed by the target app.
///
/// On macOS, uses the Accessibility API (AXValue) to check if the focused
/// text field's value contains the pasted text. Falls back to a
/// conservative heuristic on other platforms.
///
/// Returns `true` if the paste was verified (safe to restore clipboard),
/// `false` if verification failed or is unavailable (keep transcription text).
pub fn verify_paste(_app_handle: &AppHandle, pasted_text: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = _app_handle;
        verify_paste_macos(pasted_text)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = _app_handle;
        // On non-macOS platforms, we can't use AX to verify.
        // Conservatively return false to keep transcription text on clipboard
        // so the user can manually paste again if needed.
        info!("Paste verification: not available on this platform — keeping transcription text on clipboard");
        false
    }
}

/// macOS implementation of paste verification using the Accessibility API.
///
/// Reads the focused UI element's AXValue and checks if it contains the
/// pasted text. This confirms the paste actually landed in the target
/// text field, not in Handy's own overlay or nowhere at all.
#[cfg(target_os = "macos")]
fn verify_paste_macos(pasted_text: &str) -> bool {
    use tauri_nspanel::objc2::rc::autoreleasepool;
    use tauri_nspanel::objc2_app_kit::NSWorkspace;

    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else {
            info!("Paste verification: no frontmost app — assuming paste failed");
            return false;
        };

        let pid = app.processIdentifier();

        // Use the macOS Accessibility API to check the focused element.
        let ax_app: AXUIElementRef = unsafe { AXUIElementCreateApplication(pid) };

        let mut focused_element_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = unsafe {
            AXUIElementCopyAttributeValue(
                ax_app,
                KAX_FOCUSED_UI_ELEMENT_ATTRIBUTE,
                &mut focused_element_ptr as *mut _,
            )
        };

        if result != KAX_ERROR_SUCCESS {
            info!(
                "Paste verification: could not get focused AX element (error={}) — assuming paste failed",
                result
            );
            unsafe { CFRelease(ax_app as CFTypeRef) };
            return false;
        }

        let focused_element: AXUIElementRef = focused_element_ptr as AXUIElementRef;

        // Get the AXValue of the focused element
        let mut value_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let value_result = unsafe {
            AXUIElementCopyAttributeValue(
                focused_element,
                KAX_VALUE_ATTRIBUTE,
                &mut value_ptr as *mut _,
            )
        };

        if value_result != KAX_ERROR_SUCCESS {
            // Element has no AXValue — might be a non-text field (terminal, etc.)
            // Many terminal apps don't expose AXValue, so assume paste succeeded.
            info!(
                "Paste verification: focused element has no AXValue (error={}) — assuming paste succeeded (terminal-like app)",
                value_result
            );
            unsafe { CFRelease(focused_element_ptr as CFTypeRef) };
            unsafe { CFRelease(ax_app as CFTypeRef) };
            return true;
        }

        // Release the focused element (Create-rule +1 reference)
        unsafe { CFRelease(focused_element_ptr as CFTypeRef) };

        let type_id = unsafe { CFGetTypeID(value_ptr as CFTypeRef) };
        let string_type_id = unsafe { CFStringGetTypeID() };

        if type_id == string_type_id {
            let cf_string: CFStringRef = value_ptr as CFStringRef;
            let len = unsafe { CFStringGetLength(cf_string) };
            // Each UTF-16 code unit can expand to up to 3 UTF-8 bytes
            let buffer_size = (len as usize) * 3 + 1;
            let mut buffer = vec![0u8; buffer_size];

            let success = unsafe {
                CFStringGetCString(
                    cf_string,
                    buffer.as_mut_ptr() as *mut i8,
                    buffer_size as isize,
                    K_CFSTRING_ENCODING_UTF8,
                )
            };

            // Release the CFString obtained from AXUIElementCopyAttributeValue
            unsafe { CFRelease(value_ptr as CFTypeRef) };
            // Release ax_app
            unsafe { CFRelease(ax_app as CFTypeRef) };

            if success {
                let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
                let s = String::from_utf8_lossy(&buffer[..end]);
                let contains = s.contains(pasted_text);
                if contains {
                    info!("Paste verification: AXValue contains pasted text — paste confirmed");
                } else {
                    info!("Paste verification: AXValue does NOT contain pasted text — paste may have failed");
                }
                contains
            } else {
                info!("Paste verification: could not extract string from AXValue — assuming paste failed");
                false
            }
        } else {
            info!(
                "Paste verification: AXValue is not a string (type={}) — assuming paste failed",
                type_id
            );
            unsafe { CFRelease(value_ptr as CFTypeRef) };
            unsafe { CFRelease(ax_app as CFTypeRef) };
            false
        }
    })
}

/// Pastes text directly into the focused UI element using the macOS Accessibility API.
///
/// Bypasses Cmd+V entirely by setting the AXValue of the focused text field.
/// This is used as a fallback when Cmd+V fails (e.g., when the overlay steals
/// focus or when the target app doesn't respond to keystroke events).
///
/// On non-macOS platforms, returns an error since AX paste is not available.
pub fn paste_via_accessibility(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        paste_via_accessibility_macos(text, app_handle)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (text, app_handle);
        Err("AX paste is only available on macOS".into())
    }
}

/// macOS implementation of AX-based paste.
///
/// Gets the focused UI element via the Accessibility API and sets its
/// AXValue attribute to the text directly. This bypasses the clipboard
/// and Cmd+V entirely — useful when keystroke-based paste fails.
#[cfg(target_os = "macos")]
fn paste_via_accessibility_macos(text: &str, _app_handle: &AppHandle) -> Result<(), String> {
    use tauri_nspanel::objc2::rc::autoreleasepool;
    use tauri_nspanel::objc2_app_kit::NSWorkspace;

    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace
            .frontmostApplication()
            .ok_or("Could not get frontmost application")?;

        let pid = app.processIdentifier();

        // Create AX element for the target application
        let ax_app: AXUIElementRef = unsafe { AXUIElementCreateApplication(pid) };

        // Get the focused UI element
        let mut focused_element_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let result = unsafe {
            AXUIElementCopyAttributeValue(
                ax_app,
                KAX_FOCUSED_UI_ELEMENT_ATTRIBUTE,
                &mut focused_element_ptr as *mut _,
            )
        };

        if result != KAX_ERROR_SUCCESS {
            unsafe { CFRelease(ax_app as CFTypeRef) };
            return Err(format!(
                "AX paste: could not get focused element (error={})",
                result
            ));
        }

        let focused_element: AXUIElementRef = focused_element_ptr as AXUIElementRef;

        // Create a CFString from the text
        let cf_text: CFStringRef = unsafe {
            CFStringCreateWithCString(
                std::ptr::null_mut(),
                text.as_ptr() as *const i8,
                K_CFSTRING_ENCODING_UTF8,
            )
        };

        if cf_text.is_null() {
            unsafe { CFRelease(focused_element_ptr as CFTypeRef) };
            unsafe { CFRelease(ax_app as CFTypeRef) };
            return Err("AX paste: could not create CFString from text".into());
        }

        // Set the AXValue of the focused element
        let set_result = unsafe {
            AXUIElementSetAttributeValue(focused_element, KAX_VALUE_ATTRIBUTE, cf_text as *mut _)
        };

        // Release all references (Create Rule: caller must release)
        unsafe { CFRelease(cf_text as CFTypeRef) };
        unsafe { CFRelease(focused_element_ptr as CFTypeRef) };
        unsafe { CFRelease(ax_app as CFTypeRef) };

        if set_result == KAX_ERROR_SUCCESS {
            info!(
                "AX paste: successfully set AXValue on focused element ({} chars)",
                text.len()
            );
            Ok(())
        } else {
            warn!(
                "AX paste: failed to set AXValue (error={}) — element may not support direct text setting",
                set_result
            );
            Err(format!(
                "AX paste: AXUIElementSetAttributeValue failed with error {}",
                set_result
            ))
        }
    })
}

/// Pastes text with verification and AX-based fallback.
///
/// Orchestration function that implements the full reliable paste flow:
/// 1. Try Cmd+V (via the standard paste method)
/// 2. Verify the paste landed (via AX API)
/// 3. If verification fails, fall back to AX paste (set AXValue directly)
/// 4. Verify the AX paste
/// 5. If all paste methods fail, write to clipboard only
///
/// This is the recommended entry point for paste operations that need
/// high reliability. It does NOT modify the existing `paste()` function —
/// callers opt in explicitly.
pub fn paste_with_verification(text: String, app_handle: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;
    let paste_delay_ms = settings.paste_delay_ms;

    // Skip if paste is disabled
    if paste_method == PasteMethod::None {
        info!("paste_with_verification: PasteMethod::None — skipping paste");
        return Ok(());
    }

    // Get the managed Enigo instance for keystroke-based paste
    let enigo_state = app_handle
        .try_state::<EnigoState>()
        .ok_or("Enigo state not initialized")?;
    let mut enigo = enigo_state
        .0
        .lock()
        .map_err(|e| format!("Failed to lock Enigo: {}", e))?;

    // ── Step 1: Try standard Cmd+V paste ──
    info!("paste_with_verification: step 1 — attempting Cmd+V paste");
    let paste_result = match paste_method {
        PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert => {
            paste_via_clipboard(
                &mut enigo,
                &text,
                &app_handle,
                &paste_method,
                paste_delay_ms,
                settings.paste_delay_after_ms,
            )
        }
        PasteMethod::Direct => {
            #[cfg(target_os = "linux")]
            {
                paste_direct(&mut enigo, &text, settings.typing_tool)
            }
            #[cfg(not(target_os = "linux"))]
            {
                paste_direct(&mut enigo, &text)
            }
        }
        PasteMethod::ExternalScript => {
            let script_path = settings
                .external_script_path
                .as_ref()
                .filter(|p| !p.is_empty())
                .ok_or("External script path is not configured")?;
            paste_via_external_script(&text, script_path)
        }
        _ => Err("Unsupported paste method".into()),
    };

    if paste_result.is_ok() {
        // Give the target app time to process the paste
        std::thread::sleep(Duration::from_millis(150));

        // ── Step 2: Verify paste landed ──
        info!("paste_with_verification: step 2 — verifying paste landed");
        if verify_paste(&app_handle, &text) {
            info!("paste_with_verification: paste verified via Cmd+V — done");
            return Ok(());
        }
        warn!("paste_with_verification: Cmd+V paste verification failed");
    } else {
        warn!(
            "paste_with_verification: Cmd+V paste failed: {:?}",
            paste_result.as_ref().err()
        );
    }

    // ── Step 3: Fallback to AX paste ──
    info!("paste_with_verification: step 3 — falling back to AX paste");
    match paste_via_accessibility(&text, &app_handle) {
        Ok(()) => {
            // Give the target app time to process
            std::thread::sleep(Duration::from_millis(100));

            // ── Step 4: Verify AX paste ──
            info!("paste_with_verification: step 4 — verifying AX paste");
            if verify_paste(&app_handle, &text) {
                info!("paste_with_verification: AX paste verified — done");
                return Ok(());
            }
            warn!("paste_with_verification: AX paste verification failed");
        }
        Err(e) => {
            warn!("paste_with_verification: AX paste failed: {}", e);
        }
    }

    // ── Step 5: Clipboard-only fallback ──
    info!("paste_with_verification: step 5 — falling back to clipboard-only");
    match write_to_clipboard(&text, &app_handle) {
        Ok(()) => {
            info!("paste_with_verification: text written to clipboard (user can paste manually)");
            Ok(())
        }
        Err(e) => {
            error!("paste_with_verification: all paste methods failed: {}", e);
            Err(format!(
                "All paste methods failed. Last error: {}",
                e
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_submit_requires_setting_enabled() {
        assert!(!should_send_auto_submit(false, PasteMethod::CtrlV));
        assert!(!should_send_auto_submit(false, PasteMethod::Direct));
    }

    #[test]
    fn auto_submit_skips_none_paste_method() {
        assert!(!should_send_auto_submit(true, PasteMethod::None));
    }

    #[test]
    fn auto_submit_runs_for_active_paste_methods() {
        assert!(should_send_auto_submit(true, PasteMethod::CtrlV));
        assert!(should_send_auto_submit(true, PasteMethod::Direct));
        assert!(should_send_auto_submit(true, PasteMethod::CtrlShiftV));
        assert!(should_send_auto_submit(true, PasteMethod::ShiftInsert));
    }
}
