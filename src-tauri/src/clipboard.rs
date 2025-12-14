use crate::input::{self, EnigoState};
use crate::settings::{get_settings, ClipboardHandling, PasteMethod};
use enigo::Enigo;
use log::info;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "linux")]
use crate::utils::is_wayland;
#[cfg(target_os = "linux")]
use std::process::Command;

/// Pastes text using the clipboard: saves current content, writes text, sends paste keystroke, restores clipboard.
fn paste_via_clipboard(
    enigo: &mut Enigo,
    text: &str,
    app_handle: &AppHandle,
    paste_method: &PasteMethod,
) -> Result<(), String> {
    // Check for Wayland first
    #[cfg(target_os = "linux")]
    if try_wayland_send_paste(paste_method)? {
        return Ok(());
    }

    let clipboard = app_handle.clipboard();
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    match paste_method {
        PasteMethod::CtrlV => input::send_paste_ctrl_v(enigo)?,
        PasteMethod::CtrlShiftV => input::send_paste_ctrl_shift_v(enigo)?,
        PasteMethod::ShiftInsert => input::send_paste_shift_insert(enigo)?,
        _ => return Err("Invalid paste method for clipboard paste".into()),
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

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

/// Paste using dotool and return a friendly error on failure.
#[cfg(target_os = "linux")]
fn send_paste_via_dotool(paste_method: &PasteMethod) -> Result<(), String> {
    let command;
    match paste_method {
        PasteMethod::CtrlV => command = "echo key ctrl+v | dotool",
        PasteMethod::ShiftInsert => command = "echo key shift+insert | dotool",
        PasteMethod::CtrlShiftV => command = "echo key ctrl+shift+v | dotool",
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

    // Append trailing space if setting is enabled
    let text = if settings.append_trailing_space {
        format!("{} ", text)
    } else {
        text
    };

    info!("Using paste method: {:?}", paste_method);

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
        PasteMethod::Direct => input::paste_text_direct(&mut enigo, &text)?,
        PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert => {
            paste_via_clipboard(&mut enigo, &text, &app_handle, &paste_method)?
        }
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
