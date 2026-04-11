use std::{
    ffi::{CString, c_char, c_int, c_ulong, c_void},
    ptr,
};

use libc::useconds_t;

use log::debug;

use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse, NewConError,
};
use xkeysym::Keysym;

const CURRENT_WINDOW: c_ulong = 0;
const XDO_SUCCESS: c_int = 0;

type Window = c_ulong;
type Xdo = *const c_void;

#[link(name = "xdo")]
unsafe extern "C" {
    fn xdo_free(xdo: Xdo);
    fn xdo_new(display: *const c_char) -> Xdo;

    fn xdo_click_window(xdo: Xdo, window: Window, button: c_int) -> c_int;
    fn xdo_mouse_down(xdo: Xdo, window: Window, button: c_int) -> c_int;
    fn xdo_mouse_up(xdo: Xdo, window: Window, button: c_int) -> c_int;
    fn xdo_move_mouse(xdo: Xdo, x: c_int, y: c_int, screen: c_int) -> c_int;
    fn xdo_move_mouse_relative(xdo: Xdo, x: c_int, y: c_int) -> c_int;

    fn xdo_enter_text_window(
        xdo: Xdo,
        window: Window,
        string: *const c_char,
        delay: useconds_t,
    ) -> c_int;
    fn xdo_send_keysequence_window(
        xdo: Xdo,
        window: Window,
        string: *const c_char,
        delay: useconds_t,
    ) -> c_int;
    fn xdo_send_keysequence_window_down(
        xdo: Xdo,
        window: Window,
        string: *const c_char,
        delay: useconds_t,
    ) -> c_int;
    fn xdo_send_keysequence_window_up(
        xdo: Xdo,
        window: Window,
        string: *const c_char,
        delay: useconds_t,
    ) -> c_int;

    fn xdo_get_viewport_dimensions(
        xdo: Xdo,
        width: *mut c_int,
        height: *mut c_int,
        screen: c_int,
    ) -> c_int;

    fn xdo_get_mouse_location2(
        xdo: Xdo,
        x: *mut c_int,
        y: *mut c_int,
        screen: *mut c_int,
        window: *mut Window,
    ) -> c_int;
}

fn mousebutton(button: Button) -> c_int {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
        Button::ScrollUp => 4,
        Button::ScrollDown => 5,
        Button::ScrollLeft => 6,
        Button::ScrollRight => 7,
        Button::Back => 8,
        Button::Forward => 9,
    }
}

/// The main struct for handling the event emitting
pub struct Con {
    xdo: Xdo,
    delay: u32, // microseconds
}
// This is safe, we have a unique pointer.
// TODO: use Unique<c_char> once stable.
unsafe impl Send for Con {}

impl Con {
    /// Create a new Enigo instance
    /// If no `dyp_name` is provided, the $DISPLAY environment variable is read
    /// and used instead
    pub fn new(dyp_name: Option<&str>, delay: u32) -> Result<Self, NewConError> {
        debug!("using xdo");
        let xdo = match dyp_name {
            Some(name) => {
                let Ok(string) = CString::new(name) else {
                    return Err(NewConError::EstablishCon(
                        "the display name contained a null byte",
                    ));
                };
                unsafe { xdo_new(string.as_ptr()) }
            }
            None => unsafe { xdo_new(ptr::null()) },
        };
        // If it was not possible to establish a connection, a NULL pointer is returned
        if xdo.is_null() {
            return Err(NewConError::EstablishCon(
                "establishing a connection to the display name was unsuccessful",
            ));
        }
        Ok(Self {
            xdo,
            delay: delay * 1000,
        })
    }

    /// Get the delay per keypress in milliseconds
    #[must_use]
    pub fn delay(&self) -> u32 {
        self.delay / 1000
    }

    /// Set the delay per keypress in milliseconds
    pub fn set_delay(&mut self, delay: u32) {
        self.delay = delay * 1000;
    }
}

impl Drop for Con {
    fn drop(&mut self) {
        unsafe {
            xdo_free(self.xdo);
        }
    }
}

impl Keyboard for Con {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
        let Ok(string) = CString::new(text) else {
            return Err(InputError::InvalidInput(
                "the text to enter contained a NULL byte ('\\0’), which is not allowed",
            ));
        };
        debug!(
            "xdo_enter_text_window with string {string:?}, delay {}",
            self.delay
        );
        let res = unsafe {
            xdo_enter_text_window(
                self.xdo,
                CURRENT_WINDOW,
                string.as_ptr(),
                self.delay as useconds_t,
            )
        };
        if res != XDO_SUCCESS {
            return Err(InputError::Simulate("unable to enter text"));
        }
        Ok(Some(()))
    }

    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        let keysym = Keysym::from(key);
        let keysym_name = format!("{keysym:?}");
        let keysym_name = keysym_name.replace("XK_", ""); // TODO: remove if xkeysym changed their names (https://github.com/rust-windowing/xkeysym/issues/18)

        let Ok(string) = CString::new(keysym_name) else {
            // this should never happen, because none of the names contain NULL bytes
            return Err(InputError::InvalidInput(
                "the name of the keysym contained a null byte",
            ));
        };

        let res = match direction {
            Direction::Click => {
                debug!(
                    "xdo_send_keysequence_window with string {string:?}, delay {}",
                    self.delay
                );
                unsafe {
                    xdo_send_keysequence_window(
                        self.xdo,
                        CURRENT_WINDOW,
                        string.as_ptr(),
                        self.delay as useconds_t,
                    )
                }
            }
            Direction::Press => {
                debug!(
                    "xdo_send_keysequence_window_down with string {string:?}, delay {}",
                    self.delay
                );
                unsafe {
                    xdo_send_keysequence_window_down(
                        self.xdo,
                        CURRENT_WINDOW,
                        string.as_ptr(),
                        self.delay as useconds_t,
                    )
                }
            }
            Direction::Release => {
                debug!(
                    "xdo_send_keysequence_window_up with string {string:?}, delay {}",
                    self.delay
                );
                unsafe {
                    xdo_send_keysequence_window_up(
                        self.xdo,
                        CURRENT_WINDOW,
                        string.as_ptr(),
                        self.delay as useconds_t,
                    )
                }
            }
        };
        if res != XDO_SUCCESS {
            return Err(InputError::Simulate("unable to enter key"));
        }
        Ok(())
    }

    fn raw(&mut self, _keycode: u16, _direction: Direction) -> InputResult<()> {
        // TODO: Lookup the key name for the keycode and then enter that with xdotool.
        // This is a bit weird, because xdotool will then do the reverse. Maybe there is
        // a better way?
        todo!("You cant enter raw keycodes with xdotool")
    }
}

impl Mouse for Con {
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        let button = mousebutton(button);
        let res = match direction {
            Direction::Press => {
                debug!("xdo_mouse_down with mouse button {button}");
                unsafe { xdo_mouse_down(self.xdo, CURRENT_WINDOW, button) }
            }
            Direction::Release => {
                debug!("xdo_mouse_up with mouse button {button}");
                unsafe { xdo_mouse_up(self.xdo, CURRENT_WINDOW, button) }
            }
            Direction::Click => {
                debug!("xdo_click_window with mouse button {button}");
                unsafe { xdo_click_window(self.xdo, CURRENT_WINDOW, button) }
            }
        };
        if res != XDO_SUCCESS {
            return Err(InputError::Simulate("unable to enter mouse button"));
        }
        Ok(())
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        let res = match coordinate {
            Coordinate::Rel => {
                debug!("xdo_move_mouse_relative with x {x}, y {y}");
                unsafe { xdo_move_mouse_relative(self.xdo, x as c_int, y as c_int) }
            }
            Coordinate::Abs => {
                debug!("xdo_move_mouse with mouse button with x {x}, y {y}");
                unsafe { xdo_move_mouse(self.xdo, x as c_int, y as c_int, 0) }
            }
        };
        if res != XDO_SUCCESS {
            return Err(InputError::Simulate("unable to move the mouse"));
        }
        Ok(())
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        let button = match (length.is_positive(), axis) {
            (true, Axis::Vertical) => Button::ScrollDown,
            (false, Axis::Vertical) => Button::ScrollUp,
            (true, Axis::Horizontal) => Button::ScrollRight,
            (false, Axis::Horizontal) => Button::ScrollLeft,
        };

        for _ in 0..length.abs() {
            self.button(button, Direction::Click)?;
        }
        Ok(())
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        const MAIN_SCREEN: i32 = 0;
        let mut width = 0;
        let mut height = 0;

        debug!("xdo_get_viewport_dimensions");
        let res = unsafe {
            xdo_get_viewport_dimensions(self.xdo, &raw mut width, &raw mut height, MAIN_SCREEN)
        };

        if res != XDO_SUCCESS {
            return Err(InputError::Simulate("unable to get the main display"));
        }
        Ok((width, height))
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        let mut x = 0;
        let mut y = 0;
        let mut unused_screen_index = 0;
        let mut unused_window_index = CURRENT_WINDOW;
        debug!("xdo_get_mouse_location2");
        let res = unsafe {
            xdo_get_mouse_location2(
                self.xdo,
                &raw mut x,
                &raw mut y,
                &raw mut unused_screen_index,
                &raw mut unused_window_index,
            )
        };
        if res != XDO_SUCCESS {
            return Err(InputError::Simulate(
                "unable to get the position of the mouse",
            ));
        }
        Ok((x, y))
    }
}
