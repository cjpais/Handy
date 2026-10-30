use enigo::{Direction, Enigo, Key, Mouse, Settings};
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

/// Gap between characters when typing a transcript, in milliseconds. About the
/// pace of a quick typist, so the text visibly arrives letter by letter.
pub const TYPING_KEY_DELAY_MS: u64 = 50;

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

/// One thing to send to the keyboard while typing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stroke {
    /// A key event, sent with `Keyboard::key`.
    Key(Key, Direction),
    /// A character with no key on the layout, sent with `Keyboard::text`.
    Text(char),
}

/// True when the character sits on the shifted layer of a US/ABC layout.
///
/// enigo's macOS backend finds the key for `Key::Unicode('A')` but presses it
/// without Shift, so "A" arrives as "a" and "?" as "/". Shift has to be held
/// around the key, the way a person types it.
fn needs_shift(character: char) -> bool {
    character.is_ascii_uppercase() || "~!@#$%^&*()_+{}|:\"<>?".contains(character)
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
/// `send_key` delivers one stroke. It is a closure rather than a `Keyboard`
/// because the waiting and the pressing belong on different threads on macOS:
/// enigo's macOS backend traps when driven from anywhere but the main thread,
/// while sleeping there would freeze the window for the length of the
/// transcript. Callers hop to the main thread inside `send_key` alone. Tests
/// pass a closure that records the exact event sequence instead of typing into
/// whatever window happens to hold focus.
pub fn type_text_as_keystrokes<F>(
    mut send_key: F,
    text: &str,
    key_delay_ms: u64,
) -> Result<(), String>
where
    F: FnMut(Stroke) -> Result<(), String>,
{
    let click = |key| Stroke::Key(key, Direction::Click);

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

        let strokes = match character {
            // Return alone sends the message in WhatsApp, Slack and most chat
            // boxes. Shift+Return adds a new line there and in plain text fields.
            '\n' => vec![
                Stroke::Key(Key::Shift, Direction::Press),
                click(Key::Return),
                Stroke::Key(Key::Shift, Direction::Release),
            ],
            '\t' => vec![click(Key::Tab)],
            other if needs_shift(other) => vec![
                Stroke::Key(Key::Shift, Direction::Press),
                click(Key::Unicode(other)),
                Stroke::Key(Key::Shift, Direction::Release),
            ],
            // enigo maps a character it cannot find on the layout to the "a" key.
            other if !other.is_ascii() => vec![Stroke::Text(other)],
            other => vec![click(Key::Unicode(other))],
        };

        for stroke in strokes {
            send_key(stroke).map_err(|e| format!("Failed to type {:?}: {}", character, e))?;
        }

        typed += 1;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use enigo::{InputResult, Keyboard};
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

    impl RecordingKeyboard {
        /// Stands in for the main-thread hop the real caller performs.
        fn send(&mut self, stroke: Stroke) -> Result<(), String> {
            match stroke {
                Stroke::Key(key, direction) => self.key(key, direction),
                Stroke::Text(character) => self.text(&character.to_string()),
            }
            .map_err(|e| e.to_string())
        }
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
        let text = "hello, world.";

        type_text_as_keystrokes(|stroke| keyboard.send(stroke), text, 0)
            .expect("typing should succeed");

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
    fn a_new_line_is_shift_return_so_it_never_sends_the_message() {
        let mut keyboard = RecordingKeyboard::default();

        type_text_as_keystrokes(|stroke| keyboard.send(stroke), "a\r\nb\tc", 0)
            .expect("typing should succeed");

        assert_eq!(
            keyboard.events,
            vec![
                (Key::Unicode('a'), Direction::Click),
                (Key::Shift, Direction::Press),
                (Key::Return, Direction::Click),
                (Key::Shift, Direction::Release),
                (Key::Unicode('b'), Direction::Click),
                (Key::Tab, Direction::Click),
                (Key::Unicode('c'), Direction::Click),
            ]
        );
    }

    #[test]
    fn holds_shift_for_capitals_and_shifted_symbols() {
        // Found live: without Shift, a drafted reply arrived with no capitals.
        let mut keyboard = RecordingKeyboard::default();

        type_text_as_keystrokes(|stroke| keyboard.send(stroke), "Hi?", 0)
            .expect("typing should succeed");

        assert_eq!(
            keyboard.events,
            vec![
                (Key::Shift, Direction::Press),
                (Key::Unicode('H'), Direction::Click),
                (Key::Shift, Direction::Release),
                (Key::Unicode('i'), Direction::Click),
                (Key::Shift, Direction::Press),
                (Key::Unicode('?'), Direction::Click),
                (Key::Shift, Direction::Release),
            ]
        );
    }

    #[test]
    fn sends_characters_off_the_layout_one_at_a_time_as_text() {
        let mut keyboard = RecordingKeyboard::default();

        type_text_as_keystrokes(|stroke| keyboard.send(stroke), "a’é", 0)
            .expect("typing should succeed");

        assert_eq!(keyboard.events, vec![(Key::Unicode('a'), Direction::Click)]);
        assert_eq!(keyboard.bulk_text, vec!["’".to_string(), "é".to_string()]);
    }

    #[test]
    fn paces_characters_instead_of_bursting_them() {
        let mut keyboard = RecordingKeyboard::default();
        let text = "the quick brown fox";

        type_text_as_keystrokes(|stroke| keyboard.send(stroke), text, TYPING_KEY_DELAY_MS)
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
            (1500..=4000).contains(&total),
            "{characters} characters should take 1.5-4s to type, got {total}ms"
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
        type_text_as_keystrokes(
            |stroke| {
                match stroke {
                    Stroke::Key(key, direction) => enigo.key(key, direction),
                    Stroke::Text(character) => enigo.text(&character.to_string()),
                }
                .map_err(|e| e.to_string())
            },
            &text,
            TYPING_KEY_DELAY_MS,
        )
        .expect("typing failed");

        eprintln!(
            "typed {} chars in {:?}",
            text.chars().count(),
            started.elapsed()
        );
    }
}
