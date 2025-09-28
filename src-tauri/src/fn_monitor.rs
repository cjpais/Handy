use once_cell::sync::Lazy;
use rdev::{listen, Event, EventType, Key};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::settings::ShortcutBinding;
use crate::shortcut::dispatch_binding_event;

pub(crate) fn register_fn_binding(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    ensure_listener_started();

    let mut state = MONITOR_STATE
        .lock()
        .map_err(|_| "Failed to lock Fn monitor state".to_string())?;

    state.bindings.insert(
        binding.id.clone(),
        FnBindingEntry {
            app_handle: app.clone(),
            binding_id: binding.id,
            shortcut_string: binding.current_binding,
        },
    );

    Ok(())
}

pub(crate) fn unregister_fn_binding(_app: &AppHandle, binding_id: &str) -> Result<(), String> {
    let mut state = MONITOR_STATE
        .lock()
        .map_err(|_| "Failed to lock Fn monitor state".to_string())?;

    if state.bindings.remove(binding_id).is_some() && state.bindings.is_empty() {
        state.fn_pressed = false;
    }

    Ok(())
}

fn ensure_listener_started() {
    if MONITOR_STARTED.load(Ordering::SeqCst) {
        return;
    }

    if MONITOR_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let state = Arc::clone(&MONITOR_STATE);
        thread::spawn(move || {
            let result = listen(move |event| handle_event(&state, event));
            if let Err(err) = result {
                eprintln!("Fn monitor failed: {:?}", err);
                MONITOR_STARTED.store(false, Ordering::SeqCst);
            }
        });
    }
}

fn handle_event(state: &Arc<Mutex<FnMonitorState>>, event: Event) {
    let maybe_state = match event.event_type {
        EventType::KeyPress(Key::Function) | EventType::KeyPress(Key::Unknown(63)) => {
            Some(ShortcutState::Pressed)
        }
        EventType::KeyRelease(Key::Function)
        | EventType::KeyRelease(Key::Unknown(63)) => Some(ShortcutState::Released),
        _ => None,
    };

    if let Some(shortcut_state) = maybe_state {
        broadcast_event(state, shortcut_state);
    }
}

fn broadcast_event(state: &Arc<Mutex<FnMonitorState>>, shortcut_state: ShortcutState) {
    let bindings: Vec<FnBindingEntry> = {
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                eprintln!("Fn monitor state poisoned");
                return;
            }
        };

        if guard.bindings.is_empty() {
            return;
        }

        match shortcut_state {
            ShortcutState::Pressed => {
                if guard.fn_pressed {
                    return;
                }
                guard.fn_pressed = true;
            }
            ShortcutState::Released => {
                if !guard.fn_pressed {
                    return;
                }
                guard.fn_pressed = false;
            }
        }

        guard.bindings.values().cloned().collect()
    };

    if bindings.is_empty() {
        return;
    }

    for binding in bindings {
        dispatch_binding_event(
            &binding.app_handle,
            &binding.binding_id,
            &binding.shortcut_string,
            shortcut_state,
        );
    }
}

#[derive(Clone)]
struct FnBindingEntry {
    app_handle: AppHandle,
    binding_id: String,
    shortcut_string: String,
}

#[derive(Default)]
struct FnMonitorState {
    bindings: HashMap<String, FnBindingEntry>,
    fn_pressed: bool,
}

static MONITOR_STATE: Lazy<Arc<Mutex<FnMonitorState>>> =
    Lazy::new(|| Arc::new(Mutex::new(FnMonitorState::default())));

static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
