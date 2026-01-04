// macOS-specific Fn key listener using CGEventTap
// This avoids the thread-safety issues of rdev::grab

#[cfg(target_os = "macos")]
mod macos {
    use crate::actions::ACTION_MAP;
    use crate::settings::get_settings;
    use crate::ManagedToggleState;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    };
    use log::{error, info};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tauri::{AppHandle, Emitter, Manager};
    use tauri_plugin_global_shortcut::ShortcutState;

    // The Fn key on macOS sets the "SecondaryFn" flag (bit 23, value 0x800000)
    const FN_KEY_FLAG: u64 = 0x800000; // CGEventFlags::CGEventFlagSecondaryFn

    pub fn start_fn_key_listener(app: AppHandle) {
        std::thread::spawn(move || {
            let app_arc = Arc::new(app);
            let app_for_tap = app_arc.clone();

            // Track Fn key state to detect press/release (use AtomicBool for interior mutability)
            let fn_was_pressed = Arc::new(AtomicBool::new(false));
            let fn_state = fn_was_pressed.clone();

            // Create an event tap that intercepts flagsChanged events (modifier key changes)
            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default, // Can block events
                vec![CGEventType::FlagsChanged],
                move |_proxy, _event_type, event| {
                    // We only receive FlagsChanged events due to our filter above
                    let flags = event.get_flags();
                    let fn_is_pressed = (flags.bits() & FN_KEY_FLAG) != 0;
                    let was_pressed = fn_state.load(Ordering::SeqCst);

                    // Detect state change
                    if fn_is_pressed && !was_pressed {
                        // Fn key was just pressed
                        fn_state.store(true, Ordering::SeqCst);

                        // Emit event for frontend shortcut recording
                        let _ = app_for_tap.emit("fn-key-pressed", ());

                        // Check if we should suppress and handle
                        if should_suppress_fn(&app_for_tap) {
                            trigger_action(&app_for_tap, ShortcutState::Pressed);
                            return None; // Block the event
                        }
                    } else if !fn_is_pressed && was_pressed {
                        // Fn key was just released
                        fn_state.store(false, Ordering::SeqCst);

                        let _ = app_for_tap.emit("fn-key-released", ());

                        if should_suppress_fn(&app_for_tap) {
                            trigger_action(&app_for_tap, ShortcutState::Released);
                            return None; // Block the event
                        }
                    }

                    Some(event.clone())
                },
            );

            match tap {
                Ok(tap) => {
                    // Add the event tap to the current run loop
                    let loop_source = tap
                        .mach_port
                        .create_runloop_source(0)
                        .expect("Failed to create run loop source");

                    unsafe {
                        CFRunLoop::get_current().add_source(&loop_source, kCFRunLoopCommonModes);
                    }

                    tap.enable();
                    info!("Fn key event tap started successfully");

                    // Run the loop - this blocks
                    CFRunLoop::run_current();
                }
                Err(()) => {
                    error!("Failed to create CGEventTap for Fn key. Make sure Accessibility permissions are granted.");
                }
            }
        });
    }

    fn should_suppress_fn(app: &AppHandle) -> bool {
        let settings = get_settings(app);
        settings
            .bindings
            .get("transcribe")
            .map(|b| b.current_binding.eq_ignore_ascii_case("fn"))
            .unwrap_or(false)
    }

    fn trigger_action(app: &AppHandle, state: ShortcutState) {
        let binding_id = "transcribe";
        let settings = get_settings(app);

        if let Some(action) = ACTION_MAP.get(binding_id) {
            if settings.push_to_talk {
                if state == ShortcutState::Pressed {
                    action.start(app, binding_id, "Fn");
                } else {
                    action.stop(app, binding_id, "Fn");
                }
            } else {
                // Toggle mode: trigger only on press
                if state == ShortcutState::Pressed {
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
                        action.start(app, binding_id, "Fn");
                    } else {
                        action.stop(app, binding_id, "Fn");
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::start_fn_key_listener;

// Stub for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub fn start_fn_key_listener(_app: tauri::AppHandle) {
    // Fn key handling is macOS-specific
}

// Wrapper for backward compatibility with existing code
pub fn custom_fn_event_handler(app: tauri::AppHandle) {
    start_fn_key_listener(app);
}
