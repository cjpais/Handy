use std::{
    convert::TryInto as _,
    env,
    num::Wrapping,
    os::{fd::AsFd, unix::net::UnixStream},
    path::PathBuf,
    time::Instant,
};

use log::{debug, error, trace, warn};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy as _, QueueHandle, WEnum,
    protocol::{
        wl_keyboard::{self, WlKeyboard},
        wl_output::{self, Mode, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_registry,
        wl_seat::{self, Capability},
    },
};
use wayland_protocols_misc::{
    zwp_input_method_v2::client::{zwp_input_method_manager_v2, zwp_input_method_v2},
    zwp_virtual_keyboard_v1::client::{zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1},
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};
use xkbcommon::xkb;

use super::keymap2::Keymap2;
use crate::{
    Axis, Button, Coordinate, Direction, InputError, InputResult, Key, Keyboard, Mouse,
    NewConError, keycodes::ModifierBitflag,
};

pub type Keycode = u32;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
struct OutputInfo {
    width: i32,
    height: i32,
    transform: bool,
}

pub struct Con {
    event_queue: EventQueue<WaylandState>,
    state: WaylandState,
    base_time: std::time::Instant,
}

impl Con {
    /// Tries to establish a new Wayland connection
    ///
    /// # Errors
    /// TODO
    pub fn new(dpy_name: Option<&str>) -> Result<Self, NewConError> {
        // Setup Wayland connection
        let connection = Self::setup_connection(dpy_name)?;

        // Check to see if there was an error trying to connect
        if let Some(e) = connection.protocol_error() {
            error!(
                "unknown wayland initialization failure: {} {} {} {}",
                e.code, e.object_id, e.object_interface, e.message
            );
            return Err(NewConError::EstablishCon(
                "failed to connect to wayland. there was a protocol error",
            ));
        }

        let mut state = WaylandState::default();

        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();

        // Start registry
        let display = connection.display();
        let _ = display.get_registry(&qh, ()); // TODO: Check if we can drop the registry here

        // Receive the list of available globals
        event_queue
            .roundtrip(&mut state)
            .map_err(|_| NewConError::EstablishCon("Wayland roundtrip failed"))?;

        // Tell the compositor which globals the client wants to bind to
        event_queue
            .roundtrip(&mut state)
            .map_err(|_| NewConError::EstablishCon("Wayland roundtrip failed"))?;

        // Initialize the protocols (get the input_method, virtual_keyboard and
        // virtual_pointer)
        event_queue
            .roundtrip(&mut state)
            .map_err(|_| NewConError::EstablishCon("Wayland roundtrip failed"))?;

        // One extra, just to be sure
        event_queue
            .roundtrip(&mut state)
            .map_err(|_| NewConError::EstablishCon("Wayland roundtrip failed"))?;

        let mut connection = Self {
            event_queue,
            state,
            base_time: Instant::now(),
        };

        if connection.state.virtual_keyboard.is_some() {
            connection
                .update_keymap()
                .map_err(|_| NewConError::EstablishCon("Sending the initial keymap failed"))?;
        }

        connection.check_available_protocols()?;

        Ok(connection)
    }

    // Helper function for setting up the Wayland connection
    fn setup_connection(dyp_name: Option<&str>) -> Result<Connection, NewConError> {
        let connection = if let Some(dyp_name) = dyp_name {
            debug!("\x1b[93mtrying to establish a connection to: {dyp_name}\x1b[0m");
            let socket_path = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).ok_or(
                NewConError::EstablishCon("Missing XDG_RUNTIME_DIR env variable"),
            )?;
            let stream = UnixStream::connect(socket_path.join(dyp_name))
                .map_err(|_| NewConError::EstablishCon("Failed to open Unix stream"))?;
            Connection::from_socket(stream)
        } else {
            debug!("\x1b[93mtrying to establish a connection to $WAYLAND_DISPLAY\x1b[0m");
            Connection::connect_to_env()
        };

        connection.map_err(|_| {
            error!("Failed to connect to Wayland. Try setting 'WAYLAND_DISPLAY=wayland-0'.");
            NewConError::EstablishCon("Wayland connection failed.")
        })
    }

    fn check_available_protocols(&self) -> Result<(), NewConError> {
        debug!(
            "protocols available\nvirtual_keyboard: {}\ninput_method: {}\nvirtual_pointer: {}",
            self.state.virtual_keyboard.is_some(),
            self.state.input_method.is_some(),
            self.state.virtual_pointer.is_some(),
        );

        if self.state.virtual_keyboard.is_none()
            && self.state.input_method.is_none()
            && self.state.virtual_pointer.is_none()
        {
            return Err(NewConError::EstablishCon(
                "no protocol available to simulate input",
            ));
        }
        Ok(())
    }

    /// Get the duration since the Keymap was created
    fn get_time(&self) -> u32 {
        let duration = self.base_time.elapsed();
        let time = duration.as_millis();
        time.try_into().unwrap_or(u32::MAX)
    }

    /// Press/Release a keycode
    ///
    /// # Errors
    /// TODO
    fn send_key_event(&mut self, keycode: Keycode, direction: Direction) -> InputResult<()> {
        trace!("send_key_event(&mut self, keycode: {keycode}, direction: {direction:?})");
        let vk = self
            .state
            .virtual_keyboard
            .as_ref()
            .ok_or(InputError::Simulate("no way to enter key"))?;
        is_alive(vk)?;

        let time = self.get_time();
        let keycode = keycode - 8; // Adjust by 8 due to the xkb/xwayland requirements
        let direction_wayland = match direction {
            Direction::Press => 1,
            Direction::Release => 0,
            Direction::Click => {
                return Err(InputError::Simulate(
                    "impossible direction, this should never be possible. This function must never be called with Direction::Click",
                ));
            }
        };

        vk.key(time, keycode, direction_wayland);

        self.flush()?;
        Ok(())
    }

    /// Sends a modifier event with the updated bitflag of the modifiers to the
    /// compositor
    fn send_modifier_event(
        &mut self,
        depressed_mods_new: ModifierBitflag,
        latched_mods_new: ModifierBitflag,
        locked_mods_new: ModifierBitflag,
        effective_layout_new: u32,
    ) -> InputResult<()> {
        // Retrieve virtual keyboard or return an error early if None
        let vk = self
            .state
            .virtual_keyboard
            .as_ref()
            .ok_or(InputError::Simulate("no way to enter key"))?;

        // Check if virtual keyboard is still alive
        is_alive(vk)?;

        // Log the modifier event
        trace!(
            "vk.modifiers({depressed_mods_new}, {latched_mods_new}, {locked_mods_new}, {effective_layout_new})"
        );

        // Send the modifier event
        vk.modifiers(
            depressed_mods_new,
            latched_mods_new,
            locked_mods_new,
            effective_layout_new,
        );

        self.flush()?;

        Ok(())
    }

    /// Apply the current keymap
    ///
    /// # Errors
    /// TODO
    fn update_keymap(&mut self) -> InputResult<()> {
        debug!("update_keymap(&mut self)");
        let vk = self
            .state
            .virtual_keyboard
            .as_ref()
            .ok_or(InputError::Simulate("no way to apply keymap"))?;
        is_alive(vk)?;

        let keymap = match &self.state.seat_keymap {
            Some(keymap) => keymap,
            None => &Keymap2::default()
                .map_err(|()| InputError::Mapping("could not update the keymap".to_string()))?,
        };

        let (format, keymap_file, size) = keymap
            .format_file_size()
            .map_err(|()| InputError::Mapping("could not update the keymap".to_string()))?;
        vk.keymap(format, keymap_file.as_fd(), size);

        debug!("wait for response after keymap call");
        self.event_queue
            .roundtrip(&mut self.state)
            .map_err(|_| InputError::Simulate("Wayland roundtrip failed"))?;

        Ok(())
    }

    /// Flush the Wayland queue
    fn flush(&self) -> InputResult<()> {
        self.event_queue.flush().map_err(|e| {
            error!("{e:?}");
            InputError::Simulate("could not flush Wayland queue")
        })?;
        trace!("flushed event queue");
        Ok(())
    }
}

impl Drop for Con {
    // Destroy the Wayland objects we created
    fn drop(&mut self) {
        if let Some(vk) = self.state.virtual_keyboard.take() {
            vk.destroy();
        }
        if let Some(im) = self.state.input_method.take() {
            im.destroy();
        }
        if let Some(vp) = self.state.virtual_pointer.take() {
            vp.destroy();
        }

        if self.flush().is_err() {
            error!("could not flush wayland queue");
        }
        trace!("wayland objects were destroyed");

        let _ = self.event_queue.roundtrip(&mut self.state);
    }
}

#[derive(Default)]
/// Stores the manager for the various protocols
struct WaylandState {
    // interface name, global id, version
    globals: Vec<(String, u32, u32)>,
    outputs: Vec<(WlOutput, OutputInfo)>,
    keyboard_manager: Option<zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1>,
    virtual_keyboard: Option<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1>,
    im_manager: Option<zwp_input_method_manager_v2::ZwpInputMethodManagerV2>,
    input_method: Option<zwp_input_method_v2::ZwpInputMethodV2>,
    im_serial: Wrapping<u32>,
    pointer_manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    virtual_pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    seat: Option<wl_seat::WlSeat>,
    seat_keyboard: Option<WlKeyboard>,
    seat_keymap: Option<Keymap2>,
    seat_pointer: Option<WlPointer>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    #[allow(clippy::too_many_lines)]
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            // Store global to later bind to them
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                debug!("Global announced: {interface} (name: {name}, version: {version})");
                match &interface[..] {
                    "wl_seat" => {
                        let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, version, qh, ());
                        // We don't know if the seat or the im_manager is created first and we need
                        // both to get the input_method
                        if let Some(im_manager) = &state.im_manager {
                            if state.input_method.is_none() {
                                let input_method = im_manager.get_input_method(&seat, qh, ());
                                state.input_method = Some(input_method);
                                state.im_serial = Wrapping(0u32);
                            }
                        }
                        // We don't know if the seat or the keyboard_manager is created first and we
                        // need both to get the virtual_keyboard
                        if let Some(keyboard_manager) = &state.keyboard_manager {
                            if state.virtual_keyboard.is_none() {
                                let virtual_keyboard =
                                    keyboard_manager.create_virtual_keyboard(&seat, qh, ());
                                state.virtual_keyboard = Some(virtual_keyboard);
                            }
                        }
                        // We don't know if the seat or the pointer_manager is created first and we
                        // need both to get the virtual_pointer
                        if let Some(pointer_manager) = &state.pointer_manager {
                            if state.virtual_pointer.is_none() {
                                let virtual_pointer =
                                    pointer_manager.create_virtual_pointer(Some(&seat), qh, ());
                                state.virtual_pointer = Some(virtual_pointer);
                            }
                        }

                        state.seat = Some(seat);
                    }
                    "wl_output" => {
                        let wl_output =
                            registry.bind::<wl_output::WlOutput, _, _>(name, version, qh, ());
                        state.outputs.push((wl_output, OutputInfo::default()));
                    }
                    "zwp_input_method_manager_v2" => {
                        let im_manager = registry
                            .bind::<zwp_input_method_manager_v2::ZwpInputMethodManagerV2, _, _>(
                            name,
                            version,
                            qh,
                            (),
                        );
                        // We don't know if the seat or the im_manager is created first and we need
                        // both to get the input_method
                        if let Some(seat) = &state.seat {
                            if state.input_method.is_none() {
                                let input_method = im_manager.get_input_method(seat, qh, ());
                                state.input_method = Some(input_method);
                                state.im_serial = Wrapping(0u32);
                            }
                        }
                        state.im_manager = Some(im_manager);
                    }
                    "zwp_virtual_keyboard_manager_v1" => {
                        let keyboard_manager = registry
                        .bind::<zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, _, _>(
                        name,
                        version,
                        qh,
                        (),
                    );
                        // We don't know if the seat or the keyboard_manager is created first and we
                        // need both to get the virtual_keyboard
                        if let Some(seat) = &state.seat {
                            if state.virtual_keyboard.is_none() {
                                let virtual_keyboard =
                                    keyboard_manager.create_virtual_keyboard(seat, qh, ());
                                state.virtual_keyboard = Some(virtual_keyboard);
                            }
                        }
                        state.keyboard_manager = Some(keyboard_manager);
                    }
                    "zwlr_virtual_pointer_manager_v1" => {
                        let pointer_manager = registry
                        .bind::<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _, _>(
                        name,
                        version,
                        qh,
                        (),
                    );
                        // We don't know if the seat or the pointer_manager is created first and we
                        // need both to get the virtual_pointer
                        if let Some(seat) = &state.seat {
                            if state.virtual_pointer.is_none() {
                                let virtual_pointer =
                                    pointer_manager.create_virtual_pointer(Some(seat), qh, ());
                                state.virtual_pointer = Some(virtual_pointer);
                            }
                        }
                        state.pointer_manager = Some(pointer_manager);
                    }
                    _ => {}
                }
                state.globals.push((interface, name, version));
            }
            // Remove global from store when it becomes unavailable
            wl_registry::Event::GlobalRemove { name } => {
                debug!("Global removed: {name}");
                let Some((idx, (interface, name, _))) = state
                    .globals
                    .iter()
                    .enumerate()
                    .find(|(_, (_, n, _))| *n == name)
                else {
                    return;
                };

                match &interface[..] {
                    "wl_seat" => {
                        state.virtual_keyboard = None;
                        state.keyboard_manager = None;
                        state.input_method = None;
                        state.im_manager = None;
                        state.virtual_pointer = None;
                        state.pointer_manager = None;
                        state.seat_keyboard = None;
                        state.seat_keymap = None;
                        state.seat_pointer = None;
                        state.seat = None;
                    }
                    "wl_output" => {
                        state
                            .outputs
                            .retain(|(output, _)| output.id().protocol_id() != *name);
                    }
                    "zwp_input_method_manager_v2" => {
                        state.input_method = None;
                        state.im_manager = None;
                    }
                    "zwp_virtual_keyboard_manager_v1" => {
                        state.virtual_keyboard = None;
                        state.keyboard_manager = None;
                    }
                    "zwlr_virtual_pointer_manager_v1" => {
                        state.pointer_manager = None;
                        state.virtual_pointer = None;
                    }
                    _ => {}
                }
                state.globals.remove(idx);
            }
            ev => warn!("WlRegistry received unknown event:\n{ev:?}"),
        }
    }
}

impl Dispatch<zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
        event: zwp_virtual_keyboard_manager_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("ZwpVirtualKeyboardManagerV1 received unknown event:\n{event:?}");
    }
}

impl Dispatch<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _vk: &zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
        event: zwp_virtual_keyboard_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("ZwpVirtualKeyboardV1 received unknown event:\n{event:?}");
    }
}

impl Dispatch<zwp_input_method_manager_v2::ZwpInputMethodManagerV2, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
        event: zwp_input_method_manager_v2::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("ZwpInputMethodManagerV2 received unknown event:\n{event:?}");
    }
}
impl Dispatch<zwp_input_method_v2::ZwpInputMethodV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _vk: &zwp_input_method_v2::ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_v2::Event::Done => {
                debug!("ZwpInputMethodV2 received event:\nzwp_input_method_v2::Event::Done");
                state.im_serial += Wrapping(1u32);
            }
            zwp_input_method_v2::Event::Activate
            | zwp_input_method_v2::Event::Deactivate
            | zwp_input_method_v2::Event::SurroundingText {
                text: _,
                cursor: _,
                anchor: _,
            }
            | zwp_input_method_v2::Event::TextChangeCause { cause: _ }
            | zwp_input_method_v2::Event::ContentType {
                hint: _,
                purpose: _,
            } => {
                trace!("ZwpInputMethodV2 received irrelevant event:\n{event:?}");
            }
            zwp_input_method_v2::Event::Unavailable => {
                warn!("ZwpInputMethodV2 received event:\nzwp_input_method_v2::Event::Unavailable");
                warn!(
                    "It is not possible to enter text anymore! The characters will now be simulated by individual key presses instead"
                );
                state.input_method = None;
                state.im_manager = None;
            }
            _ => warn!("ZwpInputMethodV2 received unknown event:\n{event:?}"),
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        (): &(),
        _con: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { capabilities } => {
                debug!("WlSeat received event:\n{event:?}");
                let wayland_client::WEnum::Value(capabilities) = capabilities else {
                    warn!("Unknown value for the capabilities of the wl_seat: {capabilities:?}");
                    return;
                };

                // Create a WlKeyboard if the seat has the capability
                if state.seat_keyboard.is_none() && capabilities.contains(Capability::Keyboard) {
                    let seat_keyboard = seat.get_keyboard(qh, ());
                    state.seat_keyboard = Some(seat_keyboard);
                }

                // Create a WlPointer if the seat has the capability
                if state.seat_pointer.is_none() && capabilities.contains(Capability::Pointer) {
                    let seat_pointer = seat.get_pointer(qh, ());
                    state.seat_pointer = Some(seat_pointer);
                }
            }
            wl_seat::Event::Name { name: _ } => {
                trace!("WlSeat received irrelevant event:\n{event:?}");
            }
            _ => warn!("WlSeat received unknown event:\n{event:?}"),
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _seat: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                debug!(
                    "WlKeyboard received event:\nwl_keyboard::Event::Keymap {{ {format:?}, {fd:?}, {size} }}"
                );

                // Get the received format
                let format = if let WEnum::Value(format) = format {
                    format as xkb::KeymapFormat
                } else {
                    error!("invalid format received! resetting the keymap");
                    state.seat_keymap = None;
                    return;
                };

                if let Some(keymap) = &mut state.seat_keymap {
                    if keymap.update(format, fd, size).is_err() {
                        state.seat_keymap = None;
                    }
                } else {
                    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                    state.seat_keymap = Keymap2::new_from_fd(context, format, fd, size).ok();
                }
            }
            wl_keyboard::Event::Modifiers {
                serial: _,
                mods_depressed: depressed_mods,
                mods_latched: latched_mods,
                mods_locked: locked_mods,
                group: depressed_layout,
            } => {
                if let Some(keymap) = &mut state.seat_keymap {
                    // Wayland doesn't differentiates between depressed, latched and locked
                    keymap.update_modifiers(
                        depressed_mods,
                        latched_mods,
                        locked_mods,
                        depressed_layout,
                        0,
                        0,
                    );
                    debug!("modifiers updated");
                }
            }
            // On Wayland the clients only get notified about pressed keys or modifiers if they have
            // the focus. We cannot assume that is the case, so the received events don't reflect
            // the full picture and we cannot use them to keep track of the state of the keyboard
            wl_keyboard::Event::Enter { .. }
            | wl_keyboard::Event::Leave { .. }
            | wl_keyboard::Event::Key { .. }
            | wl_keyboard::Event::RepeatInfo { .. } => {
                debug!("WlKeyboard received irrelevant event:\n{event:?}");
            }
            _ => warn!("WlKeyboard received unknown event:\n{event:?}"),
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _seat: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("WlPointer received unknown event:\n{event:?}");
    }
}

impl Dispatch<wl_output::WlOutput, ()> for WaylandState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Geometry { transform, .. } => {
                debug!("WlOutput received event:\n{event:?}");
                // The width and height need to get switched if the transform changes them
                // TODO: Check if this really is needed
                if transform == WEnum::Value(wl_output::Transform::_90)
                    || transform == WEnum::Value(wl_output::Transform::_270)
                    || transform == WEnum::Value(wl_output::Transform::Flipped90)
                    || transform == WEnum::Value(wl_output::Transform::Flipped270)
                {
                    if let Some((_, output_data)) =
                        state.outputs.iter_mut().find(|(o, _)| o == output)
                    {
                        output_data.transform = true;
                    }
                }
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh: _,
            } => {
                debug!("WlOutput received event:\n{event:?}");
                if flags == WEnum::Value(Mode::Current) {
                    if let Some((_, output_data)) =
                        state.outputs.iter_mut().find(|(o, _)| o == output)
                    {
                        output_data.width = width;
                        output_data.height = height;
                    }
                }
            }
            // TODO: Check if Scale is relevant
            wl_output::Event::Done
            | wl_output::Event::Scale { factor: _ }
            | wl_output::Event::Name { name: _ }
            | wl_output::Event::Description { description: _ } => {
                trace!("WlOutput received irrelevant event:\n{event:?}");
            }
            _ => warn!("WlOutput received unknown event:\n{event:?}"),
        }
    }
}

impl Dispatch<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        event: zwlr_virtual_pointer_manager_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("ZwlrVirtualPointerManagerV1 received unknown event:\n{event:?}");
    }
}

impl Dispatch<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _vk: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
        event: zwlr_virtual_pointer_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        warn!("ZwlrVirtualPointerV1 received unknown event:\n{event:?}");
    }
}

impl Drop for WaylandState {
    // Destroy the manager for the protocols we used
    fn drop(&mut self) {
        if let Some(im_mgr) = self.im_manager.as_ref() {
            im_mgr.destroy();
        }
        if let Some(pointer_mgr) = self.pointer_manager.as_ref() {
            pointer_mgr.destroy();
        }
    }
}

impl Keyboard for Con {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
        // Process all previous events so that the serial number is correct
        self.event_queue
            .roundtrip(&mut self.state)
            .map_err(|_| InputError::Simulate("The roundtrip on Wayland failed"))?;

        let Some(im) = self.state.input_method.as_mut() else {
            return Ok(None);
        };

        is_alive(im)?;
        trace!("fast text input with imput_method protocol");

        im.commit_string(text.to_string());
        im.commit(self.state.im_serial.0);

        self.flush()?;

        Ok(Some(()))
    }

    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
        let keymap = self
            .state
            .seat_keymap
            .as_mut()
            .ok_or(InputError::Simulate("no keymap available"))?;

        let keycode = if let Some(keycode) = keymap.key_to_keycode(key) {
            keycode
        } else {
            debug!("keycode for key {key:?} was not found");

            let mapping_res = keymap.map_key(key);
            let keycode = match mapping_res {
                Err(InputError::Mapping(_)) => {
                    // Unmap and retry
                    keymap.unmap_everything()?;
                    keymap.map_key(key)?
                }

                Ok(keycode) => keycode,
                _ => return Err(InputError::Mapping("unable to map the key".to_string())),
            };

            // Apply the new keymap if there were any changes
            self.update_keymap()?;
            keycode
        };
        self.raw(keycode, direction)
    }

    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()> {
        if direction == Direction::Click || direction == Direction::Press {
            // Update keymap state
            if let Some((
                depressed_mods_new,
                latched_mods_new,
                locked_mods_new,
                effective_layout_new,
            )) = self
                .state
                .seat_keymap
                .as_mut()
                .ok_or(InputError::Simulate("no keymap available"))?
                .update_key(xkb::Keycode::new(keycode.into()), xkb::KeyDirection::Down)
            {
                trace!("it is a modifier");
                self.send_modifier_event(
                    depressed_mods_new,
                    latched_mods_new,
                    locked_mods_new,
                    effective_layout_new,
                )?;
            } else {
                self.send_key_event(keycode.into(), Direction::Press)?;
            }
        }
        if direction == Direction::Click || direction == Direction::Release {
            // Update keymap state
            if let Some((
                depressed_mods_new,
                latched_mods_new,
                locked_mods_new,
                effective_layout_new,
            )) = self
                .state
                .seat_keymap
                .as_mut()
                .ok_or(InputError::Simulate("no keymap available"))?
                .update_key(xkb::Keycode::new(keycode.into()), xkb::KeyDirection::Up)
            {
                trace!("it is a modifier");
                self.send_modifier_event(
                    depressed_mods_new,
                    latched_mods_new,
                    locked_mods_new,
                    effective_layout_new,
                )?;
            } else {
                self.send_key_event(keycode.into(), Direction::Release)?;
            }
        }
        Ok(())
    }
}

impl Mouse for Con {
    fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
        let vp = self
            .state
            .virtual_pointer
            .as_ref()
            .ok_or(InputError::Simulate("no way to enter button"))?;

        // Do nothing if one of the mouse scroll buttons was released
        // Releasing one of the scroll mouse buttons has no effect
        if direction == Direction::Release
            && matches!(
                button,
                Button::ScrollDown | Button::ScrollUp | Button::ScrollRight | Button::ScrollLeft
            )
        {
            return Ok(());
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

        if direction == Direction::Press || direction == Direction::Click {
            let time = self.get_time();
            trace!("vp.button({time}, {button}, wl_pointer::ButtonState::Pressed)");
            vp.button(time, button, wl_pointer::ButtonState::Pressed);
            vp.frame(); // TODO: Check if this is needed
        }

        if direction == Direction::Release || direction == Direction::Click {
            let time = self.get_time();
            trace!("vp.button({time}, {button}, wl_pointer::ButtonState::Released)");
            vp.button(time, button, wl_pointer::ButtonState::Released);
            vp.frame(); // TODO: Check if this is needed
        }

        self.flush()
    }

    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> InputResult<()> {
        let vp = self
            .state
            .virtual_pointer
            .as_ref()
            .ok_or(InputError::Simulate("no way to move the mouse"))?;

        let time = self.get_time();
        match coordinate {
            Coordinate::Rel => {
                trace!("vp.motion({time}, {x}, {y})");
                vp.motion(time, x as f64, y as f64);
            }
            Coordinate::Abs => {
                let (x_extend, y_extend) = self.main_display()?;
                let x_extend: u32 = x_extend
                    .try_into()
                    .map_err(|_| InputError::InvalidInput("x_extend cannot be negative"))?;
                let y_extend: u32 = y_extend
                    .try_into()
                    .map_err(|_| InputError::InvalidInput("y_extend cannot be negative"))?;
                let x: u32 = x.try_into().map_err(|_| {
                    InputError::InvalidInput("the absolute coordinates cannot be negative")
                })?;
                let y: u32 = y.try_into().map_err(|_| {
                    InputError::InvalidInput("the absolute coordinates cannot be negative")
                })?;

                trace!("vp.motion_absolute({time}, {x}, {y}, {x_extend}, {y_extend})");
                vp.motion_absolute(time, x, y, x_extend, y_extend);
            }
        }
        vp.frame(); // TODO: Check if this is needed

        self.flush()
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
        let vp = self
            .state
            .virtual_pointer
            .as_ref()
            .ok_or(InputError::Simulate("no way to scroll"))?;

        // TODO: Check what the value of length should be
        // TODO: Check if it would be better to use .axis_discrete here
        let time = self.get_time();
        let axis = match axis {
            Axis::Horizontal => wl_pointer::Axis::HorizontalScroll,
            Axis::Vertical => wl_pointer::Axis::VerticalScroll,
        };
        trace!("vp.axis(time, axis, length.into())");
        vp.axis(time, axis, length.into());
        vp.frame(); // TODO: Check if this is needed

        self.flush()
    }

    fn main_display(&self) -> InputResult<(i32, i32)> {
        // TODO: The assumption here is that the output we store in the first position
        // is the main display. This likely can be wrong
        match self.state.outputs.first() {
            // Switch width and height if the output was transformed
            Some((_, output_info)) if output_info.transform => {
                Ok((output_info.height, output_info.width))
            }
            Some((_, output_info)) => Ok((output_info.width, output_info.height)),
            None => Err(InputError::Simulate("No screens available")),
        }
    }

    fn location(&self) -> InputResult<(i32, i32)> {
        // TODO Implement this
        error!(
            "You tried to get the mouse location. I don't know how this is possible under Wayland. Let me know if there is a new protocol"
        );
        Err(InputError::Simulate("Not implemented yet"))
    }
}

fn is_alive<P: wayland_client::Proxy>(proxy: &P) -> InputResult<()> {
    if proxy.is_alive() {
        Ok(())
    } else {
        Err(InputError::Simulate("wayland proxy is dead"))
    }
}
