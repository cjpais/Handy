//! Keyboard shortcut handling using keyboard_shortcuts library
//!
//! This module wraps the keyboard_shortcuts crate to provide:
//! - Global hotkey registration and event handling
//! - Keyboard recording for the settings UI

use keyboard_shortcuts::{Hotkey, HotkeyId, HotkeyManager, HotkeyManagerExt, HotkeyState, KeyboardListener, KeyEvent};
use log::{debug, error, info, warn};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::{self, ShortcutBinding};
use crate::ManagedToggleState;

/// Commands to send to the keyboard manager thread
#[allow(dead_code)]
enum KeyboardCommand {
    Register {
        binding_id: String,
        shortcut_str: String,
        response: mpsc::Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: mpsc::Sender<Result<(), String>>,
    },
    IsRegistered {
        binding_id: String,
        response: mpsc::Sender<bool>,
    },
    Shutdown,
}

/// Handle to communicate with the keyboard manager thread
pub struct KeyboardManagerHandle {
    sender: mpsc::Sender<KeyboardCommand>,
}

impl KeyboardManagerHandle {
    /// Register a shortcut
    pub fn register(&self, binding_id: &str, shortcut_str: &str) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(KeyboardCommand::Register {
                binding_id: binding_id.to_string(),
                shortcut_str: shortcut_str.to_string(),
                response: tx,
            })
            .map_err(|_| "Keyboard manager thread not running".to_string())?;
        rx.recv().map_err(|_| "Keyboard manager thread died".to_string())?
    }

    /// Unregister a shortcut
    pub fn unregister(&self, binding_id: &str) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(KeyboardCommand::Unregister {
                binding_id: binding_id.to_string(),
                response: tx,
            })
            .map_err(|_| "Keyboard manager thread not running".to_string())?;
        rx.recv().map_err(|_| "Keyboard manager thread died".to_string())?
    }

    /// Check if a binding is registered
    pub fn is_registered(&self, binding_id: &str) -> bool {
        let (tx, rx) = mpsc::channel();
        if self
            .sender
            .send(KeyboardCommand::IsRegistered {
                binding_id: binding_id.to_string(),
                response: tx,
            })
            .is_err()
        {
            return false;
        }
        rx.recv().unwrap_or(false)
    }
}

/// State for keyboard recording
pub struct KeyboardRecordingState {
    /// Current binding being recorded
    current_binding_id: Option<String>,
    /// Flag to stop recording
    recording: Arc<AtomicBool>,
}

impl Default for KeyboardRecordingState {
    fn default() -> Self {
        Self {
            current_binding_id: None,
            recording: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Event sent to frontend during keyboard recording
#[derive(Debug, Clone, Serialize, Type)]
pub struct KeyboardRecordEvent {
    pub binding_id: String,
    pub modifiers: Vec<String>,
    pub key: Option<String>,
    pub current_combo: String,
}

/// Event sent when recording is complete
#[derive(Debug, Clone, Serialize, Type)]
pub struct KeyboardRecordingComplete {
    pub binding_id: String,
    pub shortcut: String,
}

/// Managed state for keyboard recording
pub type ManagedKeyboardRecordingState = Mutex<KeyboardRecordingState>;

/// Convert a KeyEvent to Handy-compatible key strings
fn key_event_to_handy_parts(event: &KeyEvent) -> (Vec<String>, Option<String>) {
    use keyboard_shortcuts::Modifiers;

    let mut modifiers = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if event.modifiers.contains(Modifiers::CTRL) {
            modifiers.push("ctrl".to_string());
        }
        if event.modifiers.contains(Modifiers::OPT) {
            modifiers.push("option".to_string());
        }
        if event.modifiers.contains(Modifiers::SHIFT) {
            modifiers.push("shift".to_string());
        }
        if event.modifiers.contains(Modifiers::CMD) {
            modifiers.push("command".to_string());
        }
        if event.modifiers.contains(Modifiers::FN) {
            modifiers.push("fn".to_string());
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if event.modifiers.contains(Modifiers::CTRL) {
            modifiers.push("ctrl".to_string());
        }
        if event.modifiers.contains(Modifiers::OPT) {
            modifiers.push("alt".to_string());
        }
        if event.modifiers.contains(Modifiers::SHIFT) {
            modifiers.push("shift".to_string());
        }
        if event.modifiers.contains(Modifiers::CMD) {
            modifiers.push("super".to_string());
        }
    }

    let key = event.key.map(|k| k.to_string().to_lowercase());

    (modifiers, key)
}

/// Start keyboard recording for a binding
#[tauri::command]
#[specta::specta]
pub fn start_keyboard_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    let recording_state = app.state::<ManagedKeyboardRecordingState>();
    let mut state = recording_state.lock().unwrap();

    // Check if already recording
    if state.current_binding_id.is_some() {
        return Err("Already recording a shortcut".to_string());
    }

    state.current_binding_id = Some(binding_id.clone());
    state.recording.store(true, Ordering::Relaxed);

    let recording = state.recording.clone();
    let app_clone = app.clone();
    let binding_id_clone = binding_id.clone();

    // Drop the lock before spawning the thread
    drop(state);

    // Start polling in a background thread
    thread::spawn(move || {
        // Create a new keyboard listener for this recording session
        let listener = match KeyboardListener::new() {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to create keyboard listener: {}", e);
                return;
            }
        };

        let mut all_pressed: Vec<String> = Vec::new();

        while recording.load(Ordering::Relaxed) {
            // Try to receive a key event
            if let Some(event) = listener.try_recv() {
                let (modifiers, key) = key_event_to_handy_parts(&event);

                if event.is_key_down {
                    // Build current combo from all pressed keys
                    all_pressed = modifiers.clone();
                    if let Some(ref k) = key {
                        if !all_pressed.contains(k) {
                            all_pressed.push(k.clone());
                        }
                    }

                    let current_combo = all_pressed.join("+");

                    // Emit key-down event
                    let event_data = KeyboardRecordEvent {
                        binding_id: binding_id_clone.clone(),
                        modifiers,
                        key,
                        current_combo,
                    };
                    let _ = app_clone.emit("keyboard:key-down", event_data);
                } else {
                    // Key up - check if all keys are released
                    if modifiers.is_empty() && key.is_none() {
                        // All keys released - complete the recording
                        if !all_pressed.is_empty() {
                            let shortcut = all_pressed.join("+");
                            let complete_event = KeyboardRecordingComplete {
                                binding_id: binding_id_clone.clone(),
                                shortcut,
                            };
                            let _ = app_clone.emit("keyboard:recording-complete", complete_event);

                            // Stop recording
                            recording.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(5));
        }

        // Clean up the recording state
        if let Some(recording_state) = app_clone.try_state::<ManagedKeyboardRecordingState>() {
            let mut state = recording_state.lock().unwrap();
            if state.current_binding_id.as_ref() == Some(&binding_id_clone) {
                state.current_binding_id = None;
            }
        }

        info!("Keyboard recording thread stopped for binding '{}'", binding_id_clone);
    });

    info!("Started keyboard recording for binding '{}'", binding_id);
    Ok(())
}

/// Stop keyboard recording
#[tauri::command]
#[specta::specta]
pub fn stop_keyboard_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    let recording_state = app.state::<ManagedKeyboardRecordingState>();
    let mut state = recording_state.lock().unwrap();

    if state.current_binding_id.as_ref() != Some(&binding_id) {
        return Err(format!("Not currently recording binding '{}'", binding_id));
    }

    state.recording.store(false, Ordering::Relaxed);
    state.current_binding_id = None;

    info!("Stopped keyboard recording for binding '{}'", binding_id);
    Ok(())
}

/// Cancel keyboard recording and restore original binding
#[tauri::command]
#[specta::specta]
pub fn cancel_keyboard_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    stop_keyboard_recording(app.clone(), binding_id.clone())?;

    // Emit cancellation event
    let _ = app.emit("keyboard:recording-cancelled", serde_json::json!({
        "binding_id": binding_id
    }));

    Ok(())
}

/// Start the keyboard manager thread and return a handle to communicate with it
pub fn start_keyboard_manager(app: &AppHandle) -> KeyboardManagerHandle {
    let (tx, rx) = mpsc::channel::<KeyboardCommand>();
    let app_clone = app.clone();

    thread::spawn(move || {
        // Create the hotkey manager
        let hotkey_manager = match HotkeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to create HotkeyManager: {}", e);
                return;
            }
        };

        // Maps for tracking registrations
        let mut binding_to_hotkey: HashMap<String, HotkeyId> = HashMap::new();
        let mut hotkey_to_binding: HashMap<HotkeyId, String> = HashMap::new();
        let mut hotkey_to_string: HashMap<HotkeyId, String> = HashMap::new();

        info!("Keyboard manager thread started");

        // Shared maps for tracking registrations across command and event handling
        let hotkey_to_binding_shared = Arc::new(Mutex::new(HashMap::<HotkeyId, String>::new()));
        let hotkey_to_string_shared = Arc::new(Mutex::new(HashMap::<HotkeyId, String>::new()));

        // Main command processing loop
        loop {
            // Check for commands (non-blocking with timeout)
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(cmd) => match cmd {
                    KeyboardCommand::Register {
                        binding_id,
                        shortcut_str,
                        response,
                    } => {
                        let result = (|| {
                            // Parse the shortcut
                            let hotkey: Hotkey = shortcut_str.parse().map_err(|e| {
                                format!("Failed to parse shortcut '{}': {}", shortcut_str, e)
                            })?;

                            // Check if already registered
                            if binding_to_hotkey.contains_key(&binding_id) {
                                return Err(format!("Binding '{}' is already registered", binding_id));
                            }

                            // Register
                            let hotkey_id = hotkey_manager.register(hotkey).map_err(|e| {
                                format!("Failed to register shortcut: {}", e)
                            })?;

                            // Store mappings
                            binding_to_hotkey.insert(binding_id.clone(), hotkey_id);
                            hotkey_to_binding.insert(hotkey_id, binding_id.clone());
                            hotkey_to_string.insert(hotkey_id, shortcut_str.clone());

                            // Update the shared maps for the event thread
                            hotkey_to_binding_shared.lock().unwrap().insert(hotkey_id, binding_id.clone());
                            hotkey_to_string_shared.lock().unwrap().insert(hotkey_id, shortcut_str.clone());

                            info!("Registered shortcut '{}' for binding '{}'", shortcut_str, binding_id);
                            Ok(())
                        })();
                        let _ = response.send(result);
                    }
                    KeyboardCommand::Unregister {
                        binding_id,
                        response,
                    } => {
                        let result = (|| {
                            let hotkey_id = binding_to_hotkey.get(&binding_id).copied().ok_or_else(|| {
                                format!("Binding '{}' is not registered", binding_id)
                            })?;

                            hotkey_manager.unregister(hotkey_id).map_err(|e| {
                                format!("Failed to unregister shortcut: {}", e)
                            })?;

                            binding_to_hotkey.remove(&binding_id);
                            hotkey_to_binding.remove(&hotkey_id);
                            hotkey_to_string.remove(&hotkey_id);

                            // Update the shared maps
                            hotkey_to_binding_shared.lock().unwrap().remove(&hotkey_id);
                            hotkey_to_string_shared.lock().unwrap().remove(&hotkey_id);

                            info!("Unregistered shortcut for binding '{}'", binding_id);
                            Ok(())
                        })();
                        let _ = response.send(result);
                    }
                    KeyboardCommand::IsRegistered {
                        binding_id,
                        response,
                    } => {
                        let _ = response.send(binding_to_hotkey.contains_key(&binding_id));
                    }
                    KeyboardCommand::Shutdown => {
                        info!("Keyboard manager thread shutting down");
                        break;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Check for hotkey events
                    while let Some(event) = hotkey_manager.try_recv() {
                        // Look up binding info
                        let binding_id = hotkey_to_binding.get(&event.id).cloned();
                        let shortcut_str = hotkey_to_string.get(&event.id).cloned();

                        if let (Some(binding_id), Some(shortcut_str)) = (binding_id, shortcut_str) {
                            handle_hotkey_event(&app_clone, &binding_id, &shortcut_str, event.state);
                        } else {
                            warn!("Received event for unknown hotkey ID: {:?}", event.id);
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    info!("Keyboard manager command channel disconnected");
                    break;
                }
            }
        }

        info!("Keyboard manager thread stopped");
    });

    KeyboardManagerHandle { sender: tx }
}

/// Handle a hotkey event
fn handle_hotkey_event(app: &AppHandle, binding_id: &str, shortcut_str: &str, state: HotkeyState) {
    debug!("Hotkey event: binding='{}', shortcut='{}', state={:?}", binding_id, shortcut_str, state);

    // Look up the action
    if let Some(action) = ACTION_MAP.get(binding_id) {
        let settings = settings::get_settings(app);

        if binding_id == "cancel" {
            // Cancel only fires while recording
            let audio_manager = app.state::<Arc<AudioRecordingManager>>();
            if audio_manager.is_recording() && state == HotkeyState::Pressed {
                action.start(app, binding_id, shortcut_str);
            }
        } else if settings.push_to_talk {
            // Push-to-talk mode
            match state {
                HotkeyState::Pressed => action.start(app, binding_id, shortcut_str),
                HotkeyState::Released => action.stop(app, binding_id, shortcut_str),
            }
        } else {
            // Toggle mode: toggle on press only
            if state == HotkeyState::Pressed {
                let should_start: bool;
                {
                    let toggle_state_manager = app.state::<ManagedToggleState>();
                    let mut states = toggle_state_manager
                        .lock()
                        .expect("Failed to lock toggle state manager");

                    let is_currently_active = states
                        .active_toggles
                        .entry(binding_id.to_string())
                        .or_insert(false);

                    should_start = !*is_currently_active;
                    *is_currently_active = should_start;
                }

                if should_start {
                    action.start(app, binding_id, shortcut_str);
                } else {
                    action.stop(app, binding_id, shortcut_str);
                }
            }
        }
    } else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', State: {:?}",
            binding_id, shortcut_str, state
        );
    }
}

/// Initialize shortcuts from settings
pub fn init_shortcuts(app: &AppHandle) {
    let keyboard_handle = app.state::<KeyboardManagerHandle>();
    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    // Register all default shortcuts, applying user customizations
    for (id, default_binding) in default_bindings {
        if id == "cancel" {
            continue; // Skip cancel shortcut, it will be registered dynamically
        }
        let binding = user_settings
            .bindings
            .get(&id)
            .cloned()
            .unwrap_or(default_binding);

        if let Err(e) = keyboard_handle.register(&id, &binding.current_binding) {
            error!("Failed to register shortcut {} during init: {}", id, e);
        }
    }
}

/// Register a shortcut from a ShortcutBinding
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let keyboard_handle = app.state::<KeyboardManagerHandle>();

    // Check if already registered
    if keyboard_handle.is_registered(&binding.id) {
        return Err(format!("Shortcut '{}' is already registered", binding.current_binding));
    }

    keyboard_handle.register(&binding.id, &binding.current_binding)
}

/// Unregister a shortcut
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let keyboard_handle = app.state::<KeyboardManagerHandle>();
    keyboard_handle.unregister(&binding.id)
}

/// Register the cancel shortcut (dynamically when recording starts)
pub fn register_cancel_shortcut(app: &AppHandle) {
    // Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = settings::get_settings(&app_clone).bindings.get("cancel").cloned() {
                let keyboard_handle = app_clone.state::<KeyboardManagerHandle>();
                if let Err(e) = keyboard_handle.register(&cancel_binding.id, &cancel_binding.current_binding) {
                    error!("Failed to register cancel shortcut: {}", e);
                }
            }
        });
    }
}

/// Unregister the cancel shortcut
pub fn unregister_cancel_shortcut(app: &AppHandle) {
    // Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = settings::get_settings(&app_clone).bindings.get("cancel").cloned() {
                let keyboard_handle = app_clone.state::<KeyboardManagerHandle>();
                // We ignore errors here as it might already be unregistered
                let _ = keyboard_handle.unregister(&cancel_binding.id);
            }
        });
    }
}

/// Start the hotkey event processing - this is now handled by the keyboard manager thread
/// This function is kept for API compatibility but does nothing as processing is integrated
pub fn start_hotkey_processing(_app: &AppHandle) {
    // Processing is now done in the keyboard manager thread
}
