use rodio::cpal;

/// The CPAL host feedback sounds play through. On Linux, uses ALSA host. On
/// other platforms, uses the default host. Microphone capture picks its own
/// host inside handy-recorder.
pub fn get_cpal_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    {
        cpal::host_from_id(cpal::HostId::Alsa).unwrap_or_else(|_| cpal::default_host())
    }
    #[cfg(not(target_os = "linux"))]
    {
        cpal::default_host()
    }
}
