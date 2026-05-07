/// Current status of the SpeechMike device.
/// Compiled on all platforms; `supported_platform: false` on Linux.
#[derive(serde::Serialize, Clone, Debug, specta::Type)]
pub struct SpeechMikeStatus {
    pub supported_platform: bool,
    pub connected: bool,
    pub blocked_by_other_app: bool,
    pub device_name: Option<String>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub serial_number: Option<String>,
    pub audio_device_name: Option<String>,
    pub buttons_enabled: bool,
    pub auto_select_enabled: bool,
    pub last_error: Option<String>,
    pub detected_blocking_processes: Vec<String>,
}

impl SpeechMikeStatus {
    pub fn disconnected() -> Self {
        Self {
            supported_platform: true,
            connected: false,
            blocked_by_other_app: false,
            device_name: None,
            vendor_id: None,
            product_id: None,
            serial_number: None,
            audio_device_name: None,
            buttons_enabled: false,
            auto_select_enabled: true,
            last_error: None,
            detected_blocking_processes: vec![],
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn unsupported() -> Self {
        Self {
            supported_platform: false,
            connected: false,
            blocked_by_other_app: false,
            device_name: None,
            vendor_id: None,
            product_id: None,
            serial_number: None,
            audio_device_name: None,
            buttons_enabled: false,
            auto_select_enabled: false,
            last_error: None,
            detected_blocking_processes: vec![],
        }
    }
}
