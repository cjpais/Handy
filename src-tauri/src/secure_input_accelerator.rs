use handy_keys::{Hotkey, Key, Modifiers as M};

/// Accelerator tokens accepted by global-hotkey 0.8, not handy-keys names.
fn accelerator_key(key: Key) -> Option<&'static str> {
    match key {
        Key::A => Some("KeyA"),
        Key::B => Some("KeyB"),
        Key::C => Some("KeyC"),
        Key::D => Some("KeyD"),
        Key::E => Some("KeyE"),
        Key::F => Some("KeyF"),
        Key::G => Some("KeyG"),
        Key::H => Some("KeyH"),
        Key::I => Some("KeyI"),
        Key::J => Some("KeyJ"),
        Key::K => Some("KeyK"),
        Key::L => Some("KeyL"),
        Key::M => Some("KeyM"),
        Key::N => Some("KeyN"),
        Key::O => Some("KeyO"),
        Key::P => Some("KeyP"),
        Key::Q => Some("KeyQ"),
        Key::R => Some("KeyR"),
        Key::S => Some("KeyS"),
        Key::T => Some("KeyT"),
        Key::U => Some("KeyU"),
        Key::V => Some("KeyV"),
        Key::W => Some("KeyW"),
        Key::X => Some("KeyX"),
        Key::Y => Some("KeyY"),
        Key::Z => Some("KeyZ"),
        Key::Num0 => Some("Digit0"),
        Key::Num1 => Some("Digit1"),
        Key::Num2 => Some("Digit2"),
        Key::Num3 => Some("Digit3"),
        Key::Num4 => Some("Digit4"),
        Key::Num5 => Some("Digit5"),
        Key::Num6 => Some("Digit6"),
        Key::Num7 => Some("Digit7"),
        Key::Num8 => Some("Digit8"),
        Key::Num9 => Some("Digit9"),
        Key::F1 => Some("F1"),
        Key::F2 => Some("F2"),
        Key::F3 => Some("F3"),
        Key::F4 => Some("F4"),
        Key::F5 => Some("F5"),
        Key::F6 => Some("F6"),
        Key::F7 => Some("F7"),
        Key::F8 => Some("F8"),
        Key::F9 => Some("F9"),
        Key::F10 => Some("F10"),
        Key::F11 => Some("F11"),
        Key::F12 => Some("F12"),
        Key::F13 => Some("F13"),
        Key::F14 => Some("F14"),
        Key::F15 => Some("F15"),
        Key::F16 => Some("F16"),
        Key::F17 => Some("F17"),
        Key::F18 => Some("F18"),
        Key::F19 => Some("F19"),
        Key::F20 => Some("F20"),
        Key::F21 => Some("F21"),
        Key::F22 => Some("F22"),
        Key::F23 => Some("F23"),
        Key::F24 => Some("F24"),
        Key::PlayPause => Some("MediaPlayPause"),
        Key::Stop => Some("MediaStop"),
        Key::PrevTrack => Some("MediaTrackPrevious"),
        Key::NextTrack => Some("MediaTrackNext"),
        Key::Space => Some("Space"),
        Key::Return => Some("Enter"),
        Key::Tab => Some("Tab"),
        Key::Escape => Some("Escape"),
        Key::Delete => Some("Backspace"),
        Key::ForwardDelete => Some("Delete"),
        Key::Insert => Some("Insert"),
        Key::Pause => Some("Pause"),
        Key::PrintScreen => Some("PrintScreen"),
        Key::Home => Some("Home"),
        Key::End => Some("End"),
        Key::PageUp => Some("PageUp"),
        Key::PageDown => Some("PageDown"),
        Key::LeftArrow => Some("ArrowLeft"),
        Key::RightArrow => Some("ArrowRight"),
        Key::UpArrow => Some("ArrowUp"),
        Key::DownArrow => Some("ArrowDown"),
        Key::Minus => Some("Minus"),
        Key::Equal => Some("Equal"),
        Key::LeftBracket => Some("BracketLeft"),
        Key::RightBracket => Some("BracketRight"),
        Key::Backslash => Some("Backslash"),
        Key::Semicolon => Some("Semicolon"),
        Key::Quote => Some("Quote"),
        Key::Comma => Some("Comma"),
        Key::Period => Some("Period"),
        Key::Slash => Some("Slash"),
        Key::Grave => Some("Backquote"),
        Key::Keypad0 => Some("Numpad0"),
        Key::Keypad1 => Some("Numpad1"),
        Key::Keypad2 => Some("Numpad2"),
        Key::Keypad3 => Some("Numpad3"),
        Key::Keypad4 => Some("Numpad4"),
        Key::Keypad5 => Some("Numpad5"),
        Key::Keypad6 => Some("Numpad6"),
        Key::Keypad7 => Some("Numpad7"),
        Key::Keypad8 => Some("Numpad8"),
        Key::Keypad9 => Some("Numpad9"),
        Key::KeypadDecimal => Some("NumpadDecimal"),
        Key::KeypadMultiply => Some("NumpadMultiply"),
        Key::KeypadPlus => Some("NumpadAdd"),
        Key::KeypadDivide => Some("NumpadDivide"),
        Key::KeypadEnter => Some("NumpadEnter"),
        Key::KeypadMinus => Some("NumpadSubtract"),
        Key::KeypadEquals => Some("NumpadEqual"),
        Key::CapsLock => Some("CapsLock"),
        Key::ScrollLock => Some("ScrollLock"),
        Key::NumLock => Some("NumLock"),
        // These keys have no accelerator token in global-hotkey 0.8.
        Key::Section
        | Key::JisYen
        | Key::JisUnderscore
        | Key::JisEisu
        | Key::JisKana
        | Key::KeypadClear
        | Key::KeypadComma
        | Key::Dictation
        | Key::ContextMenu
        | Key::MouseLeft
        | Key::MouseRight
        | Key::MouseMiddle
        | Key::MouseX1
        | Key::MouseX2 => None,
        _ => None,
    }
}

/// Carbon widens left/right modifiers; Fn and modifier-only hotkeys cannot be shadowed.
pub(super) fn carbon_equivalent(hotkey: &Hotkey) -> Option<(String, bool)> {
    let key = accelerator_key(hotkey.key?)?;
    if hotkey.modifiers.contains(M::FN) {
        return None;
    }

    let mut tokens = Vec::new();
    let mut degraded = false;
    for (group, token) in [
        (M::CTRL, "Ctrl"),
        (M::OPT, "Alt"),
        (M::SHIFT, "Shift"),
        (M::CMD, "Super"),
    ] {
        if hotkey.modifiers.intersects(group) {
            tokens.push(token);
            degraded |= !hotkey.modifiers.contains(group);
        }
    }
    tokens.push(key);
    Some((tokens.join("+"), degraded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Shortcut;

    // All variants from handy-keys 0.3.5 src/types/key.rs.
    const SUPPORTED: &[Key] = &[
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
        Key::Num0,
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
        Key::F13,
        Key::F14,
        Key::F15,
        Key::F16,
        Key::F17,
        Key::F18,
        Key::F19,
        Key::F20,
        Key::F21,
        Key::F22,
        Key::F23,
        Key::F24,
        Key::PlayPause,
        Key::Stop,
        Key::PrevTrack,
        Key::NextTrack,
        Key::Space,
        Key::Return,
        Key::Tab,
        Key::Escape,
        Key::Delete,
        Key::ForwardDelete,
        Key::Insert,
        Key::Pause,
        Key::PrintScreen,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::LeftArrow,
        Key::RightArrow,
        Key::UpArrow,
        Key::DownArrow,
        Key::Minus,
        Key::Equal,
        Key::LeftBracket,
        Key::RightBracket,
        Key::Backslash,
        Key::Semicolon,
        Key::Quote,
        Key::Comma,
        Key::Period,
        Key::Slash,
        Key::Grave,
        Key::Keypad0,
        Key::Keypad1,
        Key::Keypad2,
        Key::Keypad3,
        Key::Keypad4,
        Key::Keypad5,
        Key::Keypad6,
        Key::Keypad7,
        Key::Keypad8,
        Key::Keypad9,
        Key::KeypadDecimal,
        Key::KeypadMultiply,
        Key::KeypadPlus,
        Key::KeypadDivide,
        Key::KeypadEnter,
        Key::KeypadMinus,
        Key::KeypadEquals,
        Key::CapsLock,
        Key::ScrollLock,
        Key::NumLock,
    ];
    const UNSUPPORTED: &[Key] = &[
        Key::Section,
        Key::JisYen,
        Key::JisUnderscore,
        Key::JisEisu,
        Key::JisKana,
        Key::KeypadClear,
        Key::KeypadComma,
        Key::Dictation,
        Key::ContextMenu,
        Key::MouseLeft,
        Key::MouseRight,
        Key::MouseMiddle,
        Key::MouseX1,
        Key::MouseX2,
    ];

    #[test]
    fn secure_input_accelerator_all_keys() {
        assert_eq!(SUPPORTED.len() + UNSUPPORTED.len(), 126);
        for &key in SUPPORTED {
            let hotkey = Hotkey::new(M::CTRL, Some(key)).unwrap();
            let (shadow, degraded) = carbon_equivalent(&hotkey).unwrap();
            assert!(!degraded);
            assert!(shadow.parse::<Shortcut>().is_ok(), "{key:?}: {shadow}");
        }
        for &key in UNSUPPORTED {
            assert_eq!(accelerator_key(key), None, "{key:?}");
            let hotkey = Hotkey::new(M::CTRL, Some(key)).unwrap();
            assert_eq!(carbon_equivalent(&hotkey), None, "{key:?}");
        }
    }

    #[test]
    fn secure_input_accelerator_names() {
        assert_eq!(accelerator_key(Key::Return), Some("Enter"));
        assert_eq!(accelerator_key(Key::Keypad0), Some("Numpad0"));
        assert_eq!(accelerator_key(Key::KeypadEnter), Some("NumpadEnter"));
        assert_eq!(accelerator_key(Key::Delete), Some("Backspace"));
        assert_eq!(accelerator_key(Key::ForwardDelete), Some("Delete"));
        assert_eq!(accelerator_key(Key::Section), None);
        assert_eq!(accelerator_key(Key::Pause), Some("Pause"));
        assert_eq!(accelerator_key(Key::PrintScreen), Some("PrintScreen"));
        assert_eq!(accelerator_key(Key::ContextMenu), None);
    }

    #[test]
    fn secure_input_accelerator_modifiers() {
        let hotkey = Hotkey::new(M::CTRL | M::OPT | M::SHIFT | M::CMD, Some(Key::Return)).unwrap();
        let (shadow, degraded) = carbon_equivalent(&hotkey).unwrap();
        assert_eq!(shadow, "Ctrl+Alt+Shift+Super+Enter");
        assert!(!degraded);
        assert!(shadow.parse::<Shortcut>().is_ok());

        let hotkey: Hotkey = "lctrl+return".parse().unwrap();
        assert_eq!(
            carbon_equivalent(&hotkey),
            Some(("Ctrl+Enter".into(), true))
        );

        let hotkey = Hotkey::new(M::empty(), Some(Key::Return)).unwrap();
        assert_eq!(carbon_equivalent(&hotkey), Some(("Enter".into(), false)));
        let hotkey = Hotkey::new(M::FN, Some(Key::Return)).unwrap();
        assert_eq!(carbon_equivalent(&hotkey), None);
        let hotkey = Hotkey::new(M::CTRL, None).unwrap();
        assert_eq!(carbon_equivalent(&hotkey), None);
    }
}
