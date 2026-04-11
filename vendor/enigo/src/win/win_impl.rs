use std::mem::size_of;

use log::{debug, error, info, warn};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        GetKeyboardLayout, HKL, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS,
        KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE,
        MAP_VIRTUAL_KEY_TYPE, MAPVK_VK_TO_VSC_EX, MAPVK_VSC_TO_VK_EX, MOUSE_EVENT_FLAGS,
        MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
        MapVirtualKeyExW, SendInput, VIRTUAL_KEY,
    },
    WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, WHEEL_DELTA,
};

use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse,
    NewConError, Settings,
};

type ScanCode = u16;
pub const EXT: u16 = 0xFF00;

/// The main struct for handling the event emitting
pub struct Enigo {
    held: (Vec<Key>, Vec<ScanCode>), // Currently held keys
    release_keys_when_dropped: bool,
    dw_extra_info: usize,
    windows_subject_to_mouse_speed_and_acceleration_level: bool,
}

fn send_input(input: &[INPUT]) -> InputResult<()> {
    if input.is_empty() {
        return Ok(());
    }
    let Ok(input_size): Result<i32, _> = size_of::<INPUT>().try_into() else {
        return Err(InputError::InvalidInput(
            "the size of the INPUT was so large, the size exceeded i32::MAX",
        ));
    };
    let Ok(input_len) = input.len().try_into() else {
        return Err(InputError::InvalidInput(
            "the number of INPUT was so large, the length of the Vec exceeded i32::MAX",
        ));
    };
    if unsafe { SendInput(input, input_size) } == input_len {
        Ok(())
    } else {
        let last_err = std::io::Error::last_os_error();
        error!("{last_err}");
        Err(InputError::Simulate(
            "not all input events were sent. they may have been blocked by UIPI",
        ))
    }
}

fn mouse_event(
    flags: MOUSE_EVENT_FLAGS,
    data: i32,
    dx: i32,
    dy: i32,
    dw_extra_info: usize,
) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32, /* mouseData unfortunately is defined as unsigned even
                                         * though we need negative values as well */
                dwFlags: flags,
                time: 0, /* Always set it to 0 (see https://web.archive.org/web/20231004113147/https://devblogs.microsoft.com/oldnewthing/20121101-00/?p=6193) */
                dwExtraInfo: dw_extra_info,
            },
        },
    }
}

fn keybd_event(
    flags: KEYBD_EVENT_FLAGS,
    vk: VIRTUAL_KEY,
    scan: ScanCode,
    dw_extra_info: usize,
) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0, /* Always set it to 0 (see https://web.archive.org/web/20231004113147/https://devblogs.microsoft.com/oldnewthing/20121101-00/?p=6193) */
                dwExtraInfo: dw_extra_info,
            },
        },
    }
}

impl Mouse for Enigo {
    // Sends a button event to the X11 server via `XTest` extension
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mbutton(button: {button:?}, direction: {direction:?})\x1b[0m");
        let mut input = vec![];
        let button_no = match button {
            Button::Back => 1,
            Button::Forward => 2,
            _ => 0,
        };
        if direction == Direction::Click || direction == Direction::Press {
            let mouse_event_flag = match button {
                Button::Left => MOUSEEVENTF_LEFTDOWN,
                Button::Middle => MOUSEEVENTF_MIDDLEDOWN,
                Button::Right => MOUSEEVENTF_RIGHTDOWN,
                Button::Back | Button::Forward => MOUSEEVENTF_XDOWN,
                Button::ScrollUp => return self.scroll(-1, Axis::Vertical),
                Button::ScrollDown => return self.scroll(1, Axis::Vertical),
                Button::ScrollLeft => return self.scroll(-1, Axis::Horizontal),
                Button::ScrollRight => return self.scroll(1, Axis::Horizontal),
            };
            input.push(mouse_event(
                mouse_event_flag,
                button_no,
                0,
                0,
                self.dw_extra_info,
            ));
        }
        if direction == Direction::Click || direction == Direction::Release {
            let mouse_event_flag = match button {
                Button::Left => MOUSEEVENTF_LEFTUP,
                Button::Middle => MOUSEEVENTF_MIDDLEUP,
                Button::Right => MOUSEEVENTF_RIGHTUP,
                Button::Back | Button::Forward => MOUSEEVENTF_XUP,
                Button::ScrollUp
                | Button::ScrollDown
                | Button::ScrollLeft
                | Button::ScrollRight => {
                    info!(
                        "On Windows the mouse_up function has no effect when called with one of the Scroll buttons"
                    );
                    return Ok(());
                }
            };
            input.push(mouse_event(
                mouse_event_flag,
                button_no,
                0,
                0,
                self.dw_extra_info,
            ));
        }
        send_input(&input)
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        debug!("\x1b[93mmove_mouse(x: {x:?}, y: {y:?}, coordinate:{coordinate:?})\x1b[0m");
        let (flags, x, y) = if coordinate == Coordinate::Abs {
            // 0-screen width/height - 1 map to 0-65535
            // Add w/2 or h/2 to round off
            // See https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-mouse_event#remarks
            let (w, h) = self.main_display()?;
            let w = w as i64 - 1;
            let h = h as i64 - 1;
            let x = x as i64;
            let y = y as i64;
            let x = (x * 65535 + w / 2 * x.signum()) / w;
            let y = (y * 65535 + h / 2 * y.signum()) / h;
            // TODO: Check if we should use MOUSEEVENTF_VIRTUALDESK too
            (MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, x as i32, y as i32)
        } else if self.windows_subject_to_mouse_speed_and_acceleration_level {
            // Quote from documentation (http://web.archive.org/web/20241118235853/https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-mouse_event):
            // Relative mouse motion is subject to the settings for mouse speed and
            // acceleration level. An end user sets these values using the Mouse application
            // in Control Panel. An application obtains and sets these values with the
            // SystemParametersInfo function.
            //
            // The system applies two tests to the specified relative mouse motion when
            // applying acceleration. If the specified distance along either the x or y axis
            // is greater than the first mouse threshold value, and the mouse acceleration
            // level is not zero, the operating system doubles the distance. If the
            // specified distance along either the x- or y-axis is greater than the second
            // mouse threshold value, and the mouse acceleration level is equal to two, the
            // operating system doubles the distance that resulted from applying the first
            // threshold test. It is thus possible for the operating system to multiply
            // relatively-specified mouse motion along the x- or y-axis by up to four times.
            //
            // Once acceleration has been applied, the system scales the resultant value by
            // the desired mouse speed. Mouse speed can range from 1 (slowest) to 20
            // (fastest) and represents how much the pointer moves based on the distance the
            // mouse moves. The default value is 10, which results in no additional
            // modification to the mouse motion.
            debug!(
                "\x1b[93mRelative mouse move is subject to mouse speed and acceleration level\x1b[0m"
            );
            (MOUSEEVENTF_MOVE, x, y)
        } else {
            // Instead of moving the mouse by a relative amount, we calculate the resulting
            // location and move it to the absolute location so it is not subject to mouse
            // speed and acceleration levels
            debug!(
                "\x1b[93mRelative mouse move is NOT subject to mouse speed and acceleration level\x1b[0m"
            );
            let (current_x, current_y) = self.location()?;
            return self.move_mouse(current_x + x, current_y + y, Coordinate::Abs);
        };
        let input = mouse_event(flags, 0, x, y, self.dw_extra_info);
        send_input(&[input])
    }

    // Sends a scroll event to the X11 server via `XTest` extension
    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        debug!("\x1b[93mscroll(length: {length:?}, axis: {axis:?})\x1b[0m");
        let input = match axis {
            Axis::Horizontal => mouse_event(
                MOUSEEVENTF_HWHEEL,
                length * (WHEEL_DELTA as i32),
                0,
                0,
                self.dw_extra_info,
            ),
            Axis::Vertical => mouse_event(
                MOUSEEVENTF_WHEEL,
                -length * (WHEEL_DELTA as i32),
                0,
                0,
                self.dw_extra_info,
            ),
        };
        send_input(&[input])?;
        Ok(())
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mmain_display()\x1b[0m");
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if w == 0 || h == 0 {
            // Last error does not contain information about why there was an issue so it is
            // not used here
            Err(InputError::Simulate(
                "could not get the dimensions of the screen",
            ))
        } else {
            Ok((w, h))
        }
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mlocation()\x1b[0m");
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&raw mut point) }.is_ok() {
            Ok((point.x, point.y))
        } else {
            Err(InputError::Simulate(
                "could not get the current mouse location",
            ))
        }
    }
}

impl Keyboard for Enigo {
    fn fast_text(&mut self, _text: &str) -> InputResult<Option<()>> {
        Ok(None)
    }

    /// Enter the whole text string instead of entering individual keys
    /// This is much faster if you type longer text at the cost of keyboard
    /// shortcuts not getting recognized
    fn text(&mut self, text: &str) -> InputResult<()> {
        debug!("\x1b[93mtext(text: {text})\x1b[0m");
        if text.is_empty() {
            return Ok(()); // Nothing to simulate.
        }
        let mut buffer = [0; 2]; // A buffer of length 2 is large enough to encode any char in utf16

        let mut input = Vec::with_capacity(2 * text.len()); // Each char needs at least one event to press and one to release it
        for c in text.chars() {
            // Enter special characters as keys
            match c {
                '\n' => self.queue_key(&mut input, Key::Return, Direction::Click)?,
                '\r' => { // TODO: What is the correct key to type here?
                }
                '\t' => self.queue_key(&mut input, Key::Tab, Direction::Click)?,
                '\0' => Err(InputError::InvalidInput("the text contained a null byte"))?,
                _ => (),
            }

            self.queue_char(&mut input, c, &mut buffer);
        }
        send_input(&input)
    }

    /// Sends a key event to the X11 server via `XTest` extension
    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mkey(key: {key:?}, direction: {direction:?})\x1b[0m");
        let mut input = Vec::with_capacity(2);

        self.queue_key(&mut input, key, direction)?;
        send_input(&input)?;

        match direction {
            Direction::Press => {
                debug!("added the key {key:?} to the held keys");
                self.held.0.push(key);
                // TODO: Make it work that they can get released with the raw
                // function as well
            }
            Direction::Release => {
                debug!("removed the key {key:?} from the held keys");
                self.held.0.retain(|&k| k != key);
                // TODO: Make it work that they can get released with the raw
                // function as well
            }
            Direction::Click => (),
        }

        Ok(())
    }

    fn raw(&mut self, scan: u16, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mraw(scan: {scan:?}, direction: {direction:?})\x1b[0m");
        let mut input = vec![];

        let vk = VIRTUAL_KEY(Enigo::translate_key(scan, MAPVK_VSC_TO_VK_EX)?); // translate scan code to virtual key

        let mut keyflags = KEYEVENTF_SCANCODE;
        // TODO: Check if the first bytes need to be truncated if it is an extended key
        if Enigo::is_extended_key(vk) {
            keyflags |= KEYEVENTF_EXTENDEDKEY;
        }

        if direction == Direction::Click || direction == Direction::Press {
            input.push(keybd_event(keyflags, vk, scan, self.dw_extra_info));
        }
        if direction == Direction::Click || direction == Direction::Release {
            input.push(keybd_event(
                keyflags | KEYEVENTF_KEYUP,
                vk,
                scan,
                self.dw_extra_info,
            ));
        }

        send_input(&input)?;

        match direction {
            Direction::Press => {
                debug!("added the key {scan:?} to the held keys");
                self.held.1.push(scan);
                // TODO: Make it work that they can get released with the key
                // function as well
            }
            Direction::Release => {
                debug!("removed the key {scan:?} from the held keys");
                self.held.1.retain(|&k| k != scan);
                // TODO: Make it work that they can get released with the key
                // function as well
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
            windows_dw_extra_info: dw_extra_info,
            release_keys_when_dropped,
            windows_subject_to_mouse_speed_and_acceleration_level,
            ..
        } = settings;

        let held = (vec![], vec![]);

        debug!("\x1b[93mconnection established on windows\x1b[0m");

        Ok(Self {
            held,
            release_keys_when_dropped: *release_keys_when_dropped,
            dw_extra_info: dw_extra_info.unwrap_or(crate::EVENT_MARKER as usize),
            windows_subject_to_mouse_speed_and_acceleration_level:
                *windows_subject_to_mouse_speed_and_acceleration_level,
        })
    }

    pub(crate) fn get_keyboard_layout() -> HKL {
        let current_window_thread_id =
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) };
        unsafe { GetKeyboardLayout(current_window_thread_id) }
    }

    /// Generic function to translate between virtual keys and scan codes
    fn translate_key(input: u16, map_type: MAP_VIRTUAL_KEY_TYPE) -> InputResult<u16> {
        let layout = Some(Enigo::get_keyboard_layout());

        // Call MapVirtualKeyExW using the provided map_type and input
        match unsafe { MapVirtualKeyExW(input.into(), map_type, layout) }.try_into() {
            Ok(output) => {
                if output == 0 {
                    warn!(
                        "The result for the input {input:?} is zero. This usually means there was no mapping"
                    );
                }
                Ok(output)
            }
            Err(e) => {
                error!("{e:?}");
                Err(InputError::InvalidInput("result did not fit into u16"))
            }
        }
    }

    fn queue_key(
        &mut self,
        input_queue: &mut Vec<INPUT>,
        key: Key,
        direction: Direction,
    ) -> InputResult<()> {
        let Ok(vk) = VIRTUAL_KEY::try_from(key) else {
            if let Key::Unicode(c) = key {
                warn!("Unable to enter the key as a virtual key.");
                warn!("Falling back to entering it as text.");
                let mut buffer = [0; 2]; // A buffer of length 2 is large enough to encode any char in utf16
                self.queue_char(input_queue, c, &mut buffer);
                return Ok(());
            }
            return Err(InputError::Mapping(
                "This should never happen. There is a bug in the implementation".to_string(),
            ));
        };
        let scan = Enigo::translate_key(vk.0, MAPVK_VK_TO_VSC_EX)?; // Translate virtual key to scan code

        let mut keyflags = KEYBD_EVENT_FLAGS::default();

        // TODO: Check if this is needed
        //       We have a virtual key and a scan code at the end anyways
        if let Key::Unicode(_) = key {
            keyflags |= KEYEVENTF_SCANCODE;
        }

        if Enigo::is_extended_key(vk) {
            keyflags |= KEYEVENTF_EXTENDEDKEY;
        }

        if direction == Direction::Click || direction == Direction::Press {
            input_queue.push(keybd_event(keyflags, vk, scan, self.dw_extra_info));
        }
        if direction == Direction::Click || direction == Direction::Release {
            input_queue.push(keybd_event(
                keyflags | KEYEVENTF_KEYUP,
                vk,
                scan,
                self.dw_extra_info,
            ));
        }

        Ok(())
    }

    fn queue_char(&mut self, input_queue: &mut Vec<INPUT>, character: char, buffer: &mut [u16; 2]) {
        // Windows uses uft-16 encoding. We need to check
        // for variable length characters. As such some
        // characters can be 32 bit long and those are
        // encoded in what is called high and low surrogates.
        // Each are 16 bit wide and need to be sent after
        // another to the SendInput function
        let result = character.encode_utf16(buffer);
        for &utf16_surrogate in &*result {
            input_queue.push(keybd_event(
                // No need to check if it is an extended key because we only enter unicode
                // chars here
                KEYEVENTF_UNICODE,
                // Must be zero
                VIRTUAL_KEY(0),
                utf16_surrogate,
                self.dw_extra_info,
            ));
            input_queue.push(keybd_event(
                // No need to check if it is an extended key because we only enter unicode
                // chars here
                KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                // Must be zero
                VIRTUAL_KEY(0),
                // TODO: Double check if this could also be utf16_surrogate (I think it doesn't
                // make a difference)
                result[0],
                self.dw_extra_info,
            ));
        }
    }

    /// Returns a list of all currently pressed keys
    pub fn held(&mut self) -> (Vec<Key>, Vec<ScanCode>) {
        self.held.clone()
    }

    /// Returns the value that enigo's events are marked with
    #[must_use]
    pub fn get_marker_value(&self) -> usize {
        self.dw_extra_info
    }

    /// Test if the virtual key is one of the keys that need the
    /// `KEYEVENTF_EXTENDEDKEY` flag to be set
    fn is_extended_key(vk: VIRTUAL_KEY) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_DELETE, VK_DIVIDE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT,
            VK_NUMLOCK, VK_PRIOR, VK_RCONTROL, VK_RIGHT, VK_RMENU, VK_SNAPSHOT, VK_UP,
        };

        match vk {
            // Navigation keys should be injected with the extended flag to distinguish
            // them from the Numpad navigation keys. Otherwise, input Shift+<Navigation key>
            // may not have the expected result and depends on whether NUMLOCK is enabled/disabled.
            // A list of the extended keys can be found here:
            // https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input#extended-key-flag
            // TODO: The keys "BREAK (CTRL+PAUSE) key" and "ENTER key in the numeric keypad" are
            // missing
            VK_RMENU | VK_RCONTROL | VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT | VK_INSERT
            | VK_DELETE | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_NUMLOCK | VK_SNAPSHOT
            | VK_DIVIDE => true,
            _ => false,
        }
    }
}

/// Sets the current process to a specified dots per inch (dpi) awareness
/// context [see official documentation](https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-setprocessdpiawareness)
/// If you want your applications to respect the users scaling, you need to set
/// this. Otherwise the mouse coordinates and screen dimensions will be off.
///
/// It is recommended that you set the process-default DPI awareness via
/// application manifest, not an API call. See [Setting the default DPI awareness for a process](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext) for more information. Setting the process-default DPI
/// awareness via API call can lead to unexpected application behavior.
/// It also needs to be set before any APIs are used that depend on the DPI and
/// before a UI is created.
/// Enigo is a library and should not set this, because
/// it will lead to unexpected scaling of the application. Only use it for
/// examples or if you know about the consequences
///
/// # Errors
/// An error is thrown if the default API awareness mode for the process has
/// already been set (via a previous API call or within the application
/// manifest)
pub fn set_dpi_awareness() -> Result<(), ()> {
    use windows::Win32::UI::HiDpi::{PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness};

    unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE) }.map_err(|_| ())
}

impl Drop for Enigo {
    // Release the held keys before the connection is dropped
    fn drop(&mut self) {
        if !self.release_keys_when_dropped {
            return;
        }
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
}

mod test {

    #[test]
    fn extended_key() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_DELETE, VK_DIVIDE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT,
            VK_NUMLOCK, VK_PRIOR, VK_RCONTROL, VK_RIGHT, VK_RMENU, VK_SNAPSHOT, VK_UP,
        };

        let known_extended_keys = [
            VK_RMENU,    // 165
            VK_RCONTROL, // 163
            VK_UP,       // 38
            VK_DOWN,     // 40
            VK_LEFT,     // 37
            VK_RIGHT,    // 39
            VK_INSERT,   // 45
            VK_DELETE,   // 46
            VK_HOME,     // 36
            VK_END,      // 35
            VK_PRIOR,    // 33
            VK_NEXT,     // 34
            VK_NUMLOCK,  // 144
            VK_SNAPSHOT, // 44
            VK_DIVIDE,   // 111
        ];

        for key in known_extended_keys {
            assert_eq!(
                true,
                super::Enigo::is_extended_key(key),
                "Failed for {key:#?}"
            )
        }
    }

    #[test]
    fn regular_key() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK__none_, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9, VK_A,
            VK_ABNT_C1, VK_ABNT_C2, VK_ACCEPT, VK_ADD, VK_APPS, VK_ATTN, VK_B, VK_BACK,
            VK_BROWSER_BACK, VK_BROWSER_FAVORITES, VK_BROWSER_FORWARD, VK_BROWSER_HOME,
            VK_BROWSER_REFRESH, VK_BROWSER_SEARCH, VK_BROWSER_STOP, VK_C, VK_CANCEL, VK_CAPITAL,
            VK_CLEAR, VK_CONTROL, VK_CONVERT, VK_CRSEL, VK_D, VK_DBE_ALPHANUMERIC,
            VK_DBE_CODEINPUT, VK_DBE_DBCSCHAR, VK_DBE_DETERMINESTRING,
            VK_DBE_ENTERDLGCONVERSIONMODE, VK_DBE_ENTERIMECONFIGMODE, VK_DBE_ENTERWORDREGISTERMODE,
            VK_DBE_FLUSHSTRING, VK_DBE_HIRAGANA, VK_DBE_KATAKANA, VK_DBE_NOCODEINPUT,
            VK_DBE_NOROMAN, VK_DBE_ROMAN, VK_DBE_SBCSCHAR, VK_DECIMAL, VK_E, VK_EREOF, VK_ESCAPE,
            VK_EXECUTE, VK_EXSEL, VK_F, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8,
            VK_F9, VK_F10, VK_F11, VK_F12, VK_F13, VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19,
            VK_F20, VK_F21, VK_F22, VK_F23, VK_F24, VK_FINAL, VK_G, VK_GAMEPAD_A, VK_GAMEPAD_B,
            VK_GAMEPAD_DPAD_DOWN, VK_GAMEPAD_DPAD_LEFT, VK_GAMEPAD_DPAD_RIGHT, VK_GAMEPAD_DPAD_UP,
            VK_GAMEPAD_LEFT_SHOULDER, VK_GAMEPAD_LEFT_THUMBSTICK_BUTTON,
            VK_GAMEPAD_LEFT_THUMBSTICK_DOWN, VK_GAMEPAD_LEFT_THUMBSTICK_LEFT,
            VK_GAMEPAD_LEFT_THUMBSTICK_RIGHT, VK_GAMEPAD_LEFT_THUMBSTICK_UP,
            VK_GAMEPAD_LEFT_TRIGGER, VK_GAMEPAD_MENU, VK_GAMEPAD_RIGHT_SHOULDER,
            VK_GAMEPAD_RIGHT_THUMBSTICK_BUTTON, VK_GAMEPAD_RIGHT_THUMBSTICK_DOWN,
            VK_GAMEPAD_RIGHT_THUMBSTICK_LEFT, VK_GAMEPAD_RIGHT_THUMBSTICK_RIGHT,
            VK_GAMEPAD_RIGHT_THUMBSTICK_UP, VK_GAMEPAD_RIGHT_TRIGGER, VK_GAMEPAD_VIEW,
            VK_GAMEPAD_X, VK_GAMEPAD_Y, VK_H, VK_HANGEUL, VK_HANGUL, VK_HANJA, VK_HELP, VK_I,
            VK_ICO_00, VK_ICO_CLEAR, VK_ICO_HELP, VK_IME_OFF, VK_IME_ON, VK_J, VK_JUNJA, VK_K,
            VK_KANA, VK_KANJI, VK_L, VK_LAUNCH_APP1, VK_LAUNCH_APP2, VK_LAUNCH_MAIL,
            VK_LAUNCH_MEDIA_SELECT, VK_LBUTTON, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_M,
            VK_MBUTTON, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP, VK_MENU, VK_MODECHANGE, VK_MULTIPLY, VK_N, VK_NAVIGATION_ACCEPT,
            VK_NAVIGATION_CANCEL, VK_NAVIGATION_DOWN, VK_NAVIGATION_LEFT, VK_NAVIGATION_MENU,
            VK_NAVIGATION_RIGHT, VK_NAVIGATION_UP, VK_NAVIGATION_VIEW, VK_NONAME, VK_NONCONVERT,
            VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6,
            VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9, VK_O, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4,
            VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_8, VK_OEM_102, VK_OEM_ATTN, VK_OEM_AUTO,
            VK_OEM_AX, VK_OEM_BACKTAB, VK_OEM_CLEAR, VK_OEM_COMMA, VK_OEM_COPY, VK_OEM_CUSEL,
            VK_OEM_ENLW, VK_OEM_FINISH, VK_OEM_FJ_JISHO, VK_OEM_FJ_LOYA, VK_OEM_FJ_MASSHOU,
            VK_OEM_FJ_ROYA, VK_OEM_FJ_TOUROKU, VK_OEM_JUMP, VK_OEM_MINUS, VK_OEM_NEC_EQUAL,
            VK_OEM_PA1, VK_OEM_PA2, VK_OEM_PA3, VK_OEM_PERIOD, VK_OEM_PLUS, VK_OEM_RESET,
            VK_OEM_WSCTRL, VK_P, VK_PA1, VK_PACKET, VK_PAUSE, VK_PLAY, VK_PRINT, VK_PROCESSKEY,
            VK_Q, VK_R, VK_RBUTTON, VK_RETURN, VK_RSHIFT, VK_RWIN, VK_S, VK_SCROLL, VK_SELECT,
            VK_SEPARATOR, VK_SHIFT, VK_SLEEP, VK_SPACE, VK_SUBTRACT, VK_T, VK_TAB, VK_U, VK_V,
            VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP, VK_W, VK_X, VK_XBUTTON1, VK_XBUTTON2,
            VK_Y, VK_Z, VK_ZOOM,
        };

        let known_ordinary_keys = [
            VK__none_,  // 255
            VK_0,       // 48
            VK_1,       // 49
            VK_2,       // 50
            VK_3,       // 51
            VK_4,       // 52
            VK_5,       // 53
            VK_6,       // 54
            VK_7,       // 55
            VK_8,       // 56
            VK_9,       // 57
            VK_A,       // 65
            VK_ABNT_C1, // 193
            VK_ABNT_C2, // 194
            VK_ACCEPT,  // 30
            VK_ADD,     // 107
            VK_APPS,    // 93
            VK_ATTN,
            VK_B,
            VK_BACK,
            VK_BROWSER_BACK,
            VK_BROWSER_FAVORITES,
            VK_BROWSER_FORWARD,
            VK_BROWSER_HOME,
            VK_BROWSER_REFRESH,
            VK_BROWSER_SEARCH,
            VK_BROWSER_STOP,
            VK_C,
            VK_CANCEL,
            VK_CAPITAL,
            VK_CLEAR,
            VK_CONTROL,
            VK_CONVERT,
            VK_CRSEL,
            VK_D,
            VK_DBE_ALPHANUMERIC,
            VK_DBE_CODEINPUT,
            VK_DBE_DBCSCHAR,
            VK_DBE_DETERMINESTRING,
            VK_DBE_ENTERDLGCONVERSIONMODE,
            VK_DBE_ENTERIMECONFIGMODE,
            VK_DBE_ENTERWORDREGISTERMODE,
            VK_DBE_FLUSHSTRING,
            VK_DBE_HIRAGANA,
            VK_DBE_KATAKANA,
            VK_DBE_NOCODEINPUT,
            VK_DBE_NOROMAN,
            VK_DBE_ROMAN,
            VK_DBE_SBCSCHAR,
            VK_DECIMAL,
            VK_E,
            VK_EREOF,
            VK_ESCAPE,
            VK_EXECUTE,
            VK_EXSEL,
            VK_F,
            VK_F1,
            VK_F10,
            VK_F11,
            VK_F12,
            VK_F13,
            VK_F14,
            VK_F15,
            VK_F16,
            VK_F17,
            VK_F18,
            VK_F19,
            VK_F2,
            VK_F20,
            VK_F21,
            VK_F22,
            VK_F23,
            VK_F24,
            VK_F3,
            VK_F4,
            VK_F5,
            VK_F6,
            VK_F7,
            VK_F8,
            VK_F9,
            VK_FINAL,
            VK_G,
            VK_GAMEPAD_A,
            VK_GAMEPAD_B,
            VK_GAMEPAD_DPAD_DOWN,
            VK_GAMEPAD_DPAD_LEFT,
            VK_GAMEPAD_DPAD_RIGHT,
            VK_GAMEPAD_DPAD_UP,
            VK_GAMEPAD_LEFT_SHOULDER,
            VK_GAMEPAD_LEFT_THUMBSTICK_BUTTON,
            VK_GAMEPAD_LEFT_THUMBSTICK_DOWN,
            VK_GAMEPAD_LEFT_THUMBSTICK_LEFT,
            VK_GAMEPAD_LEFT_THUMBSTICK_RIGHT,
            VK_GAMEPAD_LEFT_THUMBSTICK_UP,
            VK_GAMEPAD_LEFT_TRIGGER,
            VK_GAMEPAD_MENU,
            VK_GAMEPAD_RIGHT_SHOULDER,
            VK_GAMEPAD_RIGHT_THUMBSTICK_BUTTON,
            VK_GAMEPAD_RIGHT_THUMBSTICK_DOWN,
            VK_GAMEPAD_RIGHT_THUMBSTICK_LEFT,
            VK_GAMEPAD_RIGHT_THUMBSTICK_RIGHT,
            VK_GAMEPAD_RIGHT_THUMBSTICK_UP,
            VK_GAMEPAD_RIGHT_TRIGGER,
            VK_GAMEPAD_VIEW,
            VK_GAMEPAD_X,
            VK_GAMEPAD_Y,
            VK_H,
            VK_HANGEUL,
            VK_HANGUL,
            VK_HANJA,
            VK_HELP,
            VK_I,
            VK_ICO_00,
            VK_ICO_CLEAR,
            VK_ICO_HELP,
            VK_IME_OFF,
            VK_IME_ON,
            VK_J,
            VK_JUNJA,
            VK_K,
            VK_KANA,
            VK_KANJI,
            VK_L,
            VK_LAUNCH_APP1,
            VK_LAUNCH_APP2,
            VK_LAUNCH_MAIL,
            VK_LAUNCH_MEDIA_SELECT,
            VK_LBUTTON,
            VK_LCONTROL,
            VK_LMENU,
            VK_LSHIFT,
            VK_LWIN,
            VK_M,
            VK_MBUTTON,
            VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PLAY_PAUSE,
            VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP,
            VK_MENU,
            VK_MODECHANGE,
            VK_MULTIPLY,
            VK_N,
            VK_NAVIGATION_ACCEPT,
            VK_NAVIGATION_CANCEL,
            VK_NAVIGATION_DOWN,
            VK_NAVIGATION_LEFT,
            VK_NAVIGATION_MENU,
            VK_NAVIGATION_RIGHT,
            VK_NAVIGATION_UP,
            VK_NAVIGATION_VIEW,
            VK_NONAME,
            VK_NONCONVERT,
            VK_NUMPAD0,
            VK_NUMPAD1,
            VK_NUMPAD2,
            VK_NUMPAD3,
            VK_NUMPAD4,
            VK_NUMPAD5,
            VK_NUMPAD6,
            VK_NUMPAD7,
            VK_NUMPAD8,
            VK_NUMPAD9,
            VK_O,
            VK_OEM_1,
            VK_OEM_102,
            VK_OEM_2,
            VK_OEM_3,
            VK_OEM_4,
            VK_OEM_5,
            VK_OEM_6,
            VK_OEM_7,
            VK_OEM_8,
            VK_OEM_ATTN,
            VK_OEM_AUTO,
            VK_OEM_AX,
            VK_OEM_BACKTAB,
            VK_OEM_CLEAR,
            VK_OEM_COMMA,
            VK_OEM_COPY,
            VK_OEM_CUSEL,
            VK_OEM_ENLW,
            VK_OEM_FINISH,
            VK_OEM_FJ_JISHO,
            VK_OEM_FJ_LOYA,
            VK_OEM_FJ_MASSHOU,
            VK_OEM_FJ_ROYA,
            VK_OEM_FJ_TOUROKU,
            VK_OEM_JUMP,
            VK_OEM_MINUS,
            VK_OEM_NEC_EQUAL,
            VK_OEM_PA1,
            VK_OEM_PA2,
            VK_OEM_PA3,
            VK_OEM_PERIOD,
            VK_OEM_PLUS,
            VK_OEM_RESET,
            VK_OEM_WSCTRL,
            VK_P,
            VK_PA1,
            VK_PACKET,
            VK_PAUSE,
            VK_PLAY,
            VK_PRINT,
            VK_PROCESSKEY,
            VK_Q,
            VK_R,
            VK_RBUTTON,
            VK_RETURN,
            VK_RSHIFT,
            VK_RWIN,
            VK_S,
            VK_SCROLL,
            VK_SELECT,
            VK_SEPARATOR,
            VK_SHIFT,
            VK_SLEEP,
            VK_SPACE,
            VK_SUBTRACT,
            VK_T,
            VK_TAB,
            VK_U,
            VK_V,
            VK_VOLUME_DOWN,
            VK_VOLUME_MUTE,
            VK_VOLUME_UP,
            VK_W,
            VK_X,
            VK_XBUTTON1,
            VK_XBUTTON2,
            VK_Y,
            VK_Z,
            VK_ZOOM,
        ];

        for key in known_ordinary_keys {
            assert_eq!(
                false,
                super::Enigo::is_extended_key(key),
                "Failed for {key:#?}"
            )
        }
    }
}
