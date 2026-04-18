use crate::device_initializer::InitDeviceError;
use crate::models::InitDeviceContext;
use crate::ports::BluetoothProvider;

use super::powershell::run_powershell_lines;

#[derive(Debug, Clone)]
pub struct WindowsBleDeviceInitAdapter {
    mouse_type: i32,
}

impl WindowsBleDeviceInitAdapter {
    pub fn new(mouse_type: i32) -> Self {
        Self { mouse_type }
    }

    fn get_ble_pnp_ids(&self) -> Result<Vec<String>, InitDeviceError> {
        run_powershell_lines(
            r#"Get-PnpDevice -PresentOnly |
Where-Object {
    $_.InstanceId -like 'BTHLE*' -or
    $_.InstanceId -like 'BluetoothLE*' -or
    $_.Class -eq 'Bluetooth'
} |
Select-Object -ExpandProperty InstanceId"#,
        )
    }

    fn get_ble_friendly_names(&self) -> Result<Vec<String>, InitDeviceError> {
        run_powershell_lines(
            r#"Get-PnpDevice -PresentOnly |
Where-Object {
    $_.InstanceId -like 'BTHLE*' -or
    $_.InstanceId -like 'BluetoothLE*' -or
    $_.Class -eq 'Bluetooth'
} |
Select-Object -ExpandProperty FriendlyName"#,
        )
    }
}

impl Default for WindowsBleDeviceInitAdapter {
    fn default() -> Self {
        Self::new(7)
    }
}

impl BluetoothProvider for WindowsBleDeviceInitAdapter {
    fn start_service(&self, context: &mut InitDeviceContext) -> Result<(), InitDeviceError> {
        let names = self.get_ble_friendly_names()?;
        if let Some(first_name) = names.into_iter().find(|name| !name.trim().is_empty()) {
            context.args.bluetooth_pairing_state = true;
            context.args.bluetooth_led_status_completed = true;
            if context.mouser_ble.type_name.is_empty() {
                context.mouser_ble.type_name = first_name;
            }
            context.mouser_ble.online_status_ble = true;
        }
        Ok(())
    }

    fn find_first_ble_device_id(&self) -> Result<Option<String>, InitDeviceError> {
        let ids = self.get_ble_pnp_ids()?;
        Ok(ids.into_iter().next())
    }

    fn create_and_init_from_device_id(
        &self,
        device_id: &str,
        context: &mut InitDeviceContext,
    ) -> Result<(), InitDeviceError> {
        context.args.bluetooth_pairing_state = true;
        context.args.bluetooth_led_status_completed = true;
        context.mouser_ble.bluetooth_pairing_id = device_id.to_string();
        if context.mouser_ble.serial_number_id.is_empty() {
            context.mouser_ble.serial_number_id = "unknown".to_string();
        }
        Ok(())
    }

    fn mouse_type(&self) -> i32 {
        self.mouse_type
    }
}
