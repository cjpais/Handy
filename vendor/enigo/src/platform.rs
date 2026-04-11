use crate::{Keyboard, Mouse};

// Enum without any variants
// This can never get constructed
// See https://github.com/enigo-rs/enigo/pull/269 for more details
enum Never {}

pub struct Enigo {
    never: Never,
}

impl Mouse for Enigo {
    fn button(&mut self, _: crate::Button, _: crate::Direction) -> crate::InputResult<()> {
        match self.never {}
    }

    fn move_mouse(&mut self, _: i32, _: i32, _: crate::Coordinate) -> crate::InputResult<()> {
        match self.never {}
    }

    fn scroll(&mut self, _: i32, _: crate::Axis) -> crate::InputResult<()> {
        match self.never {}
    }

    fn main_display(&self) -> crate::InputResult<(i32, i32)> {
        match self.never {}
    }

    fn location(&self) -> crate::InputResult<(i32, i32)> {
        match self.never {}
    }
}

impl Keyboard for Enigo {
    fn fast_text(&mut self, _: &str) -> crate::InputResult<Option<()>> {
        match self.never {}
    }

    fn key(&mut self, _: crate::Key, _: crate::Direction) -> crate::InputResult<()> {
        match self.never {}
    }

    fn raw(&mut self, _: u16, _: crate::Direction) -> crate::InputResult<()> {
        match self.never {}
    }
}

impl Drop for Enigo {
    fn drop(&mut self) {
        match self.never {}
    }
}
