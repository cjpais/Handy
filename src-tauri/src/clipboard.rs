use crate::settings;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

fn key_description(key: &Key) -> String {
    match key {
        Key::Other(code) => format!("keycode:{}", code),
        Key::Unicode(ch) => format!("unicode:'{}'", ch),
        Key::Space => "space".to_string(),
        Key::Return => "return".to_string(),
        Key::Tab => "tab".to_string(),
        Key::Escape => "escape".to_string(),
        Key::Meta => "meta".to_string(),
        Key::Control => "control".to_string(),
        Key::Option => "option".to_string(),
        Key::Shift => "shift".to_string(),
        Key::Alt => "alt".to_string(),
        #[allow(deprecated)]
        Key::Command => "command".to_string(),
        other => format!("{:?}", other),
    }
}

fn parse_named_special_key(token: &str) -> Option<Key> {
    match token {
        "space" => Some(Key::Space),
        "enter" | "return" => Some(Key::Return),
        "tab" => Some(Key::Tab),
        "escape" | "esc" => Some(Key::Escape),
        _ => None,
    }
}

fn parse_named_char(token: &str) -> Option<char> {
    match token {
        "comma" => Some(','),
        "period" | "dot" => Some('.'),
        "slash" => Some('/'),
        "backslash" => Some('\\'),
        "semicolon" => Some(';'),
        "apostrophe" | "quote" => Some('\''),
        "backtick" | "grave" => Some('`'),
        "minus" | "dash" => Some('-'),
        "equal" | "equals" => Some('='),
        "space" => Some(' '),
        _ => None,
    }
}

fn parse_unicode_value(token: &str) -> Result<char, String> {
    let trimmed = token.trim();
    if trimmed.starts_with("\\u{") && trimmed.ends_with('}') {
        let inner = &trimmed[3..trimmed.len() - 1];
        let code = u32::from_str_radix(inner, 16)
            .map_err(|_| format!("Invalid unicode escape '{}'", trimmed))?;
        char::from_u32(code).ok_or_else(|| format!("Invalid unicode code point '{}'", trimmed))
    } else if trimmed.chars().count() == 1 {
        Ok(trimmed.chars().next().unwrap())
    } else {
        Err(format!(
            "Unicode token '{}' must be a single character or \\u{{..}} sequence",
            trimmed
        ))
    }
}

fn parse_paste_binding(binding: &str) -> Result<(Vec<Key>, Key), String> {
    if binding.trim().is_empty() {
        return Err("Paste binding is empty".to_string());
    }

    let mut modifiers = Vec::new();
    let mut trigger: Option<Key> = None;

    for raw in binding.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }

        let lower = token.to_ascii_lowercase();

        let modifier = match lower.as_str() {
            "meta" | "command" | "cmd" | "super" => Some(Key::Meta),
            "control" | "ctrl" => Some(Key::Control),
            "option" | "alt" => Some(Key::Option),
            "shift" => Some(Key::Shift),
            _ => None,
        };

        if let Some(mod_key) = modifier {
            modifiers.push(mod_key);
            continue;
        }

        if trigger.is_some() {
            return Err(format!(
                "Paste binding '{}' contains more than one non-modifier key",
                binding
            ));
        }

        if let Some(rest) = lower.strip_prefix("keycode:") {
            let code_str = rest.trim();
            let parsed = if let Some(hex) = code_str.strip_prefix("0x") {
                u32::from_str_radix(hex, 16)
                    .map_err(|_| format!("Invalid hexadecimal keycode value '{}'", code_str))?
            } else {
                code_str
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid keycode value '{}'", code_str))?
            };
            trigger = Some(Key::Other(parsed));
            continue;
        }

        if let Some(rest) = lower.strip_prefix("unicode:") {
            let ch = parse_unicode_value(rest)?;
            trigger = Some(Key::Unicode(ch));
            continue;
        }

        if let Some(spec) = parse_named_special_key(lower.as_str()) {
            trigger = Some(spec);
            continue;
        }

        if let Some(mapped) = parse_named_char(lower.as_str()) {
            trigger = Some(Key::Unicode(mapped));
            continue;
        }

        if token.chars().count() == 1 {
            trigger = Some(Key::Unicode(token.chars().next().unwrap()));
            continue;
        }

        return Err(format!("Unsupported paste binding token '{}'", token));
    }

    let trigger = trigger.ok_or_else(|| {
        format!(
            "Paste binding '{}' does not contain a non-modifier key",
            binding
        )
    })?;

    Ok((modifiers, trigger))
}

pub fn validate_paste_binding(binding: &str) -> Result<(), String> {
    parse_paste_binding(binding).map(|_| ())
}

fn send_paste(binding: &str) -> Result<(), String> {
    let (modifiers, trigger) = parse_paste_binding(binding)?;

    println!(
        "Paste debug -> modifiers: [{}], key: {}",
        modifiers
            .iter()
            .map(key_description)
            .collect::<Vec<_>>()
            .join(", "),
        key_description(&trigger)
    );

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    for modifier in &modifiers {
        enigo
            .key(*modifier, Direction::Press)
            .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    }

    enigo
        .key(trigger, Direction::Press)
        .map_err(|e| format!("Failed to press paste key: {}", e))?;

    enigo
        .key(trigger, Direction::Release)
        .map_err(|e| format!("Failed to release paste key: {}", e))?;

    for modifier in modifiers.iter().rev() {
        enigo
            .key(*modifier, Direction::Release)
            .map_err(|e| format!("Failed to release modifier key: {}", e))?;
    }

    Ok(())
}

pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();
    let settings = settings::get_settings(&app_handle);
    let binding = settings.paste_binding.clone();

    // get the current clipboard content
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(&text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    // small delay to ensure the clipboard content has been written to
    std::thread::sleep(std::time::Duration::from_millis(50));

    if let Err(err) = send_paste(&binding) {
        eprintln!(
            "Failed to use custom paste binding '{}': {}. Falling back to default.",
            binding, err
        );
        let fallback_binding = settings::get_default_settings().paste_binding;
        send_paste(&fallback_binding)?;
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

    // restore the clipboard
    clipboard
        .write_text(&clipboard_content)
        .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

    Ok(())
}
