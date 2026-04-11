use std::os::raw::c_void;
use std::{
    thread,
    time::{Duration, Instant},
};

use core_foundation::{
    array::CFIndex,
    base::{CFRelease, OSStatus, TCFType, UInt8, UInt16, UInt32},
    data::{CFDataGetBytePtr, CFDataRef},
    dictionary::{CFDictionary, CFDictionaryRef},
    string::{CFString, CFStringRef, UniChar},
};
use core_graphics::{
    display::{CGDisplay, CGPoint},
    event::{
        CGEvent, CGEventFlags, CGEventRef, CGEventTapLocation, CGEventType, CGKeyCode,
        CGMouseButton, CGScrollEventUnit, EventField, KeyCode, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
};
use foreign_types_shared::ForeignTypeRef as _;
use log::{debug, error, info};
use objc2::msg_send;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSPoint;

use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse,
    NewConError, Settings,
};

#[repr(C)]
struct __TISInputSource;
type TISInputSourceRef = *const __TISInputSource;

#[allow(non_upper_case_globals)]
const kUCKeyTranslateNoDeadKeysBit: CFIndex = 0; // Previously was always u32. Change it back if there are bugs

#[allow(improper_ctypes)]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    // “Copy” here means +1 retain — we must CFRelease when done
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> TISInputSourceRef;

    // property key for the Unicode (‘uchr’) layout data
    #[allow(non_upper_case_globals)]
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;

    // fetches a CFDataRef containing the raw UCKeyboardLayout bytes
    #[allow(non_snake_case)]
    fn TISGetInputSourceProperty(
        inputSource: TISInputSourceRef,
        propertyKey: CFStringRef,
    ) -> CFDataRef;

    // turn keycode+modifiers → UTF‑16 string
    #[allow(non_snake_case)]
    fn UCKeyTranslate(
        keyLayoutPtr: *const UInt8, //*const UCKeyboardLayout,
        virtualKeyCode: UInt16,
        keyAction: UInt16,
        modifierKeyState: UInt32,
        keyboardType: UInt32,
        keyTranslateOptions: CFIndex,
        deadKeyState: *mut UInt32,
        maxStringLength: CFIndex,
        actualStringLength: *mut CFIndex,
        unicodeString: *mut UniChar,
    ) -> OSStatus;

    fn LMGetKbdType() -> UInt8;
}

/// The main struct for handling the event emitting
pub struct Enigo {
    event_source: CGEventSource,
    display: CGDisplay,
    held: (Vec<Key>, Vec<CGKeyCode>), // Currently held keys
    event_source_user_data: i64,
    release_keys_when_dropped: bool,
    event_flags: CGEventFlags,
    double_click_delay: Duration,
    // Instant when the last event was sent and the duration that needs to be waited for after that
    // instant to make sure all events were handled by the OS
    last_event: (Instant, Duration),
    // TODO: Use mem::variant_count::<Button>() here instead of 9 once it is stabilized
    last_mouse_click: [(i64, Instant); 9], /* For each of the nine Button variants, we
                                            * store the last time the button was clicked and
                                            * the nth click that was
                                            * This information is needed to
                                            * determine double clicks and handle cases where
                                            * another button is clicked while the other one has
                                            * not yet been released */
}

// TODO: Double check this is safe
unsafe impl Send for Enigo {}

impl Mouse for Enigo {
    // Sends a button event to the X11 server via `XTest` extension
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mbutton(button: {button:?}, direction: {direction:?})\x1b[0m");
        let (current_x, current_y) = self.location()?;

        if direction == Direction::Click || direction == Direction::Press {
            let click_count = self.nth_button_press(button, Direction::Press);
            let (button, event_type, button_number) = match button {
                Button::Left => (CGMouseButton::Left, CGEventType::LeftMouseDown, None),
                Button::Middle => (CGMouseButton::Center, CGEventType::OtherMouseDown, Some(2)),
                Button::Right => (CGMouseButton::Right, CGEventType::RightMouseDown, None),
                Button::Back => (CGMouseButton::Center, CGEventType::OtherMouseDown, Some(3)),
                Button::Forward => (CGMouseButton::Center, CGEventType::OtherMouseDown, Some(4)),
                Button::ScrollUp => return self.scroll(-1, Axis::Vertical),
                Button::ScrollDown => return self.scroll(1, Axis::Vertical),
                Button::ScrollLeft => return self.scroll(-1, Axis::Horizontal),
                Button::ScrollRight => return self.scroll(1, Axis::Horizontal),
            };
            let dest = CGPoint::new(current_x as f64, current_y as f64);

            let Ok(event) =
                CGEvent::new_mouse_event(self.event_source.clone(), event_type, dest, button)
            else {
                return Err(InputError::Simulate(
                    "failed creating event to enter mouse button",
                ));
            };

            if let Some(button_number) = button_number {
                event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, button_number);
            }
            event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click_count);
            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                self.event_source_user_data,
            );
            event.set_flags(self.event_flags);
            event.post(CGEventTapLocation::HID);
            self.update_wait_time();
        }
        if direction == Direction::Click || direction == Direction::Release {
            let click_count = self.nth_button_press(button, Direction::Release);
            let (button, event_type, button_number) = match button {
                Button::Left => (CGMouseButton::Left, CGEventType::LeftMouseUp, None),
                Button::Middle => (CGMouseButton::Center, CGEventType::OtherMouseUp, Some(2)),
                Button::Right => (CGMouseButton::Right, CGEventType::RightMouseUp, None),
                Button::Back => (CGMouseButton::Center, CGEventType::OtherMouseUp, Some(3)),
                Button::Forward => (CGMouseButton::Center, CGEventType::OtherMouseUp, Some(4)),
                Button::ScrollUp
                | Button::ScrollDown
                | Button::ScrollLeft
                | Button::ScrollRight => {
                    info!(
                        "On macOS the mouse_up function has no effect when called with one of the Scroll buttons"
                    );
                    return Ok(());
                }
            };
            let dest = CGPoint::new(current_x as f64, current_y as f64);
            let Ok(event) =
                CGEvent::new_mouse_event(self.event_source.clone(), event_type, dest, button)
            else {
                return Err(InputError::Simulate(
                    "failed creating event to enter mouse button",
                ));
            };

            if let Some(button_number) = button_number {
                event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, button_number);
            }
            event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click_count);
            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                self.event_source_user_data,
            );
            event.set_flags(self.event_flags);
            event.post(CGEventTapLocation::HID);
            self.update_wait_time();
        }
        Ok(())
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        debug!("\x1b[93mmove_mouse(x: {x:?}, y: {y:?}, coordinate:{coordinate:?})\x1b[0m");
        let pressed = unsafe { NSEvent::pressedMouseButtons() };
        let (current_x, current_y) = self.location()?;

        let (absolute, relative) = match coordinate {
            // TODO: Check the bounds
            Coordinate::Abs => ((x, y), (current_x - x, current_y - y)),
            Coordinate::Rel => ((current_x + x, current_y + y), (x, y)),
        };

        let (event_type, button) = if pressed & 1 > 0 {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if pressed & 2 > 0 {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left) // The mouse button
            // here is ignored so
            // it can be anything
        };

        let dest = CGPoint::new(absolute.0 as f64, absolute.1 as f64);
        let Ok(event) =
            CGEvent::new_mouse_event(self.event_source.clone(), event_type, dest, button)
        else {
            return Err(InputError::Simulate(
                "failed creating event to move the mouse",
            ));
        };

        // Add information by how much the mouse was moved
        event.set_integer_value_field(
            core_graphics::event::EventField::MOUSE_EVENT_DELTA_X,
            relative.0.into(),
        );
        event.set_integer_value_field(
            core_graphics::event::EventField::MOUSE_EVENT_DELTA_Y,
            relative.1.into(),
        );

        event.set_integer_value_field(
            EventField::EVENT_SOURCE_USER_DATA,
            self.event_source_user_data,
        );
        event.set_flags(self.event_flags);
        event.post(CGEventTapLocation::HID);
        self.update_wait_time();
        Ok(())
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        debug!("\x1b[93mscroll(length: {length:?}, axis: {axis:?})\x1b[0m");
        self.scroll_unit(length, ScrollEventUnit::LINE, axis)
    }

    #[cfg(all(feature = "platform_specific", target_os = "macos"))]
    fn smooth_scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        debug!("\x1b[93msmooth_scroll(length: {length:?}, axis: {axis:?})\x1b[0m");
        self.scroll_unit(length, ScrollEventUnit::PIXEL, axis)
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mmain_display()\x1b[0m");
        Ok((
            self.display.pixels_wide() as i32,
            self.display.pixels_high() as i32,
        ))
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mlocation()\x1b[0m");
        let pt = unsafe { NSEvent::mouseLocation() };
        let (x, y_inv) = (pt.x as i32, pt.y as i32);
        Ok((x, self.display.pixels_high() as i32 - y_inv))
    }
}

// https://stackoverflow.com/questions/1918841/how-to-convert-ascii-character-to-cgkeycode
impl Keyboard for Enigo {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
        // Fn to create an iterator over sub slices of a str that have the specified
        // length
        fn chunks(s: &str, len: usize) -> impl Iterator<Item = &str> {
            assert!(len > 0);
            let mut indices = s.char_indices().map(|(idx, _)| idx).peekable();

            std::iter::from_fn(move || {
                let start_idx = indices.next()?;
                for _ in 0..len - 1 {
                    indices.next();
                }
                let end_idx = match indices.peek() {
                    Some(idx) => *idx,
                    None => s.len(),
                };
                Some(&s[start_idx..end_idx])
            })
        }

        debug!("\x1b[93mfast_text(text: {text})\x1b[0m");
        // WORKAROUND: This is a fix for issue https://github.com/enigo-rs/enigo/issues/68
        // The CGEventKeyboardSetUnicodeString function (used inside of
        // event.set_string(chunk)) truncates strings down to 20 characters
        for mut chunk in chunks(text, 20) {
            let Ok(event) = CGEvent::new_keyboard_event(self.event_source.clone(), 0, true) else {
                return Err(InputError::Simulate(
                    "failed creating event to enter the text",
                ));
            };
            // WORKAROUND: This is a fix for issue https://github.com/enigo-rs/enigo/issues/260
            // This is needed to get rid of all leading line feed, tab and carriage return
            // characters. event.set_string(chunk)) silently fails if the chunk
            // starts with a newline character
            loop {
                if chunk.starts_with('\t') {
                    self.key(Key::Tab, Direction::Click)?;
                    chunk = &chunk[1..];
                    continue;
                }
                if chunk.starts_with('\r') {
                    self.fast_text("\u{200B}\r")?;
                    chunk = &chunk[1..];
                    continue;
                }
                if chunk.starts_with('\n') {
                    self.fast_text("\u{200B}\n")?;
                    chunk = &chunk[1..];
                    continue;
                }
                break;
            }

            event.set_string(chunk);
            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                self.event_source_user_data,
            );
            // We want to ignore all modifiers when entering text
            event.set_flags(CGEventFlags::CGEventFlagNull);
            event.post(CGEventTapLocation::HID);
            self.update_wait_time();
        }
        Ok(Some(()))
    }

    #[allow(clippy::too_many_lines)]
    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mkey(key: {key:?}, direction: {direction:?})\x1b[0m");
        // Nothing to do
        if key == Key::Unicode('\0') {
            return Ok(());
        }
        match key {
            Key::VolumeUp => {
                debug!("special case for handling the VolumeUp key");
                self.special_keys(0, direction)?;
            }
            Key::VolumeDown => {
                debug!("special case for handling the VolumeDown key");
                self.special_keys(1, direction)?;
            }
            Key::BrightnessUp => {
                debug!("special case for handling the BrightnessUp key");
                self.special_keys(2, direction)?;
            }
            Key::BrightnessDown => {
                debug!("special case for handling the BrightnessDown key");
                self.special_keys(3, direction)?;
            }
            Key::Power => {
                debug!("special case for handling the Power key");
                self.special_keys(6, direction)?;
            }
            Key::VolumeMute => {
                debug!("special case for handling the VolumeMute key");
                self.special_keys(7, direction)?;
            }

            Key::ContrastUp => {
                debug!("special case for handling the ContrastUp key");
                self.special_keys(11, direction)?;
            }
            Key::ContrastDown => {
                debug!("special case for handling the ContrastDown key");
                self.special_keys(12, direction)?;
            }
            Key::LaunchPanel => {
                debug!("special case for handling the LaunchPanel key");
                self.special_keys(13, direction)?;
            }
            Key::Eject => {
                debug!("special case for handling the Eject key");
                self.special_keys(14, direction)?;
            }
            Key::VidMirror => {
                debug!("special case for handling the VidMirror key");
                self.special_keys(15, direction)?;
            }
            Key::MediaPlayPause => {
                debug!("special case for handling the MediaPlayPause key");
                self.special_keys(16, direction)?;
            }
            Key::MediaNextTrack => {
                debug!("special case for handling the MediaNextTrack key");
                self.special_keys(17, direction)?;
            }
            Key::MediaPrevTrack => {
                debug!("special case for handling the MediaPrevTrack key");
                self.special_keys(18, direction)?;
            }
            Key::MediaFast => {
                debug!("special case for handling the MediaFast key");
                self.special_keys(19, direction)?;
            }
            Key::MediaRewind => {
                debug!("special case for handling the MediaRewind key");
                self.special_keys(20, direction)?;
            }
            Key::IlluminationUp => {
                debug!("special case for handling the IlluminationUp key");
                self.special_keys(21, direction)?;
            }
            Key::IlluminationDown => {
                debug!("special case for handling the IlluminationDown key");
                self.special_keys(22, direction)?;
            }
            Key::IlluminationToggle => {
                debug!("special case for handling the IlluminationToggle key");
                self.special_keys(23, direction)?;
            }
            Key::Unicode(c) => {
                let layout_keypress = get_layoutdependent_keypress(&c.to_string());
                self.raw_layoutdependent_keypress(layout_keypress, direction)?;
            }
            _ => {
                let Ok(keycode) = CGKeyCode::try_from(key) else {
                    return Err(InputError::InvalidInput(
                        "virtual keycodes on macOS have to fit into u16",
                    ));
                };
                self.raw(keycode, direction)?;
            }
        }

        // TODO: The list of keys will contain the key and also the associated keycode.
        // They are a duplicate
        match direction {
            Direction::Press => {
                debug!("added the key {key:?} to the held keys");
                self.held.0.push(key);
            }
            Direction::Release => {
                debug!("removed the key {key:?} from the held keys");
                self.held.0.retain(|&k| k != key);
            }
            Direction::Click => (),
        }

        Ok(())
    }

    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mraw(keycode: {keycode:?}, direction: {direction:?})\x1b[0m");

        if direction == Direction::Click || direction == Direction::Press {
            let Ok(event) = CGEvent::new_keyboard_event(self.event_source.clone(), keycode, true)
            else {
                return Err(InputError::Simulate(
                    "failed creating event to press the key",
                ));
            };

            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                self.event_source_user_data,
            );
            self.add_event_flag(keycode, Direction::Press);
            event.set_flags(self.event_flags);
            event.post(CGEventTapLocation::HID);
            self.update_wait_time();
        }

        if direction == Direction::Click || direction == Direction::Release {
            let Ok(event) = CGEvent::new_keyboard_event(self.event_source.clone(), keycode, false)
            else {
                return Err(InputError::Simulate(
                    "failed creating event to release the key",
                ));
            };

            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                self.event_source_user_data,
            );
            self.add_event_flag(keycode, Direction::Release);
            event.set_flags(self.event_flags);
            event.post(CGEventTapLocation::HID);
            self.update_wait_time();
        }

        match direction {
            Direction::Press => {
                debug!("added the keycode {keycode:?} to the held keys");
                self.held.1.push(keycode);
            }
            Direction::Release => {
                debug!("removed the keycode {keycode:?} from the held keys");
                self.held.1.retain(|&k| k != keycode);
            }
            Direction::Click => (),
        }

        Ok(())
    }
}

impl Enigo {
    /// Create a new Enigo struct to establish the connection to simulate input
    /// with the specified settings
    ///
    /// # Errors
    /// Have a look at the documentation of `NewConError` to see under which
    /// conditions an error will be returned.
    pub fn new(settings: &Settings) -> Result<Self, NewConError> {
        let Settings {
            release_keys_when_dropped,
            event_source_user_data,
            open_prompt_to_get_permissions,
            independent_of_keyboard_state,
            ..
        } = settings;

        if !has_permission(*open_prompt_to_get_permissions) {
            error!("The application does not have the permission to simulate input!");
            return Err(NewConError::NoPermission);
        }
        info!("The application has the permission to simulate input");

        let held = (Vec::new(), Vec::new());

        let mut event_flags = CGEventFlags::CGEventFlagNonCoalesced;
        event_flags.set(CGEventFlags::from_bits_retain(0x2000_0000), true); // I don't know if this is needed or what this flag does. Correct events have it
        // set so we also do it (until we know it is wrong)

        let double_click_delay = Duration::from_secs(1);
        let double_click_delay_setting = unsafe { NSEvent::doubleClickInterval() };
        // Returns the double click interval (https://developer.apple.com/documentation/appkit/nsevent/1528384-doubleclickinterval). This is a TimeInterval which is a f64 of the number of seconds
        let double_click_delay = double_click_delay.mul_f64(double_click_delay_setting);

        let event_source_state = if *independent_of_keyboard_state {
            CGEventSourceStateID::Private
        } else {
            CGEventSourceStateID::CombinedSessionState
        };
        let Ok(event_source) = CGEventSource::new(event_source_state) else {
            return Err(NewConError::EstablishCon("failed creating event source"));
        };

        debug!("\x1b[93mconnection established on macOS\x1b[0m");

        let last_event = (Instant::now(), Duration::from_secs(0));
        Ok(Enigo {
            event_source,
            display: CGDisplay::main(),
            held,
            release_keys_when_dropped: *release_keys_when_dropped,
            event_flags,
            double_click_delay,
            last_event,
            last_mouse_click: [(0, Instant::now()); 9],
            event_source_user_data: event_source_user_data.unwrap_or(crate::EVENT_MARKER as i64),
        })
    }

    /// Returns a list of all currently pressed keys
    pub fn held(&mut self) -> (Vec<Key>, Vec<CGKeyCode>) {
        self.held.clone()
    }

    /// Returns the value that enigo's events are marked with
    #[must_use]
    pub fn get_marker_value(&self) -> i64 {
        self.event_source_user_data
    }

    // On macOS, we have to determine ourselves if it was a double click of a mouse
    // button. The Enigo struct stores the information needed to do so. This
    // function checks if the button was pressed down again fast enough to issue a
    // double (or nth) click and returns the nth click it was. It also takes care of
    // updating the information the Enigo struct stores.
    fn nth_button_press(&mut self, button: Button, direction: Direction) -> i64 {
        if direction == Direction::Press {
            let last_time = self.last_mouse_click[button as usize].1;
            self.last_mouse_click[button as usize].1 = Instant::now();

            if last_time.elapsed() < self.double_click_delay {
                self.last_mouse_click[button as usize].0 += 1;
            } else {
                self.last_mouse_click[button as usize].0 = 1;
            }
        }
        let nth_button_press = self.last_mouse_click[button as usize].0;
        debug!("nth_button_press: {nth_button_press}");
        nth_button_press
    }

    fn special_keys(&mut self, code: isize, direction: Direction) -> InputResult<()> {
        if direction == Direction::Press || direction == Direction::Click {
            let event = unsafe {
                NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::SystemDefined, // 14
                NSPoint::ZERO,
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                8,
                (code << 16) | (0xa << 8),
                -1
            )
            };

            if let Some(event) = event {
                let cg_event = unsafe { Self::ns_event_cg_event(&event).to_owned() };
                cg_event.set_integer_value_field(
                    EventField::EVENT_SOURCE_USER_DATA,
                    self.event_source_user_data,
                );
                cg_event.set_flags(self.event_flags);
                cg_event.post(CGEventTapLocation::HID);
                self.update_wait_time();
            } else {
                return Err(InputError::Simulate(
                    "failed creating event to press special key",
                ));
            }
        }

        if direction == Direction::Release || direction == Direction::Click {
            let event = unsafe {
                NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                    NSEventType::SystemDefined, // 14
                NSPoint::ZERO,
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                8,
                (code << 16) | (0xb << 8),
                -1
            )
            };

            if let Some(event) = event {
                let cg_event = unsafe { Self::ns_event_cg_event(&event).to_owned() };
                cg_event.set_integer_value_field(
                    EventField::EVENT_SOURCE_USER_DATA,
                    self.event_source_user_data,
                );
                cg_event.set_flags(self.event_flags);
                cg_event.post(CGEventTapLocation::HID);
                self.update_wait_time();
            } else {
                return Err(InputError::Simulate(
                    "failed creating event to release special key",
                ));
            }
        }

        Ok(())
    }

    fn raw_layoutdependent_keypress(
        &mut self,
        keypress: LayoutDependentKeypress,
        direction: Direction,
    ) -> InputResult<()> {
        let modifier_keycodes = keypress.modifier.keycodes();

        match direction {
            Direction::Press => {
                for modifier_keycode in modifier_keycodes {
                    self.raw(*modifier_keycode, Direction::Press)?;
                }
                self.raw(keypress.keycode, Direction::Press)?;
            }
            Direction::Release => {
                self.raw(keypress.keycode, Direction::Release)?;
                for modifier_keycode in modifier_keycodes.iter().rev() {
                    self.raw(*modifier_keycode, Direction::Release)?;
                }
            }
            Direction::Click => {
                for modifier_keycode in modifier_keycodes {
                    self.raw(*modifier_keycode, Direction::Press)?;
                }
                self.raw(keypress.keycode, Direction::Click)?;
                for modifier_keycode in modifier_keycodes.iter().rev() {
                    self.raw(*modifier_keycode, Direction::Release)?;
                }
            }
        }

        Ok(())
    }

    unsafe fn ns_event_cg_event(event: &NSEvent) -> &CGEventRef {
        let ptr: *mut c_void = unsafe { msg_send![event, CGEvent] };
        unsafe { CGEventRef::from_ptr(ptr.cast()) }
    }

    // TODO: Remove this once the values for KeyCode were upstreamed: https://github.com/servo/core-foundation-rs/pull/712
    #[allow(clippy::match_same_arms)]
    #[allow(clippy::too_many_lines)]
    /// Adds or removes `KeyFlags` as needed by the keycode
    ///
    /// This function can never get called with `Direction::Click`!
    fn add_event_flag(&mut self, keycode: CGKeyCode, direction: Direction) {
        // Upstream these to https://github.com/servo/core-foundation-rs
        const NX_DEVICELCTLKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0001);
        const NX_DEVICELSHIFTKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0002);
        const NX_DEVICERSHIFTKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0004);
        const NX_DEVICELCMDKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0008);
        const NX_DEVICERCMDKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0010);
        const NX_DEVICELALTKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0020);
        const NX_DEVICERALTKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_0040);
        const NX_DEVICE_ALPHASHIFT_STATELESS_MASK: CGEventFlags =
            CGEventFlags::from_bits_retain(0x0000_0080);
        const NX_DEVICERCTLKEYMASK: CGEventFlags = CGEventFlags::from_bits_retain(0x0000_2000);

        type FlagOp = fn(&mut CGEventFlags, CGEventFlags);

        fn no_op(_: &mut CGEventFlags, _: CGEventFlags) {}

        // These flags have been determined by entering all keys with the previous
        // implementation that does not set the flags manually and checking the
        // resulting flags in their events. Some of the keys set the EventFlag even when
        // they are released. It's a bit weird, but for now we just copy the behavior
        // here
        let (press_fn, release_fn, event_flag): (FlagOp, FlagOp, CGEventFlags) = match keycode {
            KeyCode::RIGHT_COMMAND => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagCommand | NX_DEVICERCMDKEYMASK,
            ),
            KeyCode::COMMAND => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagCommand | NX_DEVICELCMDKEYMASK,
            ),
            KeyCode::SHIFT => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagShift | NX_DEVICELSHIFTKEYMASK,
            ),
            KeyCode::CAPS_LOCK => (
                CGEventFlags::toggle,
                no_op,
                CGEventFlags::CGEventFlagAlphaShift | NX_DEVICE_ALPHASHIFT_STATELESS_MASK, /* TODO: The NX_DEVICE_ALPHASHIFT_STATELESS_MASK did not get set when simulating CapsLock with the old implementation, but I'll go out on a limb and set it anyway. */
            ),
            KeyCode::OPTION => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagAlternate | NX_DEVICELALTKEYMASK,
            ),
            KeyCode::CONTROL => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagControl | NX_DEVICELCTLKEYMASK,
            ),
            KeyCode::RIGHT_SHIFT => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagShift | NX_DEVICERSHIFTKEYMASK,
            ),
            KeyCode::RIGHT_OPTION => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagAlternate | NX_DEVICERALTKEYMASK,
            ),
            KeyCode::RIGHT_CONTROL => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagControl | NX_DEVICERCTLKEYMASK,
            ),
            KeyCode::FUNCTION => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F17 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::ANSI_KEYPAD_DECIMAL => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_MULTIPLY => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_PLUS => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_CLEAR => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::ANSI_KEYPAD_DIVIDE => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_ENTER => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_MINUS => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::F18 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F19 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::ANSI_KEYPAD_EQUAL..=KeyCode::ANSI_KEYPAD_7 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::ANSI_KEYPAD_8..=KeyCode::ANSI_KEYPAD_9 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagNumericPad,
            ),
            KeyCode::F5..=KeyCode::F9 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F11 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F13..=KeyCode::F14 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F10 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F12 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::F15 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::HELP => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn | CGEventFlags::CGEventFlagHelp,
            ),
            KeyCode::HOME..KeyCode::LEFT_ARROW => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            KeyCode::LEFT_ARROW..0x7f => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn | CGEventFlags::CGEventFlagNumericPad,
            ),
            0x81..0x84 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            0x90 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            0x91 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            0xa0 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            0xb0..0xb3 => (
                CGEventFlags::insert,
                CGEventFlags::remove,
                CGEventFlags::CGEventFlagSecondaryFn,
            ),
            _ => (no_op, no_op, CGEventFlags::CGEventFlagNull),
        };

        let flag_fn = match direction {
            Direction::Click => {
                unreachable!(
                    "The function should never get called with Direction::Click. If it was, it's an implementation error"
                );
            }
            Direction::Press => press_fn,
            Direction::Release => release_fn,
        };

        flag_fn(&mut self.event_flags, event_flag);
    }

    fn scroll_unit(
        &mut self,
        length: i32,
        scroll_event_unit: CGScrollEventUnit,
        axis: Axis,
    ) -> InputResult<()> {
        let (ax, len_x, len_y) = match axis {
            Axis::Horizontal => (2, 0, -length),
            Axis::Vertical => (1, -length, 0),
        };

        let Ok(event) = CGEvent::new_scroll_event(
            self.event_source.clone(),
            scroll_event_unit,
            ax,
            len_x,
            len_y,
            0,
        ) else {
            return Err(InputError::Simulate("failed creating event to scroll"));
        };

        event.set_integer_value_field(
            EventField::EVENT_SOURCE_USER_DATA,
            self.event_source_user_data,
        );
        event.set_flags(self.event_flags);
        event.post(CGEventTapLocation::HID);
        self.update_wait_time();
        Ok(())
    }

    /// Save the current Instant and calculate the remaining waiting time
    /// We assume we need to wait for 20 ms for each event to make sure the OS
    /// has time to handle it. Instead of simply adding 20 ms for each event, we
    /// assume that the OS handled events between us sending events. That's why
    /// we subtract the time we already waited between events.
    fn update_wait_time(&mut self) {
        let now = Instant::now();
        let wait_time = self
            .last_event
            .1
            .saturating_sub(self.last_event.0.elapsed())
            + Duration::from_millis(20);
        self.last_event = (now, wait_time);
    }
}

/// Converts a `Key` to a `CGKeyCode`
impl TryFrom<Key> for core_graphics::event::CGKeyCode {
    type Error = ();

    #[allow(clippy::too_many_lines)]
    fn try_from(key: Key) -> Result<Self, Self::Error> {
        // A list of names is available at:
        // https://docs.rs/core-graphics/latest/core_graphics/event/struct.KeyCode.html
        // https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX10.13.sdk/System/Library/Frameworks/Carbon.framework/Versions/A/Frameworks/HIToolbox.framework/Versions/A/Headers/Events.h
        let key = match key {
            Key::Add => KeyCode::ANSI_KEYPAD_PLUS,
            Key::Alt | Key::Option => KeyCode::OPTION,
            Key::Backspace => KeyCode::DELETE,
            Key::CapsLock => KeyCode::CAPS_LOCK,
            Key::Control | Key::LControl => KeyCode::CONTROL,
            Key::Decimal => KeyCode::ANSI_KEYPAD_DECIMAL,
            Key::Delete => KeyCode::FORWARD_DELETE,
            Key::Divide => KeyCode::ANSI_KEYPAD_DIVIDE,
            Key::DownArrow => KeyCode::DOWN_ARROW,
            Key::End => KeyCode::END,
            Key::Escape => KeyCode::ESCAPE,
            Key::F1 => KeyCode::F1,
            Key::F2 => KeyCode::F2,
            Key::F3 => KeyCode::F3,
            Key::F4 => KeyCode::F4,
            Key::F5 => KeyCode::F5,
            Key::F6 => KeyCode::F6,
            Key::F7 => KeyCode::F7,
            Key::F8 => KeyCode::F8,
            Key::F9 => KeyCode::F9,
            Key::F10 => KeyCode::F10,
            Key::F11 => KeyCode::F11,
            Key::F12 => KeyCode::F12,
            Key::F13 => KeyCode::F13,
            Key::F14 => KeyCode::F14,
            Key::F15 => KeyCode::F15,
            Key::F16 => KeyCode::F16,
            Key::F17 => KeyCode::F17,
            Key::F18 => KeyCode::F18,
            Key::F19 => KeyCode::F19,
            Key::F20 => KeyCode::F20,
            Key::Function => KeyCode::FUNCTION,
            Key::Help => KeyCode::HELP,
            Key::Home => KeyCode::HOME,
            Key::Launchpad => 131,
            Key::LeftArrow => KeyCode::LEFT_ARROW,
            Key::MissionControl => 160,
            Key::Multiply => KeyCode::ANSI_KEYPAD_MULTIPLY,
            Key::Numpad0 => KeyCode::ANSI_KEYPAD_0,
            Key::Numpad1 => KeyCode::ANSI_KEYPAD_1,
            Key::Numpad2 => KeyCode::ANSI_KEYPAD_2,
            Key::Numpad3 => KeyCode::ANSI_KEYPAD_3,
            Key::Numpad4 => KeyCode::ANSI_KEYPAD_4,
            Key::Numpad5 => KeyCode::ANSI_KEYPAD_5,
            Key::Numpad6 => KeyCode::ANSI_KEYPAD_6,
            Key::Numpad7 => KeyCode::ANSI_KEYPAD_7,
            Key::Numpad8 => KeyCode::ANSI_KEYPAD_8,
            Key::Numpad9 => KeyCode::ANSI_KEYPAD_9,
            Key::PageDown => KeyCode::PAGE_DOWN,
            Key::PageUp => KeyCode::PAGE_UP,
            Key::RCommand => KeyCode::RIGHT_COMMAND,
            Key::RControl => KeyCode::RIGHT_CONTROL,
            Key::Return => KeyCode::RETURN,
            Key::RightArrow => KeyCode::RIGHT_ARROW,
            Key::RShift => KeyCode::RIGHT_SHIFT,
            Key::ROption => KeyCode::RIGHT_OPTION,
            Key::Shift | Key::LShift => KeyCode::SHIFT,
            Key::Space => KeyCode::SPACE,
            Key::Subtract => KeyCode::ANSI_KEYPAD_MINUS,
            Key::Tab => KeyCode::TAB,
            Key::UpArrow => KeyCode::UP_ARROW,
            Key::VolumeDown => KeyCode::VOLUME_DOWN,
            Key::VolumeUp => KeyCode::VOLUME_UP,
            Key::VolumeMute => KeyCode::MUTE,
            Key::Unicode(c) => get_layoutdependent_keypress(&c.to_string()).keycode,
            Key::Other(v) => {
                let Ok(v) = u16::try_from(v) else {
                    return Err(());
                };
                v
            }
            Key::Super | Key::Command | Key::Windows | Key::Meta => KeyCode::COMMAND,
            Key::BrightnessDown
            | Key::BrightnessUp
            | Key::ContrastUp
            | Key::ContrastDown
            | Key::Eject
            | Key::IlluminationDown
            | Key::IlluminationUp
            | Key::IlluminationToggle
            | Key::LaunchPanel
            | Key::MediaFast
            | Key::MediaNextTrack
            | Key::MediaPlayPause
            | Key::MediaPrevTrack
            | Key::MediaRewind
            | Key::Power
            | Key::VidMirror => return Err(()),
        };
        Ok(key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutDependentModifier {
    None,
    Shift,
    Option,
    ShiftOption,
}

impl LayoutDependentModifier {
    const fn carbon_state(self) -> u32 {
        match self {
            Self::None => 0x100,
            Self::Shift => 0x20102,
            Self::Option => 0x80120,
            Self::ShiftOption => 0xa0122,
        }
    }

    const fn keycodes(self) -> &'static [CGKeyCode] {
        match self {
            Self::None => &[],
            Self::Shift => &[KeyCode::SHIFT],
            Self::Option => &[KeyCode::OPTION],
            Self::ShiftOption => &[KeyCode::SHIFT, KeyCode::OPTION],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayoutDependentKeypress {
    keycode: CGKeyCode,
    modifier: LayoutDependentModifier,
}

fn get_layoutdependent_keypress(string: &str) -> LayoutDependentKeypress {
    let modifiers = [
        LayoutDependentModifier::None,
        LayoutDependentModifier::Shift,
        LayoutDependentModifier::Option,
        LayoutDependentModifier::ShiftOption,
    ];

    for modifier in modifiers {
        for keycode in 0..128 {
            if let Ok(key_string) = keycode_to_string(keycode, modifier.carbon_state()) {
                if string == key_string {
                    return LayoutDependentKeypress { keycode, modifier };
                }
            }
        }
    }

    LayoutDependentKeypress {
        keycode: 0,
        modifier: LayoutDependentModifier::None,
    }
}

fn keycode_to_string(keycode: u16, modifier: u32) -> Result<String, String> {
    let mut current_keyboard = unsafe { TISCopyCurrentKeyboardInputSource() };
    let mut layout_data =
        unsafe { TISGetInputSourceProperty(current_keyboard, kTISPropertyUnicodeKeyLayoutData) };
    if layout_data.is_null() {
        debug!(
            "TISGetInputSourceProperty(current_keyboard, kTISPropertyUnicodeKeyLayoutData) returned NULL"
        );
        unsafe { CFRelease(current_keyboard.cast::<c_void>()) };

        // TISGetInputSourceProperty returns null with some keyboard layout.
        // Using TISCopyCurrentKeyboardLayoutInputSource to fix NULL return.
        // See also: https://github.com/microsoft/node-native-keymap/blob/089d802efd387df4dce1f0e31898c66e28b3f67f/src/keyboard_mac.mm#L90
        current_keyboard = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
        layout_data = unsafe {
            TISGetInputSourceProperty(current_keyboard, kTISPropertyUnicodeKeyLayoutData)
        };
        if layout_data.is_null() {
            debug!(
                "TISGetInputSourceProperty(current_keyboard, kTISPropertyUnicodeKeyLayoutData) returned NULL again"
            );
            unsafe { CFRelease(current_keyboard.cast::<c_void>()) };
            current_keyboard = unsafe { TISCopyCurrentASCIICapableKeyboardLayoutInputSource() };
            layout_data = unsafe {
                TISGetInputSourceProperty(current_keyboard, kTISPropertyUnicodeKeyLayoutData)
            };
            debug_assert!(!layout_data.is_null());
            debug!("Using layout of the TISCopyCurrentASCIICapableKeyboardLayoutInputSource");
        }
    }

    let keyboard_layout = unsafe { CFDataGetBytePtr(layout_data) };

    let mut keys_down: UInt32 = 0;
    let mut chars: [UniChar; 1] = [0];
    let mut real_length = 0;
    let status = unsafe {
        UCKeyTranslate(
            keyboard_layout,
            keycode,
            3, // kUCKeyActionDisplay = 3
            modifier,
            LMGetKbdType() as u32,
            kUCKeyTranslateNoDeadKeysBit,
            &raw mut keys_down,
            chars.len() as CFIndex,
            &raw mut real_length,
            chars.as_mut_ptr(),
        )
    };
    unsafe { CFRelease(current_keyboard.cast::<c_void>()) };

    if status != 0 {
        error!("UCKeyTranslate failed with status: {status}");
        return Err(format!("OSStatus error: {status}"));
    }

    let utf16_slice = &chars[..real_length as usize];
    String::from_utf16(utf16_slice).map_err(|e| {
        error!("UTF-16 to String converstion failed: {e:?}");
        format!("FromUtf16Error: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        get_layoutdependent_keypress, keycode_to_string, LayoutDependentModifier,
    };

    #[test]
    fn layout_lookup_preserves_shift_modifier() {
        let Some((keycode, shifted)) = (0..128).find_map(|keycode| {
            let plain =
                keycode_to_string(keycode, LayoutDependentModifier::None.carbon_state()).ok()?;
            let shifted =
                keycode_to_string(keycode, LayoutDependentModifier::Shift.carbon_state()).ok()?;
            if plain.is_empty() || shifted.is_empty() || plain == shifted {
                return None;
            }
            Some((keycode, shifted))
        }) else {
            panic!("expected at least one shift-modified key on the active keyboard layout");
        };

        let keypress = get_layoutdependent_keypress(&shifted);
        assert_eq!(keypress.keycode, keycode);
        assert_eq!(keypress.modifier, LayoutDependentModifier::Shift);
    }

    #[test]
    fn layout_lookup_preserves_unmodified_key() {
        let Some((keycode, plain)) = (0..128).find_map(|keycode| {
            let plain =
                keycode_to_string(keycode, LayoutDependentModifier::None.carbon_state()).ok()?;
            if plain.is_empty() {
                return None;
            }
            Some((keycode, plain))
        }) else {
            panic!("expected at least one unmodified key on the active keyboard layout");
        };

        let keypress = get_layoutdependent_keypress(&plain);
        assert_eq!(keypress.keycode, keycode);
        assert_eq!(keypress.modifier, LayoutDependentModifier::None);
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Check if the currently running application has the permissions to simulate
/// input
///
/// Returns true if the application has the permission and is allowed to
/// simulate input
pub fn has_permission(open_prompt_to_get_permissions: bool) -> bool {
    let key = unsafe { kAXTrustedCheckOptionPrompt };
    let key = unsafe { CFString::wrap_under_create_rule(key) };

    let value = if open_prompt_to_get_permissions {
        debug!("Open the system prompt if the permissions are missing.");
        core_foundation::boolean::CFBoolean::true_value()
    } else {
        debug!("Do not open the system prompt if the permissions are missing.");
        core_foundation::boolean::CFBoolean::false_value()
    };

    let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
    let options = options.as_concrete_TypeRef();
    unsafe { AXIsProcessTrustedWithOptions(options) }
}

impl Drop for Enigo {
    // Release the held keys before the connection is dropped
    fn drop(&mut self) {
        if self.release_keys_when_dropped {
            let (held_keys, held_keycodes) = self.held();
            for key in held_keys {
                if self.key(key, Direction::Release).is_err() {
                    error!("unable to release {key:?}");
                }
            }

            for keycode in held_keycodes {
                if self.raw(keycode, Direction::Release).is_err() {
                    error!("unable to release {keycode:?}");
                }
            }
            debug!("released all held keys");
        }

        // DO NOT REMOVE THE SLEEP
        // This sleep is needed because all events that have not been
        // processed until this point would just get ignored when the
        // struct is dropped
        self.update_wait_time();
        thread::sleep(self.last_event.1.saturating_sub(Duration::from_millis(20)));
    }
}
