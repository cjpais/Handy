use cpal::traits::{DeviceTrait, HostTrait};

pub struct CpalDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub channels: u16,
    pub device: cpal::Device,
}

pub fn list_input_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for (index, device) in host.input_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());
        let is_default = Some(name.clone()) == default_name;
        let channels = device
            .default_input_config()
            .map(|c| c.channels())
            .unwrap_or(1);

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            channels,
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
        let channels = device
            .default_output_config()
            .map(|c| c.channels())
            .unwrap_or(1);

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            channels,
            device,
        });
    }

    Ok(out)
}
