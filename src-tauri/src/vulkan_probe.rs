//! Out-of-band GPU enumeration for device hot-plug detection.
//!
//! ggml registers its compute devices once per process, so its registry goes
//! stale when a laptop dGPU powers off (Windows hybrid graphics) or when a new
//! GPU appears. This module enumerates physical GPUs directly through the
//! Vulkan loader — a throwaway `VkInstance` that never touches ggml state — so
//! the app can detect that drift and rebind the transcription engine onto
//! hardware that actually exists.
//!
//! Best-effort by design: any failure returns `None` and callers keep their
//! pre-existing behavior. Only Windows x86_64 is wired up today; other
//! platforms report "no fresh information".

/// Names of the physical GPU devices currently present on the system, as the
/// Vulkan driver reports them (e.g. `"AMD Radeon(TM) Graphics"`). `None` means
/// the probe is unavailable on this platform or failed — callers must treat
/// that as "no fresh information", not "no devices".
pub fn probe_gpu_device_names() -> Option<Vec<String>> {
    probe_gpu_device_names_impl()
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn probe_gpu_device_names_impl() -> Option<Vec<String>> {
    use std::ffi::{c_char, c_void};

    const VK_SUCCESS: i32 = 0;
    const VK_STRUCTURE_TYPE_APPLICATION_INFO: i32 = 0;
    const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: i32 = 1;
    // VK_API_VERSION_1_0 == VK_MAKE_API_VERSION(0, 1, 0, 0)
    const VK_API_VERSION_1_0: u32 = 1 << 22;

    type VkInstance = *mut c_void;
    type VkPhysicalDevice = *mut c_void;

    #[repr(C)]
    struct VkApplicationInfo {
        s_type: i32,
        p_next: *const c_void,
        p_application_name: *const c_char,
        application_version: u32,
        p_engine_name: *const c_char,
        engine_version: u32,
        api_version: u32,
    }

    #[repr(C)]
    struct VkInstanceCreateInfo {
        s_type: i32,
        p_next: *const c_void,
        flags: u32,
        p_application_info: *const VkApplicationInfo,
        enabled_layer_count: u32,
        pp_enabled_layer_names: *const *const c_char,
        enabled_extension_count: u32,
        pp_enabled_extension_names: *const *const c_char,
    }

    type PfnCreateInstance = unsafe extern "system" fn(
        *const VkInstanceCreateInfo,
        *const c_void,
        *mut VkInstance,
    ) -> i32;
    type PfnEnumeratePhysicalDevices =
        unsafe extern "system" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> i32;
    // Writes the full VkPhysicalDeviceProperties struct; we only read the
    // leading fields, so the caller passes an oversized byte buffer.
    type PfnGetPhysicalDeviceProperties = unsafe extern "system" fn(VkPhysicalDevice, *mut u8);
    type PfnDestroyInstance = unsafe extern "system" fn(VkInstance, *const c_void);

    // VkPhysicalDeviceProperties contains 64-bit fields. Its output buffer
    // needs C-compatible alignment even though we only inspect its bytes.
    #[repr(C, align(16))]
    struct PhysicalDevicePropertiesStorage([u8; 1024]);

    unsafe extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const c_char) -> isize;
        fn GetProcAddress(h_module: isize, lp_proc_name: *const c_char) -> isize;
        fn FreeLibrary(h_lib_module: isize) -> i32;
    }

    let library = unsafe { LoadLibraryA(c"vulkan-1.dll".as_ptr()) };
    if library == 0 {
        return None;
    }

    // From here on every early-exit must FreeLibrary.
    let result = (|| {
        unsafe {
            let p_create_instance = GetProcAddress(library, c"vkCreateInstance".as_ptr());
            let p_enumerate_devices =
                GetProcAddress(library, c"vkEnumeratePhysicalDevices".as_ptr());
            let p_get_properties =
                GetProcAddress(library, c"vkGetPhysicalDeviceProperties".as_ptr());
            let p_destroy_instance = GetProcAddress(library, c"vkDestroyInstance".as_ptr());
            if p_create_instance == 0
                || p_enumerate_devices == 0
                || p_get_properties == 0
                || p_destroy_instance == 0
            {
                return None;
            }
            let create_instance: PfnCreateInstance = std::mem::transmute(p_create_instance);
            let enumerate_devices: PfnEnumeratePhysicalDevices =
                std::mem::transmute(p_enumerate_devices);
            let get_properties: PfnGetPhysicalDeviceProperties =
                std::mem::transmute(p_get_properties);
            let destroy_instance: PfnDestroyInstance = std::mem::transmute(p_destroy_instance);

            let app_info = VkApplicationInfo {
                s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
                p_next: std::ptr::null(),
                p_application_name: c"Handy".as_ptr(),
                application_version: 0,
                p_engine_name: std::ptr::null(),
                engine_version: 0,
                api_version: VK_API_VERSION_1_0,
            };
            let create_info = VkInstanceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                p_next: std::ptr::null(),
                flags: 0,
                p_application_info: &app_info,
                enabled_layer_count: 0,
                pp_enabled_layer_names: std::ptr::null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: std::ptr::null(),
            };

            let mut instance: VkInstance = std::ptr::null_mut();
            if create_instance(&create_info, std::ptr::null(), &mut instance) != VK_SUCCESS {
                return None;
            }

            let mut count: u32 = 0;
            if enumerate_devices(instance, &mut count, std::ptr::null_mut()) != VK_SUCCESS {
                destroy_instance(instance, std::ptr::null());
                return None;
            }
            if count == 0 {
                destroy_instance(instance, std::ptr::null());
                return Some(Vec::new());
            }
            let mut physical_devices = vec![std::ptr::null_mut::<c_void>(); count as usize];
            if enumerate_devices(instance, &mut count, physical_devices.as_mut_ptr().cast())
                != VK_SUCCESS
            {
                destroy_instance(instance, std::ptr::null());
                return None;
            }

            // VkPhysicalDeviceProperties layout (stable since Vulkan 1.0):
            //   0: apiVersion, 4: driverVersion, 8: vendorID, 12: deviceID,
            //   16: deviceType, 20: deviceName[256], ...
            const DEVICE_NAME_OFFSET: usize = 20;
            const DEVICE_NAME_LENGTH: usize = 256;
            let mut names = Vec::with_capacity(count as usize);
            for device in &physical_devices[..count as usize] {
                let mut props = PhysicalDevicePropertiesStorage([0u8; 1024]);
                get_properties(*device, props.0.as_mut_ptr());
                let name_bytes =
                    &props.0[DEVICE_NAME_OFFSET..DEVICE_NAME_OFFSET + DEVICE_NAME_LENGTH];
                let name_length = name_bytes
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(name_bytes.len());
                let name = String::from_utf8_lossy(&name_bytes[..name_length])
                    .trim_end()
                    .to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }

            destroy_instance(instance, std::ptr::null());
            Some(names)
        }
    })();

    unsafe { FreeLibrary(library) };
    result
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn probe_gpu_device_names_impl() -> Option<Vec<String>> {
    None
}
