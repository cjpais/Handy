use crate::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use std::thread;

#[test]
// Try entering various texts that were selected to test edge cases.
// Because it is hard to test if they succeed,
// we assume it worked as long as there was no panic
fn unit_text() {
    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    let sequences = vec![
        "",      /* Empty string */
        "a",     // Simple character
        "z",     // Simple character     // TODO: This enters "y" on my computer
        "9",     // Number
        "%",     // Special character
        "𝕊",     // Special char which needs two u16s to be encoded
        "❤️",    // Single emoji
        "abcde", // Simple short character string (shorter than 20 chars)
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz", /* Simple long character string (longer than 20 chars to test the restrictions of the macOS implementation) */
        "اَلْعَرَبِيَّةُ", // Short arabic string (meaning "Arabic")
        "中文",    // Short chinese string (meaning "Chinese")
        "日本語",  // Short japanese string (meaning "Japanese") // TODO: On my computer "日" is
        // not entered
        "aaaaaaaaaaaaaaaaaaa𝕊𝕊", // Long character string where a character starts at the 19th
        // byte and ends at the 20th byte
        "aaaaaaaaaaaaaaaaaaa❤️❤️", // Long character string where an emoji starts at the 19th byte
        // and ends at the 20th byte
        "𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊𝕊", // Long string where all 22 characters have a length of two in
        // the utf-16 encoding
        "اَلْعَرَبِيَّةُاَلْعَرَبِيَّةُاَلْعَرَبِيَّةُاَلْعَرَبِيَّةُاَلْعَرَبِيَّةُاَلْعَرَبِيَّةُ", // Long arabic string (longer than 20
        // chars to test the restrictions of the
        // macOS implementation)
        // TODO: This is missing the character on the very right
        "中文中文中文中文中文中文", // Long chinese string
        "日本語日本語日本語日本語日本語日本語日本語", // Long japanese string
        "H3llo World ❤️💯. What'𝕊 üp {}#𝄞\\日本語اَلْعَرَبِيَّةُ", /* Long string including characters
                                     * from various languages, emoji and
                                     * complex characters */
    ];

    for sequence in sequences {
        enigo.text(sequence).unwrap();
    }
}

#[ignore] // TODO: Currently ignored because not all chars are valid CStrings
#[test]
// Try entering all chars with the text function.
// Because it is hard to test if they succeed,
// we assume it worked as long as there was no panic
fn unit_text_all_utf16() {
    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    for c in 0x0000..0x0010_FFFF {
        if let Some(character) = char::from_u32(c) {
            let string = character.to_string();
            assert_eq!(
                enigo.text(&string),
                Ok(()),
                "Didn't expect an error for string: {string}"
            );
        }
    }
}

#[test]
// Test all the keys, make sure none of them panic
fn unit_key() {
    use strum::IntoEnumIterator;

    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    for key in Key::iter() {
        assert_eq!(
            enigo.key(key, Press),
            Ok(()),
            "Didn't expect an error for key: {key:?}"
        );
        assert_eq!(
            enigo.key(key, Release),
            Ok(()),
            "Didn't expect an error for key: {key:?}"
        );
        assert_eq!(
            enigo.key(key, Click),
            Ok(()),
            "Didn't expect an error for key: {key:?}"
        );
    }
    // Key::Raw and Key::Layout are ignored. They are tested separately
}

#[ignore]
#[test]
// Try entering all chars with Key::Layout and make sure none of them panic
fn unit_key_unicode_all_utf16() {
    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    for c in 0x0000..=0x0010_FFFF {
        if let Some(character) = char::from_u32(c) {
            assert_eq!(
                enigo.key(Key::Unicode(character), Press),
                Ok(()),
                "Didn't expect an error for character: {character}"
            );
            assert_eq!(
                enigo.key(Key::Unicode(character), Release),
                Ok(()),
                "Didn't expect an error for character: {character}"
            );
            assert_eq!(
                enigo.key(Key::Unicode(character), Click),
                Ok(()),
                "Didn't expect an error for character: {character}"
            );
        }
    }
}

#[ignore]
#[test]
// Try entering all possible raw keycodes with Key::Raw and make sure none of
// them panic
// On Windows it is expected that all keycodes > u16::MAX return an Err, because
// that is their maximum value
fn unit_key_other_all_keycodes() {
    use crate::InputError;

    thread::sleep(super::get_delay());
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    let max = if cfg!(target_os = "windows") {
        u16::MAX as u32
    } else {
        u32::MAX
    };
    for raw_keycode in 0..=max {
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Press),
            Ok(()),
            "Didn't expect an error for keycode: {raw_keycode}"
        );
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Release),
            Ok(()),
            "Didn't expect an error for keycode: {raw_keycode}"
        );
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Click),
            Ok(()),
            "Didn't expect an error for keycode: {raw_keycode}"
        );
    }

    // This will only run on Windows
    for raw_keycode in max..=u32::MAX {
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Press),
            Err(InputError::InvalidInput(
                "virtual keycodes on Windows have to fit into u16"
            )),
            "Expected an error for keycode: {raw_keycode}"
        );
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Release),
            Err(InputError::InvalidInput(
                "virtual keycodes on Windows have to fit into u16"
            )),
            "Expected an error for keycode: {raw_keycode}"
        );
        assert_eq!(
            enigo.key(Key::Other(raw_keycode), Click),
            Err(InputError::InvalidInput(
                "virtual keycodes on Windows have to fit into u16"
            )),
            "Expected an error for keycode: {raw_keycode}"
        );
    }
}
