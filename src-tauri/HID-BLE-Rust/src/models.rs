#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Receiver,
    Bluetooth,
    None,
}

impl Default for ConnectionMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mouser {
    pub serial_number_id: String,
    pub p_id: i32,
    pub v_id: i32,
    pub name: String,
    pub description: String,
    pub type_name: String,
    pub manufacturer_id: i32,
    pub online_status_usb: bool,
    pub online_status_ble: bool,
    pub online_status_2_4: bool,
    pub ble_paired_and_connected: bool,
    pub dpi: String,
    pub battery_power: String,
    pub version: String,
    pub mac: String,
    pub bluetooth_pairing_id: String,
    pub bluetooth_mac_id: u64,
    pub microphone: String,
    pub roller_key: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ArgumentsState {
    pub mouse_connection_mode: ConnectionMode,
    pub keyboard_connection_mode: ConnectionMode,
    pub stylus_pen_connection_mode: ConnectionMode,
    pub bluetooth_pairing_state: bool,
    pub bluetooth_led_status_completed: bool,
    pub is_the_bluetooth_connection_completed: bool,
    pub hid_receiver_online: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UserSettingsState {
    pub mouse_serial_number: String,
    pub v_id: String,
    pub p_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct InitDeviceContext {
    pub mouser_usb: Mouser,
    pub mouser_ble: Mouser,
    pub args: ArgumentsState,
    pub user_settings: UserSettingsState,
    pub bw_run_worker_completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceTypePidVid {
    pub device_type: i32,
    pub pid: i32,
    pub vid: i32,
}
