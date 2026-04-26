//! Detection of the AI mouse's UAC microphone endpoint.
//!
//! The receiver dongle for our AI mouse exposes its captured audio as a
//! standard USB Audio Class input device, which Windows enumerates with a
//! generic friendly name like `麦克风 (USBAudio2.0)`. Multiple unrelated
//! UAC devices on a system can share that exact friendly name, so we
//! identify the right one by looking for a matching VID/PID inside the
//! device interface path / device description at the WASAPI / MMDevice
//! layer.

/// USB VID of the AI mouse receiver. Matches the entry in
/// `aimouse_device_init`'s manufacturer rules for "TJ Mouse Receiver".
pub const AI_MOUSE_VID: u16 = 0x248A;
/// USB PID of the AI mouse receiver.
pub const AI_MOUSE_PID: u16 = 0xC0CB;

/// Look up the AI mouse microphone's friendly name (as cpal would report it)
/// by enumerating active capture endpoints and matching the USB VID/PID of
/// the AI mouse receiver. Returns `None` if no matching endpoint is active
/// — typically meaning the receiver is unplugged.
#[cfg(target_os = "windows")]
pub fn find_ai_mouse_microphone_name() -> Option<String> {
    use windows::Win32::Devices::FunctionDiscovery::{
        PKEY_DeviceInterface_FriendlyName, PKEY_Device_DeviceDesc, PKEY_Device_FriendlyName,
        PKEY_Device_InstanceId,
    };
    use windows::Win32::Media::Audio::{
        eCapture, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };

    unsafe {
        // Tolerate "already initialised" — Tauri may have done it on this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let collection = enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .ok()?;
        let count = collection.GetCount().ok()?;

        let vid_pid_marker = format!("VID_{:04X}&PID_{:04X}", AI_MOUSE_VID, AI_MOUSE_PID);

        for i in 0..count {
            let device: IMMDevice = match collection.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let endpoint_id = match device.GetId() {
                Ok(p) => {
                    let s = pwstr_to_string(p.0);
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                }
                Err(_) => String::new(),
            };

            let store = match device.OpenPropertyStore(STGM_READ) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let friendly_name =
                read_string_property(&store, &PKEY_Device_FriendlyName).unwrap_or_default();
            let interface_name =
                read_string_property(&store, &PKEY_DeviceInterface_FriendlyName)
                    .unwrap_or_default();
            let device_desc =
                read_string_property(&store, &PKEY_Device_DeviceDesc).unwrap_or_default();
            // Instance ID contains the USB device path with VID/PID, e.g.
            // "SWD\MMDEVAPI\{0.0.1.00000000}.{GUID}" which refers back to the
            // physical USB\VID_248A&PID_C0CB node.
            let instance_id =
                read_string_property(&store, &PKEY_Device_InstanceId).unwrap_or_default();

            let haystack = format!(
                "{}\n{}\n{}\n{}\n{}",
                endpoint_id, interface_name, device_desc, instance_id, friendly_name
            );
            log::debug!(
                "[ai_mouse_mic] endpoint[{i}] id={endpoint_id:?} \
                 friendly={friendly_name:?} iface={interface_name:?} \
                 desc={device_desc:?} instance={instance_id:?}"
            );
            if haystack.to_ascii_uppercase().contains(&vid_pid_marker) {
                log::debug!("[ai_mouse_mic] matched AI mouse mic: {friendly_name:?}");
                return Some(friendly_name);
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
pub fn find_ai_mouse_microphone_name() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
unsafe fn pwstr_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    String::from_utf16_lossy(slice)
}

#[cfg(target_os = "windows")]
unsafe fn read_string_property(
    store: &windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore,
    key: &windows::Win32::Foundation::PROPERTYKEY,
) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

    let value = store.GetValue(key as *const _).ok()?;
    let alloc = PropVariantToStringAlloc(&value as *const _).ok()?;
    if alloc.0.is_null() {
        return None;
    }
    let s = pwstr_to_string(alloc.0);
    CoTaskMemFree(Some(alloc.0 as *const _));
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
