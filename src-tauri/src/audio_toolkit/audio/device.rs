use cpal::traits::{DeviceTrait, HostTrait};

pub struct CpalDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub device: cpal::Device,
}

// cpal 0.17 deprecated Device::name() in favor of description()/id(). We keep name() here
// because the device-name strings are what users select in the picker and save in settings,
// and what we later match by equality. Switching to id()/description() changes both the stored
// key and the displayed text, so it belongs in a separate PR, not this dependency bump.
#[allow(deprecated)]
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

#[allow(deprecated)] // cpal 0.17 deprecated name(); kept to preserve device-name matching, see list_input_devices
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
