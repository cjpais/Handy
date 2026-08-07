/// Returns whether an open microphone stream should be reopened after device
/// enumeration discovers a different system default.
///
/// A refresh must never interrupt an active recording or override an explicit
/// microphone choice. It also cannot safely replace the stream until the OS
/// exposes a resolvable default device.
pub(crate) fn should_reopen_default_microphone(
    stream_is_open: bool,
    recording_is_idle: bool,
    desired_device_name: Option<&str>,
    active_device_name: Option<&str>,
    current_default_name: Option<&str>,
) -> bool {
    stream_is_open
        && recording_is_idle
        && desired_device_name.is_none()
        && current_default_name.is_some()
        && active_device_name != current_default_name
}

#[cfg(test)]
mod tests {
    use super::should_reopen_default_microphone;

    #[test]
    fn reopens_an_idle_default_stream_when_the_system_default_changes() {
        assert!(should_reopen_default_microphone(
            true,
            true,
            None,
            Some("AirPods"),
            Some("USB microphone"),
        ));
    }

    #[test]
    fn keeps_the_stream_when_the_default_device_is_unchanged() {
        assert!(!should_reopen_default_microphone(
            true,
            true,
            None,
            Some("AirPods"),
            Some("AirPods"),
        ));
    }

    #[test]
    fn does_not_interrupt_recording_or_an_explicit_device() {
        assert!(!should_reopen_default_microphone(
            true,
            false,
            None,
            Some("AirPods"),
            Some("USB microphone"),
        ));
        assert!(!should_reopen_default_microphone(
            true,
            true,
            Some("AirPods"),
            Some("AirPods"),
            Some("USB microphone"),
        ));
    }

    #[test]
    fn waits_for_a_resolvable_default_and_an_open_stream() {
        assert!(!should_reopen_default_microphone(
            false,
            true,
            None,
            Some("AirPods"),
            Some("USB microphone"),
        ));
        assert!(!should_reopen_default_microphone(
            true,
            true,
            None,
            Some("AirPods"),
            None,
        ));
    }

    #[test]
    fn reopens_when_an_open_stream_has_no_known_active_device() {
        assert!(should_reopen_default_microphone(
            true,
            true,
            None,
            None,
            Some("AirPods"),
        ));
    }
}
