use crate::device_initializer::InitDeviceError;
use crate::models::{InitDeviceContext, Mouser};

pub trait Logger {
    fn debug(&self, message: &str);
    fn info(&self, message: &str);
    fn error(&self, message: &str);
}

pub trait UsbHidProvider {
    fn get_hid_mouse_ids(&self) -> Result<Vec<String>, InitDeviceError>;
}

pub trait ManufacturerResolver {
    fn get_pid_vid(&self, hid_id: &str) -> Result<[i32; 2], InitDeviceError>;
    fn get_device_type(&self, hid_id: &str) -> Result<i32, InitDeviceError>;
    fn populate_usb_manufacturer(
        &self,
        pid: i32,
        vid: i32,
        mouser_usb: &mut Mouser,
    ) -> Result<(), InitDeviceError>;
}

pub trait BluetoothProvider {
    fn start_service(&self, context: &mut InitDeviceContext) -> Result<(), InitDeviceError>;
    fn find_first_ble_device_id(&self) -> Result<Option<String>, InitDeviceError>;
    fn create_and_init_from_device_id(
        &self,
        device_id: &str,
        context: &mut InitDeviceContext,
    ) -> Result<(), InitDeviceError>;
    fn mouse_type(&self) -> i32;
}

pub trait HidStarter {
    fn hid_startup(
        &self,
        vid: u16,
        pid: u16,
        manufacturer_id: i32,
    ) -> Result<(), InitDeviceError>;
}
