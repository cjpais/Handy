//! Windows virtual key code conversion utilities

use crate::types::{Key, Modifiers};

/// Windows Virtual Key codes
#[allow(dead_code)]
mod vk {
    // Control keys
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12; // Alt
    pub const CAPITAL: u16 = 0x14; // Caps Lock
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;

    // Lock keys
    pub const NUMLOCK: u16 = 0x90;
    pub const SCROLL: u16 = 0x91;

    // Navigation keys
    pub const PRIOR: u16 = 0x21; // Page Up
    pub const NEXT: u16 = 0x22; // Page Down
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const DELETE: u16 = 0x2E;

    // Numbers 0-9 are 0x30-0x39
    // Letters A-Z are 0x41-0x5A

    // Windows keys
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;

    // Numpad keys
    pub const NUMPAD0: u16 = 0x60;
    pub const NUMPAD1: u16 = 0x61;
    pub const NUMPAD2: u16 = 0x62;
    pub const NUMPAD3: u16 = 0x63;
    pub const NUMPAD4: u16 = 0x64;
    pub const NUMPAD5: u16 = 0x65;
    pub const NUMPAD6: u16 = 0x66;
    pub const NUMPAD7: u16 = 0x67;
    pub const NUMPAD8: u16 = 0x68;
    pub const NUMPAD9: u16 = 0x69;
    pub const MULTIPLY: u16 = 0x6A;
    pub const ADD: u16 = 0x6B;
    pub const SUBTRACT: u16 = 0x6D;
    pub const DECIMAL: u16 = 0x6E;
    pub const DIVIDE: u16 = 0x6F;

    // Function keys
    pub const F1: u16 = 0x70;
    pub const F2: u16 = 0x71;
    pub const F3: u16 = 0x72;
    pub const F4: u16 = 0x73;
    pub const F5: u16 = 0x74;
    pub const F6: u16 = 0x75;
    pub const F7: u16 = 0x76;
    pub const F8: u16 = 0x77;
    pub const F9: u16 = 0x78;
    pub const F10: u16 = 0x79;
    pub const F11: u16 = 0x7A;
    pub const F12: u16 = 0x7B;
    pub const F13: u16 = 0x7C;
    pub const F14: u16 = 0x7D;
    pub const F15: u16 = 0x7E;
    pub const F16: u16 = 0x7F;
    pub const F17: u16 = 0x80;
    pub const F18: u16 = 0x81;
    pub const F19: u16 = 0x82;
    pub const F20: u16 = 0x83;

    // Left/Right modifier variants
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4; // Left Alt
    pub const RMENU: u16 = 0xA5; // Right Alt

    // OEM keys (punctuation - US keyboard layout)
    pub const OEM_1: u16 = 0xBA; // ;:
    pub const OEM_PLUS: u16 = 0xBB; // =+
    pub const OEM_COMMA: u16 = 0xBC; // ,<
    pub const OEM_MINUS: u16 = 0xBD; // -_
    pub const OEM_PERIOD: u16 = 0xBE; // .>
    pub const OEM_2: u16 = 0xBF; // /?
    pub const OEM_3: u16 = 0xC0; // `~
    pub const OEM_4: u16 = 0xDB; // [{
    pub const OEM_5: u16 = 0xDC; // \|
    pub const OEM_6: u16 = 0xDD; // ]}
    pub const OEM_7: u16 = 0xDE; // '"
}

/// Convert Windows virtual key code to Key
///
/// The `is_extended` flag distinguishes keys like numpad Enter from main Enter.
pub fn vk_to_key(vk_code: u16, is_extended: bool) -> Option<Key> {
    match vk_code {
        // Letters A-Z (0x41-0x5A)
        0x41 => Some(Key::A),
        0x42 => Some(Key::B),
        0x43 => Some(Key::C),
        0x44 => Some(Key::D),
        0x45 => Some(Key::E),
        0x46 => Some(Key::F),
        0x47 => Some(Key::G),
        0x48 => Some(Key::H),
        0x49 => Some(Key::I),
        0x4A => Some(Key::J),
        0x4B => Some(Key::K),
        0x4C => Some(Key::L),
        0x4D => Some(Key::M),
        0x4E => Some(Key::N),
        0x4F => Some(Key::O),
        0x50 => Some(Key::P),
        0x51 => Some(Key::Q),
        0x52 => Some(Key::R),
        0x53 => Some(Key::S),
        0x54 => Some(Key::T),
        0x55 => Some(Key::U),
        0x56 => Some(Key::V),
        0x57 => Some(Key::W),
        0x58 => Some(Key::X),
        0x59 => Some(Key::Y),
        0x5A => Some(Key::Z),

        // Numbers 0-9 (0x30-0x39)
        0x30 => Some(Key::Num0),
        0x31 => Some(Key::Num1),
        0x32 => Some(Key::Num2),
        0x33 => Some(Key::Num3),
        0x34 => Some(Key::Num4),
        0x35 => Some(Key::Num5),
        0x36 => Some(Key::Num6),
        0x37 => Some(Key::Num7),
        0x38 => Some(Key::Num8),
        0x39 => Some(Key::Num9),

        // Numpad keys - these are always distinct from main keys
        vk::NUMPAD0 => Some(Key::Keypad0),
        vk::NUMPAD1 => Some(Key::Keypad1),
        vk::NUMPAD2 => Some(Key::Keypad2),
        vk::NUMPAD3 => Some(Key::Keypad3),
        vk::NUMPAD4 => Some(Key::Keypad4),
        vk::NUMPAD5 => Some(Key::Keypad5),
        vk::NUMPAD6 => Some(Key::Keypad6),
        vk::NUMPAD7 => Some(Key::Keypad7),
        vk::NUMPAD8 => Some(Key::Keypad8),
        vk::NUMPAD9 => Some(Key::Keypad9),
        vk::MULTIPLY => Some(Key::KeypadMultiply),
        vk::ADD => Some(Key::KeypadPlus),
        vk::SUBTRACT => Some(Key::KeypadMinus),
        vk::DECIMAL => Some(Key::KeypadDecimal),
        vk::DIVIDE => Some(Key::KeypadDivide),

        // Return - extended flag means numpad enter
        vk::RETURN if is_extended => Some(Key::KeypadEnter),
        vk::RETURN => Some(Key::Return),

        // Function keys
        vk::F1 => Some(Key::F1),
        vk::F2 => Some(Key::F2),
        vk::F3 => Some(Key::F3),
        vk::F4 => Some(Key::F4),
        vk::F5 => Some(Key::F5),
        vk::F6 => Some(Key::F6),
        vk::F7 => Some(Key::F7),
        vk::F8 => Some(Key::F8),
        vk::F9 => Some(Key::F9),
        vk::F10 => Some(Key::F10),
        vk::F11 => Some(Key::F11),
        vk::F12 => Some(Key::F12),
        vk::F13 => Some(Key::F13),
        vk::F14 => Some(Key::F14),
        vk::F15 => Some(Key::F15),
        vk::F16 => Some(Key::F16),
        vk::F17 => Some(Key::F17),
        vk::F18 => Some(Key::F18),
        vk::F19 => Some(Key::F19),
        vk::F20 => Some(Key::F20),

        // Special keys
        vk::BACK => Some(Key::Delete), // Backspace
        vk::DELETE => Some(Key::ForwardDelete),
        vk::TAB => Some(Key::Tab),
        vk::ESCAPE => Some(Key::Escape),
        vk::SPACE => Some(Key::Space),
        vk::PRIOR => Some(Key::PageUp),
        vk::NEXT => Some(Key::PageDown),
        vk::END => Some(Key::End),
        vk::HOME => Some(Key::Home),
        vk::LEFT => Some(Key::LeftArrow),
        vk::UP => Some(Key::UpArrow),
        vk::RIGHT => Some(Key::RightArrow),
        vk::DOWN => Some(Key::DownArrow),

        // Punctuation (OEM keys - US layout)
        vk::OEM_1 => Some(Key::Semicolon),
        vk::OEM_PLUS => Some(Key::Equal),
        vk::OEM_COMMA => Some(Key::Comma),
        vk::OEM_MINUS => Some(Key::Minus),
        vk::OEM_PERIOD => Some(Key::Period),
        vk::OEM_2 => Some(Key::Slash),
        vk::OEM_3 => Some(Key::Grave),
        vk::OEM_4 => Some(Key::LeftBracket),
        vk::OEM_5 => Some(Key::Backslash),
        vk::OEM_6 => Some(Key::RightBracket),
        vk::OEM_7 => Some(Key::Quote),

        // Lock keys
        vk::CAPITAL => Some(Key::CapsLock),
        vk::NUMLOCK => Some(Key::NumLock),
        vk::SCROLL => Some(Key::ScrollLock),

        _ => None,
    }
}

/// Convert Windows virtual key code to side-specific Modifier
pub fn vk_to_modifier(vk_code: u16) -> Option<Modifiers> {
    match vk_code {
        vk::LSHIFT => Some(Modifiers::SHIFT_LEFT),
        vk::RSHIFT => Some(Modifiers::SHIFT_RIGHT),
        vk::SHIFT => Some(Modifiers::SHIFT_LEFT), // generic falls back to left
        vk::LCONTROL => Some(Modifiers::CTRL_LEFT),
        vk::RCONTROL => Some(Modifiers::CTRL_RIGHT),
        vk::CONTROL => Some(Modifiers::CTRL_LEFT),
        vk::LMENU => Some(Modifiers::OPT_LEFT),
        vk::RMENU => Some(Modifiers::OPT_RIGHT),
        vk::MENU => Some(Modifiers::OPT_LEFT),
        vk::LWIN => Some(Modifiers::CMD_LEFT),
        vk::RWIN => Some(Modifiers::CMD_RIGHT),
        _ => None,
    }
}
