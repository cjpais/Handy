use anyhow::Result;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub fn check_avx2_support() -> Result<()> {
    use raw_cpuid::CpuId;
    
    let cpuid = CpuId::new();
    
    // Check for AVX2 support
    if let Some(features) = cpuid.get_extended_feature_info() {
        if features.has_avx2() {
            log::info!("AVX2 support detected");
            return Ok(());
        }
    }
    
    Err(anyhow::anyhow!(
        "AVX2 instruction set not supported on this CPU. Handy requires AVX2 support for optimal performance with Whisper models."
    ))
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
pub fn check_avx2_support() -> Result<()> {
    // On non-x86 architectures, we don't need to check for AVX2
    log::info!("Non-x86 architecture detected, skipping AVX2 check");
    Ok(())
}