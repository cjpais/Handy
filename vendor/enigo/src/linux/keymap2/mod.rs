use std::{collections::HashSet, fs::File, io::Write as _, os::fd::OwnedFd};

use log::{debug, error, trace};
use xkbcommon::xkb::{
    CONTEXT_NO_FLAGS, Context, KEYMAP_COMPILE_NO_FLAGS, KEYMAP_FORMAT_TEXT_V1, KeyDirection,
    Keycode, Keymap, KeymapFormat, LayoutIndex, LayoutMask, ModMask, STATE_LAYOUT_DEPRESSED,
    STATE_LAYOUT_EFFECTIVE, STATE_LAYOUT_LATCHED, STATE_LAYOUT_LOCKED, STATE_MODS_DEPRESSED,
    STATE_MODS_LATCHED, STATE_MODS_LOCKED, State,
};
use xkeysym::Keysym;

use crate::{InputError, InputResult, Key};

mod parse_keymap;
pub(crate) use parse_keymap::ParsedKeymap;
mod default_keymap;
use default_keymap::DEFAULT_KEYMAP;

pub struct Keymap2 {
    original_keymap: String,
    context: Context,
    keymap: Keymap,
    state: State,
    parsed_keymap: ParsedKeymap,
    pressed_keys: HashSet<Keycode>,
}

// This is safe, we have a unique pointer.
unsafe impl Send for Keymap2 {}

impl Keymap2 {
    pub(crate) fn new_from_fd(
        context: Context,
        format: KeymapFormat,
        fd: OwnedFd,
        size: u32,
    ) -> Result<Self, ()> {
        use std::io::{Read, Seek, SeekFrom};

        debug!("creating new xkb:Keymap");
        debug!("new(format: {format}, size: {size}, ...)");

        let mut keymap_file = File::from(fd);

        // Check if the file size is correct
        let metadata = keymap_file.metadata().map_err(|e| {
            error!("could not get the file's metadata! Skipping file size check. Error: {e}");
        })?;
        if metadata.len() != size.into() {
            error!("file does not have the expected size! resetting the keymap");
            return Err(());
        }

        // Read keymap to String
        let mut original_keymap = String::new();
        // Reset the cursor to the beginning of the file.
        keymap_file.seek(SeekFrom::Start(0)).map_err(|e| {
            error!("unable to seek from the start:\n{e}");
        })?;
        keymap_file
            .read_to_string(&mut original_keymap)
            .map_err(|e| {
                error!("unable to read file to string:\n{e}");
            })?;
        // The String cannot end with NULL otherwise xkbcommon will fail to parse it
        while original_keymap.ends_with('\0') {
            debug!("removed NULL byte at the end");
            original_keymap.pop();
        }
        trace!("keymap string getting parsed by xkbcommon:\n{original_keymap}");

        let keymap = Keymap::new_from_string(
            &context,
            original_keymap.clone(),
            format,
            KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| {
            error!("unable to parse the keymap with xkbcommon! resetting the keymap");
        })?;

        let state = State::new(&keymap);

        Self::new(context, original_keymap, keymap, state)
    }

    fn new(
        context: Context,
        mut original_keymap: String,
        keymap: Keymap,
        state: State,
    ) -> Result<Self, ()> {
        // The String cannot end with NULL otherwise xkbcommon will fail to parse it
        while original_keymap.ends_with('\0') {
            debug!("removed NULL byte at the end");
            original_keymap.pop();
        }
        trace!("keymap string getting parsed by xkbcommon:\n{original_keymap}");

        let parsed_keymap = ParsedKeymap::try_from(original_keymap.as_str()).map_err(|()| {
            trace!("unable to parse the new keymap");
        })?;

        Ok(Self {
            original_keymap,
            context,
            keymap,
            state,
            parsed_keymap,
            pressed_keys: HashSet::with_capacity(8),
        })
    }

    pub fn update(&mut self, format: KeymapFormat, fd: OwnedFd, size: u32) -> Result<(), ()> {
        let depressed_mods = self.state.serialize_mods(STATE_MODS_DEPRESSED);
        let latched_mods = self.state.serialize_mods(STATE_MODS_LATCHED);
        let locked_mods = self.state.serialize_mods(STATE_MODS_LOCKED);

        let depressed_layout = self.state.serialize_layout(STATE_LAYOUT_DEPRESSED);
        let latched_layout = self.state.serialize_layout(STATE_LAYOUT_LATCHED);
        let locked_layout = self.state.serialize_layout(STATE_LAYOUT_LOCKED);

        let Keymap2 {
            context,
            keymap,
            mut state,
            parsed_keymap,
            pressed_keys,
            original_keymap: _, // Never update the original keymap
        } = Self::new_from_fd(self.context.clone(), format, fd, size).map_err(|()| {
            trace!("unable to create new keymap");
        })?;

        // The docs say this is a bad idea. update_key and update_mask should not get
        // mixed. I don't know how else to get the same state though
        for key in pressed_keys {
            state.update_key(key, KeyDirection::Down);
        }

        state.update_mask(
            depressed_mods,
            latched_mods,
            locked_mods,
            depressed_layout,
            latched_layout,
            locked_layout,
        );

        self.context = context;
        self.keymap = keymap;
        self.state = state;
        self.parsed_keymap = parsed_keymap;

        Ok(())
    }

    /// Update the state and return the new bitflags for the modifiers and the
    /// effective layout if they changed. If they remained the same, None is
    /// returned
    pub fn update_key(
        &mut self,
        keycode: Keycode,
        direction: KeyDirection,
    ) -> Option<(ModMask, ModMask, ModMask, LayoutMask)> {
        let depressed_mods_old = self.state.serialize_mods(STATE_MODS_DEPRESSED);
        let latched_mods_old = self.state.serialize_mods(STATE_MODS_LATCHED);
        let locked_mods_old = self.state.serialize_mods(STATE_MODS_LOCKED);
        let effective_layout_old = self.state.serialize_layout(STATE_LAYOUT_EFFECTIVE);

        match direction {
            KeyDirection::Up => {
                self.pressed_keys.remove(&keycode);
            }
            KeyDirection::Down => {
                self.pressed_keys.insert(keycode);
            }
        }
        self.state.update_key(keycode, direction);

        let depressed_mods_new = self.state.serialize_mods(STATE_MODS_DEPRESSED);
        let latched_mods_new = self.state.serialize_mods(STATE_MODS_LATCHED);
        let locked_mods_new = self.state.serialize_mods(STATE_MODS_LOCKED);
        let effective_layout_new = self.state.serialize_layout(STATE_LAYOUT_EFFECTIVE);

        if depressed_mods_old != depressed_mods_new
            || latched_mods_old != latched_mods_new
            || locked_mods_old != locked_mods_new
            || effective_layout_old != effective_layout_new
        {
            Some((
                depressed_mods_new,
                latched_mods_new,
                locked_mods_new,
                effective_layout_new,
            ))
        } else {
            None
        }
    }

    pub fn update_modifiers(
        &mut self,
        depressed_mods: ModMask,
        latched_mods: ModMask,
        locked_mods: ModMask,
        depressed_layout: LayoutIndex,
        latched_layout: LayoutIndex,
        locked_layout: LayoutIndex,
    ) {
        self.state.update_mask(
            depressed_mods,
            latched_mods,
            locked_mods,
            depressed_layout,
            latched_layout,
            locked_layout,
        );
    }

    pub fn format_file_size(&self) -> Result<(KeymapFormat, File, u32), ()> {
        let mut keymap_file = tempfile::tempfile().map_err(|e| {
            error!("could not create temporary file. Error: {e}");
        })?;
        write!(keymap_file, "{}", self.parsed_keymap).map_err(|e| {
            error!("could not write to temporary file. Error: {e}");
        })?;
        let metadata = keymap_file.metadata().map_err(|e| {
            error!("could not get the file's metadata! Error: {e}");
        })?;
        let size = metadata.len().try_into().map_err(|_| {
            error!(
                "keymap file is {} but the maximum is {} (u32::MAX)",
                metadata.len(),
                u32::MAX
            );
        })?;

        let format = KEYMAP_FORMAT_TEXT_V1;

        Ok((format, keymap_file, size))
    }

    pub fn key_to_keycode(&self, key: Key) -> Option<u16> {
        let keysym = Keysym::from(key);
        let key_name = format!("{keysym:?}");

        (self.keymap.min_keycode().raw()..self.keymap.max_keycode().raw())
            .find(|&k| {
                let keycode = Keycode::new(k);
                let keysym = self.state.key_get_one_sym(keycode);
                format!("{keysym:?}") == key_name
            })
            .and_then(|k| u16::try_from(k).ok())
    }

    pub fn map_key(&mut self, key: Key) -> InputResult<u16> {
        let keysym = Keysym::from(key);
        let key_name = format!("{keysym:?}");
        let key_name = match key_name.strip_prefix("XK_") {
            Some(key_name) => key_name,
            None => &key_name,
        };
        self.parsed_keymap.map_key(key_name, true)
    }

    pub fn unmap_everything(&mut self) -> InputResult<()> {
        debug!("unmap_everything");
        let depressed_mods = self.state.serialize_mods(STATE_MODS_DEPRESSED);
        let latched_mods = self.state.serialize_mods(STATE_MODS_LATCHED);
        let locked_mods = self.state.serialize_mods(STATE_MODS_LOCKED);

        let depressed_layout = self.state.serialize_layout(STATE_LAYOUT_DEPRESSED);
        let latched_layout = self.state.serialize_layout(STATE_LAYOUT_LATCHED);
        let locked_layout = self.state.serialize_layout(STATE_LAYOUT_LOCKED);

        let mut keymap_file = tempfile::tempfile().map_err(|e| {
            error!("could not create temporary file. Error: {e}");
            InputError::Unmapping("unable to unmap the keys".to_string())
        })?;
        write!(keymap_file, "{}", self.original_keymap).map_err(|e| {
            error!("could not write to temporary file. Error: {e}");
            InputError::Unmapping("unable to unmap the keys".to_string())
        })?;
        let metadata = keymap_file.metadata().map_err(|e| {
            error!("could not get the file's metadata! Error: {e}");
            InputError::Unmapping("unable to unmap the keys".to_string())
        })?;
        let size = metadata.len().try_into().map_err(|_| {
            error!(
                "keymap file is {} but the maximum is {} (u32::MAX)",
                metadata.len(),
                u32::MAX
            );
            InputError::Unmapping("unable to unmap the keys".to_string())
        })?;

        let Keymap2 {
            context,
            keymap,
            mut state,
            mut parsed_keymap,
            pressed_keys: _,    // We don't change the mapping of pressed keys
            original_keymap: _, // Never update the original keymap
        } = Self::new_from_fd(
            self.context.clone(),
            KEYMAP_FORMAT_TEXT_V1,
            keymap_file.into(),
            size,
        )
        .map_err(|()| {
            trace!("unable to create new keymap");
            InputError::Unmapping("unable to unmap the keys".to_string())
        })?;

        state.update_mask(
            depressed_mods,
            latched_mods,
            locked_mods,
            depressed_layout,
            latched_layout,
            locked_layout,
        );

        // Copy the current mapping for all pressed keys so they can get released
        let pressed_keys = self.pressed_keys.iter().map(|k| u32::from(*k)).collect();
        parsed_keymap.copy_maps_for_keycodes(&self.parsed_keymap, &pressed_keys);

        self.context = context;
        self.keymap = keymap;
        self.state = state;
        self.parsed_keymap = parsed_keymap;

        Ok(())
    }

    pub fn default() -> Result<Self, ()> {
        debug!("Default keymap is used");
        let mut keymap_file = tempfile::tempfile().map_err(|e| {
            error!("could not create temporary file. Error: {e}");
        })?;
        write!(keymap_file, "{DEFAULT_KEYMAP}").map_err(|e| {
            error!("could not write DEFAULT KEYMAP to temporary file. Error: {e}");
        })?;
        let metadata = keymap_file.metadata().map_err(|e| {
            error!("could not get the file's metadata! Error: {e}");
        })?;
        let size = metadata.len().try_into().map_err(|_| {
            error!(
                "keymap file is {} but the maximum is {} (u32::MAX)",
                metadata.len(),
                u32::MAX
            );
        })?;

        let format = KEYMAP_FORMAT_TEXT_V1;
        let context = Context::new(CONTEXT_NO_FLAGS);

        Self::new_from_fd(context, format, keymap_file.into(), size)
    }
}
