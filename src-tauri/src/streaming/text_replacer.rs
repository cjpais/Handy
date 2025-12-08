//! Text replacement for streaming transcription.
//!
//! Handles replacing previously output text with updated transcription,
//! using backspace characters when accessibility APIs aren't available.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use log::debug;
use std::collections::VecDeque;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Tracks output history and provides text replacement functionality.
pub struct TextReplacer {
    /// History of output chunks for backspace-based replacement
    output_history: VecDeque<OutputChunk>,

    /// Maximum number of chunks to track
    max_history: usize,

    /// Total characters currently output (for backspace calculation)
    total_chars_output: usize,
}

/// A single output chunk with its character count
#[derive(Debug, Clone)]
struct OutputChunk {
    text: String,
    #[allow(dead_code)]
    char_count: usize,
}

impl TextReplacer {
    /// Create a new text replacer.
    ///
    /// # Arguments
    /// * `max_history` - Maximum number of output chunks to track (e.g., 5)
    pub fn new(max_history: usize) -> Self {
        Self {
            output_history: VecDeque::with_capacity(max_history),
            max_history,
            total_chars_output: 0,
        }
    }

    /// Output new text, replacing all previously output text.
    ///
    /// This will:
    /// 1. Select previous output text (Shift+Left Arrow)
    /// 2. Paste new text (replaces selection)
    /// 3. Update the output history
    pub fn replace_all(&mut self, new_text: &str, app_handle: &AppHandle) -> Result<(), String> {
        let new_char_count = new_text.chars().count();

        debug!(
            "TextReplacer: Replacing {} chars with {} chars ('{}')",
            self.total_chars_output, new_char_count, new_text
        );

        // Delete previous output
        if self.total_chars_output > 0 {
            self.send_backspaces(self.total_chars_output)?;
        }

        // Type new text
        if !new_text.is_empty() {
            self.send_text(new_text, app_handle)?;
        }

        // Update history
        self.output_history.clear();
        if !new_text.is_empty() {
            self.output_history.push_back(OutputChunk {
                text: new_text.to_string(),
                char_count: new_char_count,
            });
        }
        self.total_chars_output = new_char_count;

        Ok(())
    }

    /// Output text incrementally, appending to previous output.
    ///
    /// Use this when you want to add text without replacing what's already there.
    pub fn append(&mut self, text: &str, app_handle: &AppHandle) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }

        let char_count = text.chars().count();

        debug!("TextReplacer: Appending {} chars ('{}')", char_count, text);

        self.send_text(text, app_handle)?;

        // Track in history
        if self.output_history.len() >= self.max_history {
            if let Some(removed) = self.output_history.pop_front() {
                // Don't remove from total - we only track what we can backspace
                debug!("History full, dropping oldest chunk: '{}'", removed.text);
            }
        }

        self.output_history.push_back(OutputChunk {
            text: text.to_string(),
            char_count,
        });
        self.total_chars_output += char_count;

        Ok(())
    }

    /// Clear all tracking state (call when starting a new session).
    pub fn reset(&mut self) {
        self.output_history.clear();
        self.total_chars_output = 0;
        debug!("TextReplacer reset");
    }

    /// Get the total number of characters currently output.
    pub fn total_chars_output(&self) -> usize {
        self.total_chars_output
    }

    /// Get the full text that has been output.
    pub fn get_output_text(&self) -> String {
        self.output_history
            .iter()
            .map(|c| c.text.as_str())
            .collect()
    }

    /// Send backspace characters to delete text.
    fn send_backspaces(&self, count: usize) -> Result<(), String> {
        if count == 0 {
            return Ok(());
        }

        debug!("Sending {} backspaces", count);

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

        // Send backspaces with small delays for reliability
        for i in 0..count {
            enigo
                .key(Key::Backspace, Direction::Click)
                .map_err(|e| format!("Failed to send backspace: {}", e))?;

            // Small delay every 10 backspaces to avoid overwhelming the input system
            if i > 0 && i % 10 == 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        // Small delay after all backspaces
        std::thread::sleep(Duration::from_millis(20));

        Ok(())
    }

    /// Send text via clipboard paste (more reliable than keystroke simulation).
    fn send_text(&self, text: &str, app_handle: &AppHandle) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }

        debug!("Sending text via clipboard: '{}'", text);

        let clipboard = app_handle.clipboard();

        // Save current clipboard
        let saved_clipboard = clipboard.read_text().unwrap_or_default();

        // Write new text to clipboard
        clipboard
            .write_text(text)
            .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

        // Small delay to ensure clipboard is ready
        std::thread::sleep(Duration::from_millis(30));

        // Send paste command
        self.send_paste()?;

        // Small delay before restoring clipboard
        std::thread::sleep(Duration::from_millis(30));

        // Restore original clipboard
        clipboard
            .write_text(&saved_clipboard)
            .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

        Ok(())
    }

    /// Send the paste keyboard shortcut.
    fn send_paste(&self) -> Result<(), String> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

        #[cfg(target_os = "macos")]
        let (modifier, v_key) = (Key::Meta, Key::Other(9));
        #[cfg(target_os = "windows")]
        let (modifier, v_key) = (Key::Control, Key::Other(0x56));
        #[cfg(target_os = "linux")]
        let (modifier, v_key) = (Key::Control, Key::Unicode('v'));

        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| format!("Failed to press modifier: {}", e))?;
        enigo
            .key(v_key, Direction::Click)
            .map_err(|e| format!("Failed to click V: {}", e))?;
        std::thread::sleep(Duration::from_millis(50));
        enigo
            .key(modifier, Direction::Release)
            .map_err(|e| format!("Failed to release modifier: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_tracking() {
        let mut replacer = TextReplacer::new(5);

        assert_eq!(replacer.total_chars_output(), 0);
        assert_eq!(replacer.get_output_text(), "");

        // Simulate append (without actually sending keys)
        replacer.output_history.push_back(OutputChunk {
            text: "hello".to_string(),
            char_count: 5,
        });
        replacer.total_chars_output = 5;

        assert_eq!(replacer.total_chars_output(), 5);
        assert_eq!(replacer.get_output_text(), "hello");

        // Simulate another append
        replacer.output_history.push_back(OutputChunk {
            text: " world".to_string(),
            char_count: 6,
        });
        replacer.total_chars_output = 11;

        assert_eq!(replacer.total_chars_output(), 11);
        assert_eq!(replacer.get_output_text(), "hello world");
    }

    #[test]
    fn test_reset() {
        let mut replacer = TextReplacer::new(5);

        replacer.output_history.push_back(OutputChunk {
            text: "test".to_string(),
            char_count: 4,
        });
        replacer.total_chars_output = 4;

        replacer.reset();

        assert_eq!(replacer.total_chars_output(), 0);
        assert_eq!(replacer.get_output_text(), "");
    }
}
