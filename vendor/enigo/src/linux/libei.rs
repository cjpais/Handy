use ashpd::desktop::remote_desktop::RemoteDesktop;
use log::{debug, error, trace, warn};
use reis::{
    PendingRequestResult,
    ei::{self, Connection},
    handshake::HandshakeResp,
};
use std::{collections::HashMap, os::unix::net::UnixStream, time::Instant};
use xkbcommon::xkb;

use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse, NewConError,
};
pub type Keycode = u32;

static INTERFACES: std::sync::LazyLock<HashMap<&'static str, u32>> =
    std::sync::LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert("ei_button", 1);
        m.insert("ei_callback", 1);
        m.insert("ei_connection", 1);
        m.insert("ei_device", 2);
        m.insert("ei_keyboard", 1);
        m.insert("ei_pingpong", 1);
        m.insert("ei_pointer", 1);
        m.insert("ei_pointer_absolute", 1);
        m.insert("ei_scroll", 1);
        m.insert("ei_seat", 1);
        m
    });

#[derive(Debug, Default, PartialEq, Clone)]
struct SeatData {
    name: Option<String>,
    capabilities: HashMap<String, u64>,
}

#[derive(Debug, Default, PartialEq, Copy, Clone)]
enum DeviceState {
    #[default]
    Paused,
    Resumed,
    Emulating,
}

#[derive(Debug, Default, PartialEq, Copy, Clone)]
struct DeviceRegion {
    offset_x: u32, // region x offset in logical pixels
    offset_y: u32, // region y offset in logical pixels
    width: u32,    // region width in logical pixels
    height: u32,   // region height in logical pixels
    scale: f32,    // the physical scale for this region
}

#[derive(Debug, Default, PartialEq, Clone)]
struct DeviceData {
    name: Option<String>,
    device_type: Option<ei::device::DeviceType>,
    interfaces: HashMap<String, reis::Object>,
    state: DeviceState,
    dimensions: Option<(u32, u32)>, // width, height
    regions: Vec<DeviceRegion>,
}

impl DeviceData {
    fn interface<T: reis::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }
}

/// The main struct for handling the event emitting
#[derive(Clone)]
pub struct Con {
    // XXX best way to handle data associated with object?
    // TODO: Release seat when dropped, so compositor knows it wont be used anymore
    seats: HashMap<ei::Seat, SeatData>,
    // XXX association with seat?
    // TODO: Release device when dropped, so compositor knows it wont be used anymore
    devices: HashMap<ei::Device, DeviceData>,
    keyboards: HashMap<ei::Keyboard, xkb::Keymap>,
    /// `None` if there was no disconnect
    disconnect: Option<(ei::connection::DisconnectReason, String)>,
    sequence: u32,
    last_serial: u32,
    context: ei::Context,
    connection: Connection,
    time_created: Instant,
}

// This is safe, we have a unique pointer.
// TODO: use Unique<c_char> once stable.
unsafe impl Send for Con {}

impl Con {
    async fn open_connection() -> ei::Context {
        use ashpd::desktop::remote_desktop::DeviceType;

        trace!("open_connection");
        if let Some(context) = ei::Context::connect_to_env().unwrap() {
            trace!("done open_connection after connect_to_env");
            context
        } else {
            debug!("Unable to find ei socket. Trying xdg desktop portal.");
            let remote_desktop = RemoteDesktop::new().await.unwrap();
            trace!("New desktop");

            // device_bitmask |= DeviceType::Touchscreen;
            let session = remote_desktop.create_session().await.unwrap();
            remote_desktop
                .select_devices(
                    &session,
                    DeviceType::Keyboard | DeviceType::Pointer,
                    None, // TODO: Allow passing the restore_token via the EnigoSettings
                    ashpd::desktop::PersistMode::Application, /* TODO: Allow passing the
                           * restore_token via the
                           * EnigoSettings */
                ) // TODO: Add DeviceType::Touchscreen once we support it in enigo
                .await
                .unwrap();
            trace!("new session");
            remote_desktop.start(&session, None).await.unwrap();
            trace!("start session");
            let fd = remote_desktop.connect_to_eis(&session).await.unwrap();
            let stream = UnixStream::from(fd);
            stream.set_nonblocking(true).unwrap(); // TODO: Check if this is a good idea
            trace!("done open_connection");
            ei::Context::new(stream).unwrap()
        }
    }

    #[allow(unnecessary_wraps)] // The wrap is needed for the libei_tokio feature
    fn custom_block_on<F: Future>(f: F) -> Result<F::Output, NewConError> {
        #[cfg(feature = "libei_tokio")]
        if tokio::runtime::Handle::try_current().is_err() {
            return Ok(tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
                .map_err(|_| NewConError::EstablishCon("failed to create tokio runtime"))?
                .block_on(f));
        }
        Ok(futures::executor::block_on(f))
    }

    #[allow(clippy::unnecessary_wraps)]
    /// Create a new Enigo instance
    pub fn new() -> Result<Self, NewConError> {
        debug!("using libei");

        let libei_name = "enigo";

        let seats = HashMap::new();
        let devices = HashMap::new();
        let keyboards = HashMap::new();
        let disconnect = None;
        let sequence = 0;
        let time_created = Instant::now();

        let context = Self::custom_block_on(Self::open_connection())?;

        let HandshakeResp {
            connection,
            serial,
            negotiated_interfaces,
        } = reis::handshake::ei_handshake_blocking(
            &context,
            libei_name,
            ei::handshake::ContextType::Sender,
        )
        .unwrap();

        trace!("main: handshake");

        context
            .flush()
            .map_err(|_| NewConError::EstablishCon("unable to flush the libei context"))?;
        trace!("main: flushed");

        let mut con = Self {
            seats,
            devices,
            keyboards,
            disconnect,
            sequence,
            last_serial: serial.wrapping_add(1),
            context,
            connection,
            time_created,
        };

        con.update(libei_name)
            .map_err(|_| NewConError::EstablishCon("unable to update the libei connection"))?;

        for (device, device_data) in con.devices.iter_mut().filter(|(_, device_data)| {
            device_data.device_type == Some(reis::ei::device::DeviceType::Virtual)
                && device_data.state == DeviceState::Resumed
            // TODO: Should all devices start emulating?
            // && device_data.interface::<ei::Keyboard>().is_some()
        }) {
            println!("Start emulating");
            if !device.is_alive() {
                return Err(NewConError::EstablishCon("ei::Device is no longer alive"));
            }
            device.start_emulating(con.last_serial, con.sequence);
            con.sequence = con.sequence.wrapping_add(1);
            device_data.state = DeviceState::Emulating;
        }

        con.update(libei_name)
            .map_err(|_| NewConError::EstablishCon("unable to update the libei connection"))?;

        Ok(con)
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, libei_name: &str) -> InputResult<()> {
        let mut had_pending_events = true;

        loop {
            debug!("update");
            if self.context.read().is_err() {
                error!("err reading");
                return Err(InputError::Simulate("Failed to update libei context"));
            }

            while let Some(result) = self.context.pending_event() {
                had_pending_events = true;
                trace!("found pending_event");

                let request = match result {
                    PendingRequestResult::Request(request) => request,
                    PendingRequestResult::ParseError(msg) => {
                        todo!()
                    }
                    PendingRequestResult::InvalidObject(object_id) => {
                        // TODO
                        error!("invalid object with id {object_id}");
                        continue;
                    }
                };

                trace!("found request");
                match request {
                    ei::Event::Handshake(handshake, request) => match request {
                        ei::handshake::Event::HandshakeVersion { version: _ } => {
                            trace!("handshake version");
                            handshake.handshake_version(1);
                            handshake.name(libei_name);
                            handshake.context_type(ei::handshake::ContextType::Sender);
                            for (interface, version) in INTERFACES.iter() {
                                handshake.interface_version(interface, *version);
                            }
                            handshake.finish();
                        }
                        ei::handshake::Event::InterfaceVersion { name, version } => {
                            // TODO: Use the interface versions
                            trace!("Received: interface {name}, version {version}");
                        }
                        ei::handshake::Event::Connection {
                            connection: _,
                            serial,
                        } => {
                            trace!("handshake connection");
                            self.last_serial = serial;
                            self.sequence = serial;
                        }
                        _ => {
                            warn!("handshake else");
                        }
                    },
                    ei::Event::Connection(connection, request) => match request {
                        ei::connection::Event::Disconnected {
                            last_serial,
                            reason,
                            explanation,
                        } => {
                            self.seats.clear();
                            self.seats.shrink_to_fit();
                            self.devices.clear();
                            self.devices.shrink_to_fit();
                            self.keyboards.clear();
                            self.keyboards.shrink_to_fit();
                            self.disconnect = Some((reason, explanation));
                            self.sequence = 0;
                            self.last_serial = last_serial;
                        }
                        ei::connection::Event::Seat { seat } => {
                            trace!("connection seat");
                            self.seats.insert(seat, SeatData::default());
                        }
                        ei::connection::Event::InvalidObject {
                            last_serial,
                            invalid_id,
                        } => {
                            // TODO: Try to recover?
                            error!(
                                "the serial {last_serial} contained an invalid object with the id {invalid_id}"
                            );
                        }
                        ei::connection::Event::Ping { ping } => {
                            debug!("ping");
                            if !ping.is_alive() {
                                return Err(InputError::Simulate(
                                    "ei::Pingpong is no longer alive",
                                ));
                            }
                            ping.done(0);
                        }
                        _ => {
                            warn!("Unknown connection event");
                        }
                    },
                    ei::Event::Seat(seat, request) => {
                        trace!("connection seat");
                        let data = self.seats.get_mut(&seat).unwrap();
                        match request {
                            ei::seat::Event::Destroyed { serial } => {
                                debug!("seat was destroyed");
                                self.seats.remove(&seat);
                            }
                            ei::seat::Event::Name { name } => {
                                data.name = Some(name);
                            }
                            ei::seat::Event::Capability { mask, interface } => {
                                data.capabilities.insert(interface, mask);
                            }
                            ei::seat::Event::Done => {
                                let mut bitmask = 0;
                                if let Some(bits) = data.capabilities.get("ei_button") {
                                    bitmask |= bits;
                                }
                                if let Some(bits) = data.capabilities.get("ei_keyboard") {
                                    bitmask |= bits;
                                }
                                if let Some(bits) = data.capabilities.get("ei_pointer") {
                                    bitmask |= bits;
                                }
                                if let Some(bits) = data.capabilities.get("ei_pointer_absolute") {
                                    bitmask |= bits;
                                }
                                if let Some(bits) = data.capabilities.get("ei_scroll") {
                                    bitmask |= bits;
                                }
                                if let Some(bits) = data.capabilities.get("ei_touchscreen") {
                                    bitmask |= bits;
                                }

                                seat.bind(bitmask);
                                trace!("done binding to seat");
                            }
                            ei::seat::Event::Device { device } => {
                                self.devices.insert(device, DeviceData::default());
                            }
                            _ => {
                                warn!("Unknown seat event");
                            }
                        }
                    }
                    ei::Event::Device(device, request) => {
                        trace!("device event");
                        let data = self.devices.get_mut(&device).unwrap();
                        match request {
                            ei::device::Event::Destroyed { serial } => {
                                debug!("device was destroyed");
                                self.devices.remove(&device);
                            }
                            ei::device::Event::Name { name } => {
                                trace!("device name");
                                data.name = Some(name);
                            }
                            ei::device::Event::DeviceType { device_type } => {
                                trace!("device type");
                                data.device_type = Some(device_type);
                            }
                            ei::device::Event::Dimensions { width, height } => {
                                trace!("device type");
                                data.dimensions = Some((width, height));
                            }
                            ei::device::Event::Region {
                                offset_x,
                                offset_y,
                                width,
                                hight: height,
                                scale,
                            } => {
                                trace!("device type");
                                data.regions.push(DeviceRegion {
                                    offset_x,
                                    offset_y,
                                    width,
                                    height,
                                    scale,
                                });
                            }
                            ei::device::Event::Interface { object } => {
                                trace!("device interface");
                                data.interfaces
                                    .insert(object.interface().to_string(), object);
                            }
                            ei::device::Event::Done => {
                                trace!("device done");
                            }
                            ei::device::Event::Resumed { serial } => {
                                debug!("device resumed");
                                self.last_serial = serial;
                                data.state = DeviceState::Resumed;
                            }
                            ei::device::Event::Paused { serial } => {
                                debug!("device paused");
                                self.last_serial = serial;
                                data.state = DeviceState::Paused;
                            }
                            _ => {
                                warn!("device else");
                            }
                        }
                    }
                    ei::Event::Keyboard(keyboard, request) => {
                        trace!("keyboard event");
                        match request {
                            ei::keyboard::Event::Destroyed { serial } => {
                                debug!("keyboard was destroyed");
                                self.keyboards.remove(&keyboard);
                            }
                            ei::keyboard::Event::Keymap {
                                keymap_type,
                                size,
                                keymap,
                            } => {
                                if keymap_type != ei::keyboard::KeymapType::Xkb {
                                    error!("The keymap is of the wrong type");
                                }
                                let context = xkb::Context::new(0);
                                self.keyboards.insert(
                                    keyboard,
                                    unsafe {
                                        xkb::Keymap::new_from_fd(
                                            &context,
                                            keymap,
                                            size as _,
                                            xkb::KEYMAP_FORMAT_TEXT_V1,
                                            0,
                                        )
                                    }
                                    .unwrap()
                                    .unwrap(),
                                );
                            }
                            ei::keyboard::Event::Modifiers {
                                serial,
                                depressed,
                                locked,
                                latched,
                                group,
                            } => { // TODO: Handle updated modifiers
                                // Notification that the EIS
                                // implementation has changed modifier states
                                // on this device. Future ei_keyboard.key
                                // requests must take the new modifier state
                                // into account.
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        warn!("else");
                    }
                }
            }

            trace!("devices: {:?}", self.devices);

            if self.context.flush().is_ok() {
                trace!("flush success");
            } else {
                error!("flush fail");
            }

            // This is needed so anything is typed
            std::thread::sleep(std::time::Duration::from_millis(10));
            trace!("update flush");
            trace!("update done");

            // We can stop looking for updates if there was no event to be handled
            // previously
            if !had_pending_events {
                break;
            }
            had_pending_events = false;
        }
        Ok(())
    }
}

impl Keyboard for Con {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
        warn!("fast text entry is not yet implemented with libei");
        // TODO: Add fast method
        Ok(None)
    }

    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        if let Some((device, device_data)) = self
            .devices
            .iter_mut()
            .find(|(_, device_data)| device_data.interface::<ei::Keyboard>().is_some())
        {
            if let Some((keyboard, keymap)) = self.keyboards.iter().next() {
                let keycode = key_to_keycode(keymap, key)?;

                if !keyboard.is_alive() {
                    return Err(InputError::Simulate("ei::Keyboard is no longer alive"));
                }

                if direction == Direction::Press || direction == Direction::Click {
                    keyboard.key(keycode - 8, ei::keyboard::KeyState::Press);

                    // It is a client bug to send more than one key request for the same key within
                    // the same ei_device.frame and the EIS implementation may ignore either or all
                    // key state changes and/or disconnect the client
                    // (source https://libinput.pages.freedesktop.org/libei/interfaces/ei_keyboard/index.html#ei_keyboardkey).
                    // That's why we need to call frame for the press and the release
                    let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?
                    device.frame(self.sequence, elapsed);
                    self.sequence = self.sequence.wrapping_add(1);
                }
                if direction == Direction::Release || direction == Direction::Click {
                    keyboard.key(keycode - 8, ei::keyboard::KeyState::Released);

                    let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?
                    device.frame(self.sequence, elapsed);
                    self.sequence = self.sequence.wrapping_add(1);
                }

                self.update("enigo").map_err(|_| {
                    InputError::Simulate("unable to update the libei connection to scroll")
                })?;
            }
        }
        Ok(())
    }

    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()> {
        let keycode = keycode as u32;

        if let Some((device, device_data)) = self
            .devices
            .iter_mut()
            .find(|(_, device_data)| device_data.interface::<ei::Keyboard>().is_some())
        {
            let keyboard = device_data.interface::<ei::Keyboard>().unwrap();
            if !keyboard.is_alive() {
                return Err(InputError::Simulate("ei::Keyboard is no longer alive"));
            }

            if direction == Direction::Press || direction == Direction::Click {
                keyboard.key(keycode - 8, ei::keyboard::KeyState::Press);
            }
            if direction == Direction::Release || direction == Direction::Click {
                keyboard.key(keycode - 8, ei::keyboard::KeyState::Released);
            }

            let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?

            device.frame(self.sequence, elapsed);
            self.sequence = self.sequence.wrapping_add(1);
            self.update("enigo").map_err(|_| {
                InputError::Simulate("unable to update the libei connection to scroll")
            })?;
        }
        Ok(())
    }
}

impl Mouse for Con {
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        if let Some((device, device_data)) = self
            .devices
            .iter_mut()
            .find(|(_, device_data)| device_data.interface::<ei::Button>().is_some())
        {
            // Do nothing if one of the mouse scroll buttons was released
            // Releasing one of the scroll mouse buttons has no effect
            if direction == Direction::Release {
                match button {
                    Button::Left
                    | Button::Right
                    | Button::Back
                    | Button::Forward
                    | Button::Middle => {}
                    Button::ScrollDown
                    | Button::ScrollUp
                    | Button::ScrollRight
                    | Button::ScrollLeft => return Ok(()),
                }
            }

            let button = match button {
                // Taken from /linux/input-event-codes.h
                Button::Left => 0x110,
                Button::Right => 0x111,
                Button::Back => 0x116,
                Button::Forward => 0x115,
                Button::Middle => 0x112,
                Button::ScrollDown => return self.scroll(1, Axis::Vertical),
                Button::ScrollUp => return self.scroll(-1, Axis::Vertical),
                Button::ScrollRight => return self.scroll(1, Axis::Horizontal),
                Button::ScrollLeft => return self.scroll(-1, Axis::Horizontal),
            };

            let vp = device_data.interface::<ei::Button>().unwrap();
            if !vp.is_alive() {
                return Err(InputError::Simulate("ei::Button is no longer alive"));
            }

            if direction == Direction::Press || direction == Direction::Click {
                trace!("vp.button({button}, ei::button::ButtonState::Pressed)");
                vp.button(button, ei::button::ButtonState::Press);
                // self.update("enigo");
                let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?
                device.frame(self.sequence, elapsed);
                self.sequence = self.sequence.wrapping_add(1);
            }

            if direction == Direction::Release || direction == Direction::Click {
                trace!("vp.button({button}, ei::button::ButtonState::Released)");
                vp.button(button, ei::button::ButtonState::Released);
                // self.update("enigo");
                let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?
                device.frame(self.sequence, elapsed);
                self.sequence = self.sequence.wrapping_add(1);
            }
            self.update("enigo").map_err(|_| {
                InputError::Simulate("unable to update the libei connection to simulate a button")
            })?;
        }
        Ok(())
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (x as f32, y as f32);
        match coordinate {
            Coordinate::Rel => {
                trace!("vp.motion_relative({x}, {y})");
                if let Some((device, device_data)) = self
                    .devices
                    .iter()
                    .find(|(_, device_data)| device_data.interface::<ei::Pointer>().is_some())
                {
                    let vp = device_data.interface::<ei::Pointer>().unwrap();
                    if !vp.is_alive() {
                        return Err(InputError::Simulate("ei::Pointer is no longer alive"));
                    }
                    vp.motion_relative(x, y);

                    let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?

                    device.frame(self.sequence, elapsed);
                    self.sequence = self.sequence.wrapping_add(1);

                    self.update("enigo").map_err(|_| {
                        InputError::Simulate(
                            "unable to update the libei connection to move the mouse",
                        )
                    })?;
                    return Ok(());
                }
            }
            Coordinate::Abs => {
                if x < 0.0 || y < 0.0 {
                    return Err(InputError::InvalidInput(
                        "the absolute coordinates cannot be negative",
                    ));
                }
                trace!("vp.motion_absolute({x}, {y}, u32::MAX, u32::MAX)");
                if let Some((device, device_data)) = self.devices.iter().find(|(_, device_data)| {
                    device_data.interface::<ei::PointerAbsolute>().is_some()
                }) {
                    let vp = device_data.interface::<ei::PointerAbsolute>().unwrap();
                    if !vp.is_alive() {
                        return Err(InputError::Simulate(
                            "ei::PointerAbsolute is no longer alive",
                        ));
                    }
                    vp.motion_absolute(x, y);

                    let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?

                    device.frame(self.sequence, elapsed);
                    self.sequence = self.sequence.wrapping_add(1);

                    self.update("enigo").map_err(|_| {
                        InputError::Simulate(
                            "unable to update the libei connection to move the mouse",
                        )
                    })?;
                    return Ok(());
                }
            }
        }
        // TODO: Improve the error
        Err(InputError::Simulate(
            "None of the devices implements the move mouse interface so there is no way to move it",
        ))
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        #[allow(clippy::cast_precision_loss)]
        let length = length as f32;
        if let Some((device, device_data)) = self
            .devices
            .iter()
            .find(|(_, device_data)| device_data.interface::<ei::Scroll>().is_some())
        {
            let (x, y) = match axis {
                Axis::Horizontal => (length, 0.0),
                Axis::Vertical => (0.0, length),
            };
            trace!("vp.scroll({x}, {y})");
            let vp = device_data.interface::<ei::Scroll>().unwrap();
            if !vp.is_alive() {
                return Err(InputError::Simulate("ei::Scroll is no longer alive"));
            }
            vp.scroll(x, y);

            let elapsed = self.time_created.elapsed().as_secs(); // Is seconds fine?

            device.frame(self.sequence, elapsed);
            self.sequence = self.sequence.wrapping_add(1);
            self.update("enigo").map_err(|_| {
                InputError::Simulate("unable to update the libei connection to scroll")
            })?;
            return Ok(());
        }
        Err(InputError::Simulate(
            "None of the devices implements the Scroll interface so there is no way to scroll",
        ))
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        // TODO Implement this
        error!(
            "You tried to get the dimensions of the main display. I don't know how this is possible under Wayland. Let me know if there is a new protocol"
        );
        Err(InputError::Simulate("Not implemented yet"))
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        // TODO Implement this
        error!(
            "You tried to get the mouse location. I don't know how this is possible under Wayland. Let me know if there is a new protocol"
        );
        Err(InputError::Simulate("Not implemented yet"))
    }
}

impl Drop for Con {
    fn drop(&mut self) {
        // TODO: Is it needed to filter or can we just stop emulating on all devices??
        for (device, _) in self.devices.iter().filter(|(_, device_data)| {
            device_data.device_type == Some(reis::ei::device::DeviceType::Virtual)
                && device_data.state == DeviceState::Emulating
        }) {
            println!("DROPPED");
            device.stop_emulating(self.last_serial);
            self.last_serial = self.last_serial.wrapping_add(1);
        }
        self.connection.disconnect(); // Let the server know we voluntarily disconnected

        let _ = self.context.flush(); // Ignore the errors if the connection was
        // dropped
    }
}

fn key_to_keycode(keymap: &xkb::Keymap, key: Key) -> InputResult<Keycode> {
    let all_keycodes = keymap.min_keycode().raw()..keymap.max_keycode().raw();

    let keysym = xkb::Keysym::from(key);
    let mut keycode = None;
    'outer: for i in all_keycodes.clone() {
        for j in 0..=1 {
            let syms = keymap.key_get_syms_by_level(xkb::Keycode::new(i), 0, j);
            if syms.contains(&keysym) {
                keycode = Some(i);
                break 'outer;
            }
        }
    }
    // Panics if the keysym was not mapped
    keycode.ok_or(crate::InputError::InvalidInput("Key is not mapped"))
}
