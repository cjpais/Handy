use crate::settings::{get_settings, PasteMethod};
use enigo::{Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Sends a paste command (Cmd+V or Ctrl+V) using platform-specific virtual key codes.
/// This ensures the paste works regardless of keyboard layout (e.g., Russian, AZERTY, DVORAK).
fn send_paste() -> Result<(), String> {
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


/// Selects the last n characters by holding Shift and pressing Left Arrow n times.
/// This allows selecting only the recently pasted text instead of the entire input field.
fn select_last_n(n: usize) -> Result<(), String> {
    if n == 0 {
        return Ok(());
    }

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    // Platform-specific left arrow key definitions
    #[cfg(target_os = "macos")]
    let left_key_code = Key::Other(123); // macOS: Left Arrow
    #[cfg(target_os = "windows")]
    let left_key_code = Key::Other(0x25); // Windows: VK_LEFT
    #[cfg(target_os = "linux")]
    {
        // Linux: fallback to select all for reliability
        return send_select_all();
    }

    // Press and hold Shift
    enigo
        .key(Key::Shift, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press Shift key: {}", e))?;

    // Press Left Arrow n times to select n characters backwards
    for _ in 0..n {
        enigo
            .key(left_key_code, enigo::Direction::Click)
            .map_err(|e| format!("Failed to click Left Arrow: {}", e))?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Release Shift
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

/// Pastes text using the clipboard method (Ctrl+V/Cmd+V).
/// Saves the current clipboard, writes the text, sends paste command, then restores the clipboard.
fn paste_via_clipboard(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();

    // get the current clipboard content
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    // small delay to ensure the clipboard content has been written to
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_paste()?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    // restore the clipboard
    clipboard
        .write_text(&clipboard_content)
        .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

    Ok(())
}

/// Pastes text and then selects it for further processing (like polishing).
/// This allows the user to see the transcribed text before any additional processing.
pub fn paste_and_select(text: String, app_handle: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;

    println!("Using paste method: {:?}", paste_method);

    // First paste the text
    match paste_method {
        PasteMethod::CtrlV => paste_via_clipboard(&text, &app_handle)?,
        PasteMethod::Direct => paste_via_direct_input(&text)?,
    }

    // Small delay to ensure text is pasted
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Select only the pasted text (not the entire input field)
    let char_count = text.chars().count();
    select_last_n(char_count)?;

    Ok(())
}

pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;

    println!("Using paste method: {:?}", paste_method);

    match paste_method {
        PasteMethod::CtrlV => paste_via_clipboard(&text, &app_handle),
        PasteMethod::Direct => paste_via_direct_input(&text),
    }
}

pub fn copy_selected_text(_app: &AppHandle) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    #[cfg(target_os = "macos")]
    let (modifier_key, c_key_code) = (Key::Meta, Key::Other(8));
    #[cfg(target_os = "windows")]
    let (modifier_key, c_key_code) = (Key::Control, Key::Other(0x43)); // VK_C
    #[cfg(target_os = "linux")]
    let (modifier_key, c_key_code) = (Key::Control, Key::Unicode('c'));

    // Press modifier + C
    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    enigo
        .key(c_key_code, enigo::Direction::Click)
        .map_err(|e| format!("Failed to click C key: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e))?;

    Ok(())
}
