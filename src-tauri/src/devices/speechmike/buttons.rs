/// Physical buttons on a Philips SpeechMike device.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SpeechMikeButton {
    Record,
    Stop,
    Eol,
    InsertOverwrite,
    Trigger,
    Forward,
    Rewind,
    /// Report ID we have not yet mapped to a named button.
    Unknown(u8),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ButtonEventKind {
    Press,
    Release,
    StateOn,
    StateOff,
}

#[derive(Debug, Clone)]
pub struct ButtonEvent {
    pub button: SpeechMikeButton,
    pub kind: ButtonEventKind,
    pub raw_report: Vec<u8>,
}

/// Parse a raw HID report buffer into a ButtonEvent.
///
/// Phase-2 note: the byte-to-button mapping is filled in empirically from
/// live-device logs (enable `livesttt_raw_hid_debug` and press buttons).
/// Until mapped, all reports are returned as `Unknown(report_id)` and the
/// full buffer is logged at debug level.
pub fn parse_button_event(raw: &[u8]) -> Option<ButtonEvent> {
    if raw.is_empty() {
        return None;
    }
    let report_id = raw[0];
    log::debug!(
        "SpeechMike HID report_id={:#04x} bytes={}",
        report_id,
        raw.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Some(ButtonEvent {
        button: SpeechMikeButton::Unknown(report_id),
        kind: ButtonEventKind::Press,
        raw_report: raw.to_vec(),
    })
}
