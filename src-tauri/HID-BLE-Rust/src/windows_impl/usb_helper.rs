use crate::device_initializer::InitDeviceError;
use crate::ports::UsbHidProvider;

use super::powershell::run_powershell_lines;

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsUsbHidProvider;

impl UsbHidProvider for WindowsUsbHidProvider {
    fn get_hid_mouse_ids(&self) -> Result<Vec<String>, InitDeviceError> {
        run_powershell_lines(
            r#"Get-PnpDevice -PresentOnly |
Where-Object { $_.InstanceId -like 'HID*' } |
Select-Object -ExpandProperty InstanceId"#,
        )
    }
}
