use crate::types::{Key, Modifiers};
use objc2_core_graphics::CGEventFlags;

/// macOS virtual key code type
pub type CGKeyCode = u16;

// macOS virtual key codes from Carbon/HIToolbox/Events.h
#[allow(dead_code)]
mod keycodes {
    pub const A: u16 = 0x00;
    pub const S: u16 = 0x01;
    pub const D: u16 = 0x02;
    pub const F: u16 = 0x03;
    pub const H: u16 = 0x04;
    pub const G: u16 = 0x05;
    pub const Z: u16 = 0x06;
    pub const X: u16 = 0x07;
    pub const C: u16 = 0x08;
    pub const V: u16 = 0x09;
    pub const B: u16 = 0x0B;
    pub const Q: u16 = 0x0C;
    pub const W: u16 = 0x0D;
    pub const E: u16 = 0x0E;
    pub const R: u16 = 0x0F;
    pub const Y: u16 = 0x10;
    pub const T: u16 = 0x11;
    pub const NUM_1: u16 = 0x12;
    pub const NUM_2: u16 = 0x13;
    pub const NUM_3: u16 = 0x14;
    pub const NUM_4: u16 = 0x15;
    pub const NUM_6: u16 = 0x16;
    pub const NUM_5: u16 = 0x17;
    pub const EQUAL: u16 = 0x18;
    pub const NUM_9: u16 = 0x19;
    pub const NUM_7: u16 = 0x1A;
    pub const MINUS: u16 = 0x1B;
    pub const NUM_8: u16 = 0x1C;
    pub const NUM_0: u16 = 0x1D;
    pub const RIGHT_BRACKET: u16 = 0x1E;
    pub const O: u16 = 0x1F;
    pub const U: u16 = 0x20;
    pub const LEFT_BRACKET: u16 = 0x21;
    pub const I: u16 = 0x22;
    pub const P: u16 = 0x23;
    pub const RETURN: u16 = 0x24;
    pub const L: u16 = 0x25;
    pub const J: u16 = 0x26;
    pub const QUOTE: u16 = 0x27;
    pub const K: u16 = 0x28;
    pub const SEMICOLON: u16 = 0x29;
    pub const BACKSLASH: u16 = 0x2A;
    pub const COMMA: u16 = 0x2B;
    pub const SLASH: u16 = 0x2C;
    pub const N: u16 = 0x2D;
    pub const M: u16 = 0x2E;
    pub const PERIOD: u16 = 0x2F;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const GRAVE: u16 = 0x32;
    pub const DELETE: u16 = 0x33;
    pub const ESCAPE: u16 = 0x35;
    pub const COMMAND: u16 = 0x37;
    pub const SHIFT: u16 = 0x38;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const OPTION: u16 = 0x3A;
    pub const CONTROL: u16 = 0x3B;
    pub const RIGHT_SHIFT: u16 = 0x3C;
    pub const RIGHT_OPTION: u16 = 0x3D;
    pub const RIGHT_CONTROL: u16 = 0x3E;
    pub const FUNCTION: u16 = 0x3F;
    pub const F17: u16 = 0x40;
    pub const KEYPAD_DECIMAL: u16 = 0x41;
    pub const KEYPAD_MULTIPLY: u16 = 0x43;
    pub const KEYPAD_PLUS: u16 = 0x45;
    pub const KEYPAD_CLEAR: u16 = 0x47;
    pub const KEYPAD_DIVIDE: u16 = 0x4B;
    pub const KEYPAD_ENTER: u16 = 0x4C;
    pub const KEYPAD_MINUS: u16 = 0x4E;
    pub const F18: u16 = 0x4F;
    pub const F19: u16 = 0x50;
    pub const F20: u16 = 0x5A;
    pub const RIGHT_COMMAND: u16 = 0x36;
    pub const KEYPAD_EQUALS: u16 = 0x51;
    pub const KEYPAD_0: u16 = 0x52;
    pub const KEYPAD_1: u16 = 0x53;
    pub const KEYPAD_2: u16 = 0x54;
    pub const KEYPAD_3: u16 = 0x55;
    pub const KEYPAD_4: u16 = 0x56;
    pub const KEYPAD_5: u16 = 0x57;
    pub const KEYPAD_6: u16 = 0x58;
    pub const KEYPAD_7: u16 = 0x59;
    pub const KEYPAD_8: u16 = 0x5B;
    pub const KEYPAD_9: u16 = 0x5C;
    pub const F5: u16 = 0x60;
    pub const F6: u16 = 0x61;
    pub const F7: u16 = 0x62;
    pub const F3: u16 = 0x63;
    pub const F8: u16 = 0x64;
    pub const F9: u16 = 0x65;
    pub const F11: u16 = 0x67;
    pub const F13: u16 = 0x69;
    pub const F16: u16 = 0x6A;
    pub const F14: u16 = 0x6B;
    pub const F10: u16 = 0x6D;
    pub const F12: u16 = 0x6F;
    pub const F15: u16 = 0x71;
    pub const HOME: u16 = 0x73;
    pub const PAGE_UP: u16 = 0x74;
    pub const FORWARD_DELETE: u16 = 0x75;
    pub const F4: u16 = 0x76;
    pub const END: u16 = 0x77;
    pub const F2: u16 = 0x78;
    pub const PAGE_DOWN: u16 = 0x79;
    pub const F1: u16 = 0x7A;
    pub const LEFT_ARROW: u16 = 0x7B;
    pub const RIGHT_ARROW: u16 = 0x7C;
    pub const DOWN_ARROW: u16 = 0x7D;
    pub const UP_ARROW: u16 = 0x7E;
}

/// Convert a macOS virtual keycode to a Key enum
pub fn keycode_to_key(keycode: CGKeyCode) -> Option<Key> {
    match keycode {
        keycodes::A => Some(Key::A),
        keycodes::B => Some(Key::B),
        keycodes::C => Some(Key::C),
        keycodes::D => Some(Key::D),
        keycodes::E => Some(Key::E),
        keycodes::F => Some(Key::F),
        keycodes::G => Some(Key::G),
        keycodes::H => Some(Key::H),
        keycodes::I => Some(Key::I),
        keycodes::J => Some(Key::J),
        keycodes::K => Some(Key::K),
        keycodes::L => Some(Key::L),
        keycodes::M => Some(Key::M),
        keycodes::N => Some(Key::N),
        keycodes::O => Some(Key::O),
        keycodes::P => Some(Key::P),
        keycodes::Q => Some(Key::Q),
        keycodes::R => Some(Key::R),
        keycodes::S => Some(Key::S),
        keycodes::T => Some(Key::T),
        keycodes::U => Some(Key::U),
        keycodes::V => Some(Key::V),
        keycodes::W => Some(Key::W),
        keycodes::X => Some(Key::X),
        keycodes::Y => Some(Key::Y),
        keycodes::Z => Some(Key::Z),
        keycodes::NUM_0 => Some(Key::Num0),
        keycodes::NUM_1 => Some(Key::Num1),
        keycodes::NUM_2 => Some(Key::Num2),
        keycodes::NUM_3 => Some(Key::Num3),
        keycodes::NUM_4 => Some(Key::Num4),
        keycodes::NUM_5 => Some(Key::Num5),
        keycodes::NUM_6 => Some(Key::Num6),
        keycodes::NUM_7 => Some(Key::Num7),
        keycodes::NUM_8 => Some(Key::Num8),
        keycodes::NUM_9 => Some(Key::Num9),
        keycodes::F1 => Some(Key::F1),
        keycodes::F2 => Some(Key::F2),
        keycodes::F3 => Some(Key::F3),
        keycodes::F4 => Some(Key::F4),
        keycodes::F5 => Some(Key::F5),
        keycodes::F6 => Some(Key::F6),
        keycodes::F7 => Some(Key::F7),
        keycodes::F8 => Some(Key::F8),
        keycodes::F9 => Some(Key::F9),
        keycodes::F10 => Some(Key::F10),
        keycodes::F11 => Some(Key::F11),
        keycodes::F12 => Some(Key::F12),
        keycodes::F13 => Some(Key::F13),
        keycodes::F14 => Some(Key::F14),
        keycodes::F15 => Some(Key::F15),
        keycodes::F16 => Some(Key::F16),
        keycodes::F17 => Some(Key::F17),
        keycodes::F18 => Some(Key::F18),
        keycodes::F19 => Some(Key::F19),
        keycodes::F20 => Some(Key::F20),
        keycodes::SPACE => Some(Key::Space),
        keycodes::RETURN => Some(Key::Return),
        keycodes::TAB => Some(Key::Tab),
        keycodes::ESCAPE => Some(Key::Escape),
        keycodes::DELETE => Some(Key::Delete),
        keycodes::FORWARD_DELETE => Some(Key::ForwardDelete),
        keycodes::HOME => Some(Key::Home),
        keycodes::END => Some(Key::End),
        keycodes::PAGE_UP => Some(Key::PageUp),
        keycodes::PAGE_DOWN => Some(Key::PageDown),
        keycodes::LEFT_ARROW => Some(Key::LeftArrow),
        keycodes::RIGHT_ARROW => Some(Key::RightArrow),
        keycodes::UP_ARROW => Some(Key::UpArrow),
        keycodes::DOWN_ARROW => Some(Key::DownArrow),
        keycodes::MINUS => Some(Key::Minus),
        keycodes::EQUAL => Some(Key::Equal),
        keycodes::LEFT_BRACKET => Some(Key::LeftBracket),
        keycodes::RIGHT_BRACKET => Some(Key::RightBracket),
        keycodes::BACKSLASH => Some(Key::Backslash),
        keycodes::SEMICOLON => Some(Key::Semicolon),
        keycodes::QUOTE => Some(Key::Quote),
        keycodes::COMMA => Some(Key::Comma),
        keycodes::PERIOD => Some(Key::Period),
        keycodes::SLASH => Some(Key::Slash),
        keycodes::GRAVE => Some(Key::Grave),
        keycodes::KEYPAD_0 => Some(Key::Keypad0),
        keycodes::KEYPAD_1 => Some(Key::Keypad1),
        keycodes::KEYPAD_2 => Some(Key::Keypad2),
        keycodes::KEYPAD_3 => Some(Key::Keypad3),
        keycodes::KEYPAD_4 => Some(Key::Keypad4),
        keycodes::KEYPAD_5 => Some(Key::Keypad5),
        keycodes::KEYPAD_6 => Some(Key::Keypad6),
        keycodes::KEYPAD_7 => Some(Key::Keypad7),
        keycodes::KEYPAD_8 => Some(Key::Keypad8),
        keycodes::KEYPAD_9 => Some(Key::Keypad9),
        keycodes::KEYPAD_DECIMAL => Some(Key::KeypadDecimal),
        keycodes::KEYPAD_MULTIPLY => Some(Key::KeypadMultiply),
        keycodes::KEYPAD_PLUS => Some(Key::KeypadPlus),
        keycodes::KEYPAD_CLEAR => Some(Key::KeypadClear),
        keycodes::KEYPAD_DIVIDE => Some(Key::KeypadDivide),
        keycodes::KEYPAD_ENTER => Some(Key::KeypadEnter),
        keycodes::KEYPAD_MINUS => Some(Key::KeypadMinus),
        keycodes::KEYPAD_EQUALS => Some(Key::KeypadEquals),
        keycodes::CAPS_LOCK => Some(Key::CapsLock),
        _ => None,
    }
}

/// Convert a modifier keycode to the corresponding side-specific Modifier flag
pub fn keycode_to_modifier(keycode: CGKeyCode) -> Option<Modifiers> {
    match keycode {
        keycodes::COMMAND => Some(Modifiers::CMD_LEFT),
        keycodes::RIGHT_COMMAND => Some(Modifiers::CMD_RIGHT),
        keycodes::SHIFT => Some(Modifiers::SHIFT_LEFT),
        keycodes::RIGHT_SHIFT => Some(Modifiers::SHIFT_RIGHT),
        keycodes::CONTROL => Some(Modifiers::CTRL_LEFT),
        keycodes::RIGHT_CONTROL => Some(Modifiers::CTRL_RIGHT),
        keycodes::OPTION => Some(Modifiers::OPT_LEFT),
        keycodes::RIGHT_OPTION => Some(Modifiers::OPT_RIGHT),
        keycodes::FUNCTION => Some(Modifiers::FN),
        _ => None,
    }
}

/// Check whether CGEventFlags indicate the FN key is held.
///
/// Used alongside keycode-tracked modifier state for side-specific modifiers,
/// since CGEventFlags are side-agnostic for Cmd/Shift/Ctrl/Opt but FN only
/// appears in flags.
pub fn flags_have_fn(flags: CGEventFlags) -> bool {
    flags.contains(CGEventFlags::MaskSecondaryFn)
}

/// Check whether CGEventFlags indicate the alpha-shift (Caps Lock) state.
pub fn flags_have_alpha_shift(flags: CGEventFlags) -> bool {
    flags.contains(CGEventFlags::MaskAlphaShift)
}
