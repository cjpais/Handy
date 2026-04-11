use log::{debug, error, trace, warn};

use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse,
    NewConError, Settings,
};

// If none of these features is enabled, there is no way to simulate input
#[cfg(not(any(
    feature = "wayland",
    feature = "x11rb",
    feature = "xdo",
    feature = "libei_tokio",
    feature = "libei_smol"
)))]
compile_error!(
    "either feature `wayland`, `x11rb`, `xdo`, `libei_tokio` or `libei_smol` must be enabled for this crate when using linux"
);

#[cfg(all(feature = "libei_tokio", feature = "libei_smol"))]
compile_error!(
    "the features `libei_tokio` and `libei_smol` are mutually exclusive! You have to chose which async runtime you want to use"
);

#[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
mod libei;

#[cfg(feature = "wayland")]
mod wayland;
#[cfg(any(feature = "x11rb", feature = "xdo"))]
#[cfg_attr(feature = "x11rb", path = "x11rb.rs")]
#[cfg_attr(not(feature = "x11rb"), path = "xdo.rs")]
mod x11;

#[cfg(any(feature = "wayland", feature = "x11rb"))]
mod keymap;

#[cfg(feature = "wayland")]
pub mod keymap2;

pub struct Enigo {
    held: (Vec<Key>, Vec<u16>), // Currently held keys and held keycodes
    release_keys_when_dropped: bool,
    #[cfg(feature = "wayland")]
    wayland: Option<wayland::Con>,
    #[cfg(any(feature = "x11rb", feature = "xdo"))]
    x11: Option<x11::Con>,
    #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
    libei: Option<libei::Con>,
}

impl Enigo {
    /// Create a new Enigo struct to establish the connection to simulate input
    /// with the specified settings
    ///
    /// # Errors
    /// Have a look at the documentation of `NewConError` to see under which
    /// conditions an error will be returned.
    pub fn new(settings: &Settings) -> Result<Self, NewConError> {
        let mut connection_established = false;
        #[allow(unused_variables)]
        let Settings {
            linux_delay,
            x11_display,
            wayland_display,
            release_keys_when_dropped,
            ..
        } = settings;

        let held = (Vec::new(), Vec::new());
        #[cfg(feature = "wayland")]
        let wayland = match wayland::Con::new(wayland_display.as_deref()) {
            Ok(con) => {
                connection_established = true;
                debug!("wayland connection established");
                Some(con)
            }
            Err(e) => {
                warn!("{e}");
                None
            }
        };
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        match x11_display {
            Some(name) => {
                debug!("\x1b[93mtrying to establish a x11 connection to: {name}\x1b[0m");
            }
            None => {
                debug!("\x1b[93mtrying to establish a x11 connection to $DISPLAY\x1b[0m");
            }
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        let x11 = match x11::Con::new(x11_display.as_deref(), *linux_delay) {
            Ok(con) => {
                connection_established = true;
                debug!("x11 connection established");
                Some(con)
            }
            Err(e) => {
                warn!("failed to establish x11 connection: {e}");
                None
            }
        };
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        let libei = match libei::Con::new() {
            Ok(con) => {
                connection_established = true;
                debug!("libei connection established");
                Some(con)
            }
            Err(e) => {
                warn!("failed to establish libei connection: {e}");
                None
            }
        };
        if !connection_established {
            error!("no successful connection");
            return Err(NewConError::EstablishCon("no successful connection"));
        }

        Ok(Self {
            held,
            release_keys_when_dropped: *release_keys_when_dropped,
            #[cfg(feature = "wayland")]
            wayland,
            #[cfg(any(feature = "x11rb", feature = "xdo"))]
            x11,
            #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
            libei,
        })
    }

    /// Get the delay per keypress
    #[must_use]
    pub fn delay(&self) -> u32 {
        // On Wayland there is no delay

        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_ref() {
            return con.delay();
        }
        0
    }

    /// Set the delay per keypress
    #[allow(unused_variables)]
    pub fn set_delay(&mut self, delay: u32) {
        // On Wayland there is no delay

        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            con.set_delay(delay);
        }
    }

    /// Returns a list of all currently pressed keys
    pub fn held(&mut self) -> (Vec<Key>, Vec<u16>) {
        self.held.clone()
    }
}

impl Mouse for Enigo {
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mbutton(button: {button:?}, direction: {direction:?})\x1b[0m");
        let mut success = false;
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try sending button event via libei");
            con.button(button, direction)?;
            debug!("sent button event via libei");
            success = true;
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try sending button event via wayland");
            con.button(button, direction)?;
            debug!("sent button event via wayland");
            success = true;
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try sending button event via x11");
            con.button(button, direction)?;
            debug!("sent button event via x11");
            success = true;
        }
        if success {
            debug!("sent button event");
            Ok(())
        } else {
            Err(InputError::Simulate("No protocol to enter the result"))
        }
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        debug!("\x1b[93mmove_mouse(x: {x:?}, y: {y:?}, coordinate:{coordinate:?})\x1b[0m");
        let mut success = false;
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try moving the mouse via libei");
            con.move_mouse(x, y, coordinate)?;
            debug!("moved the mouse via libei");
            success = true;
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try moving the mouse via wayland");
            con.move_mouse(x, y, coordinate)?;
            debug!("moved the mouse via wayland");
            success = true;
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try moving the mouse via x11");
            con.move_mouse(x, y, coordinate)?;
            debug!("moved the mouse via x11");
            success = true;
        }
        if success {
            debug!("moved the mouse");
            Ok(())
        } else {
            Err(InputError::Simulate("No protocol to enter the result"))
        }
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        debug!("\x1b[93mscroll(length: {length:?}, axis: {axis:?})\x1b[0m");
        let mut success = false;
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try scrolling via libei");
            con.scroll(length, axis)?;
            debug!("scrolled via libei");
            success = true;
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try scrolling via wayland");
            con.scroll(length, axis)?;
            debug!("scrolled via wayland");
            success = true;
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try scrolling via x11");
            con.scroll(length, axis)?;
            debug!("scrolled via x11");
            success = true;
        }
        if success {
            debug!("scrolled");
            Ok(())
        } else {
            Err(InputError::Simulate("No protocol to enter the result"))
        }
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mmain_display()\x1b[0m");
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_ref() {
            trace!("try getting the dimensions of the display via libei");
            return con.main_display();
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_ref() {
            trace!("try getting the dimensions of the display via wayland");
            return con.main_display();
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_ref() {
            trace!("try getting the dimensions of the display via x11");
            return con.main_display();
        }
        Err(InputError::Simulate("No protocol to enter the result"))
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        debug!("\x1b[93mlocation()\x1b[0m");
        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_ref() {
            trace!("try getting the mouse location via libei");
            return con.location();
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_ref() {
            trace!("try getting the mouse location via wayland");
            return con.location();
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_ref() {
            trace!("try getting the mouse location via x11");
            return con.location();
        }
        Err(InputError::Simulate("No protocol to enter the result"))
    }
}

impl Keyboard for Enigo {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
        debug!("\x1b[93mfast_text(text: {text})\x1b[0m");

        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try entering text fast via libei");
            con.text(text)?;
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try entering text fast via wayland");
            con.text(text)?;
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try entering text fast via x11");
            con.text(text)?;
        }
        debug!("entered the text fast");
        Ok(Some(()))
    }

    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mkey(key: {key:?}, direction: {direction:?})\x1b[0m");
        // Nothing to do
        if key == Key::Unicode('\0') {
            debug!("entering the null byte is a noop");
            return Ok(());
        }

        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try entering the key via libei");
            con.key(key, direction)?;
            debug!("entered the key via libei");
        }

        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try entering the key via wayland");
            con.key(key, direction)?;
            debug!("entered the key via wayland");
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try entering the key via x11");
            con.key(key, direction)?;
            debug!("entered the key via x11");
        }

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

        debug!("entered the key");
        Ok(())
    }

    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()> {
        debug!("\x1b[93mraw(keycode: {keycode:?}, direction: {direction:?})\x1b[0m");

        #[cfg(any(feature = "libei_tokio", feature = "libei_smol"))]
        if let Some(con) = self.libei.as_mut() {
            trace!("try entering the keycode via libei");
            con.raw(keycode, direction)?;
            debug!("entered the keycode via libei");
        }
        #[cfg(feature = "wayland")]
        if let Some(con) = self.wayland.as_mut() {
            trace!("try entering the keycode via wayland");
            con.raw(keycode, direction)?;
            debug!("entered the keycode via wayland");
        }
        #[cfg(any(feature = "x11rb", feature = "xdo"))]
        if let Some(con) = self.x11.as_mut() {
            trace!("try entering the keycode via x11");
            con.raw(keycode, direction)?;
            debug!("entered the keycode via x11");
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

        debug!("entered the keycode");
        Ok(())
    }
}

impl Drop for Enigo {
    // Release the held keys before the connection is dropped
    fn drop(&mut self) {
        if !self.release_keys_when_dropped {
            return;
        }
        let (held_keys, held_keycodes) = self.held();
        for &key in &held_keys {
            if self.key(key, Direction::Release).is_err() {
                error!("unable to release {key:?}");
            }
        }
        for &keycode in &held_keycodes {
            if self.raw(keycode, Direction::Release).is_err() {
                error!("unable to release {keycode:?}");
            }
        }
        debug!("released all held keys and held keycodes");
    }
}
