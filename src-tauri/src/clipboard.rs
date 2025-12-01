use crate::settings::{get_settings, ClipboardHandling, PasteMethod};
use enigo::Enigo;
use enigo::Key;
use enigo::Keyboard;
use enigo::Settings;
use log::info;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "linux")]
use crate::utils::is_wayland;
#[cfg(target_os = "linux")]
use std::process::Command;

/// Sends a Ctrl+V or Cmd+V paste command using platform-specific virtual key codes.
/// This ensures the paste works regardless of keyboard layout (e.g., Russian, AZERTY, DVORAK).
fn send_paste_ctrl_v() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if try_wayland_send_paste(&PasteMethod::CtrlV)? {
        return Ok(());
    }

    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9));
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    // Press modifier + V
    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    enigo
        .key(v_key_code, enigo::Direction::Click)
        .map_err(|e| format!("Failed to click V key: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e))?;

    Ok(())
}

/// Sends a Shift+Insert paste command (Windows and Linux only).
/// This is more universal for terminal applications and legacy software.
fn send_paste_shift_insert() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if try_wayland_send_paste(&PasteMethod::ShiftInsert)? {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    let insert_key_code = Key::Other(0x2D); // VK_INSERT
    #[cfg(not(target_os = "windows"))]
    let insert_key_code = Key::Other(0x76); // XK_Insert (keycode 118 / 0x76, also used as fallback)

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    // Press Shift + Insert
    enigo
        .key(Key::Shift, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press Shift key: {}", e))?;
    enigo
        .key(insert_key_code, enigo::Direction::Click)
        .map_err(|e| format!("Failed to click Insert key: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    enigo
        .key(Key::Shift, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release Shift key: {}", e))?;

    Ok(())
}

/// Pastes text directly using the enigo text method.
/// This tries to use system input methods if possible, otherwise simulates keystrokes one by one.
fn paste_via_direct_input(text: &str) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    enigo
        .text(text)
        .map_err(|e| format!("Failed to send text directly: {}", e))?;

    Ok(())
}

/// Pastes text using the clipboard method with Ctrl+V/Cmd+V.
/// Saves the current clipboard, writes the text, sends paste command, then restores the clipboard.
fn paste_via_clipboard_ctrl_v(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();

    // get the current clipboard content
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    // small delay to ensure the clipboard content has been written to
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_paste_ctrl_v()?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    // restore the clipboard
    clipboard
        .write_text(&clipboard_content)
        .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

    Ok(())
}

/// Pastes text using the clipboard method with Shift+Insert (Windows/Linux only).
/// Saves the current clipboard, writes the text, sends paste command, then restores the clipboard.
fn paste_via_clipboard_shift_insert(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();

    // get the current clipboard content
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    // small delay to ensure the clipboard content has been written to
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_paste_shift_insert()?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    // restore the clipboard
    clipboard
        .write_text(&clipboard_content)
        .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

    Ok(())
}

/// Attempts to paste using Wayland-specific tools (`wtype` or `dotool`).
/// Returns `Ok(true)` if a Wayland tool handled the paste, `Ok(false)` if not applicable,
/// or `Err` on failure from the underlying tool.
#[cfg(target_os = "linux")]
fn try_wayland_send_paste(paste_method: &PasteMethod) -> Result<bool, String> {
    if is_wayland() {
        if is_wtype_available() {
            send_paste_via_wtype(paste_method)?;
            return Ok(true);
        } else if is_dotool_available() {
            send_paste_via_dotool(paste_method)?;
            return Ok(true);
        }
    }

    Ok(false)
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

/// Paste using wtype and return a friendly error on failure.
#[cfg(target_os = "linux")]
fn send_paste_via_wtype(paste_method: &PasteMethod) -> Result<(), String> {
    let args: Vec<&str> = match paste_method {
        PasteMethod::CtrlV => vec!["-M", "ctrl", "-k", "v"],
        PasteMethod::ShiftInsert => vec!["-M", "shift", "-k", "Insert"],
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

/// Paste using dotool and return a friendly error on failure.
#[cfg(target_os = "linux")]
fn send_paste_via_dotool(paste_method: &PasteMethod) -> Result<(), String> {
    let command;
    match paste_method {
        PasteMethod::CtrlV => command = "echo key ctrl+v | dotool",
        PasteMethod::ShiftInsert => command = "echo key shift+insert | dotool",
        _ => return Err("Unsupported paste method".into()),
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|e| format!("Failed to execute dotool: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dotool failed: {}", stderr));
    }

    Ok(())
}

pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;

    info!("Using paste method: {:?}", paste_method);

    // Perform the paste operation
    match paste_method {
        PasteMethod::None => {
            // Intentionally do not perform any paste action; history/clipboard update
            info!("PasteMethod::None selected - skipping paste action");
        }
        PasteMethod::CtrlV => paste_via_clipboard_ctrl_v(&text, &app_handle)?,
        PasteMethod::Direct => paste_via_direct_input(&text)?,
        PasteMethod::ShiftInsert => paste_via_clipboard_shift_insert(&text, &app_handle)?,
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
