use std::cmp::min;

use wayland_client::{
    protocol::{wl_keyboard, wl_registry, wl_seat},
    Connection, Dispatch, QueueHandle, WEnum,
};
use xkbcommon::xkb;

const XKB_KEYCODE_OFFSET: u32 = 8;
const MAX_KEYMAP_ROUNDTRIPS: usize = 8;

const KEY_BACKSPACE: i32 = 14;
const KEY_TAB: i32 = 15;
const KEY_ENTER: i32 = 28;
const KEY_LEFTCTRL: i32 = 29;
const KEY_LEFTSHIFT: i32 = 42;
const KEY_LEFTALT: i32 = 56;
const KEY_RIGHTALT: i32 = 100;

/// Physical keyboard modifier that must be held while sending a keycode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModifierKey {
    /// Left shift modifier.
    Shift,
    /// Left control modifier.
    Control,
    /// Left alt modifier.
    Alt,
    /// Right alt / ISO level 3 modifier.
    AltGr,
}

impl ModifierKey {
    /// Returns the Linux evdev keycode for this modifier.
    pub fn keycode(self) -> i32 {
        match self {
            Self::Shift => KEY_LEFTSHIFT,
            Self::Control => KEY_LEFTCTRL,
            Self::Alt => KEY_LEFTALT,
            Self::AltGr => KEY_RIGHTALT,
        }
    }
}

/// One physical key press, expressed as an evdev keycode plus held modifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyStroke {
    /// Linux evdev keycode to press and release.
    pub keycode: i32,
    /// Modifiers that must be held while the keycode is sent.
    pub modifiers: Vec<ModifierKey>,
}

/// Current Wayland keyboard map used to convert characters to physical keycodes.
pub struct KeyboardMap {
    keymap: xkb::Keymap,
    active_layout: Option<xkb::LayoutIndex>,
    modifiers: ModifierIndexes,
}

impl KeyboardMap {
    /// Loads the compositor-provided Wayland keymap for the current session.
    pub fn load_current() -> Result<Self, String> {
        let connection = Connection::connect_to_env()
            .map_err(|err| format!("Failed to connect to Wayland display: {err}"))?;
        let mut event_queue = connection.new_event_queue();
        let queue_handle = event_queue.handle();
        let display = connection.display();
        display.get_registry(&queue_handle, ());

        let mut state = WaylandKeyboardState::default();
        // A few roundtrips may be needed: registry globals arrive first, then
        // seat capabilities, then the keyboard keymap and current layout.
        for _ in 0..MAX_KEYMAP_ROUNDTRIPS {
            event_queue
                .roundtrip(&mut state)
                .map_err(|err| format!("Failed to read Wayland keymap: {err}"))?;
            if state.error.is_some() || (state.keymap.is_some() && state.active_layout.is_some()) {
                break;
            }
        }

        if let Some(error) = state.error {
            return Err(error);
        }

        let keymap = state
            .keymap
            .ok_or_else(|| "Wayland keyboard keymap was not advertised".to_string())?;
        let modifiers = ModifierIndexes::new(&keymap);
        Ok(Self {
            keymap,
            active_layout: state.active_layout,
            modifiers,
        })
    }

    /// Finds the physical key sequence that produces the requested character.
    pub fn find_character(&self, character: char) -> Option<KeyStroke> {
        match character {
            '\n' | '\r' => Some(KeyStroke {
                keycode: KEY_ENTER,
                modifiers: Vec::new(),
            }),
            '\t' => Some(KeyStroke {
                keycode: KEY_TAB,
                modifiers: Vec::new(),
            }),
            '\u{8}' => Some(KeyStroke {
                keycode: KEY_BACKSPACE,
                modifiers: Vec::new(),
            }),
            _ => {
                let keysym = xkb::utf32_to_keysym(character as u32);
                if keysym.raw() == xkb::keysyms::KEY_NoSymbol {
                    return None;
                }
                self.find_keysym(keysym)
            }
        }
    }

    fn find_keysym(&self, target: xkb::Keysym) -> Option<KeyStroke> {
        let mut result = None;
        let layout = self.active_layout();

        self.keymap.key_for_each(|keymap, keycode| {
            if result.is_some() {
                return;
            }

            if layout >= keymap.num_layouts_for_key(keycode) {
                return;
            }

            let level_count = keymap.num_levels_for_key(keycode, layout);
            for level in 0..level_count {
                if !keymap
                    .key_get_syms_by_level(keycode, layout, level)
                    .contains(&target)
                {
                    continue;
                }

                if let Some(modifiers) = self.modifiers_for_level(keymap, keycode, layout, level) {
                    if let Some(evdev_keycode) = evdev_keycode(keycode) {
                        result = Some(KeyStroke {
                            keycode: evdev_keycode,
                            modifiers,
                        });
                        return;
                    }
                }
            }
        });

        result
    }

    fn active_layout(&self) -> xkb::LayoutIndex {
        self.active_layout.unwrap_or(0)
    }

    fn modifiers_for_level(
        &self,
        keymap: &xkb::Keymap,
        keycode: xkb::Keycode,
        layout: xkb::LayoutIndex,
        level: xkb::LevelIndex,
    ) -> Option<Vec<ModifierKey>> {
        let mut masks = [xkb::ModMask::default(); 16];
        let mask_count = keymap.key_get_mods_for_level(keycode, layout, level, &mut masks);
        let mask_count = min(mask_count, masks.len());

        // Prefer the shortest modifier sequence when several combinations
        // produce the same keysym in the active layout.
        masks[..mask_count]
            .iter()
            .filter_map(|mask| self.modifiers.keys_for_mask(*mask))
            .min_by_key(Vec::len)
    }
}

#[derive(Default)]
struct WaylandKeyboardState {
    keymap: Option<xkb::Keymap>,
    active_layout: Option<xkb::LayoutIndex>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    error: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandKeyboardState {
    fn event(
        _state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue_handle: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "wl_seat" {
                registry.bind::<wl_seat::WlSeat, _, _>(name, min(version, 8), queue_handle, ());
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandKeyboardState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        queue_handle: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(queue_handle, ()));
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandKeyboardState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if !matches!(format, WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)) {
                    state.error = Some("Wayland keyboard keymap is not XKB v1".to_string());
                    return;
                }

                let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                // The compositor owns the advertised keymap fd; xkbcommon maps
                // it read-only and returns an owned keymap object on success.
                let keymap = unsafe {
                    xkb::Keymap::new_from_fd(
                        &context,
                        fd,
                        size as usize,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkb::COMPILE_NO_FLAGS,
                    )
                };

                match keymap {
                    Ok(Some(keymap)) => state.keymap = Some(keymap),
                    Ok(None) => {
                        state.error =
                            Some("Wayland keyboard keymap could not be compiled".to_string());
                    }
                    Err(err) => {
                        state.error = Some(format!("Failed to map Wayland keyboard keymap: {err}"));
                    }
                }
            }
            wl_keyboard::Event::Modifiers { group, .. } => {
                state.active_layout = Some(group);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
struct ModifierIndexes {
    shift: Option<xkb::ModIndex>,
    control: Option<xkb::ModIndex>,
    alt: Option<xkb::ModIndex>,
    alt_gr: Option<xkb::ModIndex>,
}

impl ModifierIndexes {
    fn new(keymap: &xkb::Keymap) -> Self {
        Self {
            shift: valid_mod_index(keymap.mod_get_index(xkb::MOD_NAME_SHIFT)),
            control: valid_mod_index(keymap.mod_get_index(xkb::MOD_NAME_CTRL)),
            alt: valid_mod_index(keymap.mod_get_index(xkb::MOD_NAME_ALT)),
            alt_gr: valid_mod_index(keymap.mod_get_index(xkb::MOD_NAME_ISO_LEVEL3_SHIFT)),
        }
    }

    fn keys_for_mask(&self, mask: xkb::ModMask) -> Option<Vec<ModifierKey>> {
        let mut modifiers = Vec::new();
        let mut handled_mask = 0;

        self.push_if_present(
            mask,
            &mut handled_mask,
            self.shift,
            ModifierKey::Shift,
            &mut modifiers,
        );
        self.push_if_present(
            mask,
            &mut handled_mask,
            self.control,
            ModifierKey::Control,
            &mut modifiers,
        );
        self.push_if_present(
            mask,
            &mut handled_mask,
            self.alt,
            ModifierKey::Alt,
            &mut modifiers,
        );
        self.push_if_present(
            mask,
            &mut handled_mask,
            self.alt_gr,
            ModifierKey::AltGr,
            &mut modifiers,
        );

        if mask == handled_mask {
            Some(modifiers)
        } else {
            None
        }
    }

    fn push_if_present(
        &self,
        mask: xkb::ModMask,
        handled_mask: &mut xkb::ModMask,
        index: Option<xkb::ModIndex>,
        key: ModifierKey,
        modifiers: &mut Vec<ModifierKey>,
    ) {
        let Some(index) = index else {
            return;
        };

        let modifier_mask = 1_u32 << index;
        if mask & modifier_mask != 0 {
            *handled_mask |= modifier_mask;
            modifiers.push(key);
        }
    }
}

fn valid_mod_index(index: xkb::ModIndex) -> Option<xkb::ModIndex> {
    if index == xkb::MOD_INVALID || index >= xkb::ModMask::BITS {
        None
    } else {
        Some(index)
    }
}

fn evdev_keycode(keycode: xkb::Keycode) -> Option<i32> {
    keycode
        .raw()
        .checked_sub(XKB_KEYCODE_OFFSET)
        .and_then(|value| i32::try_from(value).ok())
}
