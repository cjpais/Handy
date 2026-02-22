//! Keyboard key and mouse button definitions and parsing

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

/// Keyboard keys and mouse buttons that can be used in hotkey combinations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Key {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    // Numbers
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,

    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,

    // Special keys
    Space,
    Return,
    Tab,
    Escape,
    Delete,
    ForwardDelete,
    Home,
    End,
    PageUp,
    PageDown,

    // Arrow keys
    LeftArrow,
    RightArrow,
    UpArrow,
    DownArrow,

    // Punctuation and symbols
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Grave,

    // Keypad
    Keypad0,
    Keypad1,
    Keypad2,
    Keypad3,
    Keypad4,
    Keypad5,
    Keypad6,
    Keypad7,
    Keypad8,
    Keypad9,
    KeypadDecimal,
    KeypadMultiply,
    KeypadPlus,
    KeypadClear,
    KeypadDivide,
    KeypadEnter,
    KeypadMinus,
    KeypadEquals,

    // Lock keys
    CapsLock,
    ScrollLock,
    NumLock,

    // Mouse buttons
    MouseLeft,
    MouseRight,
    MouseMiddle,
    /// Extra button 1 (often "back" on mice with side buttons)
    MouseX1,
    /// Extra button 2 (often "forward" on mice with side buttons)
    MouseX2,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::A => write!(f, "A"),
            Key::B => write!(f, "B"),
            Key::C => write!(f, "C"),
            Key::D => write!(f, "D"),
            Key::E => write!(f, "E"),
            Key::F => write!(f, "F"),
            Key::G => write!(f, "G"),
            Key::H => write!(f, "H"),
            Key::I => write!(f, "I"),
            Key::J => write!(f, "J"),
            Key::K => write!(f, "K"),
            Key::L => write!(f, "L"),
            Key::M => write!(f, "M"),
            Key::N => write!(f, "N"),
            Key::O => write!(f, "O"),
            Key::P => write!(f, "P"),
            Key::Q => write!(f, "Q"),
            Key::R => write!(f, "R"),
            Key::S => write!(f, "S"),
            Key::T => write!(f, "T"),
            Key::U => write!(f, "U"),
            Key::V => write!(f, "V"),
            Key::W => write!(f, "W"),
            Key::X => write!(f, "X"),
            Key::Y => write!(f, "Y"),
            Key::Z => write!(f, "Z"),
            Key::Num0 => write!(f, "0"),
            Key::Num1 => write!(f, "1"),
            Key::Num2 => write!(f, "2"),
            Key::Num3 => write!(f, "3"),
            Key::Num4 => write!(f, "4"),
            Key::Num5 => write!(f, "5"),
            Key::Num6 => write!(f, "6"),
            Key::Num7 => write!(f, "7"),
            Key::Num8 => write!(f, "8"),
            Key::Num9 => write!(f, "9"),
            Key::F1 => write!(f, "F1"),
            Key::F2 => write!(f, "F2"),
            Key::F3 => write!(f, "F3"),
            Key::F4 => write!(f, "F4"),
            Key::F5 => write!(f, "F5"),
            Key::F6 => write!(f, "F6"),
            Key::F7 => write!(f, "F7"),
            Key::F8 => write!(f, "F8"),
            Key::F9 => write!(f, "F9"),
            Key::F10 => write!(f, "F10"),
            Key::F11 => write!(f, "F11"),
            Key::F12 => write!(f, "F12"),
            Key::F13 => write!(f, "F13"),
            Key::F14 => write!(f, "F14"),
            Key::F15 => write!(f, "F15"),
            Key::F16 => write!(f, "F16"),
            Key::F17 => write!(f, "F17"),
            Key::F18 => write!(f, "F18"),
            Key::F19 => write!(f, "F19"),
            Key::F20 => write!(f, "F20"),
            Key::Space => write!(f, "Space"),
            Key::Return => write!(f, "Return"),
            Key::Tab => write!(f, "Tab"),
            Key::Escape => write!(f, "Escape"),
            Key::Delete => write!(f, "Delete"),
            Key::ForwardDelete => write!(f, "ForwardDelete"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PageUp"),
            Key::PageDown => write!(f, "PageDown"),
            Key::LeftArrow => write!(f, "Left"),
            Key::RightArrow => write!(f, "Right"),
            Key::UpArrow => write!(f, "Up"),
            Key::DownArrow => write!(f, "Down"),
            Key::Minus => write!(f, "-"),
            Key::Equal => write!(f, "="),
            Key::LeftBracket => write!(f, "["),
            Key::RightBracket => write!(f, "]"),
            Key::Backslash => write!(f, "\\"),
            Key::Semicolon => write!(f, ";"),
            Key::Quote => write!(f, "'"),
            Key::Comma => write!(f, ","),
            Key::Period => write!(f, "."),
            Key::Slash => write!(f, "/"),
            Key::Grave => write!(f, "`"),
            Key::Keypad0 => write!(f, "Keypad0"),
            Key::Keypad1 => write!(f, "Keypad1"),
            Key::Keypad2 => write!(f, "Keypad2"),
            Key::Keypad3 => write!(f, "Keypad3"),
            Key::Keypad4 => write!(f, "Keypad4"),
            Key::Keypad5 => write!(f, "Keypad5"),
            Key::Keypad6 => write!(f, "Keypad6"),
            Key::Keypad7 => write!(f, "Keypad7"),
            Key::Keypad8 => write!(f, "Keypad8"),
            Key::Keypad9 => write!(f, "Keypad9"),
            Key::KeypadDecimal => write!(f, "Keypad."),
            Key::KeypadMultiply => write!(f, "Keypad*"),
            Key::KeypadPlus => write!(f, "Keypad+"),
            Key::KeypadClear => write!(f, "KeypadClear"),
            Key::KeypadDivide => write!(f, "Keypad/"),
            Key::KeypadEnter => write!(f, "KeypadEnter"),
            Key::KeypadMinus => write!(f, "Keypad-"),
            Key::KeypadEquals => write!(f, "Keypad="),
            Key::CapsLock => write!(f, "CapsLock"),
            Key::ScrollLock => write!(f, "ScrollLock"),
            Key::NumLock => write!(f, "NumLock"),
            Key::MouseLeft => write!(f, "MouseLeft"),
            Key::MouseRight => write!(f, "MouseRight"),
            Key::MouseMiddle => write!(f, "MouseMiddle"),
            Key::MouseX1 => write!(f, "MouseX1"),
            Key::MouseX2 => write!(f, "MouseX2"),
        }
    }
}

impl FromStr for Key {
    type Err = Error;

    /// Parse a key from its string representation (case-insensitive)
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        match s.to_lowercase().as_str() {
            // Letters
            "a" => Ok(Key::A),
            "b" => Ok(Key::B),
            "c" => Ok(Key::C),
            "d" => Ok(Key::D),
            "e" => Ok(Key::E),
            "f" => Ok(Key::F),
            "g" => Ok(Key::G),
            "h" => Ok(Key::H),
            "i" => Ok(Key::I),
            "j" => Ok(Key::J),
            "k" => Ok(Key::K),
            "l" => Ok(Key::L),
            "m" => Ok(Key::M),
            "n" => Ok(Key::N),
            "o" => Ok(Key::O),
            "p" => Ok(Key::P),
            "q" => Ok(Key::Q),
            "r" => Ok(Key::R),
            "s" => Ok(Key::S),
            "t" => Ok(Key::T),
            "u" => Ok(Key::U),
            "v" => Ok(Key::V),
            "w" => Ok(Key::W),
            "x" => Ok(Key::X),
            "y" => Ok(Key::Y),
            "z" => Ok(Key::Z),

            // Numbers
            "0" | "num0" => Ok(Key::Num0),
            "1" | "num1" => Ok(Key::Num1),
            "2" | "num2" => Ok(Key::Num2),
            "3" | "num3" => Ok(Key::Num3),
            "4" | "num4" => Ok(Key::Num4),
            "5" | "num5" => Ok(Key::Num5),
            "6" | "num6" => Ok(Key::Num6),
            "7" | "num7" => Ok(Key::Num7),
            "8" | "num8" => Ok(Key::Num8),
            "9" | "num9" => Ok(Key::Num9),

            // Function keys
            "f1" => Ok(Key::F1),
            "f2" => Ok(Key::F2),
            "f3" => Ok(Key::F3),
            "f4" => Ok(Key::F4),
            "f5" => Ok(Key::F5),
            "f6" => Ok(Key::F6),
            "f7" => Ok(Key::F7),
            "f8" => Ok(Key::F8),
            "f9" => Ok(Key::F9),
            "f10" => Ok(Key::F10),
            "f11" => Ok(Key::F11),
            "f12" => Ok(Key::F12),
            "f13" => Ok(Key::F13),
            "f14" => Ok(Key::F14),
            "f15" => Ok(Key::F15),
            "f16" => Ok(Key::F16),
            "f17" => Ok(Key::F17),
            "f18" => Ok(Key::F18),
            "f19" => Ok(Key::F19),
            "f20" => Ok(Key::F20),

            // Special keys
            "space" | " " => Ok(Key::Space),
            "return" | "enter" => Ok(Key::Return),
            "tab" => Ok(Key::Tab),
            "escape" | "esc" => Ok(Key::Escape),
            "delete" | "backspace" => Ok(Key::Delete),
            "forwarddelete" | "del" => Ok(Key::ForwardDelete),
            "home" => Ok(Key::Home),
            "end" => Ok(Key::End),
            "pageup" => Ok(Key::PageUp),
            "pagedown" => Ok(Key::PageDown),

            // Arrow keys
            "left" | "leftarrow" => Ok(Key::LeftArrow),
            "right" | "rightarrow" => Ok(Key::RightArrow),
            "up" | "uparrow" => Ok(Key::UpArrow),
            "down" | "downarrow" => Ok(Key::DownArrow),

            // Punctuation and symbols
            "-" | "minus" => Ok(Key::Minus),
            "=" | "equal" | "equals" => Ok(Key::Equal),
            "[" | "leftbracket" => Ok(Key::LeftBracket),
            "]" | "rightbracket" => Ok(Key::RightBracket),
            "\\" | "backslash" => Ok(Key::Backslash),
            ";" | "semicolon" => Ok(Key::Semicolon),
            "'" | "quote" => Ok(Key::Quote),
            "," | "comma" => Ok(Key::Comma),
            "." | "period" => Ok(Key::Period),
            "/" | "slash" => Ok(Key::Slash),
            "`" | "grave" | "backtick" => Ok(Key::Grave),

            // Keypad
            "keypad0" => Ok(Key::Keypad0),
            "keypad1" => Ok(Key::Keypad1),
            "keypad2" => Ok(Key::Keypad2),
            "keypad3" => Ok(Key::Keypad3),
            "keypad4" => Ok(Key::Keypad4),
            "keypad5" => Ok(Key::Keypad5),
            "keypad6" => Ok(Key::Keypad6),
            "keypad7" => Ok(Key::Keypad7),
            "keypad8" => Ok(Key::Keypad8),
            "keypad9" => Ok(Key::Keypad9),
            "keypad." | "keypaddecimal" => Ok(Key::KeypadDecimal),
            "keypad*" | "keypadmultiply" => Ok(Key::KeypadMultiply),
            "keypad+" | "keypadplus" => Ok(Key::KeypadPlus),
            "keypadclear" => Ok(Key::KeypadClear),
            "keypad/" | "keypaddivide" => Ok(Key::KeypadDivide),
            "keypadenter" => Ok(Key::KeypadEnter),
            "keypad-" | "keypadminus" => Ok(Key::KeypadMinus),
            "keypad=" | "keypadequals" => Ok(Key::KeypadEquals),

            // Lock keys
            "capslock" | "caps" => Ok(Key::CapsLock),
            "scrolllock" | "scroll" => Ok(Key::ScrollLock),
            "numlock" => Ok(Key::NumLock),

            // Mouse buttons
            "mouseleft" | "leftclick" | "lmb" | "mouse1" => Ok(Key::MouseLeft),
            "mouseright" | "rightclick" | "rmb" | "mouse2" => Ok(Key::MouseRight),
            "mousemiddle" | "middleclick" | "mmb" | "mouse3" => Ok(Key::MouseMiddle),
            "mousex1" | "mouse4" | "back" | "xbutton1" => Ok(Key::MouseX1),
            "mousex2" | "mouse5" | "forward" | "xbutton2" => Ok(Key::MouseX2),

            _ => Err(Error::UnknownKey(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_letters() {
        assert_eq!("a".parse::<Key>().unwrap(), Key::A);
        assert_eq!("A".parse::<Key>().unwrap(), Key::A);
        assert_eq!("z".parse::<Key>().unwrap(), Key::Z);
    }

    #[test]
    fn parse_numbers() {
        assert_eq!("0".parse::<Key>().unwrap(), Key::Num0);
        assert_eq!("9".parse::<Key>().unwrap(), Key::Num9);
        assert_eq!("num5".parse::<Key>().unwrap(), Key::Num5);
    }

    #[test]
    fn parse_function_keys() {
        assert_eq!("F1".parse::<Key>().unwrap(), Key::F1);
        assert_eq!("f12".parse::<Key>().unwrap(), Key::F12);
        assert_eq!("F20".parse::<Key>().unwrap(), Key::F20);
    }

    #[test]
    fn parse_special_keys() {
        assert_eq!("Space".parse::<Key>().unwrap(), Key::Space);
        assert_eq!("return".parse::<Key>().unwrap(), Key::Return);
        assert_eq!("enter".parse::<Key>().unwrap(), Key::Return);
        assert_eq!("Tab".parse::<Key>().unwrap(), Key::Tab);
        assert_eq!("Escape".parse::<Key>().unwrap(), Key::Escape);
        assert_eq!("esc".parse::<Key>().unwrap(), Key::Escape);
        assert_eq!("Delete".parse::<Key>().unwrap(), Key::Delete);
        assert_eq!("backspace".parse::<Key>().unwrap(), Key::Delete);
    }

    #[test]
    fn parse_arrow_keys() {
        assert_eq!("Left".parse::<Key>().unwrap(), Key::LeftArrow);
        assert_eq!("leftarrow".parse::<Key>().unwrap(), Key::LeftArrow);
        assert_eq!("Right".parse::<Key>().unwrap(), Key::RightArrow);
        assert_eq!("Up".parse::<Key>().unwrap(), Key::UpArrow);
        assert_eq!("Down".parse::<Key>().unwrap(), Key::DownArrow);
    }

    #[test]
    fn parse_punctuation() {
        assert_eq!("-".parse::<Key>().unwrap(), Key::Minus);
        assert_eq!("minus".parse::<Key>().unwrap(), Key::Minus);
        assert_eq!("=".parse::<Key>().unwrap(), Key::Equal);
        assert_eq!("[".parse::<Key>().unwrap(), Key::LeftBracket);
        assert_eq!("]".parse::<Key>().unwrap(), Key::RightBracket);
        assert_eq!("/".parse::<Key>().unwrap(), Key::Slash);
        assert_eq!("`".parse::<Key>().unwrap(), Key::Grave);
    }

    #[test]
    fn parse_unknown_key_fails() {
        assert!("unknown".parse::<Key>().is_err());
        assert!("".parse::<Key>().is_err());
    }

    #[test]
    fn key_display_roundtrip() {
        // Test that parsing the display output gives the same key
        let keys = [
            Key::A,
            Key::Z,
            Key::Num0,
            Key::Num9,
            Key::F1,
            Key::F12,
            Key::Space,
            Key::Return,
            Key::Tab,
            Key::Escape,
            Key::LeftArrow,
            Key::RightArrow,
        ];
        for key in keys {
            let displayed = format!("{}", key);
            let parsed: Key = displayed.parse().unwrap();
            assert_eq!(parsed, key, "Roundtrip failed for {:?}", key);
        }
    }
}
