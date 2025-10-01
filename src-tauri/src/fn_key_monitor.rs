use rdev::{listen, Event, EventType, Key};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use crate::actions::ACTION_MAP;
use crate::settings;
use crate::ManagedToggleState;

pub struct FnKeyMonitor {
    fn_pressed: Arc<Mutex<bool>>,
}

impl FnKeyMonitor {
    pub fn new() -> Self {
        Self {
            fn_pressed: Arc::new(Mutex::new(false)),
        }
    }

    pub fn start_monitoring(&self, app_handle: AppHandle) -> Result<(), String> {
        let fn_pressed = Arc::clone(&self.fn_pressed);

        std::thread::spawn(move || {
            let callback = move |event: Event| {
                match event.event_type {
                    EventType::KeyPress(key) => {
                        match key {
                            // Detect Fn key variants on macOS
                            Key::Function | Key::Unknown(179) => {
                                if let Ok(mut pressed) = fn_pressed.lock() {
                                    if !*pressed {
                                        *pressed = true;
                                        let _ = app_handle.emit("fn-key-pressed", ());
                                        Self::handle_fn_shortcut(&app_handle, true);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    EventType::KeyRelease(key) => {
                        match key {
                            // Detect Fn key variants on macOS
                            Key::Function | Key::Unknown(179) => {
                                if let Ok(mut pressed) = fn_pressed.lock() {
                                    if *pressed {
                                        *pressed = false;
                                        let _ = app_handle.emit("fn-key-released", ());
                                        Self::handle_fn_shortcut(&app_handle, false);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            };

            // Start listening for events
            if let Err(error) = listen(callback) {
                eprintln!("Error starting rdev listener: {:?}", error);
            }
        });

        Ok(())
    }


    fn handle_fn_shortcut(app_handle: &AppHandle, is_pressed: bool) {
        let settings = settings::get_settings(app_handle);

        // Find bindings that use the fn key
        for (binding_id, binding) in settings.bindings.iter() {
            if binding.current_binding.contains("fn") {
                if let Some(action) = ACTION_MAP.get(binding_id) {
                    if settings.push_to_talk {
                        if is_pressed {
                            action.start(app_handle, binding_id, &binding.current_binding);
                        } else {
                            action.stop(app_handle, binding_id, &binding.current_binding);
                        }
                    } else {
                        if is_pressed {
                            let toggle_state_manager = app_handle.state::<ManagedToggleState>();
                            let mut states = toggle_state_manager.lock().expect("Failed to lock toggle state manager");
                            let is_currently_active = states.active_toggles
                                .entry(binding_id.clone())
                                .or_insert(false);

                            if *is_currently_active {
                                action.stop(app_handle, binding_id, &binding.current_binding);
                                *is_currently_active = false;
                            } else {
                                action.start(app_handle, binding_id, &binding.current_binding);
                                *is_currently_active = true;
                            }
                        }
                    }
                }
            }
        }
    }
}