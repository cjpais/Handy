//! Linux key conversion utilities (using rdev)

use crate::types::{Key, Modifiers};

/// Convert rdev::Key to our Key type
pub fn rdev_key_to_key(key: rdev::Key) -> Option<Key> {
    use rdev::Key as RK;
    match key {
        // Letters
        RK::KeyA => Some(Key::A),
        RK::KeyB => Some(Key::B),
        RK::KeyC => Some(Key::C),
        RK::KeyD => Some(Key::D),
        RK::KeyE => Some(Key::E),
        RK::KeyF => Some(Key::F),
        RK::KeyG => Some(Key::G),
        RK::KeyH => Some(Key::H),
        RK::KeyI => Some(Key::I),
        RK::KeyJ => Some(Key::J),
        RK::KeyK => Some(Key::K),
        RK::KeyL => Some(Key::L),
        RK::KeyM => Some(Key::M),
        RK::KeyN => Some(Key::N),
        RK::KeyO => Some(Key::O),
        RK::KeyP => Some(Key::P),
        RK::KeyQ => Some(Key::Q),
        RK::KeyR => Some(Key::R),
        RK::KeyS => Some(Key::S),
        RK::KeyT => Some(Key::T),
        RK::KeyU => Some(Key::U),
        RK::KeyV => Some(Key::V),
        RK::KeyW => Some(Key::W),
        RK::KeyX => Some(Key::X),
        RK::KeyY => Some(Key::Y),
        RK::KeyZ => Some(Key::Z),

        // Numbers
        RK::Num0 => Some(Key::Num0),
        RK::Num1 => Some(Key::Num1),
        RK::Num2 => Some(Key::Num2),
        RK::Num3 => Some(Key::Num3),
        RK::Num4 => Some(Key::Num4),
        RK::Num5 => Some(Key::Num5),
        RK::Num6 => Some(Key::Num6),
        RK::Num7 => Some(Key::Num7),
        RK::Num8 => Some(Key::Num8),
        RK::Num9 => Some(Key::Num9),

        // Function keys
        // Note: rdev on Linux only supports F1-F12
        RK::F1 => Some(Key::F1),
        RK::F2 => Some(Key::F2),
        RK::F3 => Some(Key::F3),
        RK::F4 => Some(Key::F4),
        RK::F5 => Some(Key::F5),
        RK::F6 => Some(Key::F6),
        RK::F7 => Some(Key::F7),
        RK::F8 => Some(Key::F8),
        RK::F9 => Some(Key::F9),
        RK::F10 => Some(Key::F10),
        RK::F11 => Some(Key::F11),
        RK::F12 => Some(Key::F12),

        // Special keys
        RK::Space => Some(Key::Space),
        RK::Return => Some(Key::Return),
        RK::Tab => Some(Key::Tab),
        RK::Escape => Some(Key::Escape),
        RK::Backspace => Some(Key::Delete),
        RK::Delete => Some(Key::ForwardDelete),
        RK::Home => Some(Key::Home),
        RK::End => Some(Key::End),
        RK::PageUp => Some(Key::PageUp),
        RK::PageDown => Some(Key::PageDown),

        // Arrow keys
        RK::LeftArrow => Some(Key::LeftArrow),
        RK::RightArrow => Some(Key::RightArrow),
        RK::UpArrow => Some(Key::UpArrow),
        RK::DownArrow => Some(Key::DownArrow),

        // Punctuation and symbols
        RK::Minus => Some(Key::Minus),
        RK::Equal => Some(Key::Equal),
        RK::LeftBracket => Some(Key::LeftBracket),
        RK::RightBracket => Some(Key::RightBracket),
        RK::BackSlash => Some(Key::Backslash),
        RK::SemiColon => Some(Key::Semicolon),
        RK::Quote => Some(Key::Quote),
        RK::Comma => Some(Key::Comma),
        RK::Dot => Some(Key::Period),
        RK::Slash => Some(Key::Slash),
        RK::BackQuote => Some(Key::Grave),

        // Keypad
        RK::Kp0 => Some(Key::Keypad0),
        RK::Kp1 => Some(Key::Keypad1),
        RK::Kp2 => Some(Key::Keypad2),
        RK::Kp3 => Some(Key::Keypad3),
        RK::Kp4 => Some(Key::Keypad4),
        RK::Kp5 => Some(Key::Keypad5),
        RK::Kp6 => Some(Key::Keypad6),
        RK::Kp7 => Some(Key::Keypad7),
        RK::Kp8 => Some(Key::Keypad8),
        RK::Kp9 => Some(Key::Keypad9),
        RK::KpMinus => Some(Key::KeypadMinus),
        RK::KpPlus => Some(Key::KeypadPlus),
        RK::KpMultiply => Some(Key::KeypadMultiply),
        RK::KpDivide => Some(Key::KeypadDivide),
        RK::KpDelete => Some(Key::KeypadDecimal),
        RK::KpReturn => Some(Key::KeypadEnter),

        // Lock keys
        RK::CapsLock => Some(Key::CapsLock),
        RK::ScrollLock => Some(Key::ScrollLock),
        RK::NumLock => Some(Key::NumLock),

        _ => None,
    }
}

/// Convert an rdev modifier key to our side-specific Modifiers type
pub fn rdev_key_to_modifier(key: rdev::Key) -> Option<Modifiers> {
    use rdev::Key as RK;
    match key {
        RK::ShiftLeft => Some(Modifiers::SHIFT_LEFT),
        RK::ShiftRight => Some(Modifiers::SHIFT_RIGHT),
        RK::ControlLeft => Some(Modifiers::CTRL_LEFT),
        RK::ControlRight => Some(Modifiers::CTRL_RIGHT),
        RK::Alt => Some(Modifiers::OPT_LEFT),
        RK::AltGr => Some(Modifiers::OPT_RIGHT),
        RK::MetaLeft => Some(Modifiers::CMD_LEFT),
        RK::MetaRight => Some(Modifiers::CMD_RIGHT),
        _ => None,
    }
}

/// Convert rdev::Button to our Key type
pub fn rdev_button_to_key(button: rdev::Button) -> Option<Key> {
    use rdev::Button as RB;
    match button {
        RB::Left => Some(Key::MouseLeft),
        RB::Right => Some(Key::MouseRight),
        RB::Middle => Some(Key::MouseMiddle),
        RB::Unknown(8) => Some(Key::MouseX1), // Back button (X11 button 8)
        RB::Unknown(9) => Some(Key::MouseX2), // Forward button (X11 button 9)
        _ => None,
    }
}

/// Update modifier state based on key event
pub fn update_modifiers(current: Modifiers, key: rdev::Key, pressed: bool) -> Modifiers {
    use rdev::Key as RK;
    let modifier = match key {
        RK::ShiftLeft => Modifiers::SHIFT_LEFT,
        RK::ShiftRight => Modifiers::SHIFT_RIGHT,
        RK::ControlLeft => Modifiers::CTRL_LEFT,
        RK::ControlRight => Modifiers::CTRL_RIGHT,
        RK::Alt => Modifiers::OPT_LEFT,
        RK::AltGr => Modifiers::OPT_RIGHT,
        RK::MetaLeft => Modifiers::CMD_LEFT,
        RK::MetaRight => Modifiers::CMD_RIGHT,
        _ => return current,
    };

    if pressed {
        current | modifier
    } else {
        current & !modifier
    }
}
