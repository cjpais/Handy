use crate::device_initializer::InitDeviceError;
use crate::ports::HidStarter;

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsHidStarter;

impl HidStarter for WindowsHidStarter {
    fn hid_startup(
        &self,
        _vid: u16,
        _pid: u16,
        _manufacturer_id: i32,
    ) -> Result<(), InitDeviceError> {
        // The original C# code binds HID callbacks through Moser_HID_Startup.HIDStartup.
        // This adapter keeps the Rust side compilable and lets the host app plug a real HID implementation later.
        Ok(())
    }
}
