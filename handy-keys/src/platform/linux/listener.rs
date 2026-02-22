//! Linux keyboard listener using rdev
//!
//! # Shutdown Behavior
//!
//! When dropped, the listener stops processing events. The underlying thread
//! remains alive (rdev limitation) but becomes idle because rdev::grab()
//! blocks indefinitely and cannot be interrupted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::error::Result;
use crate::platform::state::{BlockingHotkeys, ListenerState};
use crate::types::{KeyEvent, Modifiers};

use super::keycode::{rdev_button_to_key, rdev_key_to_key, rdev_key_to_modifier, update_modifiers};
use crate::types::Key;

/// Internal listener state returned to KeyboardListener
pub(crate) struct LinuxListenerState {
    pub event_receiver: Receiver<KeyEvent>,
    pub thread_handle: Option<JoinHandle<()>>,
    pub running: Arc<AtomicBool>,
    pub blocking_hotkeys: Option<BlockingHotkeys>,
}

/// Spawn an rdev-based keyboard listener for Linux
pub(crate) fn spawn(blocking_hotkeys: Option<BlockingHotkeys>) -> Result<LinuxListenerState> {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(ListenerState::new(tx, blocking_hotkeys.clone())));
    let running = Arc::new(AtomicBool::new(true));

    let thread_state = Arc::clone(&state);
    let thread_running = Arc::clone(&running);

    let handle = thread::spawn(move || {
        let callback = move |event: rdev::Event| -> Option<rdev::Event> {
            // Check if we should stop processing events
            if !thread_running.load(Ordering::SeqCst) {
                return Some(event);
            }

            let mut should_block = false;

            if let Ok(mut state) = thread_state.lock() {
                match event.event_type {
                    rdev::EventType::KeyPress(rdev_key) => {
                        if let Some(changed_modifier) = rdev_key_to_modifier(rdev_key) {
                            let prev_mods = state.current_modifiers;
                            state.current_modifiers =
                                update_modifiers(state.current_modifiers, rdev_key, true);

                            // Emit modifier change event
                            if state.current_modifiers != prev_mods {
                                // Check if this modifier-only combo should be blocked
                                should_block = state.should_block(state.current_modifiers, None);

                                let _ = state.event_sender.send(KeyEvent {
                                    modifiers: state.current_modifiers,
                                    key: None,
                                    is_key_down: true,
                                    changed_modifier: Some(changed_modifier),
                                });
                            }
                        } else if let Some(key) = rdev_key_to_key(rdev_key) {
                            // Check if this should be blocked
                            should_block = state.should_block(state.current_modifiers, Some(key));

                            let _ = state.event_sender.send(KeyEvent {
                                modifiers: state.current_modifiers,
                                key: Some(key),
                                is_key_down: true,
                                changed_modifier: None,
                            });
                        }
                    }
                    rdev::EventType::KeyRelease(rdev_key) => {
                        if let Some(changed_modifier) = rdev_key_to_modifier(rdev_key) {
                            let prev_mods = state.current_modifiers;
                            state.current_modifiers =
                                update_modifiers(state.current_modifiers, rdev_key, false);

                            // Emit modifier change event
                            if state.current_modifiers != prev_mods {
                                let _ = state.event_sender.send(KeyEvent {
                                    modifiers: state.current_modifiers,
                                    key: None,
                                    is_key_down: false,
                                    changed_modifier: Some(changed_modifier),
                                });
                            }
                        } else if let Some(key) = rdev_key_to_key(rdev_key) {
                            // Block key up if we blocked key down (to be consistent)
                            should_block = state.should_block(state.current_modifiers, Some(key));

                            let _ = state.event_sender.send(KeyEvent {
                                modifiers: state.current_modifiers,
                                key: Some(key),
                                is_key_down: false,
                                changed_modifier: None,
                            });
                        }
                    }
                    rdev::EventType::ButtonPress(button) => {
                        if let Some(key) = rdev_button_to_key(button) {
                            // Only report left/right clicks when modifiers are held
                            let is_common = matches!(key, Key::MouseLeft | Key::MouseRight);
                            if !is_common || !state.current_modifiers.is_empty() {
                                let _ = state.event_sender.send(KeyEvent {
                                    modifiers: state.current_modifiers,
                                    key: Some(key),
                                    is_key_down: true,
                                    changed_modifier: None,
                                });
                            }
                        }
                    }
                    rdev::EventType::ButtonRelease(button) => {
                        if let Some(key) = rdev_button_to_key(button) {
                            let is_common = matches!(key, Key::MouseLeft | Key::MouseRight);
                            if !is_common || !state.current_modifiers.is_empty() {
                                let _ = state.event_sender.send(KeyEvent {
                                    modifiers: state.current_modifiers,
                                    key: Some(key),
                                    is_key_down: false,
                                    changed_modifier: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }

            if should_block {
                None // Block the event
            } else {
                Some(event) // Pass through
            }
        };

        // Start grabbing - this blocks indefinitely
        if let Err(e) = rdev::grab(callback) {
            eprintln!("rdev grab error: {:?}", e);
        }
    });

    Ok(LinuxListenerState {
        event_receiver: rx,
        thread_handle: Some(handle),
        running,
        blocking_hotkeys,
    })
}
