/// Philips USB Vendor ID shared by all SpeechMike, SpeechOne, and SpeechControl devices.
pub const PHILIPS_SPEECHMIKE_VID: u16 = 0x0911;

/// Substrings looked for in audio device names when auto-selecting a microphone.
const AUDIO_NAME_KEYWORDS: &[&str] = &["SpeechMike", "Speech Mike", "SpeechOne", "Philips"];

pub fn is_philips_speechmike(vendor_id: u16) -> bool {
    vendor_id == PHILIPS_SPEECHMIKE_VID
}

/// Find an audio input device whose name matches the connected SpeechMike.
///
/// Strategy:
/// 1. Case-insensitive substring match against `product_string` from HID.
/// 2. Fallback: any device whose name contains a known Philips keyword.
pub fn find_matching_audio_device(product_string: &str) -> Option<String> {
    let devices = crate::audio_toolkit::list_input_devices().ok()?;
    let product_lower = product_string.to_lowercase();

    // Prefer a device whose name overlaps with the HID product string.
    if let Some(d) = devices.iter().find(|d| {
        let n = d.name.to_lowercase();
        n.contains(&product_lower) || product_lower.contains(n.as_str())
    }) {
        return Some(d.name.clone());
    }

    // Fallback: any device with a known Philips keyword.
    devices
        .into_iter()
        .find(|d| AUDIO_NAME_KEYWORDS.iter().any(|kw| d.name.contains(kw)))
        .map(|d| d.name)
}
