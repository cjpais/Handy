use std::thread;
use std::time::{Duration, Instant};

use crate::models::{ConnectionMode, DeviceTypePidVid, InitDeviceContext};
use crate::ports::{BluetoothProvider, HidStarter, Logger, ManufacturerResolver, UsbHidProvider};

#[derive(Debug, Clone)]
pub struct InitDeviceReport {
    pub device_type_list: Vec<DeviceTypePidVid>,
    pub final_mode: ConnectionMode,
    pub usb_connected: bool,
    pub ble_connected: bool,
}

#[derive(Debug, Clone)]
pub enum InitDeviceError {
    Provider(String),
    Conversion(String),
}

impl std::fmt::Display for InitDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(message) => write!(f, "{message}"),
            Self::Conversion(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for InitDeviceError {}

pub struct DeviceInitializer<U, R, B, H, L>
where
    U: UsbHidProvider,
    R: ManufacturerResolver,
    B: BluetoothProvider,
    H: HidStarter,
    L: Logger,
{
    usb_provider: U,
    resolver: R,
    bluetooth: B,
    hid_starter: H,
    logger: L,
}

impl<U, R, B, H, L> DeviceInitializer<U, R, B, H, L>
where
    U: UsbHidProvider,
    R: ManufacturerResolver,
    B: BluetoothProvider,
    H: HidStarter,
    L: Logger,
{
    pub fn new(usb_provider: U, resolver: R, bluetooth: B, hid_starter: H, logger: L) -> Self {
        Self {
            usb_provider,
            resolver,
            bluetooth,
            hid_starter,
            logger,
        }
    }

    pub fn init_device(
        &self,
        context: &mut InitDeviceContext,
    ) -> Result<InitDeviceReport, InitDeviceError> {
        self.logger.debug("Starting HID initialization...");

        context.args.mouse_connection_mode = ConnectionMode::None;
        context.args.keyboard_connection_mode = ConnectionMode::None;
        context.args.stylus_pen_connection_mode = ConnectionMode::None;

        let hid_ids = self.usb_provider.get_hid_mouse_ids()?;
        let mut device_type_list = Vec::new();

        for hid_id in hid_ids {
            let pv = self.resolver.get_pid_vid(&hid_id)?;
            let device_type = self.resolver.get_device_type(&hid_id)?;
            if !device_type_list
                .iter()
                .any(|item: &DeviceTypePidVid| item.device_type == device_type)
            {
                device_type_list.push(DeviceTypePidVid {
                    device_type,
                    pid: pv[1],
                    vid: pv[0],
                });
            }
        }

        if context.mouser_usb.manufacturer_id == 0 || context.mouser_usb.manufacturer_id == 1 {
            context.bw_run_worker_completed = false;
            context.args.mouse_connection_mode = ConnectionMode::Receiver;
            context.args.hid_receiver_online = true;
            self.logger.debug("Known receiver manufacturer detected.");
        } else if context.mouser_usb.type_name.is_empty() {
            self.try_bluetooth(context)?;
            self.logger.debug("Bluetooth fallback branch completed.");
        }

        self.try_hid_candidates(context, &device_type_list)?;
        context.bw_run_worker_completed = false;
        self.logger.debug("Device initialization completed.");

        Ok(InitDeviceReport {
            final_mode: context.args.mouse_connection_mode,
            usb_connected: context.mouser_usb.online_status_usb,
            ble_connected: context.mouser_ble.online_status_ble,
            device_type_list,
        })
    }

    fn try_bluetooth(&self, context: &mut InitDeviceContext) -> Result<(), InitDeviceError> {
        self.logger.debug("Starting Bluetooth initialization...");
        context.args.is_the_bluetooth_connection_completed = false;

        self.bluetooth.start_service(context)?;

        if let Some(device_id) = self.bluetooth.find_first_ble_device_id()? {
            self.bluetooth
                .create_and_init_from_device_id(&device_id, context)?;
        }

        self.logger.debug("Waiting for Bluetooth pairing or scan completion.");
        self.wait_until(Duration::from_secs(3), || {
            context.args.bluetooth_pairing_state || context.args.bluetooth_led_status_completed
        });

        self.logger.debug("Bluetooth device scan finished.");

        match self.bluetooth.mouse_type() {
            3 => {
                self.wait_until(Duration::from_secs(5), || {
                    !context.mouser_ble.type_name.is_empty()
                        && !context.mouser_ble.battery_power.is_empty()
                });
            }
            7 => {
                self.wait_until(Duration::from_secs(5), || !context.mouser_ble.type_name.is_empty());
            }
            _ => {
                context.mouser_ble.serial_number_id = "unknown".to_string();
            }
        }

        if !context.mouser_ble.type_name.is_empty() {
            context.mouser_ble.online_status_ble = true;
            self.logger.info("Bluetooth device connected.");
            self.logger
                .debug(&format!("Bluetooth model: {}", context.mouser_ble.type_name));
            context.args.mouse_connection_mode = ConnectionMode::Bluetooth;
            context.args.is_the_bluetooth_connection_completed = true;
        } else {
            context.mouser_ble.online_status_ble = false;
            self.logger.error("Bluetooth connection failed because device type is empty.");
            context.args.mouse_connection_mode = ConnectionMode::None;
        }

        Ok(())
    }

    fn try_hid_candidates(
        &self,
        context: &mut InitDeviceContext,
        device_type_list: &[DeviceTypePidVid],
    ) -> Result<(), InitDeviceError> {
        for item in device_type_list {
            context.mouser_usb.v_id = item.vid;
            context.mouser_usb.p_id = item.pid;

            self.resolver
                .populate_usb_manufacturer(item.pid, item.vid, &mut context.mouser_usb)?;

            if item.device_type != 8 && item.device_type != 11 {
                let vid = u16::try_from(item.vid).map_err(|_| {
                    InitDeviceError::Conversion(format!("vid out of u16 range: {}", item.vid))
                })?;
                let pid = u16::try_from(item.pid).map_err(|_| {
                    InitDeviceError::Conversion(format!("pid out of u16 range: {}", item.pid))
                })?;

                self.hid_starter
                    .hid_startup(vid, pid, context.mouser_usb.manufacturer_id)?;

                self.process_manufacturer_info(context);
            }

            if context.args.hid_receiver_online {
                if context.mouser_usb.type_name.is_empty() {
                    self.logger.error("Receiver exists but HID model fetch failed.");
                    context.mouser_usb.online_status_usb = false;
                } else {
                    context.mouser_usb.online_status_usb = true;
                    self.logger.info("2.4G receiver device connected.");
                    context.args.mouse_connection_mode = ConnectionMode::Receiver;
                    context.user_settings.mouse_serial_number = context.mouser_usb.serial_number_id.clone();
                    context.user_settings.v_id = context.mouser_usb.v_id.to_string();
                    context.user_settings.p_id = context.mouser_usb.p_id.to_string();
                }
            } else {
                context.mouser_usb.online_status_usb = false;
            }
        }

        Ok(())
    }

    fn process_manufacturer_info(&self, context: &mut InitDeviceContext) {
        if context.mouser_usb.manufacturer_id == 0 {
            self.wait_until(Duration::from_secs(3), || {
                !context.mouser_usb.type_name.is_empty()
                    && !context.mouser_usb.battery_power.is_empty()
            });
        }
    }

    fn wait_until<F>(&self, timeout: Duration, mut predicate: F)
    where
        F: FnMut() -> bool,
    {
        let started = Instant::now();
        while !predicate() {
            if started.elapsed() >= timeout {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ArgumentsState, InitDeviceContext, Mouser, UserSettingsState};
    use crate::ports::{BluetoothProvider, HidStarter, Logger, ManufacturerResolver, UsbHidProvider};
    use std::cell::RefCell;

    struct TestLogger;
    impl Logger for TestLogger {
        fn debug(&self, _message: &str) {}
        fn info(&self, _message: &str) {}
        fn error(&self, _message: &str) {}
    }

    struct TestUsbProvider;
    impl UsbHidProvider for TestUsbProvider {
        fn get_hid_mouse_ids(&self) -> Result<Vec<String>, InitDeviceError> {
            Ok(vec!["hid-1".to_string()])
        }
    }

    struct TestResolver;
    impl ManufacturerResolver for TestResolver {
        fn get_pid_vid(&self, _hid_id: &str) -> Result<[i32; 2], InitDeviceError> {
            Ok([4660, 22136])
        }

        fn get_device_type(&self, _hid_id: &str) -> Result<i32, InitDeviceError> {
            Ok(0)
        }

        fn populate_usb_manufacturer(
            &self,
            pid: i32,
            vid: i32,
            mouser_usb: &mut Mouser,
        ) -> Result<(), InitDeviceError> {
            mouser_usb.p_id = pid;
            mouser_usb.v_id = vid;
            mouser_usb.manufacturer_id = 0;
            mouser_usb.type_name = "TJ Mouse".to_string();
            mouser_usb.battery_power = "100".to_string();
            mouser_usb.serial_number_id = "SN-001".to_string();
            Ok(())
        }
    }

    struct TestBluetooth {
        mouse_type: i32,
    }
    impl BluetoothProvider for TestBluetooth {
        fn start_service(&self, context: &mut InitDeviceContext) -> Result<(), InitDeviceError> {
            context.args.bluetooth_pairing_state = true;
            context.mouser_ble.type_name = "BLE Mouse".to_string();
            context.mouser_ble.battery_power = "80".to_string();
            Ok(())
        }

        fn find_first_ble_device_id(&self) -> Result<Option<String>, InitDeviceError> {
            Ok(Some("ble-001".to_string()))
        }

        fn create_and_init_from_device_id(
            &self,
            _device_id: &str,
            _context: &mut InitDeviceContext,
        ) -> Result<(), InitDeviceError> {
            Ok(())
        }

        fn mouse_type(&self) -> i32 {
            self.mouse_type
        }
    }

    struct TestHidStarter {
        calls: RefCell<Vec<(u16, u16, i32)>>,
    }
    impl TestHidStarter {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl HidStarter for TestHidStarter {
        fn hid_startup(
            &self,
            vid: u16,
            pid: u16,
            manufacturer_id: i32,
        ) -> Result<(), InitDeviceError> {
            self.calls.borrow_mut().push((vid, pid, manufacturer_id));
            Ok(())
        }
    }

    #[test]
    fn init_device_prefers_receiver_when_usb_manufacturer_already_known() {
        let initializer = DeviceInitializer::new(
            TestUsbProvider,
            TestResolver,
            TestBluetooth { mouse_type: 3 },
            TestHidStarter::new(),
            TestLogger,
        );

        let mut context = InitDeviceContext {
            mouser_usb: Mouser {
                manufacturer_id: 0,
                ..Default::default()
            },
            mouser_ble: Mouser::default(),
            args: ArgumentsState::default(),
            user_settings: UserSettingsState::default(),
            bw_run_worker_completed: true,
        };

        let report = initializer.init_device(&mut context).unwrap();
        assert_eq!(report.final_mode, ConnectionMode::Receiver);
        assert!(context.args.hid_receiver_online);
    }

    #[test]
    fn init_device_can_fall_back_to_bluetooth() {
        let initializer = DeviceInitializer::new(
            TestUsbProvider,
            TestResolver,
            TestBluetooth { mouse_type: 7 },
            TestHidStarter::new(),
            TestLogger,
        );

        let mut context = InitDeviceContext {
            mouser_usb: Mouser {
                manufacturer_id: -1,
                ..Default::default()
            },
            mouser_ble: Mouser::default(),
            args: ArgumentsState::default(),
            user_settings: UserSettingsState::default(),
            bw_run_worker_completed: true,
        };

        let report = initializer.init_device(&mut context).unwrap();
        assert!(report.ble_connected);
        assert_eq!(context.args.mouse_connection_mode, ConnectionMode::Bluetooth);
    }
}
