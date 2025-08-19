pub fn is_vulkan_available() -> bool {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        use ash::vk;
        
        let result = unsafe {
            let entry = match ash::Entry::load() {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Failed to load Vulkan entry: {:?}", e);
                    return false;
                }
            };

            let app_desc = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 0, 0));
            let instance_desc = vk::InstanceCreateInfo::default().application_info(&app_desc);

            let instance = match entry.create_instance(&instance_desc, None) {
                Ok(inst) => inst,
                Err(e) => {
                    eprintln!("Failed to create Vulkan instance: {:?}", e);
                    return false;
                }
            };

            instance.destroy_instance(None);
            println!("Vulkan support is successfully checked and working.");
            true
        };
        
        if !result {
            eprintln!("ERROR: Vulkan is not available on this system. Handy requires Vulkan support on Linux/Windows.");
            std::process::exit(1);
        }
        
        result
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        println!("Vulkan check skipped on this platform (macOS)");
        true
    }
}