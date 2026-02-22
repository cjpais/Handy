//! Platform-agnostic hotkey manager built on top of KeyboardListener

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::error::{Error, Result};
use crate::listener::{BlockingHotkeys, KeyboardListener};
use crate::types::{Hotkey, HotkeyEvent, HotkeyId, HotkeyState, KeyEvent};

/// Internal state shared between the manager and the processing thread
struct ManagerState {
    hotkeys: HashMap<HotkeyId, Hotkey>,
    next_id: u32,
    /// Track which hotkeys are currently pressed
    pressed_hotkeys: HashSet<HotkeyId>,
}

impl ManagerState {
    fn new() -> Self {
        Self {
            hotkeys: HashMap::new(),
            next_id: 0,
            pressed_hotkeys: HashSet::new(),
        }
    }

    /// Process a key event and return any matching hotkey events
    fn process_event(&mut self, event: &KeyEvent) -> Vec<HotkeyEvent> {
        let mut results = Vec::new();

        if event.is_key_down {
            // Check for hotkeys that should be pressed
            let to_press: Vec<HotkeyId> = self
                .hotkeys
                .iter()
                .filter(|(&id, hotkey)| {
                    hotkey.modifiers.matches(event.modifiers)
                        && hotkey.key == event.key
                        && !self.pressed_hotkeys.contains(&id)
                })
                .map(|(&id, _)| id)
                .collect();

            for id in to_press {
                self.pressed_hotkeys.insert(id);
                results.push(HotkeyEvent {
                    id,
                    state: HotkeyState::Pressed,
                });
            }
        } else {
            // Check for hotkeys that should be released
            // A hotkey is released when either its key is released or its modifiers change
            let to_release: Vec<HotkeyId> = self
                .hotkeys
                .iter()
                .filter(|(&id, hotkey)| {
                    self.pressed_hotkeys.contains(&id)
                        && (hotkey.key == event.key
                            || (event.key.is_none() && !hotkey.modifiers.matches(event.modifiers)))
                })
                .map(|(&id, _)| id)
                .collect();

            for id in to_release {
                self.pressed_hotkeys.remove(&id);
                results.push(HotkeyEvent {
                    id,
                    state: HotkeyState::Released,
                });
            }
        }

        results
    }
}

/// Platform-agnostic Hotkey Manager
///
/// This manager wraps a `KeyboardListener` and filters events against
/// registered hotkeys, emitting `HotkeyEvent`s when matches occur.
///
/// Registered hotkeys are blocked from reaching other applications.
/// Note: On Linux/Wayland, blocking may not work due to compositor restrictions.
pub struct HotkeyManager {
    state: Arc<Mutex<ManagerState>>,
    event_receiver: Receiver<HotkeyEvent>,
    _thread_handle: Option<JoinHandle<()>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Shared set of hotkeys to block
    blocking_hotkeys: Option<BlockingHotkeys>,
}

impl HotkeyManager {
    /// Create a new HotkeyManager (non-blocking mode)
    ///
    /// On macOS, this will check for accessibility permissions and fail if not granted.
    pub fn new() -> Result<Self> {
        let listener = KeyboardListener::new()?;

        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(ManagerState::new()));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let thread_state = Arc::clone(&state);
        let thread_running = Arc::clone(&running);

        let handle = thread::spawn(move || {
            Self::event_loop(listener, thread_state, tx, thread_running);
        });

        Ok(Self {
            state,
            event_receiver: rx,
            _thread_handle: Some(handle),
            running,
            blocking_hotkeys: None,
        })
    }

    /// Create a new HotkeyManager with blocking support
    ///
    /// On macOS, this will check for accessibility permissions and fail if not granted.
    /// Registered hotkeys will be blocked from reaching other applications.
    ///
    /// Note: On Linux/Wayland, blocking may not work due to compositor restrictions.
    pub fn new_with_blocking() -> Result<Self> {
        let blocking_hotkeys: BlockingHotkeys = Arc::new(Mutex::new(HashSet::new()));
        let listener = KeyboardListener::new_with_blocking(blocking_hotkeys.clone())?;

        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(ManagerState::new()));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let thread_state = Arc::clone(&state);
        let thread_running = Arc::clone(&running);

        let handle = thread::spawn(move || {
            Self::event_loop(listener, thread_state, tx, thread_running);
        });

        Ok(Self {
            state,
            event_receiver: rx,
            _thread_handle: Some(handle),
            running,
            blocking_hotkeys: Some(blocking_hotkeys),
        })
    }

    /// Event processing loop
    fn event_loop(
        listener: KeyboardListener,
        state: Arc<Mutex<ManagerState>>,
        sender: Sender<HotkeyEvent>,
        running: Arc<std::sync::atomic::AtomicBool>,
    ) {
        const RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            // Block until we receive an event or timeout (to check running flag)
            match listener.recv_timeout(RECV_TIMEOUT) {
                Ok(key_event) => {
                    if let Ok(mut state) = state.lock() {
                        let hotkey_events = state.process_event(&key_event);
                        for event in hotkey_events {
                            if sender.send(event).is_err() {
                                // Receiver dropped, exit
                                return;
                            }
                        }
                    }
                }
                Err(crate::error::Error::Timeout) => {
                    // No event received, loop continues to check running flag
                }
                Err(_) => {
                    // Listener disconnected, exit
                    return;
                }
            }
        }
    }

    /// Register a hotkey and return its unique ID
    ///
    /// Returns an error if the hotkey is already registered.
    pub fn register(&self, hotkey: Hotkey) -> Result<HotkeyId> {
        let mut state = self.state.lock().map_err(|_| Error::MutexPoisoned)?;

        // Check if already registered
        for (id, existing) in &state.hotkeys {
            if existing == &hotkey {
                return Err(Error::HotkeyAlreadyRegistered(format!(
                    "{} (id: {:?})",
                    hotkey, id
                )));
            }
        }

        let id = HotkeyId(state.next_id);
        state.next_id += 1;
        state.hotkeys.insert(id, hotkey);

        // Add to blocking set
        if let Some(blocking_hotkeys) = &self.blocking_hotkeys {
            if let Ok(mut blocking) = blocking_hotkeys.lock() {
                blocking.insert(hotkey);
            }
        }

        Ok(id)
    }

    /// Unregister a hotkey by its ID
    ///
    /// Returns an error if the hotkey ID is not found.
    pub fn unregister(&self, id: HotkeyId) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::MutexPoisoned)?;

        let hotkey = state.hotkeys.remove(&id);
        if hotkey.is_none() {
            return Err(Error::HotkeyNotFound(id));
        }

        // Remove from blocking set
        if let Some(blocking_hotkeys) = &self.blocking_hotkeys {
            if let Some(hotkey) = hotkey {
                if let Ok(mut blocking) = blocking_hotkeys.lock() {
                    blocking.remove(&hotkey);
                }
            }
        }

        Ok(())
    }

    /// Get the hotkey definition associated with an ID
    ///
    /// Returns `None` if the ID is not found.
    pub fn get_hotkey(&self, id: HotkeyId) -> Option<Hotkey> {
        let state = self.state.lock().ok()?;
        state.hotkeys.get(&id).copied()
    }

    /// Blocking receive for hotkey events
    ///
    /// Blocks until a hotkey event is received or the event loop stops.
    pub fn recv(&self) -> Result<HotkeyEvent> {
        self.event_receiver
            .recv()
            .map_err(|_| Error::EventLoopNotRunning)
    }

    /// Non-blocking receive for hotkey events
    ///
    /// Returns `Some(event)` if an event is available, `None` otherwise.
    pub fn try_recv(&self) -> Option<HotkeyEvent> {
        match self.event_receiver.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Get the number of currently registered hotkeys
    pub fn hotkey_count(&self) -> usize {
        let state = if let Ok(s) = self.state.lock() {
            s
        } else {
            return 0;
        };
        state.hotkeys.len()
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        // Join the thread to ensure clean shutdown
        if let Some(handle) = self._thread_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Key, Modifiers};

    fn make_key_event(modifiers: Modifiers, key: Option<Key>, is_key_down: bool) -> KeyEvent {
        KeyEvent {
            modifiers,
            key,
            is_key_down,
            changed_modifier: None,
        }
    }

    fn make_modifier_event(
        modifiers: Modifiers,
        is_key_down: bool,
        changed: Modifiers,
    ) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: None,
            is_key_down,
            changed_modifier: Some(changed),
        }
    }

    mod manager_state {
        use super::*;

        #[test]
        fn register_and_lookup_hotkey() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();

            let id = HotkeyId(state.next_id);
            state.next_id += 1;
            state.hotkeys.insert(id, hotkey);

            assert_eq!(state.hotkeys.get(&id), Some(&hotkey));
            assert_eq!(state.hotkeys.len(), 1);
        }

        #[test]
        fn hotkey_press_generates_event() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Simulate Cmd+K key down (event uses side-specific modifier)
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, id);
            assert_eq!(results[0].state, HotkeyState::Pressed);
            assert!(state.pressed_hotkeys.contains(&id));
        }

        #[test]
        fn hotkey_release_generates_event() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Press first
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            state.process_event(&event);

            // Then release the key
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), false);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, id);
            assert_eq!(results[0].state, HotkeyState::Released);
            assert!(!state.pressed_hotkeys.contains(&id));
        }

        #[test]
        fn no_duplicate_press_events() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Press once
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);
            assert_eq!(results.len(), 1);

            // Press again (key repeat) - should not generate another event
            let results = state.process_event(&event);
            assert_eq!(results.len(), 0);
        }

        #[test]
        fn modifier_release_triggers_hotkey_release() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Press Cmd+K
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            state.process_event(&event);
            assert!(state.pressed_hotkeys.contains(&id));

            // Release Cmd (while K is still held) - modifier event
            let event = make_modifier_event(Modifiers::empty(), false, Modifiers::CMD_LEFT);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].state, HotkeyState::Released);
            assert!(!state.pressed_hotkeys.contains(&id));
        }

        #[test]
        fn wrong_modifiers_dont_trigger() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            state.hotkeys.insert(HotkeyId(0), hotkey);

            // Press Shift+K instead of Cmd+K
            let event = make_key_event(Modifiers::SHIFT_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 0);
        }

        #[test]
        fn modifier_only_hotkey() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD | Modifiers::SHIFT, None).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Press Cmd+Shift (no key) — events use side-specific modifiers
            let event = make_modifier_event(
                Modifiers::CMD_LEFT | Modifiers::SHIFT_LEFT,
                true,
                Modifiers::SHIFT_LEFT,
            );
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].state, HotkeyState::Pressed);
        }

        #[test]
        fn multiple_hotkeys_same_key() {
            let mut state = ManagerState::new();

            // Cmd+K and Ctrl+K
            let hotkey1 = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let hotkey2 = Hotkey::new(Modifiers::CTRL, Key::K).unwrap();
            let id1 = HotkeyId(0);
            let id2 = HotkeyId(1);
            state.hotkeys.insert(id1, hotkey1);
            state.hotkeys.insert(id2, hotkey2);

            // Press Cmd+K
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, id1);

            // Press Ctrl+K (release Cmd first)
            state.pressed_hotkeys.clear();
            let event = make_key_event(Modifiers::CTRL_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, id2);
        }

        #[test]
        fn key_only_hotkey() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::empty(), Key::F1).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Press F1 with no modifiers
            let event = make_key_event(Modifiers::empty(), Some(Key::F1), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].state, HotkeyState::Pressed);

            // F1 with modifiers should NOT trigger
            state.pressed_hotkeys.clear();
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::F1), true);
            let results = state.process_event(&event);

            assert_eq!(results.len(), 0);
        }

        #[test]
        fn side_specific_hotkey_matches_correct_side() {
            let mut state = ManagerState::new();
            // Register CtrlRight+Space
            let hotkey = Hotkey::new(Modifiers::CTRL_RIGHT, Key::Space).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Left ctrl should not trigger
            let event = make_key_event(Modifiers::CTRL_LEFT, Some(Key::Space), true);
            assert_eq!(state.process_event(&event).len(), 0);

            // Right ctrl should trigger
            let event = make_key_event(Modifiers::CTRL_RIGHT, Some(Key::Space), true);
            let results = state.process_event(&event);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].state, HotkeyState::Pressed);
        }

        #[test]
        fn compound_hotkey_matches_either_side() {
            let mut state = ManagerState::new();
            let hotkey = Hotkey::new(Modifiers::CMD, Key::K).unwrap();
            let id = HotkeyId(0);
            state.hotkeys.insert(id, hotkey);

            // Left Cmd triggers
            let event = make_key_event(Modifiers::CMD_LEFT, Some(Key::K), true);
            let results = state.process_event(&event);
            assert_eq!(results.len(), 1);

            // Release
            state.pressed_hotkeys.clear();

            // Right Cmd also triggers
            let event = make_key_event(Modifiers::CMD_RIGHT, Some(Key::K), true);
            let results = state.process_event(&event);
            assert_eq!(results.len(), 1);
        }
    }
}
