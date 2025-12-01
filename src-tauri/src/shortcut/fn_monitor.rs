//! macOS fn/Globe key monitoring using NSEvent.addGlobalMonitor
//!
//! This module provides fn key support on macOS by monitoring NSEventModifierFlags::Function.
//! It requires Accessibility permission (same as enigo for pasting).
//!
//! # Architecture
//! - Uses NSEvent::addGlobalMonitorForEventsMatchingMask_handler for event monitoring
//! - Must run on the main thread
//! - Listen-only (cannot block events, which is fine for our use case)
//!
//! # Known Limitations
//! - Stops receiving events when Secure Input is enabled (password fields, 1Password, etc.)
//! - fn+key combinations conflict with system shortcuts; fn alone is safe

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use log::{debug, error, info, warn};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use once_cell::sync::Lazy;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::settings::ShortcutBinding;

// FFI bindings for Accessibility API
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

// Key for prompting user in AXIsProcessTrustedWithOptions
const K_AX_TRUSTED_CHECK_OPTION_PROMPT: &str = "AXTrustedCheckOptionPrompt";

/// Check if the app has Accessibility permission.
/// If `prompt` is true, shows the system dialog to grant permission if not already granted.
pub fn check_accessibility_permission(prompt: bool) -> bool {
    unsafe {
        if prompt {
            // Create options dictionary with prompt = true
            let key = NSString::from_str(K_AX_TRUSTED_CHECK_OPTION_PROMPT);
            let value = NSNumber::new_bool(true);
            let keys: &[&NSString] = &[&key];
            let values: &[&NSNumber] = &[&value];
            let options = NSDictionary::from_slices(keys, values);
            AXIsProcessTrustedWithOptions(Retained::as_ptr(&options) as *const std::ffi::c_void)
        } else {
            AXIsProcessTrustedWithOptions(std::ptr::null())
        }
    }
}

/// Check if Accessibility permission is granted (without prompting)
pub fn has_accessibility_permission() -> bool {
    check_accessibility_permission(false)
}

/// Request Accessibility permission (shows system dialog if not granted)
pub fn request_accessibility_permission() -> bool {
    check_accessibility_permission(true)
}

/// Entry for a registered fn key binding
#[derive(Clone)]
struct FnBindingEntry {
    app_handle: AppHandle,
    binding_id: String,
    shortcut_string: String,
}

/// State shared between the monitor callback and registration functions
#[derive(Default)]
struct FnMonitorState {
    bindings: HashMap<String, FnBindingEntry>,
    fn_pressed: bool,
}

/// Handle to the monitor, stored per-thread (must be main thread)
#[derive(Default)]
struct FnMonitorHandle {
    monitor_token: Option<Retained<AnyObject>>,
    #[allow(dead_code)]
    handler: Option<RcBlock<dyn Fn(NonNull<NSEvent>) + 'static>>,
}

static MONITOR_STATE: Lazy<Arc<Mutex<FnMonitorState>>> =
    Lazy::new(|| Arc::new(Mutex::new(FnMonitorState::default())));

static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static MONITOR_HANDLE: RefCell<FnMonitorHandle> = RefCell::new(FnMonitorHandle::default());
}

/// Register an fn key binding.
/// The binding's `current_binding` should be "fn" for this to work.
pub fn register_fn_binding(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    debug!(
        "Registering fn binding: id='{}', binding='{}'",
        binding.id, binding.current_binding
    );

    ensure_monitor_started(app)?;

    let mut state = MONITOR_STATE
        .lock()
        .map_err(|_| "Failed to lock fn monitor state".to_string())?;

    state.bindings.insert(
        binding.id.clone(),
        FnBindingEntry {
            app_handle: app.clone(),
            binding_id: binding.id,
            shortcut_string: binding.current_binding,
        },
    );

    debug!(
        "fn binding registered successfully. Total fn bindings: {}",
        state.bindings.len()
    );
    Ok(())
}

/// Unregister an fn key binding
pub fn unregister_fn_binding(_app: &AppHandle, binding_id: &str) -> Result<(), String> {
    debug!("Unregistering fn binding: id='{binding_id}'");

    let mut state = MONITOR_STATE
        .lock()
        .map_err(|_| "Failed to lock fn monitor state".to_string())?;

    if state.bindings.remove(binding_id).is_some() {
        debug!(
            "fn binding removed. Remaining fn bindings: {}",
            state.bindings.len()
        );
        if state.bindings.is_empty() {
            // Reset pressed state when no bindings remain
            state.fn_pressed = false;
        }
    } else {
        debug!("fn binding '{binding_id}' was not registered");
    }

    Ok(())
}

/// Ensure the global fn key monitor is started on the main thread
fn ensure_monitor_started(app: &AppHandle) -> Result<(), String> {
    if MONITOR_STARTED.load(Ordering::SeqCst) {
        debug!("fn monitor already started");
        return Ok(());
    }

    debug!("Starting fn key monitor...");

    // Check Accessibility permission first (shows system dialog if not granted)
    if !has_accessibility_permission() {
        info!("Accessibility permission not granted, prompting user...");
        let granted = request_accessibility_permission();
        if !granted {
            return Err(
                "Accessibility permission is required for fn key shortcuts. \
                Please grant permission in System Settings > Privacy & Security > Accessibility, \
                then restart Handy."
                    .to_string(),
            );
        }
        info!("Accessibility permission granted");
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
                // SAFETY: The event pointer is valid for the duration of the callback
                let event_ref = unsafe { event.as_ref() };

                // Only process modifier flag changes
                let event_type = event_ref.r#type();
                if event_type != NSEventType::FlagsChanged {
                    return;
                }

                let flags = event_ref.modifierFlags();
                process_modifier_flags(&state_for_handler, flags);
            });

            // Install the global monitor
            let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
                NSEventMask::FlagsChanged,
                &handler,
            );

            match monitor {
                Some(token) => {
                    debug!("fn key monitor installed successfully");
                    handle.monitor_token = Some(token);
                    handle.handler = Some(handler);
                    MONITOR_STARTED.store(true, Ordering::SeqCst);
                    let _ = tx.send(Ok(()));
                }
                None => {
                    error!("Failed to install fn key monitor - Accessibility permission may be missing");
                    handle.monitor_token = None;
                    handle.handler = None;
                    MONITOR_STARTED.store(false, Ordering::SeqCst);
                    let _ = tx.send(Err(
                        "Failed to install fn key monitor. Please grant Handy Accessibility permission in System Settings > Privacy & Security > Accessibility.".to_string()
                    ));
                }
            }
        });
    });

    if let Err(err) = schedule_result {
        return Err(format!(
            "Failed to schedule fn monitor on main thread: {err}"
        ));
    }

    rx.recv()
        .unwrap_or_else(|_| Err("fn monitor setup did not complete".to_string()))
}

/// Process modifier flag changes and dispatch events for fn key
fn process_modifier_flags(state: &Arc<Mutex<FnMonitorState>>, flags: NSEventModifierFlags) {
    let is_pressed = flags.contains(NSEventModifierFlags::Function);

    let bindings: Vec<FnBindingEntry> = {
        let mut guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("fn monitor state lock poisoned");
                return;
            }
        };

        // Skip if state hasn't changed
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

    debug!(
        "fn key {}, dispatching to {} binding(s)",
        if is_pressed { "pressed" } else { "released" },
        bindings.len()
    );

    // Dispatch to all registered fn bindings
    for binding in bindings {
        super::dispatch_binding_event(
            &binding.app_handle,
            &binding.binding_id,
            &binding.shortcut_string,
            shortcut_state,
        );
    }
}
