//! Enigo lets you simulate mouse and keyboard input-events as if they were
//! made by the actual hardware. It is available on Linux (X11), macOS and
//! Windows.
//!
//! It can be used for testing user interfaces on different platforms, building
//! remote control applications or just automating tasks for user interfaces
//! unaccessible by a public API or scripting language.
//!
//! This library is in an early alpha status, the API will change in
//! in the future.
//!
//! In order to use the library, you only have to know about three
//! things:
//! - [`Keyboard`] (trait): used to simulate a key click, enter text or
//!   something similar
//! - [`Mouse`] (trait): do something with the mouse or you find out the display
//!   size
//! - [`Enigo`] (struct): implements the two traits [`Keyboard`] and [`Mouse`]
//!
//! This crate previously included a simple DSL. This is no longer the case. In order to simplify the codebase and also allow serializing objects, you can now serialize and deserialize most enums and structs of this crate. You can use this instead of the DSL. This feature is hidden behind the `serde` feature. Have a look at the `serde` example to see how to use it to serialize Tokens in the [RON](https://crates.io/crates/ron) format.

//! # Examples
//! ```no_run
//! use enigo::{
//!     Button, Coordinate,
//!     Direction::{Click, Press, Release},
//!     Enigo, Key, Keyboard, Mouse, Settings,
//! };
//! let mut enigo = Enigo::new(&Settings::default()).unwrap();
//! // Paste
//! enigo.key(Key::Control, Press);
//! enigo.key(Key::Unicode('v'), Click);
//! enigo.key(Key::Control, Release);
//! // Do things with the mouse
//! enigo.move_mouse(500, 200, Coordinate::Abs);
//! enigo.button(Button::Left, Press);
//! enigo.move_mouse(100, 100, Coordinate::Rel);
//! enigo.button(Button::Left, Release);
//! // Enter text
//! enigo.text("hello world");
//! ```

#![deny(clippy::pedantic)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(deprecated)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use log::{debug, error};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(test)]
use strum_macros::EnumIter;

/// This crate contains the [`crate::agent::Token`] struct and the
/// [`crate::agent::Agent`] trait. A token is an instruction for the [`Enigo`]
/// struct to do something. If you want Enigo to simulate input, you then have
/// to tell the enigo struct to [`crate::agent::Agent::execute`] the token. Have
/// a look at the `serde` example if you'd like to read some code to see how it
/// works.
pub mod agent;

#[cfg_attr(all(unix, not(target_os = "macos")), path = "linux/mod.rs")]
#[cfg_attr(target_os = "macos", path = "macos/mod.rs")]
#[cfg_attr(target_os = "windows", path = "win/mod.rs")]
mod platform;
pub use platform::Enigo;

#[cfg(target_os = "windows")]
pub use platform::EXT;
#[cfg(target_os = "windows")]
pub use platform::set_dpi_awareness;

mod keycodes;
/// Contains the available keycodes
pub use keycodes::Key;

/// Arbitrary value to be able to distinguish events created by enigo
pub const EVENT_MARKER: u32 = 100;

/// Represents a mouse button and is used in e.g
/// [`Mouse::button`].
// Warning! If there are ANY CHANGES to this enum, we
// need to change the size of the array in the macOS implementation of the Enigo
// struct that stores the nth click for each Button
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(test, derive(EnumIter))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[doc(alias = "MouseButton")]
pub enum Button {
    /// Left mouse button
    #[cfg_attr(feature = "serde", serde(alias = "L"))]
    #[cfg_attr(feature = "serde", serde(alias = "l"))]
    #[default]
    Left,
    /// Middle mouse button
    #[cfg_attr(feature = "serde", serde(alias = "M"))]
    #[cfg_attr(feature = "serde", serde(alias = "m"))]
    Middle,
    /// Right mouse button
    #[cfg_attr(feature = "serde", serde(alias = "R"))]
    #[cfg_attr(feature = "serde", serde(alias = "r"))]
    Right,
    /// 4th mouse button. Typically performs the same function as `Browser_Back`
    #[cfg_attr(feature = "serde", serde(alias = "B"))]
    #[cfg_attr(feature = "serde", serde(alias = "b"))]
    Back,
    /// 5th mouse button. Typically performs the same function as
    /// `Browser_Forward`
    #[cfg_attr(feature = "serde", serde(alias = "F"))]
    #[cfg_attr(feature = "serde", serde(alias = "f"))]
    Forward,

    /// Scroll up button. It is better to use the
    /// [`Mouse::scroll`] method to scroll.
    #[cfg_attr(feature = "serde", serde(alias = "SU"))]
    #[cfg_attr(feature = "serde", serde(alias = "su"))]
    ScrollUp,
    /// Scroll down button. It is better to use the
    /// [`Mouse::scroll`] method to scroll.
    #[cfg_attr(feature = "serde", serde(alias = "SD"))]
    #[cfg_attr(feature = "serde", serde(alias = "sd"))]
    ScrollDown,
    /// Scroll left button. It is better to use the
    /// [`Mouse::scroll`] method to scroll.
    #[cfg_attr(feature = "serde", serde(alias = "SL"))]
    #[cfg_attr(feature = "serde", serde(alias = "sl"))]
    ScrollLeft,
    /// Scroll right button. It is better to use the
    /// [`Mouse::scroll`] method to scroll.
    #[cfg_attr(feature = "serde", serde(alias = "SR"))]
    #[cfg_attr(feature = "serde", serde(alias = "sr"))]
    ScrollRight,
}

impl fmt::Debug for Enigo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Enigo")
    }
}

/// The direction of a key or button
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[cfg_attr(feature = "serde", serde(alias = "P"))]
    #[cfg_attr(feature = "serde", serde(alias = "p"))]
    #[cfg_attr(feature = "serde", serde(alias = "Pressed"))]
    #[cfg_attr(feature = "serde", serde(alias = "pressed"))]
    Press,
    #[cfg_attr(feature = "serde", serde(alias = "R"))]
    #[cfg_attr(feature = "serde", serde(alias = "r"))]
    #[cfg_attr(feature = "serde", serde(alias = "Released"))]
    #[cfg_attr(feature = "serde", serde(alias = "released"))]
    Release,
    /// Equivalent to a press followed by a release
    #[cfg_attr(feature = "serde", serde(alias = "C"))]
    #[cfg_attr(feature = "serde", serde(alias = "c"))]
    #[cfg_attr(feature = "serde", serde(alias = "Clicked"))]
    #[cfg_attr(feature = "serde", serde(alias = "clicked"))]
    #[default]
    Click,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Specifies the axis for scrolling
pub enum Axis {
    #[cfg_attr(feature = "serde", serde(alias = "H"))]
    #[cfg_attr(feature = "serde", serde(alias = "h"))]
    Horizontal,
    #[cfg_attr(feature = "serde", serde(alias = "V"))]
    #[cfg_attr(feature = "serde", serde(alias = "v"))]
    #[default]
    Vertical,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Specifies if a coordinate is relative or absolute
pub enum Coordinate {
    #[doc(alias = "Absolute")]
    #[cfg_attr(feature = "serde", serde(alias = "A"))]
    #[cfg_attr(feature = "serde", serde(alias = "a"))]
    #[default]
    Abs,
    #[doc(alias = "Relative")]
    #[cfg_attr(feature = "serde", serde(alias = "R"))]
    #[cfg_attr(feature = "serde", serde(alias = "r"))]
    Rel,
}

/// Contains functions to simulate key presses/releases and to input text.
///
/// For entering text, the [`Keyboard::text`] function is best.
/// If you want to enter a key without having to worry about the layout or the
/// keymap, use the [`Keyboard::key`] function. If you want a
/// specific (physical) key to be pressed (e.g WASD for games), use the
/// [`Keyboard::raw`] function. The resulting keysym will depend
/// on the layout/keymap.
#[doc(alias = "KeyboardControllable")]
pub trait Keyboard {
    /// Do not use this directly. Use the [`Keyboard::text`] function.
    ///
    /// Enter the whole text string instead of entering individual keys
    /// This is much faster if you type longer text at the cost of keyboard
    /// shortcuts not getting recognized.
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(hidden)]
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>>;

    /// Enter the text
    /// Use a fast method to enter the text, if it is available. You can use
    /// unicode here like: ❤️. This works regardless of the current keyboard
    /// layout. You cannot use this function for entering shortcuts or
    /// something similar. For shortcuts, use the
    /// [`Keyboard::key`] method instead.
    ///
    /// # Errors
    /// The text should not contain any NULL bytes (`\0`). Have a look at the
    /// documentation of [`InputError`] to see under which other conditions an
    /// error will be returned.
    #[doc(alias = "key_sequence")]
    fn text(&mut self, text: &str) -> InputResult<()> {
        if text.is_empty() {
            debug!("The text to enter was empty");
            return Ok(()); // Nothing to simulate.
        }

        // Fall back to entering single keys if no fast text entry is available
        let fast_text_res = self.fast_text(text);
        match fast_text_res {
            Ok(Some(())) => {
                debug!("fast text entry was successful");
                Ok(())
            }
            Ok(None) => {
                debug!("fast text entry not available. Trying to enter individual letters now");
                for c in text.chars() {
                    self.key(Key::Unicode(c), Direction::Click)?;
                }
                Ok(())
            }
            Err(e) => {
                error!("{e}");
                Err(e)
            }
        }
    }

    /// Sends an individual key event. It will enter the keysym (virtual key).
    /// Have a look at the [`Keyboard::raw`] function, if you
    /// want to enter a keycode.
    ///
    /// Some of the keys are specific to a platform.
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "key_down", alias = "key_up", alias = "key_click")]
    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()>;

    /// Sends a raw keycode. The keycode may or may not be mapped on the current
    /// layout. You have to make sure of that yourself. This can be useful if
    /// you want to simulate a press regardless of the layout (WASD on video
    /// games). Have a look at the [`Keyboard::key`] function,
    /// if you just want to enter a specific key and don't want to worry about
    /// the layout/keymap. Windows only: If you want to enter the keycode
    /// (scancode) of an extended key, you need to set extra bits. You can
    /// for example do: `enigo.raw(45 | EXT, Direction::Click)`
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "Key::Raw")]
    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()>;
}

/// Contains functions to control the mouse and to get the size of the display.
/// Enigo uses a cartesian coordinate system for specifying coordinates. The
/// origin in this system is located in the top-left corner of the current
/// screen, with positive values extending along the axes down and to the
/// right of the origin point and it is measured in pixels. The same coordinate
/// system is used on all operating systems.
#[doc(alias = "MouseControllable")]
pub trait Mouse {
    /// Sends an individual mouse button event. You can use this for example to
    /// simulate a click of the left mouse key. Some of the buttons are specific
    /// to a platform.
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "mouse_down", alias = "mouse_up", alias = "mouse_click")]
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()>;

    /// Move the mouse cursor to the specified x and y coordinates.
    ///
    /// You can specify absolute coordinates or relative from the current
    /// position.
    ///
    /// If you use absolute coordinates, the top left corner of your monitor
    /// screen is x=0 y=0. Move the cursor down the screen by increasing the y
    /// and to the right by increasing x coordinate.
    ///
    /// If you use relative coordinates, a positive x value moves the mouse
    /// cursor `x` pixels to the right. A negative value for `x` moves the mouse
    /// cursor to the left. A positive value of y moves the mouse cursor down, a
    /// negative one moves the mouse cursor up.
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "mouse_move_to", alias = "mouse_move_relative")]
    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()>;

    /// Send a mouse scroll event
    ///
    /// # Arguments
    /// * `axis` - The axis to scroll on
    /// * `length` - Number of 15° (click) rotations of the mouse wheel to
    ///   scroll. How many lines will be scrolled depends on the current setting
    ///   of the operating system.
    ///
    /// With [`Axis::Vertical`], a positive length will result in scrolling down
    /// and negative ones up. With [`Axis::Horizontal`], a positive length
    /// will result in scrolling to the right and negative ones to the left
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "mouse_scroll_x", alias = "mouse_scroll_y")]
    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()>;

    /// Smooth scroll using pixel-based scrolling on macOS
    ///
    /// This function provides pixel-level precision for scrolling
    /// offering smoother scrolling behavior compared to the
    /// standard line-based scrolling.
    ///
    /// # Arguments
    /// * `length` - The number of pixels to scroll. Positive values scroll
    /// down/right, negative values scroll up/left.
    /// * `axis` - The axis to scroll along (Horizontal or Vertical)
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[cfg_attr(docsrs, doc(cfg(feature = "platform_specific")))]
    #[cfg(all(feature = "platform_specific", any(target_os = "macos", doc)))]
    #[doc(alias = "mouse_smooth_scroll_x", alias = "mouse_smooth_scroll_y")]
    fn smooth_scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        unimplemented!()
    }

    /// Get the (width, height) of the main display in pixels. This currently
    /// only works on the main display
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "main_display_size")]
    fn main_display(&self) -> InputResult<(i32, i32)>;

    /// Get the location of the mouse in pixels
    ///
    /// # Errors
    /// Have a look at the documentation of [`InputError`] to see under which
    /// conditions an error will be returned.
    #[doc(alias = "mouse_location")]
    fn location(&self) -> InputResult<(i32, i32)>;
}

pub type InputResult<T> = Result<T, InputError>;

/// Error when simulating input
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputError {
    /// Mapping a keycode to a keysym failed
    Mapping(String),
    /// Unmapping a keycode failed
    Unmapping(String),
    /// There was no space to map any keycodes
    NoEmptyKeycodes,
    /// There was an error with the protocol
    Simulate(&'static str),
    /// The input you want to simulate is invalid
    /// This happens for example if you want to enter text that contains NULL
    /// bytes (`\0`)
    InvalidInput(&'static str),
}

impl Display for InputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let string = match self {
            InputError::Mapping(e) => format!("error when mapping keycode to keysym: ({e})"),
            InputError::Unmapping(e) => format!("error when unmapping keysym: ({e})"),
            InputError::NoEmptyKeycodes => {
                "there were no empty keycodes that could be used".to_string()
            }
            InputError::Simulate(e) => format!("simulating input failed: ({e})"),
            InputError::InvalidInput(e) => format!("you tried to simulate invalid input: ({e})"),
        };
        write!(f, "{string}")
    }
}

impl Error for InputError {}

/// Error when establishing a new connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewConError {
    /// Error while creating the connection
    EstablishCon(&'static str),
    /// The application does not have the permission to simulate input
    NoPermission,
    /// Error when receiving a reply
    Reply,
    /// The keymap is full, so there was no space to map any keycodes to keysyms
    NoEmptyKeycodes,
}

impl Display for NewConError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let string = match self {
            NewConError::EstablishCon(e) => format!("no connection could be established: ({e})"),
            NewConError::NoPermission => {
                "the application does not have the permission to simulate input".to_string()
            }
            NewConError::Reply => {
                "there was an error with the reply from the display server. this should not happen"
                    .to_string()
            }
            NewConError::NoEmptyKeycodes => {
                "there were no empty keycodes that could be used".to_string()
            }
        };
        write!(f, "{string}")
    }
}

impl Error for NewConError {}

/// Settings for creating the Enigo struct and it's behavior
#[allow(dead_code)] // It is not dead code on other platforms
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Settings {
    /// Sleep delay on Linux X11
    pub linux_delay: u32,
    /// Display name to connect to when using Linux X11
    pub x11_display: Option<String>,
    /// Display name to connect to when using Linux Wayland
    pub wayland_display: Option<String>,
    /// Arbitrary value to be able to distinguish events created by enigo
    /// All events will be marked with this value in the dwExtraInfo field
    pub windows_dw_extra_info: Option<usize>,
    /// Arbitrary value to be able to distinguish events created by enigo
    /// All events will be marked with this value in the
    /// `EVENT_SOURCE_USER_DATA` field
    pub event_source_user_data: Option<i64>,
    /// Set this to true if you want all held keys to get released when Enigo
    /// gets dropped. The default is true.
    pub release_keys_when_dropped: bool,
    /// Open a prompt to ask the user for the permission to simulate input if
    /// they are missing. This only works on macOS. The default is true.
    pub open_prompt_to_get_permissions: bool,
    /// The simulated input is independent from the pressed keys on the
    /// physical keyboard. This only works on macOS.
    /// The default is true. If the Shift key for example is pressed,
    /// following simulated input will not be capitalized.
    pub independent_of_keyboard_state: bool,
    /// If this is set to true, the relative mouse motion will be subject to the
    /// settings for mouse speed and acceleration level. An end user sets
    /// these values using the Mouse application in Control Panel. An
    /// application obtains and sets these values with the
    /// `windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoA`
    /// function. The default value is false.
    pub windows_subject_to_mouse_speed_and_acceleration_level: bool,
}

impl Default for Settings {
    fn default() -> Self {
        debug!("using default settings");
        Self {
            linux_delay: 12,
            x11_display: None,
            wayland_display: None,
            windows_dw_extra_info: None,
            event_source_user_data: None,
            release_keys_when_dropped: true,
            open_prompt_to_get_permissions: true,
            independent_of_keyboard_state: true,
            windows_subject_to_mouse_speed_and_acceleration_level: false,
        }
    }
}

#[cfg(test)]
/// Module containing all the platform independent tests for the traits
mod tests;
