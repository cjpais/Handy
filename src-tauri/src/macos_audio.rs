//! CoreAudio helpers for macOS audio device metadata and output muting.
//!
//! The production backend is a thin wrapper over CoreAudio's `AudioObject*`
//! property APIs. The higher-level code is written against `AudioPropertyBackend`
//! so mute strategy selection and restore behavior can be tested without touching
//! the host's real audio devices.

use std::collections::BTreeSet;
use std::fmt;

#[cfg(target_os = "macos")]
use std::{ffi::c_void, ptr, sync::Mutex};

#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;
#[cfg(target_os = "macos")]
use core_foundation::string::{CFString, CFStringRef};
#[cfg(target_os = "macos")]
use std::sync::OnceLock;

pub type AudioObjectId = u32;
pub type OsStatus = i32;

type UInt32 = u32;
type Float32 = f32;

const fn fourcc(bytes: &[u8; 4]) -> u32 {
    ((bytes[0] as u32) << 24)
        | ((bytes[1] as u32) << 16)
        | ((bytes[2] as u32) << 8)
        | (bytes[3] as u32)
}

pub const AUDIO_OBJECT_UNKNOWN: AudioObjectId = 0;
const AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectId = 1;

const SELECTOR_DEVICES: u32 = fourcc(b"dev#");
const SELECTOR_DEFAULT_INPUT_DEVICE: u32 = fourcc(b"dIn ");
const SELECTOR_DEFAULT_OUTPUT_DEVICE: u32 = fourcc(b"dOut");
const SELECTOR_DEFAULT_SYSTEM_OUTPUT_DEVICE: u32 = fourcc(b"sOut");
const SELECTOR_TRANSLATE_UID_TO_DEVICE: u32 = fourcc(b"uidd");
const SELECTOR_OBJECT_NAME: u32 = fourcc(b"lnam");
const SELECTOR_DEVICE_UID: u32 = fourcc(b"uid ");
const SELECTOR_STREAM_CONFIGURATION: u32 = fourcc(b"slay");
const SELECTOR_PREFERRED_CHANNELS_FOR_STEREO: u32 = fourcc(b"dch2");
const SELECTOR_MUTE: u32 = fourcc(b"mute");
const SELECTOR_VOLUME_SCALAR: u32 = fourcc(b"volm");

const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_INPUT: u32 = fourcc(b"inpt");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const ELEMENT_MAIN: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

impl PropertyAddress {
    const fn new(selector: u32, scope: u32, element: u32) -> Self {
        Self {
            selector,
            scope,
            element,
        }
    }

    const fn global(selector: u32) -> Self {
        Self::new(selector, SCOPE_GLOBAL, ELEMENT_MAIN)
    }

    #[cfg(test)]
    const fn input(selector: u32, element: u32) -> Self {
        Self::new(selector, SCOPE_INPUT, element)
    }

    const fn output(selector: u32, element: u32) -> Self {
        Self::new(selector, SCOPE_OUTPUT, element)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreAudioError {
    CoreAudio {
        operation: &'static str,
        status: OsStatus,
    },
    InvalidPropertySize {
        property: &'static str,
        expected_multiple: usize,
        actual: usize,
    },
    UnknownDefaultOutputDevice,
    NoMuteCapability {
        device_id: AudioObjectId,
    },
    NoDevicesProperty,
    MissingStringProperty {
        device_id: AudioObjectId,
        property: &'static str,
    },
}

impl fmt::Display for CoreAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreAudio { operation, status } => {
                write!(f, "CoreAudio {} failed with OSStatus {}", operation, status)
            }
            Self::InvalidPropertySize {
                property,
                expected_multiple,
                actual,
            } => write!(
                f,
                "CoreAudio property {} returned {} bytes; expected a multiple of {} bytes",
                property, actual, expected_multiple
            ),
            Self::UnknownDefaultOutputDevice => {
                write!(f, "CoreAudio default output device is unknown")
            }
            Self::NoMuteCapability { device_id } => write!(
                f,
                "CoreAudio output device {} has no settable mute or volume controls",
                device_id
            ),
            Self::NoDevicesProperty => write!(f, "CoreAudio system object has no devices property"),
            Self::MissingStringProperty {
                device_id,
                property,
            } => {
                write!(f, "CoreAudio device {} is missing {}", device_id, property)
            }
        }
    }
}

impl std::error::Error for CoreAudioError {}

pub trait AudioPropertyBackend {
    fn has_property(&self, object_id: AudioObjectId, address: PropertyAddress) -> bool;
    fn is_property_settable(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
    ) -> Result<bool, CoreAudioError>;
    fn get_u32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<u32, CoreAudioError>;
    fn set_u32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        value: u32,
        property: &'static str,
    ) -> Result<(), CoreAudioError>;
    fn get_f32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<f32, CoreAudioError>;
    fn set_f32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        value: f32,
        property: &'static str,
    ) -> Result<(), CoreAudioError>;
    fn get_u32_array(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<Vec<u32>, CoreAudioError>;
    fn get_string(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<String, CoreAudioError>;
    fn stream_channel_count(
        &self,
        object_id: AudioObjectId,
        scope: u32,
    ) -> Result<u32, CoreAudioError>;
    fn device_id_for_uid(&self, uid: &str) -> Result<Option<AudioObjectId>, CoreAudioError>;
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default)]
pub struct RealCoreAudioBackend;

#[cfg(target_os = "macos")]
#[repr(C)]
struct AudioBuffer {
    m_number_channels: UInt32,
    m_data_byte_size: UInt32,
    m_data: *mut c_void,
}

#[cfg(target_os = "macos")]
fn audio_buffer_list_buffers_offset() -> usize {
    let align = std::mem::align_of::<AudioBuffer>();
    (std::mem::size_of::<UInt32>() + align - 1) & !(align - 1)
}

#[cfg(target_os = "macos")]
type AudioObjectPropertyListenerProc =
    extern "C" fn(AudioObjectId, UInt32, *const PropertyAddress, *mut c_void) -> OsStatus;

#[cfg(target_os = "macos")]
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
    ) -> u8;

    fn AudioObjectIsPropertySettable(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
        out_is_settable: *mut u8,
    ) -> OsStatus;

    fn AudioObjectGetPropertyDataSize(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
        in_qualifier_data_size: UInt32,
        in_qualifier_data: *const c_void,
        out_data_size: *mut UInt32,
    ) -> OsStatus;

    fn AudioObjectGetPropertyData(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
        in_qualifier_data_size: UInt32,
        in_qualifier_data: *const c_void,
        io_data_size: *mut UInt32,
        out_data: *mut c_void,
    ) -> OsStatus;

    fn AudioObjectSetPropertyData(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
        in_qualifier_data_size: UInt32,
        in_qualifier_data: *const c_void,
        in_data_size: UInt32,
        in_data: *const c_void,
    ) -> OsStatus;

    fn AudioObjectAddPropertyListener(
        in_object_id: AudioObjectId,
        in_address: *const PropertyAddress,
        in_listener: AudioObjectPropertyListenerProc,
        in_client_data: *mut c_void,
    ) -> OsStatus;
}

#[cfg(target_os = "macos")]
impl RealCoreAudioBackend {
    fn property_data_size(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        operation: &'static str,
    ) -> Result<usize, CoreAudioError> {
        let mut size: UInt32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                object_id,
                &address,
                0,
                ptr::null(),
                &mut size as *mut UInt32,
            )
        };
        if status == 0 {
            Ok(size as usize)
        } else {
            Err(CoreAudioError::CoreAudio { operation, status })
        }
    }

    fn get_bytes(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<Vec<u8>, CoreAudioError> {
        let size = self.property_data_size(object_id, address, property)?;
        let mut bytes = vec![0u8; size];
        let mut io_size = size as UInt32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                &mut io_size,
                bytes.as_mut_ptr() as *mut c_void,
            )
        };
        if status != 0 {
            return Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            });
        }
        bytes.truncate(io_size as usize);
        Ok(bytes)
    }
}

#[cfg(target_os = "macos")]
impl AudioPropertyBackend for RealCoreAudioBackend {
    fn has_property(&self, object_id: AudioObjectId, address: PropertyAddress) -> bool {
        unsafe { AudioObjectHasProperty(object_id, &address) != 0 }
    }

    fn is_property_settable(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
    ) -> Result<bool, CoreAudioError> {
        let mut settable = 0u8;
        let status =
            unsafe { AudioObjectIsPropertySettable(object_id, &address, &mut settable as *mut u8) };
        if status == 0 {
            Ok(settable != 0)
        } else {
            Err(CoreAudioError::CoreAudio {
                operation: "query property settable",
                status,
            })
        }
    }

    fn get_u32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<u32, CoreAudioError> {
        let mut value: UInt32 = 0;
        let mut size = std::mem::size_of::<UInt32>() as UInt32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut UInt32 as *mut c_void,
            )
        };
        if status == 0 {
            Ok(value)
        } else {
            Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            })
        }
    }

    fn set_u32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        value: u32,
        property: &'static str,
    ) -> Result<(), CoreAudioError> {
        let value = value as UInt32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                std::mem::size_of::<UInt32>() as UInt32,
                &value as *const UInt32 as *const c_void,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            })
        }
    }

    fn get_f32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<f32, CoreAudioError> {
        let mut value: Float32 = 0.0;
        let mut size = std::mem::size_of::<Float32>() as UInt32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut Float32 as *mut c_void,
            )
        };
        if status == 0 {
            Ok(value)
        } else {
            Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            })
        }
    }

    fn set_f32(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        value: f32,
        property: &'static str,
    ) -> Result<(), CoreAudioError> {
        let value = value as Float32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                std::mem::size_of::<Float32>() as UInt32,
                &value as *const Float32 as *const c_void,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            })
        }
    }

    fn get_u32_array(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<Vec<u32>, CoreAudioError> {
        let bytes = self.get_bytes(object_id, address, property)?;
        if bytes.len() % std::mem::size_of::<UInt32>() != 0 {
            return Err(CoreAudioError::InvalidPropertySize {
                property,
                expected_multiple: std::mem::size_of::<UInt32>(),
                actual: bytes.len(),
            });
        }

        let mut values = Vec::with_capacity(bytes.len() / std::mem::size_of::<UInt32>());
        for chunk in bytes.chunks_exact(std::mem::size_of::<UInt32>()) {
            values.push(UInt32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(values)
    }

    fn get_string(
        &self,
        object_id: AudioObjectId,
        address: PropertyAddress,
        property: &'static str,
    ) -> Result<String, CoreAudioError> {
        if !self.has_property(object_id, address) {
            return Err(CoreAudioError::MissingStringProperty {
                device_id: object_id,
                property,
            });
        }

        let mut value: CFStringRef = ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as UInt32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut CFStringRef as *mut c_void,
            )
        };
        if status != 0 {
            return Err(CoreAudioError::CoreAudio {
                operation: property,
                status,
            });
        }
        if value.is_null() {
            return Err(CoreAudioError::MissingStringProperty {
                device_id: object_id,
                property,
            });
        }

        let string = unsafe { CFString::wrap_under_create_rule(value).to_string() };
        Ok(string)
    }

    fn stream_channel_count(
        &self,
        object_id: AudioObjectId,
        scope: u32,
    ) -> Result<u32, CoreAudioError> {
        let address = PropertyAddress::new(SELECTOR_STREAM_CONFIGURATION, scope, ELEMENT_MAIN);
        if !self.has_property(object_id, address) {
            return Ok(0);
        }

        let bytes = self.get_bytes(object_id, address, "get stream configuration")?;
        if bytes.len() < std::mem::size_of::<UInt32>() {
            return Err(CoreAudioError::InvalidPropertySize {
                property: "stream configuration",
                expected_multiple: std::mem::size_of::<UInt32>(),
                actual: bytes.len(),
            });
        }

        let buffer_count = UInt32::from_ne_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let buffers_offset = audio_buffer_list_buffers_offset();
        let expected = buffers_offset + buffer_count * std::mem::size_of::<AudioBuffer>();
        if bytes.len() < expected {
            return Err(CoreAudioError::InvalidPropertySize {
                property: "stream configuration",
                expected_multiple: expected,
                actual: bytes.len(),
            });
        }

        let mut total = 0u32;
        let mut offset = buffers_offset;
        for _ in 0..buffer_count {
            let buffer = unsafe {
                std::ptr::read_unaligned(bytes.as_ptr().add(offset) as *const AudioBuffer)
            };
            total = total.saturating_add(buffer.m_number_channels);
            offset += std::mem::size_of::<AudioBuffer>();
        }
        Ok(total)
    }

    fn device_id_for_uid(&self, uid: &str) -> Result<Option<AudioObjectId>, CoreAudioError> {
        let uid = CFString::new(uid);
        let uid_ref = uid.as_concrete_TypeRef();
        let address = PropertyAddress::global(SELECTOR_TRANSLATE_UID_TO_DEVICE);
        let mut device_id = AUDIO_OBJECT_UNKNOWN;
        let mut size = std::mem::size_of::<AudioObjectId>() as UInt32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                AUDIO_OBJECT_SYSTEM_OBJECT,
                &address,
                std::mem::size_of::<CFStringRef>() as UInt32,
                &uid_ref as *const CFStringRef as *const c_void,
                &mut size,
                &mut device_id as *mut AudioObjectId as *mut c_void,
            )
        };

        if status != 0 {
            return Err(CoreAudioError::CoreAudio {
                operation: "translate device uid",
                status,
            });
        }

        if device_id == AUDIO_OBJECT_UNKNOWN {
            Ok(None)
        } else {
            Ok(Some(device_id))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AppliedMuteStrategy {
    MasterMute { previous: bool },
    ChannelMute { previous: Vec<(u32, bool)> },
    MasterVolume { previous: f32 },
    ChannelVolume { previous: Vec<(u32, f32)> },
}

#[derive(Clone, Debug, PartialEq)]
struct AppliedMute {
    device_id: AudioObjectId,
    device_uid: String,
    strategy: AppliedMuteStrategy,
}

#[derive(Debug)]
pub struct OutputMuteController<B> {
    backend: B,
    applied: Vec<AppliedMute>,
}

impl<B: AudioPropertyBackend> OutputMuteController<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            applied: Vec::new(),
        }
    }

    pub fn mute_default_output(&mut self) -> Result<(), CoreAudioError> {
        if !self.applied.is_empty() {
            return Ok(());
        }

        self.apply_mute_to_default_output()
    }

    pub fn handle_default_output_change(&mut self) -> Result<(), CoreAudioError> {
        if self.applied.is_empty() {
            return Ok(());
        }

        let device_id = default_output_device(&self.backend)?;
        if self
            .applied
            .iter()
            .any(|applied| applied.device_id == device_id)
        {
            return Ok(());
        }

        self.apply_mute_to_default_output()
    }

    pub fn restore(&mut self) -> Result<(), CoreAudioError> {
        if self.applied.is_empty() {
            return Ok(());
        }

        let applied = std::mem::take(&mut self.applied);
        for saved in applied.iter().rev() {
            if let Err(error) = restore_applied_mute(&self.backend, saved) {
                self.applied = applied;
                return Err(error);
            }
        }

        Ok(())
    }

    fn apply_mute_to_default_output(&mut self) -> Result<(), CoreAudioError> {
        let device_id = default_output_device(&self.backend)?;
        let device_uid = device_uid(&self.backend, device_id)?;
        let strategy = apply_best_mute_strategy(&self.backend, device_id)?;
        self.applied.push(AppliedMute {
            device_id,
            device_uid,
            strategy,
        });
        Ok(())
    }

    #[cfg(test)]
    fn applied_strategy(&self) -> Option<&AppliedMuteStrategy> {
        self.applied.first().map(|applied| &applied.strategy)
    }
}

fn default_output_device<B: AudioPropertyBackend>(
    backend: &B,
) -> Result<AudioObjectId, CoreAudioError> {
    let address = PropertyAddress::global(SELECTOR_DEFAULT_OUTPUT_DEVICE);
    let device_id = backend.get_u32(
        AUDIO_OBJECT_SYSTEM_OBJECT,
        address,
        "get default output device",
    )?;
    if device_id == AUDIO_OBJECT_UNKNOWN {
        Err(CoreAudioError::UnknownDefaultOutputDevice)
    } else {
        Ok(device_id)
    }
}

fn device_uid<B: AudioPropertyBackend>(
    backend: &B,
    device_id: AudioObjectId,
) -> Result<String, CoreAudioError> {
    backend.get_string(
        device_id,
        PropertyAddress::global(SELECTOR_DEVICE_UID),
        "get device uid",
    )
}

fn restore_device_id<B: AudioPropertyBackend>(
    backend: &B,
    applied: &AppliedMute,
) -> Result<Option<AudioObjectId>, CoreAudioError> {
    backend.device_id_for_uid(&applied.device_uid)
}

fn is_settable<B: AudioPropertyBackend>(
    backend: &B,
    object_id: AudioObjectId,
    address: PropertyAddress,
) -> Result<bool, CoreAudioError> {
    if !backend.has_property(object_id, address) {
        return Ok(false);
    }
    backend.is_property_settable(object_id, address)
}

fn preferred_output_channels<B: AudioPropertyBackend>(
    backend: &B,
    device_id: AudioObjectId,
) -> Vec<u32> {
    let mut channels = BTreeSet::new();
    let address = PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
    if backend.has_property(device_id, address) {
        if let Ok(values) =
            backend.get_u32_array(device_id, address, "get preferred stereo channels")
        {
            for channel in values
                .into_iter()
                .filter(|channel| *channel != ELEMENT_MAIN)
            {
                channels.insert(channel);
            }
        }
    }

    if channels.is_empty() {
        channels.insert(1);
        channels.insert(2);
    }

    channels.into_iter().collect()
}

fn apply_best_mute_strategy<B: AudioPropertyBackend>(
    backend: &B,
    device_id: AudioObjectId,
) -> Result<AppliedMuteStrategy, CoreAudioError> {
    let master_mute = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
    if is_settable(backend, device_id, master_mute)? {
        let previous = backend.get_u32(device_id, master_mute, "get master mute")? != 0;
        backend.set_u32(device_id, master_mute, 1, "set master mute")?;
        return Ok(AppliedMuteStrategy::MasterMute { previous });
    }

    let channels = preferred_output_channels(backend, device_id);
    let mut previous_channel_mutes: Vec<(u32, bool)> = Vec::new();
    for channel in &channels {
        let address = PropertyAddress::output(SELECTOR_MUTE, *channel);
        if is_settable(backend, device_id, address)? {
            let previous = backend.get_u32(device_id, address, "get channel mute")? != 0;
            if let Err(error) = backend.set_u32(device_id, address, 1, "set channel mute") {
                for (restored_channel, was_muted) in &previous_channel_mutes {
                    let _ = backend.set_u32(
                        device_id,
                        PropertyAddress::output(SELECTOR_MUTE, *restored_channel),
                        u32::from(*was_muted),
                        "rollback channel mute",
                    );
                }
                return Err(error);
            }
            previous_channel_mutes.push((*channel, previous));
        }
    }
    if !previous_channel_mutes.is_empty() {
        return Ok(AppliedMuteStrategy::ChannelMute {
            previous: previous_channel_mutes,
        });
    }

    let master_volume = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, ELEMENT_MAIN);
    if is_settable(backend, device_id, master_volume)? {
        let previous = backend.get_f32(device_id, master_volume, "get master volume")?;
        backend.set_f32(device_id, master_volume, 0.0, "set master volume")?;
        return Ok(AppliedMuteStrategy::MasterVolume { previous });
    }

    let mut previous_channel_volumes: Vec<(u32, f32)> = Vec::new();
    for channel in &channels {
        let address = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, *channel);
        if is_settable(backend, device_id, address)? {
            let previous = backend.get_f32(device_id, address, "get channel volume")?;
            if let Err(error) = backend.set_f32(device_id, address, 0.0, "set channel volume") {
                for (restored_channel, volume) in &previous_channel_volumes {
                    let _ = backend.set_f32(
                        device_id,
                        PropertyAddress::output(SELECTOR_VOLUME_SCALAR, *restored_channel),
                        *volume,
                        "rollback channel volume",
                    );
                }
                return Err(error);
            }
            previous_channel_volumes.push((*channel, previous));
        }
    }
    if !previous_channel_volumes.is_empty() {
        return Ok(AppliedMuteStrategy::ChannelVolume {
            previous: previous_channel_volumes,
        });
    }

    Err(CoreAudioError::NoMuteCapability { device_id })
}

fn restore_applied_mute<B: AudioPropertyBackend>(
    backend: &B,
    applied: &AppliedMute,
) -> Result<(), CoreAudioError> {
    let Some(device_id) = restore_device_id(backend, applied)? else {
        return Ok(());
    };

    match &applied.strategy {
        AppliedMuteStrategy::MasterMute { previous } => backend.set_u32(
            device_id,
            PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN),
            u32::from(*previous),
            "restore master mute",
        ),
        AppliedMuteStrategy::ChannelMute { previous } => {
            for (channel, was_muted) in previous {
                backend.set_u32(
                    device_id,
                    PropertyAddress::output(SELECTOR_MUTE, *channel),
                    u32::from(*was_muted),
                    "restore channel mute",
                )?;
            }
            Ok(())
        }
        AppliedMuteStrategy::MasterVolume { previous } => backend.set_f32(
            device_id,
            PropertyAddress::output(SELECTOR_VOLUME_SCALAR, ELEMENT_MAIN),
            *previous,
            "restore master volume",
        ),
        AppliedMuteStrategy::ChannelVolume { previous } => {
            for (channel, volume) in previous {
                backend.set_f32(
                    device_id,
                    PropertyAddress::output(SELECTOR_VOLUME_SCALAR, *channel),
                    *volume,
                    "restore channel volume",
                )?;
            }
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreAudioDeviceInfo {
    pub id: AudioObjectId,
    pub uid: String,
    pub name: String,
    pub input_channels: u32,
    pub output_channels: u32,
    pub is_default_input: bool,
    pub is_default_output: bool,
    pub is_default_system_output: bool,
}

pub fn list_audio_devices_with_backend<B: AudioPropertyBackend>(
    backend: &B,
) -> Result<Vec<CoreAudioDeviceInfo>, CoreAudioError> {
    let devices_address = PropertyAddress::global(SELECTOR_DEVICES);
    if !backend.has_property(AUDIO_OBJECT_SYSTEM_OBJECT, devices_address) {
        return Err(CoreAudioError::NoDevicesProperty);
    }

    let device_ids = backend.get_u32_array(
        AUDIO_OBJECT_SYSTEM_OBJECT,
        devices_address,
        "get audio devices",
    )?;
    let default_input = backend
        .get_u32(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_INPUT_DEVICE),
            "get default input device",
        )
        .unwrap_or(AUDIO_OBJECT_UNKNOWN);
    let default_output = backend
        .get_u32(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_OUTPUT_DEVICE),
            "get default output device",
        )
        .unwrap_or(AUDIO_OBJECT_UNKNOWN);
    let default_system_output = backend
        .get_u32(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_SYSTEM_OUTPUT_DEVICE),
            "get default system output device",
        )
        .unwrap_or(AUDIO_OBJECT_UNKNOWN);

    let mut devices = Vec::new();
    for id in device_ids {
        if id == AUDIO_OBJECT_UNKNOWN {
            continue;
        }

        let input_channels = backend.stream_channel_count(id, SCOPE_INPUT).unwrap_or(0);
        let output_channels = backend.stream_channel_count(id, SCOPE_OUTPUT).unwrap_or(0);
        if input_channels == 0 && output_channels == 0 {
            continue;
        }

        let name = backend
            .get_string(
                id,
                PropertyAddress::global(SELECTOR_OBJECT_NAME),
                "get device name",
            )
            .unwrap_or_else(|_| format!("Audio Device {}", id));
        let uid = backend
            .get_string(
                id,
                PropertyAddress::global(SELECTOR_DEVICE_UID),
                "get device uid",
            )
            .unwrap_or_else(|_| id.to_string());

        devices.push(CoreAudioDeviceInfo {
            id,
            uid,
            name,
            input_channels,
            output_channels,
            is_default_input: id == default_input,
            is_default_output: id == default_output,
            is_default_system_output: id == default_system_output,
        });
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.uid.cmp(&b.uid)));
    Ok(devices)
}

#[cfg(target_os = "macos")]
pub fn list_audio_devices() -> Result<Vec<CoreAudioDeviceInfo>, CoreAudioError> {
    list_audio_devices_with_backend(&RealCoreAudioBackend)
}

#[cfg(target_os = "macos")]
static DEFAULT_OUTPUT_MUTE_CONTROLLER: OnceLock<Mutex<OutputMuteController<RealCoreAudioBackend>>> =
    OnceLock::new();

// Registered once for the app lifetime. The process owns this singleton mute
// controller, so there is no shorter lifecycle that needs explicit listener
// removal before shutdown.
#[cfg(target_os = "macos")]
static DEFAULT_OUTPUT_LISTENER: OnceLock<Result<(), CoreAudioError>> = OnceLock::new();

#[cfg(target_os = "macos")]
extern "C" fn default_output_changed_listener(
    _object_id: AudioObjectId,
    _address_count: UInt32,
    _addresses: *const PropertyAddress,
    _client_data: *mut c_void,
) -> OsStatus {
    if let Some(controller) = DEFAULT_OUTPUT_MUTE_CONTROLLER.get() {
        let _ = controller.lock().unwrap().handle_default_output_change();
    }
    0
}

#[cfg(target_os = "macos")]
fn ensure_default_output_listener() -> Result<(), CoreAudioError> {
    DEFAULT_OUTPUT_LISTENER
        .get_or_init(|| {
            let address = PropertyAddress::global(SELECTOR_DEFAULT_OUTPUT_DEVICE);
            let status = unsafe {
                AudioObjectAddPropertyListener(
                    AUDIO_OBJECT_SYSTEM_OBJECT,
                    &address,
                    default_output_changed_listener,
                    ptr::null_mut(),
                )
            };

            if status == 0 {
                Ok(())
            } else {
                Err(CoreAudioError::CoreAudio {
                    operation: "add default output listener",
                    status,
                })
            }
        })
        .clone()
}

#[cfg(target_os = "macos")]
fn default_output_mute_controller() -> &'static Mutex<OutputMuteController<RealCoreAudioBackend>> {
    DEFAULT_OUTPUT_MUTE_CONTROLLER
        .get_or_init(|| Mutex::new(OutputMuteController::new(RealCoreAudioBackend)))
}

#[cfg(target_os = "macos")]
pub fn mute_default_output() -> Result<(), CoreAudioError> {
    ensure_default_output_listener()?;
    default_output_mute_controller()
        .lock()
        .unwrap()
        .mute_default_output()
}

#[cfg(target_os = "macos")]
pub fn restore_default_output() -> Result<(), CoreAudioError> {
    default_output_mute_controller().lock().unwrap().restore()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq)]
    enum FakeValue {
        U32(u32),
        F32(f32),
        U32Array(Vec<u32>),
        String(String),
        Channels(u32),
    }

    #[derive(Clone, Debug, Default)]
    struct FakeBackend {
        inner: Arc<Mutex<FakeInner>>,
    }

    #[derive(Debug, Default)]
    struct FakeInner {
        values: BTreeMap<(AudioObjectId, PropertyAddress), FakeValue>,
        settable: BTreeSet<(AudioObjectId, PropertyAddress)>,
        fail_writes: BTreeSet<(AudioObjectId, PropertyAddress)>,
        writes: Vec<(AudioObjectId, PropertyAddress, FakeValue)>,
    }

    impl FakeBackend {
        fn set_value(&self, object_id: AudioObjectId, address: PropertyAddress, value: FakeValue) {
            self.inner
                .lock()
                .unwrap()
                .values
                .insert((object_id, address), value);
        }

        fn set_settable(&self, object_id: AudioObjectId, address: PropertyAddress) {
            self.inner
                .lock()
                .unwrap()
                .settable
                .insert((object_id, address));
        }

        fn fail_set(&self, object_id: AudioObjectId, address: PropertyAddress) {
            self.inner
                .lock()
                .unwrap()
                .fail_writes
                .insert((object_id, address));
        }

        fn default_output(&self, device_id: AudioObjectId) {
            self.set_value(
                AUDIO_OBJECT_SYSTEM_OBJECT,
                PropertyAddress::global(SELECTOR_DEFAULT_OUTPUT_DEVICE),
                FakeValue::U32(device_id),
            );
        }

        fn device_uid(&self, device_id: AudioObjectId, uid: &str) {
            self.set_value(
                device_id,
                PropertyAddress::global(SELECTOR_DEVICE_UID),
                FakeValue::String(uid.to_string()),
            );
        }

        fn writes(&self) -> Vec<(AudioObjectId, PropertyAddress, FakeValue)> {
            self.inner.lock().unwrap().writes.clone()
        }
    }

    impl AudioPropertyBackend for FakeBackend {
        fn has_property(&self, object_id: AudioObjectId, address: PropertyAddress) -> bool {
            self.inner
                .lock()
                .unwrap()
                .values
                .contains_key(&(object_id, address))
        }

        fn is_property_settable(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
        ) -> Result<bool, CoreAudioError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .settable
                .contains(&(object_id, address)))
        }

        fn get_u32(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            _property: &'static str,
        ) -> Result<u32, CoreAudioError> {
            match self.inner.lock().unwrap().values.get(&(object_id, address)) {
                Some(FakeValue::U32(value)) => Ok(*value),
                _ => Err(CoreAudioError::CoreAudio {
                    operation: "fake get u32",
                    status: -1,
                }),
            }
        }

        fn set_u32(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            value: u32,
            _property: &'static str,
        ) -> Result<(), CoreAudioError> {
            let mut inner = self.inner.lock().unwrap();
            if inner.fail_writes.contains(&(object_id, address)) {
                return Err(CoreAudioError::CoreAudio {
                    operation: "fake set u32",
                    status: -2,
                });
            }
            inner
                .values
                .insert((object_id, address), FakeValue::U32(value));
            inner
                .writes
                .push((object_id, address, FakeValue::U32(value)));
            Ok(())
        }

        fn get_f32(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            _property: &'static str,
        ) -> Result<f32, CoreAudioError> {
            match self.inner.lock().unwrap().values.get(&(object_id, address)) {
                Some(FakeValue::F32(value)) => Ok(*value),
                _ => Err(CoreAudioError::CoreAudio {
                    operation: "fake get f32",
                    status: -1,
                }),
            }
        }

        fn set_f32(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            value: f32,
            _property: &'static str,
        ) -> Result<(), CoreAudioError> {
            let mut inner = self.inner.lock().unwrap();
            if inner.fail_writes.contains(&(object_id, address)) {
                return Err(CoreAudioError::CoreAudio {
                    operation: "fake set f32",
                    status: -2,
                });
            }
            inner
                .values
                .insert((object_id, address), FakeValue::F32(value));
            inner
                .writes
                .push((object_id, address, FakeValue::F32(value)));
            Ok(())
        }

        fn get_u32_array(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            _property: &'static str,
        ) -> Result<Vec<u32>, CoreAudioError> {
            match self.inner.lock().unwrap().values.get(&(object_id, address)) {
                Some(FakeValue::U32Array(value)) => Ok(value.clone()),
                _ => Err(CoreAudioError::CoreAudio {
                    operation: "fake get u32 array",
                    status: -1,
                }),
            }
        }

        fn get_string(
            &self,
            object_id: AudioObjectId,
            address: PropertyAddress,
            _property: &'static str,
        ) -> Result<String, CoreAudioError> {
            match self.inner.lock().unwrap().values.get(&(object_id, address)) {
                Some(FakeValue::String(value)) => Ok(value.clone()),
                _ => Err(CoreAudioError::MissingStringProperty {
                    device_id: object_id,
                    property: "fake string",
                }),
            }
        }

        fn stream_channel_count(
            &self,
            object_id: AudioObjectId,
            scope: u32,
        ) -> Result<u32, CoreAudioError> {
            match self.inner.lock().unwrap().values.get(&(
                object_id,
                PropertyAddress::new(SELECTOR_STREAM_CONFIGURATION, scope, ELEMENT_MAIN),
            )) {
                Some(FakeValue::Channels(value)) => Ok(*value),
                _ => Ok(0),
            }
        }

        fn device_id_for_uid(&self, uid: &str) -> Result<Option<AudioObjectId>, CoreAudioError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner
                .values
                .iter()
                .find_map(|((object_id, address), value)| {
                    if *address == PropertyAddress::global(SELECTOR_DEVICE_UID)
                        && value == &FakeValue::String(uid.to_string())
                    {
                        Some(*object_id)
                    } else {
                        None
                    }
                }))
        }
    }

    fn fake_with_default_output() -> (FakeBackend, OutputMuteController<FakeBackend>) {
        let backend = FakeBackend::default();
        backend.default_output(42);
        backend.device_uid(42, "device-42");
        let controller = OutputMuteController::new(backend.clone());
        (backend, controller)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn audio_buffer_list_parser_accounts_for_c_alignment_padding() {
        assert_eq!(audio_buffer_list_buffers_offset(), 8);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires real macOS audio hardware; run with HANDY_AUDIO_HW=1"]
    fn hardware_default_output_mute_roundtrip() {
        if std::env::var("HANDY_AUDIO_HW").ok().as_deref() != Some("1") {
            eprintln!("set HANDY_AUDIO_HW=1 to run the CoreAudio hardware mute test");
            return;
        }

        let mut controller = OutputMuteController::new(RealCoreAudioBackend);
        controller.mute_default_output().unwrap();
        assert!(!controller.applied.is_empty());
        controller.restore().unwrap();
        assert!(controller.applied.is_empty());
    }

    #[test]
    fn master_mute_is_preferred_and_restored() {
        let (backend, mut controller) = fake_with_default_output();
        let mute = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
        backend.set_value(42, mute, FakeValue::U32(0));
        backend.set_settable(42, mute);

        controller.mute_default_output().unwrap();
        assert_eq!(
            controller.applied_strategy(),
            Some(&AppliedMuteStrategy::MasterMute { previous: false })
        );
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![(42, mute, FakeValue::U32(1)), (42, mute, FakeValue::U32(0)),]
        );
    }

    #[test]
    fn channel_mute_falls_back_from_unsettable_master() {
        let (backend, mut controller) = fake_with_default_output();
        let master = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
        let ch1 = PropertyAddress::output(SELECTOR_MUTE, 1);
        let ch2 = PropertyAddress::output(SELECTOR_MUTE, 2);
        let preferred =
            PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
        backend.set_value(42, master, FakeValue::U32(0));
        backend.set_value(42, preferred, FakeValue::U32Array(vec![1, 2]));
        backend.set_value(42, ch1, FakeValue::U32(0));
        backend.set_value(42, ch2, FakeValue::U32(1));
        backend.set_settable(42, ch1);
        backend.set_settable(42, ch2);

        controller.mute_default_output().unwrap();
        assert_eq!(
            controller.applied_strategy(),
            Some(&AppliedMuteStrategy::ChannelMute {
                previous: vec![(1, false), (2, true)]
            })
        );
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, ch1, FakeValue::U32(1)),
                (42, ch2, FakeValue::U32(1)),
                (42, ch1, FakeValue::U32(0)),
                (42, ch2, FakeValue::U32(1)),
            ]
        );
    }

    #[test]
    fn failed_channel_mute_rolls_back_previous_channels() {
        let (backend, mut controller) = fake_with_default_output();
        let preferred =
            PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
        let ch1 = PropertyAddress::output(SELECTOR_MUTE, 1);
        let ch2 = PropertyAddress::output(SELECTOR_MUTE, 2);
        backend.set_value(42, preferred, FakeValue::U32Array(vec![1, 2]));
        backend.set_value(42, ch1, FakeValue::U32(0));
        backend.set_value(42, ch2, FakeValue::U32(0));
        backend.set_settable(42, ch1);
        backend.set_settable(42, ch2);
        backend.fail_set(42, ch2);

        assert!(controller.mute_default_output().is_err());

        assert_eq!(
            backend.writes(),
            vec![(42, ch1, FakeValue::U32(1)), (42, ch1, FakeValue::U32(0)),]
        );
    }

    #[test]
    fn master_volume_soft_mute_is_used_when_no_mute_controls_exist() {
        let (backend, mut controller) = fake_with_default_output();
        let volume = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, ELEMENT_MAIN);
        backend.set_value(42, volume, FakeValue::F32(0.7));
        backend.set_settable(42, volume);

        controller.mute_default_output().unwrap();
        assert_eq!(
            controller.applied_strategy(),
            Some(&AppliedMuteStrategy::MasterVolume { previous: 0.7 })
        );
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, volume, FakeValue::F32(0.0)),
                (42, volume, FakeValue::F32(0.7)),
            ]
        );
    }

    #[test]
    fn channel_volume_soft_mute_uses_available_channels() {
        let (backend, mut controller) = fake_with_default_output();
        let preferred =
            PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
        let ch1 = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, 1);
        let ch2 = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, 2);
        backend.set_value(42, preferred, FakeValue::U32Array(vec![1, 2]));
        backend.set_value(42, ch1, FakeValue::F32(0.5));
        backend.set_value(42, ch2, FakeValue::F32(0.6));
        backend.set_settable(42, ch1);
        backend.set_settable(42, ch2);

        controller.mute_default_output().unwrap();
        assert_eq!(
            controller.applied_strategy(),
            Some(&AppliedMuteStrategy::ChannelVolume {
                previous: vec![(1, 0.5), (2, 0.6)]
            })
        );
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, ch1, FakeValue::F32(0.0)),
                (42, ch2, FakeValue::F32(0.0)),
                (42, ch1, FakeValue::F32(0.5)),
                (42, ch2, FakeValue::F32(0.6)),
            ]
        );
    }

    #[test]
    fn failed_channel_volume_rolls_back_previous_channels() {
        let (backend, mut controller) = fake_with_default_output();
        let preferred =
            PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
        let ch1 = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, 1);
        let ch2 = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, 2);
        backend.set_value(42, preferred, FakeValue::U32Array(vec![1, 2]));
        backend.set_value(42, ch1, FakeValue::F32(0.4));
        backend.set_value(42, ch2, FakeValue::F32(0.5));
        backend.set_settable(42, ch1);
        backend.set_settable(42, ch2);
        backend.fail_set(42, ch2);

        assert!(controller.mute_default_output().is_err());

        assert_eq!(
            backend.writes(),
            vec![
                (42, ch1, FakeValue::F32(0.0)),
                (42, ch1, FakeValue::F32(0.4)),
            ]
        );
    }

    #[test]
    fn mute_is_idempotent_until_restored() {
        let (backend, mut controller) = fake_with_default_output();
        let volume = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, ELEMENT_MAIN);
        backend.set_value(42, volume, FakeValue::F32(0.8));
        backend.set_settable(42, volume);

        controller.mute_default_output().unwrap();
        controller.mute_default_output().unwrap();
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, volume, FakeValue::F32(0.0)),
                (42, volume, FakeValue::F32(0.8)),
            ]
        );
    }

    #[test]
    fn restore_without_prior_mute_is_noop() {
        let (backend, mut controller) = fake_with_default_output();
        controller.restore().unwrap();
        assert!(backend.writes().is_empty());
    }

    #[test]
    fn no_settable_controls_returns_error() {
        let (_backend, mut controller) = fake_with_default_output();
        assert_eq!(
            controller.mute_default_output(),
            Err(CoreAudioError::NoMuteCapability { device_id: 42 })
        );
    }

    #[test]
    fn unknown_default_output_is_reported() {
        let backend = FakeBackend::default();
        backend.default_output(AUDIO_OBJECT_UNKNOWN);
        let mut controller = OutputMuteController::new(backend);

        assert_eq!(
            controller.mute_default_output(),
            Err(CoreAudioError::UnknownDefaultOutputDevice)
        );
    }

    #[test]
    fn preferred_channels_are_deduped_and_ignore_main_element() {
        let (backend, mut controller) = fake_with_default_output();
        let preferred =
            PropertyAddress::output(SELECTOR_PREFERRED_CHANNELS_FOR_STEREO, ELEMENT_MAIN);
        let ch2 = PropertyAddress::output(SELECTOR_MUTE, 2);
        backend.set_value(42, preferred, FakeValue::U32Array(vec![0, 2, 2]));
        backend.set_value(42, ch2, FakeValue::U32(0));
        backend.set_settable(42, ch2);

        controller.mute_default_output().unwrap();

        assert_eq!(backend.writes(), vec![(42, ch2, FakeValue::U32(1))]);
    }

    #[test]
    fn default_output_change_mutes_new_device_and_restores_both() {
        let (backend, mut controller) = fake_with_default_output();
        let dev42_mute = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
        backend.set_value(42, dev42_mute, FakeValue::U32(0));
        backend.set_settable(42, dev42_mute);

        let dev84_mute = PropertyAddress::output(SELECTOR_VOLUME_SCALAR, ELEMENT_MAIN);
        backend.device_uid(84, "device-84");
        backend.set_value(84, dev84_mute, FakeValue::F32(0.6));
        backend.set_settable(84, dev84_mute);

        controller.mute_default_output().unwrap();
        backend.default_output(84);
        controller.handle_default_output_change().unwrap();
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, dev42_mute, FakeValue::U32(1)),
                (84, dev84_mute, FakeValue::F32(0.0)),
                (84, dev84_mute, FakeValue::F32(0.6)),
                (42, dev42_mute, FakeValue::U32(0)),
            ]
        );
    }

    #[test]
    fn restore_targets_device_uid_when_coreaudio_id_changes() {
        let (backend, mut controller) = fake_with_default_output();
        let old_mute = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
        let new_mute = PropertyAddress::output(SELECTOR_MUTE, ELEMENT_MAIN);
        backend.set_value(42, old_mute, FakeValue::U32(0));
        backend.set_settable(42, old_mute);

        controller.mute_default_output().unwrap();
        backend.set_value(
            42,
            PropertyAddress::global(SELECTOR_DEVICE_UID),
            FakeValue::String("stale-device".into()),
        );
        backend.device_uid(420, "device-42");
        backend.set_value(420, new_mute, FakeValue::U32(1));
        backend.set_settable(420, new_mute);
        controller.restore().unwrap();

        assert_eq!(
            backend.writes(),
            vec![
                (42, old_mute, FakeValue::U32(1)),
                (420, new_mute, FakeValue::U32(0)),
            ]
        );
    }

    #[test]
    fn device_listing_uses_coreaudio_defaults_and_channel_counts() {
        let backend = FakeBackend::default();
        backend.set_value(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEVICES),
            FakeValue::U32Array(vec![10, 20, 30]),
        );
        backend.set_value(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_INPUT_DEVICE),
            FakeValue::U32(10),
        );
        backend.set_value(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_OUTPUT_DEVICE),
            FakeValue::U32(20),
        );
        backend.set_value(
            AUDIO_OBJECT_SYSTEM_OBJECT,
            PropertyAddress::global(SELECTOR_DEFAULT_SYSTEM_OUTPUT_DEVICE),
            FakeValue::U32(30),
        );
        for (id, name, uid, input, output) in [
            (10, "Mic", "mic-uid", 1, 0),
            (20, "Speakers", "speaker-uid", 0, 2),
            (30, "Display", "display-uid", 0, 2),
        ] {
            backend.set_value(
                id,
                PropertyAddress::global(SELECTOR_OBJECT_NAME),
                FakeValue::String(name.into()),
            );
            backend.set_value(
                id,
                PropertyAddress::global(SELECTOR_DEVICE_UID),
                FakeValue::String(uid.into()),
            );
            backend.set_value(
                id,
                PropertyAddress::input(SELECTOR_STREAM_CONFIGURATION, ELEMENT_MAIN),
                FakeValue::Channels(input),
            );
            backend.set_value(
                id,
                PropertyAddress::output(SELECTOR_STREAM_CONFIGURATION, ELEMENT_MAIN),
                FakeValue::Channels(output),
            );
        }

        let devices = list_audio_devices_with_backend(&backend).unwrap();

        assert_eq!(devices.len(), 3);
        let mic = devices.iter().find(|device| device.name == "Mic").unwrap();
        assert!(mic.is_default_input);
        let speakers = devices
            .iter()
            .find(|device| device.name == "Speakers")
            .unwrap();
        assert!(speakers.is_default_output);
        let display = devices
            .iter()
            .find(|device| device.name == "Display")
            .unwrap();
        assert!(display.is_default_system_output);
    }
}
