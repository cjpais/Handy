use enigo::{Enigo, Key, Keyboard, Mouse, Settings};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Wrapper for Enigo to store in Tauri's managed state.
/// Enigo is wrapped in a Mutex since it requires mutable access.
pub struct EnigoState(pub Mutex<Enigo>);

impl EnigoState {
    pub fn new() -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;
        Ok(Self(Mutex::new(enigo)))
    }
}

/// Get the current mouse cursor position using the managed Enigo instance.
/// Returns None if the state is not available or if getting the location fails.
pub fn get_cursor_position(app_handle: &AppHandle) -> Option<(i32, i32)> {
    let enigo_state = app_handle.try_state::<EnigoState>()?;
    let enigo = enigo_state.0.lock().ok()?;
    enigo.location().ok()
}

/// Sends a Ctrl+V or Cmd+V paste command using platform-specific virtual key codes.
/// This ensures the paste works regardless of keyboard layout (e.g., Russian, AZERTY, DVORAK).
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
pub fn send_paste_ctrl_v(enigo: &mut Enigo) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9));
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

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

/// Sends a Ctrl+Shift+V paste command.
/// This is commonly used in terminal applications on Linux to paste without formatting.
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
pub fn send_paste_ctrl_shift_v(enigo: &mut Enigo) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9)); // Cmd+Shift+V on macOS
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    // Press Ctrl/Cmd + Shift + V
    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    enigo
        .key(Key::Shift, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press Shift key: {}", e))?;
    enigo
        .key(v_key_code, enigo::Direction::Click)
        .map_err(|e| format!("Failed to click V key: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    enigo
        .key(Key::Shift, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release Shift key: {}", e))?;
    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e))?;

    Ok(())
}

/// Sends a Shift+Insert paste command (Windows and Linux only).
/// This is more universal for terminal applications and legacy software.
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
pub fn send_paste_shift_insert(enigo: &mut Enigo) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let insert_key_code = Key::Other(0x2D); // VK_INSERT
    #[cfg(not(target_os = "windows"))]
    let insert_key_code = Key::Other(0x76); // XK_Insert (keycode 118 / 0x76, also used as fallback)

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
pub fn paste_text_direct(enigo: &mut Enigo, text: &str) -> Result<(), String> {
    enigo
        .text(text)
        .map_err(|e| format!("Failed to send text directly: {}", e))?;

    Ok(())
}

/// Types text as individual keyboard clicks with short pauses between keys.
/// This compatibility path is slower than `text()`, but remote desktop clients
/// tend to handle it more like physical keyboard input.
pub fn paste_text_slow(enigo: &mut Enigo, text: &str, delay_ms: u64) -> Result<(), String> {
    for ch in text.chars() {
        match slow_type_key_for_char(ch) {
            Some(SlowTypeKey::Key { key, shift }) => {
                if shift {
                    enigo
                        .key(Key::Shift, enigo::Direction::Press)
                        .map_err(|e| format!("Failed to press Shift key: {}", e))?;
                }

                let key_result = enigo
                    .key(key, enigo::Direction::Click)
                    .map_err(|e| format!("Failed to click key for '{}': {}", ch, e));

                if shift {
                    enigo
                        .key(Key::Shift, enigo::Direction::Release)
                        .map_err(|e| format!("Failed to release Shift key: {}", e))?;
                }

                key_result?;
            }
            Some(SlowTypeKey::TextFallback(text)) => {
                enigo
                    .text(text)
                    .map_err(|e| format!("Failed to send fallback text for '{}': {}", ch, e))?;
            }
            None => {
                return Err(format!(
                    "Character '{}' is not supported by slow direct typing",
                    ch
                ));
            }
        }

        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    }

    Ok(())
}

#[derive(Debug, PartialEq)]
enum SlowTypeKey {
    Key { key: Key, shift: bool },
    TextFallback(&'static str),
}

fn slow_type_key_for_char(ch: char) -> Option<SlowTypeKey> {
    let (key, shift) = match ch {
        'a'..='z' | '0'..='9' => (Key::Unicode(ch), false),
        'A'..='Z' => (Key::Unicode(ch.to_ascii_lowercase()), true),
        ' ' => (Key::Space, false),
        '\n' | '\r' => (Key::Return, false),
        '\t' => (Key::Tab, false),
        '!' => (Key::Unicode('1'), true),
        '@' => (Key::Unicode('2'), true),
        '#' => (Key::Unicode('3'), true),
        '$' => (Key::Unicode('4'), true),
        '%' => (Key::Unicode('5'), true),
        '^' => (Key::Unicode('6'), true),
        '&' => (Key::Unicode('7'), true),
        '*' => (Key::Unicode('8'), true),
        '(' => (Key::Unicode('9'), true),
        ')' => (Key::Unicode('0'), true),
        '-' => (Key::Unicode('-'), false),
        '_' => (Key::Unicode('-'), true),
        '=' => (Key::Unicode('='), false),
        '+' => (Key::Unicode('='), true),
        '[' => (Key::Unicode('['), false),
        '{' => (Key::Unicode('['), true),
        ']' => (Key::Unicode(']'), false),
        '}' => (Key::Unicode(']'), true),
        '\\' => (Key::Unicode('\\'), false),
        '|' => (Key::Unicode('\\'), true),
        ';' => (Key::Unicode(';'), false),
        ':' => (Key::Unicode(';'), true),
        '\'' => (Key::Unicode('\''), false),
        '"' => (Key::Unicode('\''), true),
        ',' => (Key::Unicode(','), false),
        '<' => (Key::Unicode(','), true),
        '.' => (Key::Unicode('.'), false),
        '>' => (Key::Unicode('.'), true),
        '/' => (Key::Unicode('/'), false),
        '?' => (Key::Unicode('/'), true),
        '`' => (Key::Unicode('`'), false),
        '~' => (Key::Unicode('`'), true),
        '\u{2019}' => return Some(SlowTypeKey::TextFallback("'")),
        '\u{201c}' | '\u{201d}' => return Some(SlowTypeKey::TextFallback("\"")),
        _ => return None,
    };

    Some(SlowTypeKey::Key { key, shift })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_type_maps_common_transcription_text() {
        assert_eq!(
            slow_type_key_for_char('T'),
            Some(SlowTypeKey::Key {
                key: Key::Unicode('t'),
                shift: true
            })
        );
        assert_eq!(
            slow_type_key_for_char(','),
            Some(SlowTypeKey::Key {
                key: Key::Unicode(','),
                shift: false
            })
        );
        assert_eq!(
            slow_type_key_for_char('?'),
            Some(SlowTypeKey::Key {
                key: Key::Unicode('/'),
                shift: true
            })
        );
        assert_eq!(
            slow_type_key_for_char(' '),
            Some(SlowTypeKey::Key {
                key: Key::Space,
                shift: false
            })
        );
    }

    #[test]
    fn slow_type_rejects_unsupported_characters() {
        assert_eq!(slow_type_key_for_char('\u{00e9}'), None);
    }
}
