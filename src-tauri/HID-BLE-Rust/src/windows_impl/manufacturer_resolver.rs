use crate::device_initializer::InitDeviceError;
use crate::models::Mouser;
use crate::ports::ManufacturerResolver;

#[derive(Debug, Clone)]
pub struct ManufacturerRule {
    pub vid: i32,
    pub pid: i32,
    pub device_type: i32,
    pub manufacturer_id: i32,
    pub type_name: &'static str,
}

#[derive(Debug, Clone)]
pub struct WindowsManufacturerResolver {
    rules: Vec<ManufacturerRule>,
}

impl WindowsManufacturerResolver {
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

impl Default for WindowsManufacturerResolver {
    fn default() -> Self {
        Self::with_default_rules()
    }
}

impl ManufacturerResolver for WindowsManufacturerResolver {
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
    fn parse_vid_pid_from_hid_instance_id() {
        let resolver = WindowsManufacturerResolver::default();
        let parsed = resolver
            .get_pid_vid(r"HID\VID_248A&PID_C0BB&REV_0001")
            .unwrap();
        assert_eq!(parsed, [0x248A, 0xC0BB]);
    }

    #[test]
    fn default_rules_match_wpf_receiver_mapping() {
        let resolver = WindowsManufacturerResolver::default();
        assert_eq!(
            resolver
                .get_device_type(r"HID\VID_248A&PID_C0CB&REV_0001")
                .unwrap(),
            1
        );
        assert_eq!(
            resolver
                .get_device_type(r"HID\VID_1E9D&PID_0867&REV_0001")
                .unwrap(),
            0
        );
        assert_eq!(
            resolver
                .get_device_type(r"HID\VID_0D8C&PID_0312&REV_0001")
                .unwrap(),
            0
        );
    }
}
