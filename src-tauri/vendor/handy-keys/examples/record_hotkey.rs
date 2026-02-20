//! Example: Record a hotkey from keyboard input
//!
//! This example demonstrates how to use KeyboardListener to capture
//! keyboard events for a "record hotkey" UI flow.
//!
//! Run with: cargo run --example record_hotkey

use handy_keys::{KeyboardListener, Result};
use std::time::Duration;

fn main() -> Result<()> {
    println!("Recording keyboard events...");
    println!("Press keys to see events. Press Escape to exit.");
    println!();

    let listener = KeyboardListener::new()?;

    loop {
        // Non-blocking check for events
        if let Some(event) = listener.try_recv() {
            let state = if event.is_key_down { "DOWN" } else { "UP" };

            // Build a display string for the current combination
            let combo = if event.modifiers.is_empty() {
                match event.key {
                    Some(key) => format!("{}", key),
                    None => "(no key)".to_string(),
                }
            } else {
                match event.key {
                    Some(key) => format!("{}+{}", event.modifiers, key),
                    None => format!("{}", event.modifiers),
                }
            };

            println!("[{}] {}", state, combo);

            // Check for Escape to exit
            if event.key == Some(handy_keys::Key::Escape) && event.is_key_down {
                println!("\nEscape pressed, exiting...");
                break;
            }
        }

        // Small sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}
