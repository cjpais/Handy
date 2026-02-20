//! macOS keyboard listener using CGEventTap

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventSource, CGEventSourceStateID,
    CGEventTapCallBack, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::error::{Error, Result};
use crate::platform::state::{BlockingHotkeys, ListenerState};
use crate::types::{Key, KeyEvent, Modifiers};

use super::keycode::{flags_have_alpha_shift, flags_have_fn, keycode_to_key, keycode_to_modifier};
use super::permissions::check_accessibility;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventKeyboardGetUnicodeString(
        event: *const std::ffi::c_void,
        maxStringLength: usize,
        actualStringLength: *mut usize,
        unicodeString: *mut u16,
    );
}

fn key_from_event_unicode(cg_event: &CGEvent) -> Option<Key> {
    let mut chars = [0u16; 8];
    let mut len: usize = 0;
    unsafe {
        CGEventKeyboardGetUnicodeString(
            cg_event as *const _ as *const std::ffi::c_void,
            chars.len(),
            &mut len,
            chars.as_mut_ptr(),
        );
    }

    if len > 0 {
        let text = String::from_utf16_lossy(&chars[..len.min(chars.len())]);
        if let Some(ch) = text.chars().next() {
            return Key::from_layout_char(ch);
        }
    }
    None
}

/// Internal listener state returned to KeyboardListener
pub(crate) struct MacOSListenerState {
    pub event_receiver: Receiver<KeyEvent>,
    pub thread_handle: Option<JoinHandle<()>>,
    pub running: Arc<AtomicBool>,
    pub blocking_hotkeys: Option<BlockingHotkeys>,
}

/// Spawn a macOS keyboard listener using CGEventTap
pub(crate) fn spawn(blocking_hotkeys: Option<BlockingHotkeys>) -> Result<MacOSListenerState> {
    if !check_accessibility() {
        return Err(Error::AccessibilityNotGranted);
    }

    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(ListenerState::new(tx, blocking_hotkeys.clone())));
    let running = Arc::new(AtomicBool::new(true));

    // Channel to communicate event tap creation success/failure
    let (init_tx, init_rx) = mpsc::channel::<std::result::Result<(), String>>();

    let thread_state = Arc::clone(&state);
    let thread_running = Arc::clone(&running);

    let handle = thread::spawn(move || {
        run_event_tap(thread_state, thread_running, init_tx);
    });

    // Wait for the event tap to be created
    match init_rx.recv() {
        Ok(Ok(())) => {
            // Event tap created successfully
        }
        Ok(Err(msg)) => {
            return Err(Error::EventTapCreationFailed(msg));
        }
        Err(_) => {
            return Err(Error::EventTapCreationFailed(
                "Event tap thread terminated unexpectedly".to_string(),
            ));
        }
    }

    Ok(MacOSListenerState {
        event_receiver: rx,
        thread_handle: Some(handle),
        running,
        blocking_hotkeys,
    })
}

/// Reconcile internally tracked modifiers against the actual CGEventFlags from the OS.
///
/// This corrects drift caused by missed events (e.g., tap disabled by timeout, system
/// interruptions like Mission Control or screen lock). Should only be called for
/// non-FlagsChanged events, where flags reflect the current state with no change pending.
fn reconcile_modifiers(current: &mut Modifiers, flags: CGEventFlags) {
    // If OS says a modifier group is NOT held, clear our tracked bits.
    // This fixes "stuck modifier" from missed release events.
    if !flags.contains(CGEventFlags::MaskControl) {
        current.remove(Modifiers::CTRL_LEFT | Modifiers::CTRL_RIGHT);
    }
    if !flags.contains(CGEventFlags::MaskShift) {
        current.remove(Modifiers::SHIFT_LEFT | Modifiers::SHIFT_RIGHT);
    }
    if !flags.contains(CGEventFlags::MaskCommand) {
        current.remove(Modifiers::CMD_LEFT | Modifiers::CMD_RIGHT);
    }
    if !flags.contains(CGEventFlags::MaskAlternate) {
        current.remove(Modifiers::OPT_LEFT | Modifiers::OPT_RIGHT);
    }

    // If OS says a modifier group IS held but we have no bits for it,
    // we missed a press event. Default to left side as fallback.
    if flags.contains(CGEventFlags::MaskControl) && !current.intersects(Modifiers::CTRL) {
        current.insert(Modifiers::CTRL_LEFT);
    }
    if flags.contains(CGEventFlags::MaskShift) && !current.intersects(Modifiers::SHIFT) {
        current.insert(Modifiers::SHIFT_LEFT);
    }
    if flags.contains(CGEventFlags::MaskCommand) && !current.intersects(Modifiers::CMD) {
        current.insert(Modifiers::CMD_LEFT);
    }
    if flags.contains(CGEventFlags::MaskAlternate) && !current.intersects(Modifiers::OPT) {
        current.insert(Modifiers::OPT_LEFT);
    }
}

/// The callback function for the event tap
///
/// Returns NULL to block the event, or the event pointer to pass it through.
unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // Safety: user_info is our state pointer
    let state = &*(user_info as *const Mutex<ListenerState>);

    let cg_event = event.as_ref();
    let flags = CGEvent::flags(Some(cg_event));

    let mut should_block = false;

    if let Ok(mut state) = state.lock() {
        // Reconcile tracked modifiers against OS flags for non-FlagsChanged events.
        // On FlagsChanged, flags reflect the state *after* the current change, so
        // reconciling would fight with the toggle logic.
        if event_type != CGEventType::FlagsChanged {
            reconcile_modifiers(&mut state.current_modifiers, flags);
        }

        // Build side-specific modifiers from internally tracked state + FN from flags
        let modifiers = if flags_have_fn(flags) {
            state.current_modifiers | Modifiers::FN
        } else {
            state.current_modifiers & !Modifiers::FN
        };

        match event_type {
            CGEventType::KeyDown => {
                let keycode = CGEvent::integer_value_field(
                    Some(cg_event),
                    CGEventField::KeyboardEventKeycode,
                ) as u16;

                let key = key_from_event_unicode(cg_event).or_else(|| keycode_to_key(keycode));

                // Skip special function key events (e.g., F3 triggering Mission Control).
                // These have MaskSecondaryFn set but use special keycodes (like 0xA0)
                // that we don't recognize. Without this check, they'd be reported as
                // "Fn pressed" with no key.
                if key.is_none() && flags_have_fn(flags) {
                    return event.as_ptr();
                }

                // Check if this should be blocked
                should_block = state.should_block(modifiers, key);

                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key,
                    is_key_down: true,
                    changed_modifier: None,
                });
            }
            CGEventType::KeyUp => {
                let keycode = CGEvent::integer_value_field(
                    Some(cg_event),
                    CGEventField::KeyboardEventKeycode,
                ) as u16;

                let key = key_from_event_unicode(cg_event).or_else(|| keycode_to_key(keycode));

                // Skip special function key events (same as KeyDown)
                if key.is_none() && flags_have_fn(flags) {
                    return event.as_ptr();
                }

                // Block key up if we blocked key down (to be consistent)
                should_block = state.should_block(modifiers, key);

                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key,
                    is_key_down: false,
                    changed_modifier: None,
                });
            }
            CGEventType::FlagsChanged => {
                let keycode = CGEvent::integer_value_field(
                    Some(cg_event),
                    CGEventField::KeyboardEventKeycode,
                ) as u16;

                let changed_modifier = keycode_to_modifier(keycode);

                // Check if this is a lock key (e.g., Caps Lock) which comes through
                // as FlagsChanged but isn't a traditional modifier
                let lock_key = keycode_to_key(keycode);

                // Handle lock keys specially - they come through FlagsChanged
                // but don't change our tracked modifier state
                if let Some(key) = lock_key {
                    let is_key_down = flags_have_alpha_shift(flags);

                    should_block = state.should_block(modifiers, Some(key));

                    let _ = state.event_sender.send(KeyEvent {
                        modifiers,
                        key: Some(key),
                        is_key_down,
                        changed_modifier: None,
                    });
                } else if let Some(modifier_bit) = changed_modifier {
                    // Regular modifier key — use keycode to toggle the specific bit
                    let was_set = state.current_modifiers.contains(modifier_bit);
                    let is_key_down = !was_set;

                    if is_key_down {
                        state.current_modifiers |= modifier_bit;
                    } else {
                        state.current_modifiers &= !modifier_bit;
                    }

                    // Re-derive modifiers after update (include FN from flags)
                    let new_modifiers = if flags_have_fn(flags) {
                        state.current_modifiers | Modifiers::FN
                    } else {
                        state.current_modifiers & !Modifiers::FN
                    };

                    // Check if this modifier-only combo should be blocked
                    if is_key_down {
                        should_block = state.should_block(new_modifiers, None);
                    }

                    let _ = state.event_sender.send(KeyEvent {
                        modifiers: new_modifiers,
                        key: None,
                        is_key_down,
                        changed_modifier: changed_modifier,
                    });
                } else if keycode == 0x3F {
                    // FN key itself — tracked via flags, not keycode state
                    let had_fn = modifiers.contains(Modifiers::FN);
                    let has_fn = flags_have_fn(flags);
                    if had_fn != has_fn {
                        let new_modifiers = if has_fn {
                            state.current_modifiers | Modifiers::FN
                        } else {
                            state.current_modifiers & !Modifiers::FN
                        };

                        if has_fn {
                            should_block = state.should_block(new_modifiers, None);
                        }

                        let _ = state.event_sender.send(KeyEvent {
                            modifiers: new_modifiers,
                            key: None,
                            is_key_down: has_fn,
                            changed_modifier: Some(Modifiers::FN),
                        });
                    }
                }
            }
            // Mouse button events
            // Only report left/right clicks when modifiers are held (to avoid noise)
            CGEventType::LeftMouseDown if !modifiers.is_empty() => {
                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key: Some(Key::MouseLeft),
                    is_key_down: true,
                    changed_modifier: None,
                });
            }
            CGEventType::LeftMouseUp if !modifiers.is_empty() => {
                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key: Some(Key::MouseLeft),
                    is_key_down: false,
                    changed_modifier: None,
                });
            }
            CGEventType::RightMouseDown if !modifiers.is_empty() => {
                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key: Some(Key::MouseRight),
                    is_key_down: true,
                    changed_modifier: None,
                });
            }
            CGEventType::RightMouseUp if !modifiers.is_empty() => {
                let _ = state.event_sender.send(KeyEvent {
                    modifiers,
                    key: Some(Key::MouseRight),
                    is_key_down: false,
                    changed_modifier: None,
                });
            }
            // Pass through unmodified left/right clicks
            CGEventType::LeftMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::RightMouseDown
            | CGEventType::RightMouseUp => {}
            CGEventType::OtherMouseDown => {
                let button_number = CGEvent::integer_value_field(
                    Some(cg_event),
                    CGEventField::MouseEventButtonNumber,
                );
                let key = match button_number {
                    2 => Some(Key::MouseMiddle),
                    3 => Some(Key::MouseX1),
                    4 => Some(Key::MouseX2),
                    _ => None, // Unknown button
                };
                if let Some(key) = key {
                    let _ = state.event_sender.send(KeyEvent {
                        modifiers,
                        key: Some(key),
                        is_key_down: true,
                        changed_modifier: None,
                    });
                }
            }
            CGEventType::OtherMouseUp => {
                let button_number = CGEvent::integer_value_field(
                    Some(cg_event),
                    CGEventField::MouseEventButtonNumber,
                );
                let key = match button_number {
                    2 => Some(Key::MouseMiddle),
                    3 => Some(Key::MouseX1),
                    4 => Some(Key::MouseX2),
                    _ => None,
                };
                if let Some(key) = key {
                    let _ = state.event_sender.send(KeyEvent {
                        modifiers,
                        key: Some(key),
                        is_key_down: false,
                        changed_modifier: None,
                    });
                }
            }
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                // macOS disabled the tap (callback latency or user input).
                // The run loop thread will re-enable it; reconcile modifiers
                // now since we may have missed events while disabled.
                reconcile_modifiers(
                    &mut state.current_modifiers,
                    CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState),
                );
            }
            _ => {}
        }
    }

    if should_block {
        // Block the event from reaching other applications
        std::ptr::null_mut()
    } else {
        // Pass the event through unchanged
        event.as_ptr()
    }
}

/// Run the event tap in a dedicated thread
fn run_event_tap(
    state: Arc<Mutex<ListenerState>>,
    running: Arc<AtomicBool>,
    init_tx: Sender<std::result::Result<(), String>>,
) {
    // Event types we want to monitor
    let event_mask: CGEventMask = (1 << CGEventType::KeyDown.0)
        | (1 << CGEventType::KeyUp.0)
        | (1 << CGEventType::FlagsChanged.0)
        // Mouse buttons
        | (1 << CGEventType::LeftMouseDown.0)
        | (1 << CGEventType::LeftMouseUp.0)
        | (1 << CGEventType::RightMouseDown.0)
        | (1 << CGEventType::RightMouseUp.0)
        | (1 << CGEventType::OtherMouseDown.0)
        | (1 << CGEventType::OtherMouseUp.0);

    // Store state in a raw pointer for the callback
    let state_ptr = Arc::into_raw(Arc::clone(&state)) as *mut c_void;

    let callback: CGEventTapCallBack = Some(event_tap_callback);

    // Use Default mode (not ListenOnly) to enable optional event blocking
    let tap: Option<CFRetained<CFMachPort>> = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            event_mask,
            callback,
            state_ptr,
        )
    };

    let tap = match tap {
        Some(t) => t,
        None => {
            // Cleanup
            unsafe {
                let _ = Arc::from_raw(state_ptr as *const Mutex<ListenerState>);
            }
            let _ = init_tx.send(Err(
                "Failed to create event tap. Your terminal app may need accessibility permission in System Settings > Privacy & Security > Accessibility".to_string()
            ));
            return;
        }
    };

    // Create run loop source
    let source: Option<CFRetained<CFRunLoopSource>> =
        CFMachPort::new_run_loop_source(None, Some(&tap), 0);

    let source = match source {
        Some(s) => s,
        None => {
            unsafe {
                CFMachPort::invalidate(&tap);
                let _ = Arc::from_raw(state_ptr as *const Mutex<ListenerState>);
            }
            let _ = init_tx.send(Err("Failed to create run loop source".to_string()));
            return;
        }
    };

    // Get the current run loop and add the source
    let run_loop = CFRunLoop::current();

    // Unwrap the Option<CFRetained<CFRunLoop>> - current() should always succeed on a valid thread
    let run_loop = match run_loop {
        Some(rl) => rl,
        None => {
            unsafe {
                CFMachPort::invalidate(&tap);
                let _ = Arc::from_raw(state_ptr as *const Mutex<ListenerState>);
            }
            let _ = init_tx.send(Err("Failed to get current run loop".to_string()));
            return;
        }
    };

    run_loop.add_source(Some(&source), unsafe {
        objc2_core_foundation::kCFRunLoopCommonModes
    });
    CGEvent::tap_enable(&tap, true);

    // Signal successful initialization
    let _ = init_tx.send(Ok(()));

    // Run the loop
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        // Run for a short interval, then check if we should stop
        CFRunLoop::run_in_mode(
            unsafe { objc2_core_foundation::kCFRunLoopDefaultMode },
            0.1, // 100ms timeout
            true,
        );

        // Re-enable tap if macOS disabled it due to callback latency
        if !CGEvent::tap_is_enabled(&tap) {
            CGEvent::tap_enable(&tap, true);
        }
    }

    // Cleanup
    run_loop.remove_source(Some(&source), unsafe {
        objc2_core_foundation::kCFRunLoopCommonModes
    });
    CGEvent::tap_enable(&tap, false);
    CFMachPort::invalidate(&tap);
    unsafe {
        let _ = Arc::from_raw(state_ptr as *const Mutex<ListenerState>);
    }
}
