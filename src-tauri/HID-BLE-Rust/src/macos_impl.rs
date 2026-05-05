use hidapi::HidApi;

use crate::device_initializer::InitDeviceError;
use crate::models::Mouser;
use crate::ports::{HidStarter, ManufacturerResolver, UsbHidProvider};
use crate::shared_hid_reader::{HidDataCallback, SharedHidStarter};

pub use crate::shared_hid_reader::MoserDispatcher;

#[derive(Debug, Clone)]
pub struct ManufacturerRule {
    pub vid: i32,
    pub pid: i32,
    pub device_type: i32,
    pub manufacturer_id: i32,
    pub type_name: &'static str,
}

#[derive(Debug, Clone)]
pub struct MacosManufacturerResolver {
    rules: Vec<ManufacturerRule>,
}

impl MacosManufacturerResolver {
    pub fn new(rules: Vec<ManufacturerRule>) -> Self {
        Self { rules }
    }

    pub fn with_default_rules() -> Self {
        Self::new(vec![
            ManufacturerRule {
                vid: 0x1E9D,
                pid: 0x0867,
                device_type: 0,
                manufacturer_id: 0,
                type_name: "TJ Mouse",
            },
            ManufacturerRule {
                vid: 0x0D8C,
                pid: 0x0312,
                device_type: 0,
                manufacturer_id: 0,
                type_name: "TJ Mouse",
            },
            ManufacturerRule {
                vid: 0x248A,
                pid: 0xC0CB,
                device_type: 1,
                manufacturer_id: 1,
                type_name: "TJ Mouse Receiver",
            },
            ManufacturerRule {
                vid: 0x248A,
                pid: 0xC0BB,
                device_type: 7,
                manufacturer_id: 7,
                type_name: "YZW BLE Mouse",
            },
        ])
    }

    fn parse_vid_pid(&self, hid_id: &str) -> Result<(i32, i32), InitDeviceError> {
        let upper = hid_id.to_ascii_uppercase();
        let vid = extract_hex_after(&upper, "VID_")
            .ok_or_else(|| InitDeviceError::Provider(format!("VID not found in: {hid_id}")))?;
        let pid = extract_hex_after(&upper, "PID_")
            .ok_or_else(|| InitDeviceError::Provider(format!("PID not found in: {hid_id}")))?;
        Ok((vid, pid))
    }

    fn find_rule(&self, vid: i32, pid: i32) -> Option<&ManufacturerRule> {
        self.rules
            .iter()
            .find(|rule| rule.vid == vid && rule.pid == pid)
    }
}

impl Default for MacosManufacturerResolver {
    fn default() -> Self {
        Self::with_default_rules()
    }
}

impl ManufacturerResolver for MacosManufacturerResolver {
    fn get_pid_vid(&self, hid_id: &str) -> Result<[i32; 2], InitDeviceError> {
        let (vid, pid) = self.parse_vid_pid(hid_id)?;
        Ok([vid, pid])
    }

    fn get_device_type(&self, hid_id: &str) -> Result<i32, InitDeviceError> {
        let (vid, pid) = self.parse_vid_pid(hid_id)?;
        Ok(self
            .find_rule(vid, pid)
            .map(|rule| rule.device_type)
            .unwrap_or(-1))
    }

    fn populate_usb_manufacturer(
        &self,
        pid: i32,
        vid: i32,
        mouser_usb: &mut Mouser,
    ) -> Result<(), InitDeviceError> {
        mouser_usb.p_id = pid;
        mouser_usb.v_id = vid;

        if let Some(rule) = self.find_rule(vid, pid) {
            mouser_usb.manufacturer_id = rule.manufacturer_id;
            if mouser_usb.type_name.is_empty() {
                mouser_usb.type_name = rule.type_name.to_string();
            }
        }

        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MacosUsbHidProvider;

impl UsbHidProvider for MacosUsbHidProvider {
    fn get_hid_mouse_ids(&self) -> Result<Vec<String>, InitDeviceError> {
        let api = HidApi::new()
            .map_err(|e| InitDeviceError::Provider(format!("HidApi::new failed: {e}")))?;

        Ok(api
            .device_list()
            .map(|info| {
                format!(
                    "HID\\VID_{:04X}&PID_{:04X}&UP_{:04X}&U_{:04X}\\{}",
                    info.vendor_id(),
                    info.product_id(),
                    info.usage_page(),
                    info.usage(),
                    info.path().to_string_lossy()
                )
            })
            .collect())
    }
}

#[derive(Clone)]
pub struct MacosHidStarter {
    inner: SharedHidStarter,
}

impl MacosHidStarter {
    pub fn new() -> Self {
        Self {
            inner: SharedHidStarter::new("macos-hid-reader"),
        }
    }

    pub fn set_data_callback(&self, callback: HidDataCallback) {
        self.inner.set_data_callback(callback);
    }

    pub fn stop_all(&self) {
        self.inner.stop_all();
    }
}

impl Default for MacosHidStarter {
    fn default() -> Self {
        Self::new()
    }
}

impl HidStarter for MacosHidStarter {
    fn hid_startup(&self, vid: u16, pid: u16, manufacturer_id: i32) -> Result<(), InitDeviceError> {
        self.inner.hid_startup_with_filter(
            vid,
            pid,
            manufacturer_id,
            "macos-vendor-defined",
            |usage_page, _usage| usage_page >= 0xFF00,
        )
    }
}

fn extract_hex_after(value: &str, marker: &str) -> Option<i32> {
    let start = value.find(marker)? + marker.len();
    let hex: String = value[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .collect();

    if hex.is_empty() {
        return None;
    }

    i32::from_str_radix(&hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vid_pid_from_synthetic_hid_id() {
        let resolver = MacosManufacturerResolver::default();
        let parsed = resolver
            .get_pid_vid(r"HID\VID_248A&PID_C0CB&UP_FF00&U_0001\IOService:/foo")
            .unwrap();
        assert_eq!(parsed, [0x248A, 0xC0CB]);
    }

    #[test]
    fn default_rules_match_receiver_mapping() {
        let resolver = MacosManufacturerResolver::default();
        assert_eq!(
            resolver
                .get_device_type(r"HID\VID_248A&PID_C0CB&UP_FF00&U_0001")
                .unwrap(),
            1
        );
    }
}
