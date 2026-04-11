use crate::{Axis, Button, Coordinate, Direction, Enigo, InputResult, Key, Keyboard, Mouse};

use log::error;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    /// Call the [`Keyboard::text`] fn with the string as text
    #[cfg_attr(feature = "serde", serde(alias = "T"))]
    #[cfg_attr(feature = "serde", serde(alias = "t"))]
    Text(String),
    /// Call the [`Keyboard::key`] fn with the given key and direction
    #[cfg_attr(feature = "serde", serde(alias = "K"))]
    #[cfg_attr(feature = "serde", serde(alias = "k"))]
    Key(
        Key,
        #[cfg_attr(feature = "serde", serde(default))] Direction,
    ),
    /// Call the [`Keyboard::raw`] fn with the given keycode and direction
    #[cfg_attr(feature = "serde", serde(alias = "R"))]
    #[cfg_attr(feature = "serde", serde(alias = "r"))]
    Raw(
        u16,
        #[cfg_attr(feature = "serde", serde(default))] Direction,
    ),
    /// Call the [`Mouse::button`] fn with the given mouse button and direction
    #[cfg_attr(feature = "serde", serde(alias = "B"))]
    #[cfg_attr(feature = "serde", serde(alias = "b"))]
    Button(
        Button,
        #[cfg_attr(feature = "serde", serde(default))] Direction,
    ),
    /// Call the [`Mouse::move_mouse`] fn. The first i32 is the value to move on
    /// the x-axis and the second i32 is the value to move on the y-axis. The
    /// coordinate defines if the given coordinates are absolute of relative to
    /// the current position of the mouse.
    #[cfg_attr(feature = "serde", serde(alias = "M"))]
    #[cfg_attr(feature = "serde", serde(alias = "m"))]
    MoveMouse(
        i32,
        i32,
        #[cfg_attr(feature = "serde", serde(default))] Coordinate,
    ),
    /// Call the [`Mouse::scroll`] fn.
    #[cfg_attr(feature = "serde", serde(alias = "S"))]
    #[cfg_attr(feature = "serde", serde(alias = "s"))]
    Scroll(i32, #[cfg_attr(feature = "serde", serde(default))] Axis),
    /// Call the [`Mouse::smooth_scroll`] fn.
    /// Only available on macOS with platform_specific feature.
    #[cfg(all(feature = "platform_specific", target_os = "macos"))]
    #[cfg_attr(feature = "serde", serde(alias = "SS"))]
    #[cfg_attr(feature = "serde", serde(alias = "ss"))]
    SmoothScroll(i32, #[cfg_attr(feature = "serde", serde(default))] Axis),
    /// Call the [`Mouse::location`] fn and compare the return values with
    /// the values of this enum. Log an error if they are not equal.
    /// This variant contains the EXPECTED location of the mouse
    #[cfg_attr(feature = "serde", serde(alias = "L"))]
    #[cfg_attr(feature = "serde", serde(alias = "l"))]
    Location(i32, i32),
    /// Call the [`Mouse::main_display`] fn and compare the return values with
    /// the values of this enum. Log an error if they are not equal.
    /// This variant contains the EXPECTED size of the main display
    #[cfg_attr(feature = "serde", serde(alias = "D"))]
    #[cfg_attr(feature = "serde", serde(alias = "d"))]
    MainDisplay(i32, i32),
}

pub trait Agent
where
    Self: Keyboard,
    Self: Mouse,
{
    /// Execute the action associated with the token. A [`Token::Text`] will
    /// enter text, a [`Token::Scroll`] will scroll and so forth. Have a look at
    /// the documentation of the [`Token`] enum for more information.
    ///
    /// # Errors
    ///
    /// Same as the individual functions. Have a look at [`InputResult`] for a
    /// list of possible errors
    fn execute(&mut self, token: &Token) -> InputResult<()> {
        match token {
            Token::Text(text) => self.text(text),
            Token::Key(key, direction) => self.key(*key, *direction),
            Token::Raw(keycode, direction) => self.raw(*keycode, *direction),
            Token::Button(button, direction) => self.button(*button, *direction),
            Token::MoveMouse(x, y, coordinate) => self.move_mouse(*x, *y, *coordinate),
            Token::Scroll(length, axis) => self.scroll(*length, *axis),
            #[cfg(all(feature = "platform_specific", target_os = "macos"))]
            Token::SmoothScroll(length, axis) => self.smooth_scroll(*length, *axis),
            Token::Location(expected_x, expected_y) => match self.location() {
                Ok((actual_x, actual_y)) => {
                    if actual_x != *expected_x || actual_y != *expected_y {
                        error!("The mouse is not at the expected location");
                    }
                    Ok(())
                }
                Err(e) => {
                    error!("There was an error getting the location of the mouse");
                    Err(e)
                }
            },
            Token::MainDisplay(expected_width, expected_height) => match self.main_display() {
                Ok((actual_x, actual_y)) => {
                    if actual_x != *expected_width || actual_y != *expected_height {
                        error!("The size of the main display is not what was expected");
                    }
                    Ok(())
                }
                Err(e) => {
                    error!("There was an error getting the size of the main display");
                    Err(e)
                }
            },
        }
    }
}

impl Agent for Enigo {}
