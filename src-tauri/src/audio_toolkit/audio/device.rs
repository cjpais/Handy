use cpal::traits::{DeviceTrait, HostTrait};

use super::recorder_backend::CaptureBackend;

pub struct CpalDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub device: cpal::Device,
}

/// A selectable microphone, independent of the backend that produced it.
///
/// `id` is the backend's stable handle — a cpal device name, or a PipeWire
/// `node.name`. `name` is what the user sees AND what is persisted in
/// `selected_microphone`, so it must stay stable across enumerations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
}

/// Enumerate microphones for the backend that is actually in use.
///
/// Under cpal's ALSA host Linux only ever reports "default", which is why the
/// PipeWire backend enumerates the graph's real source nodes instead. Every
/// other backend keeps the existing cpal enumeration untouched.
pub fn list_input_devices_for_backend(
    backend: CaptureBackend,
) -> Result<Vec<InputDeviceInfo>, Box<dyn std::error::Error>> {
    match backend {
        #[cfg(target_os = "linux")]
        CaptureBackend::PipeWire => Ok(super::pipewire_recorder::list_pipewire_sources()?
            .into_iter()
            .map(|source| InputDeviceInfo {
                id: source.node_name,
                name: source.description,
            })
            .collect()),
        #[cfg(not(target_os = "linux"))]
        CaptureBackend::PipeWire => Ok(Vec::new()),
        CaptureBackend::Cpal => Ok(list_input_devices()?
            .into_iter()
            .map(|device| InputDeviceInfo {
                id: device.name.clone(),
                name: device.name,
            })
            .collect()),
    }
}

/// Map a persisted `selected_microphone` to a PipeWire `node.name`.
///
/// The persisted value is the human-readable description, so it is matched
/// against the description first and against `node.name` second (a value saved
/// under a different scheme, or copied between machines, still resolves).
/// `None` means "no match" — the caller falls back to the default source rather
/// than failing, so a name selected under the cpal backend degrades gracefully
/// when the backend changes.
#[cfg(target_os = "linux")]
pub fn resolve_pipewire_target(selected_name: &str) -> Option<String> {
    let sources = match super::pipewire_recorder::list_pipewire_sources() {
        Ok(sources) => sources,
        Err(e) => {
            log::warn!("Could not enumerate PipeWire sources ({e}); using the default source");
            return None;
        }
    };

    if let Some(source) = sources.iter().find(|s| s.description == selected_name) {
        return Some(source.node_name.clone());
    }
    if let Some(source) = sources.iter().find(|s| s.node_name == selected_name) {
        return Some(source.node_name.clone());
    }

    log::warn!(
        "Selected microphone '{selected_name}' is not a PipeWire source; using the default source"
    );
    None
}

pub fn list_input_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for (index, device) in host.input_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device,
        });
    }

    Ok(out)
}

pub fn list_output_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for (index, device) in host.output_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device,
        });
    }

    Ok(out)
}
