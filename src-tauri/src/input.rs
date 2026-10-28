use enigo::{Enigo, Key, Keyboard, Mouse, Settings};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
mod macos {
    use super::Key;
    use log::{debug, warn};
    use std::ffi::c_void;

    type TisInputSourceRef = *const c_void;
    type CfDataRef = *const c_void;
    type CfStringRef = *const c_void;

    // kVK_ANSI_V. This is the behavior Handy used before layout-aware
    // resolution and remains the safest fallback if macOS cannot expose the
    // active layout.
    const ANSI_V_KEYCODE: u16 = 9;
    const KEYCODE_COUNT: u16 = 128;
    const UC_KEY_ACTION_DISPLAY: u16 = 3;
    const UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: u32 = 1;
    // Carbon's cmdKey is bit 8. UCKeyTranslate expects Carbon modifiers shifted
    // right by 8, so Command is represented by bit 0 here.
    const COMMAND_MODIFIER_STATE: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> TisInputSourceRef;
        fn TISGetInputSourceProperty(
            input_source: TisInputSourceRef,
            property_key: CfStringRef,
        ) -> CfDataRef;
        static kTISPropertyUnicodeKeyLayoutData: CfStringRef;
        fn UCKeyTranslate(
            key_layout: *const u8,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
        fn LMGetKbdType() -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: CfDataRef) -> *const u8;
        fn CFRelease(value: *const c_void);
    }

    struct InputSource(TisInputSourceRef);

    impl Drop for InputSource {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: TISCopyCurrentKeyboardLayoutInputSource returned this
                // retained reference, so this balances that ownership.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn find_keycode(mut matches: impl FnMut(u16) -> bool) -> Option<u16> {
        (0..KEYCODE_COUNT).find(|&keycode| matches(keycode))
    }

    /// Resolves the physical key that macOS interprets as `v` while Command is
    /// held. Including Command is important: non-Latin layouts commonly map
    /// Cmd shortcuts to their ANSI equivalents, while standard Dvorak does not.
    ///
    /// TIS APIs must run on the main thread; paste reaches this via
    /// `clipboard::with_enigo`, which hops there.
    fn resolve_command_v_keycode() -> Result<u16, String> {
        // SAFETY: This function is called on the macOS main thread. The returned
        // source follows the Create Rule and is released by InputSource::drop.
        let source = InputSource(unsafe { TISCopyCurrentKeyboardLayoutInputSource() });
        if source.0.is_null() {
            return Err("macOS returned no current keyboard layout input source".into());
        }

        // SAFETY: The source remains retained for the duration of the scan and
        // the property constant is provided by Carbon.
        let layout_data =
            unsafe { TISGetInputSourceProperty(source.0, kTISPropertyUnicodeKeyLayoutData) };
        if layout_data.is_null() {
            return Err("current macOS keyboard layout has no Unicode layout data".into());
        }

        // SAFETY: layout_data is a CFData owned by the retained input source and
        // remains valid until source is dropped after the scan.
        let layout = unsafe { CFDataGetBytePtr(layout_data) };
        if layout.is_null() {
            return Err("current macOS keyboard layout data is empty".into());
        }

        // SAFETY: LMGetKbdType has no arguments and returns the current physical
        // keyboard type used by UCKeyTranslate.
        let keyboard_type = unsafe { LMGetKbdType() } as u32;
        let keycode = find_keycode(|keycode| {
            let mut dead_key_state = 0;
            let mut chars = [0_u16; 4];
            let mut length = 0_usize;

            // SAFETY: layout points to valid UCKeyboardLayout bytes while source
            // is retained. All output pointers reference initialized local
            // storage of the declared sizes.
            let status = unsafe {
                UCKeyTranslate(
                    layout,
                    keycode,
                    UC_KEY_ACTION_DISPLAY,
                    COMMAND_MODIFIER_STATE,
                    keyboard_type,
                    UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                    &mut dead_key_state,
                    chars.len(),
                    &mut length,
                    chars.as_mut_ptr(),
                )
            };

            status == 0 && length == 1 && chars[0] == u16::from(b'v')
        })
        .ok_or_else(|| "could not map Cmd+V in the current macOS keyboard layout".to_string())?;

        Ok(keycode)
    }

    pub(super) fn command_v_key() -> Key {
        match resolve_command_v_keycode() {
            Ok(keycode) => {
                debug!("Resolved Cmd+V for the active macOS layout to keycode {keycode}");
                Key::Other(u32::from(keycode))
            }
            Err(error) => {
                warn!(
                    "Could not resolve Cmd+V for the active macOS layout ({error}); using ANSI V keycode {ANSI_V_KEYCODE}"
                );
                Key::Other(u32::from(ANSI_V_KEYCODE))
            }
        }
    }
}

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
///
/// `hold_ms` is how long the modifier stays held after the V click before being
/// released. Most applications read the modifier from the V event's flags and
/// need no hold at all, but applications that poll global keyboard state when
/// handling the key need the modifier to still be down — the hold insures
/// against those. Callers that can detect a failed chord (e.g. the
/// receipt-sequenced paste path) may use a much shorter hold.
pub fn send_paste_ctrl_v(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, macos::command_v_key());
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

    std::thread::sleep(std::time::Duration::from_millis(hold_ms));

    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e))?;

    Ok(())
}

/// Sends a Ctrl+Shift+V paste command.
/// This is commonly used in terminal applications on Linux to paste without formatting.
/// Note: On Wayland, this may not work - callers should check for Wayland and use alternative methods.
pub fn send_paste_ctrl_shift_v(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, macos::command_v_key());
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

    std::thread::sleep(std::time::Duration::from_millis(hold_ms));

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
pub fn send_paste_shift_insert(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String> {
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

    std::thread::sleep(std::time::Duration::from_millis(hold_ms));

    enigo
        .key(Key::Shift, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release Shift key: {}", e))?;

    Ok(())
}

/// Gap between characters when typing a transcript, in milliseconds. Fast for a
/// person, but far enough from instant that every character is its own event.
pub const TYPING_KEY_DELAY_MS: u64 = 12;

/// Spreads the gap around `base_ms` so the rhythm is uneven, the way a real
/// keyboard is. Deterministic, so it needs no random number generator.
fn jittered_delay_ms(base_ms: u64, index: usize) -> u64 {
    if base_ms == 0 {
        return 0;
    }

    // Vary by up to 40% either side of the base.
    let spread = (base_ms * 2 / 5).clamp(1, base_ms);
    let scrambled = (index as u64).wrapping_mul(2_654_435_761) >> 13;
    base_ms - spread + scrambled % (2 * spread + 1)
}

/// Types `text` one character at a time, each character its own key event.
///
/// `Enigo::text` is deliberately not used. On macOS it posts one event per
/// 20-character chunk and on Windows a single `SendInput` for the whole string,
/// so the text lands in bursts rather than as typing. Fields that reject pasted
/// content watch how text arrives, so each character is sent separately with an
/// uneven gap between them.
///
/// This sleeps for roughly `key_delay_ms` per character, so callers must run it
/// off the main thread.
///
/// Generic over the keyboard so tests can record the exact event sequence
/// instead of typing into whatever window happens to hold focus.
pub fn type_text_as_keystrokes<K: Keyboard>(
    keyboard: &mut K,
    text: &str,
    key_delay_ms: u64,
) -> Result<(), String> {
    let mut typed = 0_usize;

    for character in text.chars() {
        // The CR of a CRLF pair would otherwise submit the field a second time.
        if character == '\r' {
            continue;
        }

        if typed > 0 {
            let delay = jittered_delay_ms(key_delay_ms, typed);
            if delay > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
        }

        let key = match character {
            '\n' => Key::Return,
            '\t' => Key::Tab,
            other => Key::Unicode(other),
        };

        keyboard
            .key(key, enigo::Direction::Click)
            .map_err(|e| format!("Failed to type {:?}: {}", character, e))?;

        typed += 1;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use enigo::{Direction, InputResult};
    use std::collections::HashSet;
    use std::time::Instant;

    /// Stands in for a real keyboard so a test can see exactly what was sent,
    /// instead of typing into whatever window happens to hold focus.
    #[derive(Default)]
    struct RecordingKeyboard {
        events: Vec<(Key, Direction)>,
        gaps_ms: Vec<u128>,
        bulk_text: Vec<String>,
        last: Option<Instant>,
    }

    impl Keyboard for RecordingKeyboard {
        fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
            self.bulk_text.push(text.to_string());
            Ok(Some(()))
        }

        fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
            let now = Instant::now();
            if let Some(previous) = self.last {
                self.gaps_ms.push(now.duration_since(previous).as_millis());
            }
            self.last = Some(now);
            self.events.push((key, direction));
            Ok(())
        }

        fn raw(&mut self, _keycode: u16, _direction: Direction) -> InputResult<()> {
            Ok(())
        }
    }

    #[test]
    fn sends_one_key_event_per_character_and_never_bulk_text() {
        let mut keyboard = RecordingKeyboard::default();
        let text = "Hello, world!";

        type_text_as_keystrokes(&mut keyboard, text, 0).expect("typing should succeed");

        assert!(
            keyboard.bulk_text.is_empty(),
            "bulk text entry must not be used; arriving all at once is what gives a paste away"
        );
        assert_eq!(keyboard.events.len(), text.chars().count());
        for (index, character) in text.chars().enumerate() {
            assert_eq!(
                keyboard.events[index],
                (Key::Unicode(character), Direction::Click)
            );
        }
    }

    #[test]
    fn maps_newline_and_tab_and_drops_the_cr_of_a_crlf_pair() {
        let mut keyboard = RecordingKeyboard::default();

        type_text_as_keystrokes(&mut keyboard, "a\r\nb\tc", 0).expect("typing should succeed");

        let keys: Vec<Key> = keyboard.events.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            vec![
                Key::Unicode('a'),
                Key::Return,
                Key::Unicode('b'),
                Key::Tab,
                Key::Unicode('c'),
            ]
        );
    }

    #[test]
    fn paces_characters_instead_of_bursting_them() {
        let mut keyboard = RecordingKeyboard::default();
        let text = "the quick brown fox";

        type_text_as_keystrokes(&mut keyboard, text, TYPING_KEY_DELAY_MS)
            .expect("typing should succeed");

        assert_eq!(keyboard.events.len(), text.chars().count());
        assert!(
            keyboard.gaps_ms.iter().all(|&gap| gap >= 5),
            "every character needs a real gap before it, got {:?}",
            keyboard.gaps_ms
        );
        assert!(
            keyboard.gaps_ms.iter().collect::<HashSet<_>>().len() > 1,
            "the rhythm must vary rather than tick like a metronome, got {:?}",
            keyboard.gaps_ms
        );
    }

    #[test]
    fn jitter_stays_within_its_spread() {
        for base in [0_u64, 1, 2, 5, 12, 40, 250] {
            for index in 0..512_usize {
                let delay = jittered_delay_ms(base, index);

                if base == 0 {
                    assert_eq!(delay, 0);
                    continue;
                }

                let spread = (base * 2 / 5).clamp(1, base);
                assert!(
                    delay >= base - spread && delay <= base + spread,
                    "base {base} index {index} produced {delay}"
                );
            }
        }
    }

    #[test]
    fn jitter_does_not_produce_a_constant_rhythm() {
        let delays: Vec<u64> = (0..16).map(|i| jittered_delay_ms(12, i)).collect();

        assert!(
            delays.windows(2).any(|pair| pair[0] != pair[1]),
            "typing rhythm should vary, got {delays:?}"
        );
    }

    #[test]
    fn a_transcript_takes_human_time_to_type() {
        // Measured on the bulk path this replaces: 52 characters were inserted
        // in 353 microseconds, which no person could produce. The same text
        // must now land in human territory.
        let characters = 52_usize;
        let total: u64 = (1..characters)
            .map(|i| jittered_delay_ms(TYPING_KEY_DELAY_MS, i))
            .sum();

        assert!(
            (300..=900).contains(&total),
            "{characters} characters should take 0.3-0.9s to type, got {total}ms"
        );
    }

    /// Manual probe. Types into whatever window holds focus, so it is ignored by
    /// default. Focus a text field, then run:
    ///
    /// `PROBE_DELAY_SECS=8 cargo test --lib typing_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn typing_probe() {
        let delay_secs = std::env::var("PROBE_DELAY_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8_u64);
        let text = std::env::var("PROBE_TEXT")
            .unwrap_or_else(|_| "The quick brown fox jumps over the lazy dog.".to_string());

        eprintln!("focus a text field; typing in {delay_secs}s");
        std::thread::sleep(std::time::Duration::from_secs(delay_secs));

        let mut enigo = Enigo::new(&Settings::default()).expect("could not create Enigo");
        let started = std::time::Instant::now();
        type_text_as_keystrokes(&mut enigo, &text, TYPING_KEY_DELAY_MS).expect("typing failed");

        eprintln!(
            "typed {} chars in {:?}",
            text.chars().count(),
            started.elapsed()
        );
    }
}
