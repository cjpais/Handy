//! Handy-keys based keyboard shortcut implementation
//!
//! This module provides an alternative to Tauri's global-shortcut plugin
//! using the handy-keys library for more control over keyboard events.
//!
//! ## Architecture
//!
//! The implementation uses a dedicated manager thread that owns the keyboard listener:
//!
//! ```text
//! ┌─────────────────┐     commands      ┌──────────────────────┐
//! │   Main Thread   │ ───────────────▶ │   Manager Thread     │
//! │                 │   (via channel)   │                      │
//! │ - register()    │                   │ - owns listener      │
//! │ - unregister()  │                   │ - polls for events   │
//! └─────────────────┘                   │ - dispatches actions │
//!                                       └──────────────────────┘
//! ```
//!
//! This design ensures thread-safety since keyboard state is only accessed
//! from a single thread. Commands (register/unregister) are sent via an mpsc
//! channel and responses are synchronously awaited.
//!
//! ## Recording Mode
//!
//! For UI key capture, a separate `KeyboardListener` is created on-demand and
//! polled from a dedicated recording thread. Events are emitted to the frontend
//! via Tauri's event system.

use handy_keys::{Hotkey, KeyEvent, KeyboardListener};
use log::{debug, error, info};
use serde::Serialize;
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{self, get_settings, ShortcutBinding};
use crate::transcription_coordinator::{is_transcribe_binding, TranscriptionCoordinator};

use super::handler::handle_shortcut_event;

/// Commands that can be sent to the hotkey manager thread
enum ManagerCommand {
    Register {
        binding_id: String,
        hotkey_string: String,
        response: Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: Sender<Result<(), String>>,
    },
    Shutdown,
}

/// State for the handy-keys shortcut manager
pub struct HandyKeysState {
    /// Channel to send commands to the manager thread (wrapped in Mutex for Sync)
    command_sender: Mutex<Sender<ManagerCommand>>,
    /// Handle to the manager thread (wrapped in Mutex for Sync, allows proper join on drop)
    thread_handle: Mutex<Option<JoinHandle<()>>>,
    /// Recording listener for UI key capture (only active during recording)
    recording_listener: Mutex<Option<KeyboardListener>>,
    /// Flag indicating if we're in recording mode
    is_recording: AtomicBool,
    /// The binding ID being recorded (if any)
    recording_binding_id: Mutex<Option<String>>,
    /// Flag to stop recording loop
    recording_running: Arc<AtomicBool>,
}

/// Key event sent to frontend during recording mode
#[derive(Debug, Clone, Serialize, Type)]
pub struct FrontendKeyEvent {
    /// Currently pressed modifier keys
    pub modifiers: Vec<String>,
    /// The key that was pressed (if any)
    pub key: Option<String>,
    /// Whether this is a key down event
    pub is_key_down: bool,
    /// The full hotkey string (e.g., "option+space")
    pub hotkey_string: String,
}

impl HandyKeysState {
    /// Create a new HandyKeysState
    pub fn new(app: AppHandle) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCommand>();

        // Start the manager thread
        let app_clone = app.clone();
        let thread_handle = thread::spawn(move || {
            Self::manager_thread(cmd_rx, app_clone);
        });

        Ok(Self {
            command_sender: Mutex::new(cmd_tx),
            thread_handle: Mutex::new(Some(thread_handle)),
            recording_listener: Mutex::new(None),
            is_recording: AtomicBool::new(false),
            recording_binding_id: Mutex::new(None),
            recording_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The main manager thread - owns the keyboard listener and processes commands
    fn manager_thread(cmd_rx: Receiver<ManagerCommand>, app: AppHandle) {
        info!("handy-keys manager thread started");

        // Observe rather than grab key events. Blocking a modifier-only shortcut
        // such as `fn` prevents normal macOS chords like `fn+delete` from reaching
        // the focused application.
        let listener = match KeyboardListener::new() {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to create KeyboardListener: {}", e);
                return;
            }
        };

        // Track registered hotkeys and the bindings currently held down.
        let mut binding_to_hotkey: HashMap<String, Hotkey> = HashMap::new();
        let mut binding_to_hotkey_string: HashMap<String, String> = HashMap::new();
        let mut pressed_bindings: HashSet<String> = HashSet::new();
        let mut active_fn_transcribe_binding: Option<String> = None;
        let mut suppress_fn_transcribe_until_release = false;

        loop {
            // Check for raw key events (non-blocking).
            while let Some(event) = listener.try_recv() {
                Self::process_key_event(
                    &app,
                    &event,
                    &binding_to_hotkey,
                    &binding_to_hotkey_string,
                    &mut pressed_bindings,
                    &mut active_fn_transcribe_binding,
                    &mut suppress_fn_transcribe_until_release,
                );
            }

            // Check for commands (non-blocking with timeout)
            match cmd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(cmd) => match cmd {
                    ManagerCommand::Register {
                        binding_id,
                        hotkey_string,
                        response,
                    } => {
                        let result = Self::do_register(
                            &mut binding_to_hotkey,
                            &mut binding_to_hotkey_string,
                            &binding_id,
                            &hotkey_string,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Unregister {
                        binding_id,
                        response,
                    } => {
                        let result = Self::do_unregister(
                            &mut binding_to_hotkey,
                            &mut binding_to_hotkey_string,
                            &mut pressed_bindings,
                            &mut active_fn_transcribe_binding,
                            &mut suppress_fn_transcribe_until_release,
                            &binding_id,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Shutdown => {
                        info!("handy-keys manager thread shutting down");
                        break;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // No command, continue
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    info!("Command channel disconnected, shutting down");
                    break;
                }
            }
        }

        info!("handy-keys manager thread stopped");
    }

    /// Process a raw event against the registered hotkeys without consuming it.
    fn process_key_event(
        app: &AppHandle,
        event: &KeyEvent,
        binding_to_hotkey: &HashMap<String, Hotkey>,
        binding_to_hotkey_string: &HashMap<String, String>,
        pressed_bindings: &mut HashSet<String>,
        active_fn_transcribe_binding: &mut Option<String>,
        suppress_fn_transcribe_until_release: &mut bool,
    ) {
        if *suppress_fn_transcribe_until_release {
            if !event.modifiers.contains(handy_keys::Modifiers::FN) {
                debug!("Clearing suppressed fn transcription chord after fn release");
                *suppress_fn_transcribe_until_release = false;
            }
            return;
        }

        if should_cancel_active_fn(event, active_fn_transcribe_binding.is_some()) {
            debug!("Cancelling fn transcription because another key was pressed");
            if let Some(binding_id) = active_fn_transcribe_binding.take() {
                pressed_bindings.remove(&binding_id);
            }
            *suppress_fn_transcribe_until_release = true;
            request_coordinated_cancel(app);
            return;
        }

        if event.is_key_down {
            let to_press: Vec<String> = binding_to_hotkey
                .iter()
                .filter(|(binding_id, hotkey)| {
                    hotkey.modifiers.matches(event.modifiers)
                        && hotkey.key == event.key
                        && !pressed_bindings.contains(*binding_id)
                })
                .map(|(binding_id, _)| binding_id.clone())
                .collect();

            for binding_id in to_press {
                pressed_bindings.insert(binding_id.clone());

                if let Some(hotkey_string) = binding_to_hotkey_string.get(&binding_id) {
                    debug!(
                        "handy-keys event: binding={}, hotkey={}, state=Pressed",
                        binding_id, hotkey_string
                    );
                    if is_fn_transcribe_binding(&binding_id, hotkey_string) {
                        *active_fn_transcribe_binding = Some(binding_id.clone());
                    }
                    handle_shortcut_event(app, &binding_id, hotkey_string, true);
                }
            }
        } else {
            let to_release: Vec<String> = binding_to_hotkey
                .iter()
                .filter(|(binding_id, hotkey)| {
                    pressed_bindings.contains(*binding_id)
                        && (hotkey.key == event.key
                            || (event.key.is_none() && !hotkey.modifiers.matches(event.modifiers)))
                })
                .map(|(binding_id, _)| binding_id.clone())
                .collect();

            for binding_id in to_release {
                pressed_bindings.remove(&binding_id);

                if active_fn_transcribe_binding.as_deref() == Some(&binding_id) {
                    *active_fn_transcribe_binding = None;
                }

                if let Some(hotkey_string) = binding_to_hotkey_string.get(&binding_id) {
                    debug!(
                        "handy-keys event: binding={}, hotkey={}, state=Released",
                        binding_id, hotkey_string
                    );
                    handle_shortcut_event(app, &binding_id, hotkey_string, false);
                }
            }
        }
    }

    /// Register a hotkey
    fn do_register(
        binding_to_hotkey: &mut HashMap<String, Hotkey>,
        binding_to_hotkey_string: &mut HashMap<String, String>,
        binding_id: &str,
        hotkey_string: &str,
    ) -> Result<(), String> {
        let hotkey: Hotkey = hotkey_string
            .parse()
            .map_err(|e| format!("Failed to parse hotkey '{}': {}", hotkey_string, e))?;

        if binding_to_hotkey
            .values()
            .any(|existing| existing == &hotkey)
        {
            return Err(format!("Hotkey already registered: {}", hotkey_string));
        }

        binding_to_hotkey.insert(binding_id.to_string(), hotkey);
        binding_to_hotkey_string.insert(binding_id.to_string(), hotkey_string.to_string());

        debug!(
            "Registered handy-keys shortcut: {} -> {:?}",
            binding_id, hotkey
        );
        Ok(())
    }

    /// Unregister a hotkey
    fn do_unregister(
        binding_to_hotkey: &mut HashMap<String, Hotkey>,
        binding_to_hotkey_string: &mut HashMap<String, String>,
        pressed_bindings: &mut HashSet<String>,
        active_fn_transcribe_binding: &mut Option<String>,
        suppress_fn_transcribe_until_release: &mut bool,
        binding_id: &str,
    ) -> Result<(), String> {
        if binding_to_hotkey.remove(binding_id).is_some() {
            binding_to_hotkey_string.remove(binding_id);
            pressed_bindings.remove(binding_id);
            if active_fn_transcribe_binding.as_deref() == Some(binding_id) {
                *active_fn_transcribe_binding = None;
            }
            if is_transcribe_binding(binding_id) {
                *suppress_fn_transcribe_until_release = false;
            }
            debug!("Unregistered handy-keys shortcut: {}", binding_id);
        }
        Ok(())
    }

    /// Register a shortcut binding
    pub fn register(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Register {
                binding_id: binding.id.clone(),
                hotkey_string: binding.current_binding.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send register command")?;

        rx.recv()
            .map_err(|_| "Failed to receive register response")?
    }

    /// Unregister a shortcut binding
    pub fn unregister(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Unregister {
                binding_id: binding.id.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send unregister command")?;

        rx.recv()
            .map_err(|_| "Failed to receive unregister response")?
    }

    /// Start recording mode for a specific binding
    pub fn start_recording(&self, app: &AppHandle, binding_id: String) -> Result<(), String> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err("Already recording".into());
        }

        // Create a new keyboard listener for recording
        let listener = KeyboardListener::new()
            .map_err(|e| format!("Failed to create keyboard listener: {}", e))?;

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = Some(listener);
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = Some(binding_id);
        }

        self.is_recording.store(true, Ordering::SeqCst);
        self.recording_running.store(true, Ordering::SeqCst);

        // Start a thread to emit key events to the frontend
        let app_clone = app.clone();
        let recording_running = Arc::clone(&self.recording_running);
        thread::spawn(move || {
            Self::recording_loop(app_clone, recording_running);
        });

        debug!("Started handy-keys recording mode");
        Ok(())
    }

    /// Recording loop - emits key events to frontend during recording
    fn recording_loop(app: AppHandle, running: Arc<AtomicBool>) {
        while running.load(Ordering::SeqCst) {
            let event = {
                let state = match app.try_state::<HandyKeysState>() {
                    Some(s) => s,
                    None => break,
                };
                let listener = state.recording_listener.lock().ok();
                listener.as_ref().and_then(|l| l.as_ref()?.try_recv())
            };

            if let Some(key_event) = event {
                // Convert to frontend-friendly format
                let frontend_event = FrontendKeyEvent {
                    modifiers: modifiers_to_strings(key_event.modifiers),
                    key: key_event.key.map(|k| k.to_string().to_lowercase()),
                    is_key_down: key_event.is_key_down,
                    hotkey_string: key_event
                        .as_hotkey()
                        .map(|h| h.to_handy_string())
                        .unwrap_or_default(),
                };

                // Emit to frontend
                if let Err(e) = app.emit("handy-keys-event", &frontend_event) {
                    error!("Failed to emit key event: {}", e);
                }
            } else {
                thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        debug!("Recording loop ended");
    }

    /// Stop recording mode
    pub fn stop_recording(&self) -> Result<(), String> {
        self.is_recording.store(false, Ordering::SeqCst);
        self.recording_running.store(false, Ordering::SeqCst);

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = None;
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = None;
        }

        debug!("Stopped handy-keys recording mode");
        Ok(())
    }
}

fn is_fn_transcribe_binding(binding_id: &str, hotkey_string: &str) -> bool {
    is_transcribe_binding(binding_id) && hotkey_string.eq_ignore_ascii_case("fn")
}

fn should_cancel_active_fn(event: &KeyEvent, fn_transcription_active: bool) -> bool {
    fn_transcription_active
        && event.is_key_down
        && event.key.is_some_and(|key| !is_mouse_key(key))
        && event.modifiers.contains(handy_keys::Modifiers::FN)
}

fn request_coordinated_cancel(app: &AppHandle) {
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.request_cancel_operation();
    } else {
        log::warn!("TranscriptionCoordinator is not initialized; fn combo cancel ignored");
    }
}

fn is_mouse_key(key: handy_keys::Key) -> bool {
    matches!(
        key,
        handy_keys::Key::MouseLeft
            | handy_keys::Key::MouseRight
            | handy_keys::Key::MouseMiddle
            | handy_keys::Key::MouseX1
            | handy_keys::Key::MouseX2
    )
}

impl Drop for HandyKeysState {
    fn drop(&mut self) {
        // Signal recording to stop
        self.recording_running.store(false, Ordering::SeqCst);
        self.is_recording.store(false, Ordering::SeqCst);

        // Send shutdown command
        if let Ok(sender) = self.command_sender.lock() {
            let _ = sender.send(ManagerCommand::Shutdown);
        }

        // Wait for the manager thread to finish
        if let Ok(mut handle) = self.thread_handle.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }
    }
}

/// Convert handy-keys Modifiers to a list of strings
fn modifiers_to_strings(modifiers: handy_keys::Modifiers) -> Vec<String> {
    let mut result = Vec::new();

    if modifiers.contains(handy_keys::Modifiers::CTRL) {
        result.push("ctrl".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::OPT) {
        #[cfg(target_os = "macos")]
        result.push("option".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("alt".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::SHIFT) {
        result.push("shift".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::CMD) {
        #[cfg(target_os = "macos")]
        result.push("command".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("super".to_string());
    }
    if modifiers.contains(handy_keys::Modifiers::FN) {
        result.push("fn".to_string());
    }

    result
}

/// Validate a shortcut string for the HandyKeys implementation.
/// HandyKeys is more permissive: allows modifier-only combos and the fn key.
pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    // HandyKeys accepts modifier-only, key-only, and modifier+key combos
    // Just verify the string is parseable
    raw.parse::<Hotkey>()
        .map(|_| ())
        .map_err(|e| format!("Invalid shortcut for HandyKeys: {}", e))
}

/// Initialize handy-keys shortcuts
pub fn init_shortcuts(app: &AppHandle) -> Result<(), String> {
    let state = HandyKeysState::new(app.clone())?;

    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    // Register all bindings except cancel (which is dynamic)
    for (id, default_binding) in default_bindings {
        if id == "cancel" {
            continue;
        }
        // Skip post-processing shortcut when the feature is disabled
        if id == "transcribe_with_post_process" && !user_settings.post_process_enabled {
            continue;
        }

        let binding = user_settings
            .bindings
            .get(&id)
            .cloned()
            .unwrap_or(default_binding);

        if let Err(e) = state.register(&binding) {
            error!(
                "Failed to register handy-keys shortcut {} during init: {}",
                id, e
            );
        }
    }

    app.manage(state);
    info!("handy-keys shortcuts initialized");
    Ok(())
}

/// Register the cancel shortcut (called when recording starts)
pub fn register_cancel_shortcut(app: &AppHandle) {
    // Disabled on Linux due to instability
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                if let Some(state) = app_clone.try_state::<HandyKeysState>() {
                    if let Err(e) = state.register(&cancel_binding) {
                        error!("Failed to register cancel shortcut: {}", e);
                    }
                }
            }
        });
    }
}

/// Unregister the cancel shortcut (called when recording stops)
pub fn unregister_cancel_shortcut(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                if let Some(state) = app_clone.try_state::<HandyKeysState>() {
                    let _ = state.unregister(&cancel_binding);
                }
            }
        });
    }
}

/// Register a shortcut
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<HandyKeysState>()
        .ok_or("HandyKeysState not initialized")?;
    state.register(&binding)
}

/// Unregister a shortcut
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<HandyKeysState>()
        .ok_or("HandyKeysState not initialized")?;
    state.unregister(&binding)
}

/// Start key recording mode
#[tauri::command]
#[specta::specta]
pub fn start_handy_keys_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::HandyKeys {
        return Err("handy-keys is not the active keyboard implementation".into());
    }

    let state = app
        .try_state::<HandyKeysState>()
        .ok_or("HandyKeysState not initialized")?;
    state.start_recording(&app, binding_id)
}

/// Stop key recording mode
#[tauri::command]
#[specta::specta]
pub fn stop_handy_keys_recording(app: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::HandyKeys {
        return Err("handy-keys is not the active keyboard implementation".into());
    }

    let state = app
        .try_state::<HandyKeysState>()
        .ok_or("HandyKeysState not initialized")?;
    state.stop_recording()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_fn_detection_is_limited_to_transcription_bindings() {
        assert!(is_fn_transcribe_binding("transcribe", "fn"));
        assert!(is_fn_transcribe_binding(
            "transcribe_with_post_process",
            "FN"
        ));
        assert!(!is_fn_transcribe_binding("cancel", "fn"));
        assert!(!is_fn_transcribe_binding("transcribe", "fn+delete"));
    }

    #[test]
    fn mouse_buttons_do_not_cancel_bare_fn_dictation() {
        assert!(is_mouse_key(handy_keys::Key::MouseLeft));
        assert!(is_mouse_key(handy_keys::Key::MouseRight));
        assert!(!is_mouse_key(handy_keys::Key::Delete));
    }
}
