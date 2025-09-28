use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
use once_cell::sync::Lazy;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::settings::ShortcutBinding;
use crate::shortcut::dispatch_binding_event;

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

#[derive(Default)]
struct FnMonitorHandle {
    monitor_token: Option<Retained<AnyObject>>,
    handler: Option<RcBlock<dyn Fn(NonNull<NSEvent>) + 'static>>,
}

static MONITOR_STATE: Lazy<Arc<Mutex<FnMonitorState>>> =
    Lazy::new(|| Arc::new(Mutex::new(FnMonitorState::default())));

static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static MONITOR_HANDLE: RefCell<FnMonitorHandle> = RefCell::new(FnMonitorHandle::default());
}

pub(crate) fn register_fn_binding(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    ensure_monitor_started(app)?;

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

fn ensure_monitor_started(app: &AppHandle) -> Result<(), String> {
    if MONITOR_STARTED.load(Ordering::SeqCst) {
        return Ok(());
    }

    let state = Arc::clone(&MONITOR_STATE);
    let (tx, rx) = mpsc::channel();

    let schedule_result = app.run_on_main_thread(move || {
        MONITOR_HANDLE.with(|handle_cell| {
            let mut handle = handle_cell.borrow_mut();
            if handle.monitor_token.is_some() {
                MONITOR_STARTED.store(true, Ordering::SeqCst);
                let _ = tx.send(Ok(()));
                return;
            }

            let state_for_handler = Arc::clone(&state);
            let handler = RcBlock::new(move |event: NonNull<NSEvent>| {
                let event_ref = unsafe { event.as_ref() };

                let event_type = unsafe { event_ref.r#type() };
                if event_type != NSEventType::FlagsChanged {
                    return;
                }

                let flags = unsafe { event_ref.modifierFlags() };
                process_modifier_flags(&state_for_handler, flags);
            });

            let monitor = unsafe {
                NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
                    NSEventMask::FlagsChanged,
                    &handler,
                )
            };

            match monitor {
                Some(token) => {
                    handle.monitor_token = Some(token);
                    handle.handler = Some(handler);
                    MONITOR_STARTED.store(true, Ordering::SeqCst);
                    let _ = tx.send(Ok(()));
                }
                None => {
                    handle.monitor_token = None;
                    handle.handler = None;
                    MONITOR_STARTED.store(false, Ordering::SeqCst);
                    let _ = tx.send(Err("Failed to install Fn accessibility monitor. Make sure Handy has Accessibility permission in System Settings.".to_string()));
                }
            }
        });
    });

    if let Err(err) = schedule_result {
        return Err(format!(
            "Failed to schedule Fn monitor on the macOS main thread: {}",
            err
        ));
    }

    rx.recv()
        .unwrap_or_else(|_| Err("Fn monitor setup did not complete".to_string()))
}

fn process_modifier_flags(state: &Arc<Mutex<FnMonitorState>>, flags: NSEventModifierFlags) {
    let is_pressed = flags.contains(NSEventModifierFlags::Function);

    let bindings: Vec<FnBindingEntry> = {
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                eprintln!("Fn monitor state poisoned");
                return;
            }
        };

        if guard.fn_pressed == is_pressed {
            return;
        }

        guard.fn_pressed = is_pressed;

        if guard.bindings.is_empty() {
            return;
        }

        guard.bindings.values().cloned().collect()
    };

    let shortcut_state = if is_pressed {
        ShortcutState::Pressed
    } else {
        ShortcutState::Released
    };

    for binding in bindings {
        dispatch_binding_event(
            &binding.app_handle,
            &binding.binding_id,
            &binding.shortcut_string,
            shortcut_state,
        );
    }
}
