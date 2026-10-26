use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait},
};

/// An output device for feedback sounds. Microphones are listed by
/// `handy_recorder::list_input_devices`.
pub struct OutputDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

/// The name feedback sounds match `selected_output_device` against.
pub fn output_device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_owned())
}

pub fn list_output_devices() -> Result<Vec<OutputDeviceInfo>, Box<dyn std::error::Error>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| output_device_name(&d));

    let mut out = Vec::<OutputDeviceInfo>::new();

    for (index, device) in host.output_devices()?.enumerate() {
        let name = output_device_name(&device).unwrap_or_else(|| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(OutputDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
        });
    }

    Ok(out)
}
