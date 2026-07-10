//! Tracks the most recent text Handy pasted so voice edit commands
//! ("scratch that", "make it shorter") can operate on it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LastOutput {
    /// Exactly what was typed into the focused app (including any trailing
    /// space appended by the paste layer).
    pub text: String,
    /// `chars().count()` of `text` — the number of Backspace presses needed
    /// to remove it. (Backspace deletes grapheme clusters in some apps;
    /// multi-codepoint emoji may over-count. Dictated text is effectively
    /// plain prose, so this is acceptable.)
    pub char_count: usize,
    pub pasted_at: Instant,
}

/// Managed Tauri state. `None` when there is nothing edit-eligible (startup,
/// after a delete, after auto-submit, or after the window expired).
#[derive(Default)]
pub struct LastOutputState(Mutex<Option<LastOutput>>);

impl LastOutputState {
    pub fn record(&self, text: String) {
        let char_count = text.chars().count();
        *self.0.lock().unwrap() = Some(LastOutput {
            text,
            char_count,
            pasted_at: Instant::now(),
        });
    }

    /// The last output if it is younger than `window`.
    pub fn get_fresh(&self, window: Duration) -> Option<LastOutput> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .filter(|o| o.pasted_at.elapsed() < window)
            .cloned()
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}
