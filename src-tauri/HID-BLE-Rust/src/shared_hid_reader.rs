use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::device_initializer::InitDeviceError;
use crate::moser_hid_startup::{HandlerConfig, MoserHidStartupHandler, MoserHost};
use crate::ports::HidStarter;

/// Boxed callback that receives a single HID input report payload.
pub type HidDataCallback = Arc<dyn Fn(&[u8]) + Send + Sync + 'static>;

#[derive(Default)]
struct ReaderRegistry {
    running: Vec<ReaderEntry>,
}

struct ReaderEntry {
    key: String,
    stop: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct SharedHidStarter {
    registry: Arc<Mutex<ReaderRegistry>>,
    callback: Arc<Mutex<Option<HidDataCallback>>>,
    thread_name_prefix: &'static str,
}

impl SharedHidStarter {
    pub fn new(thread_name_prefix: &'static str) -> Self {
        Self {
            registry: Arc::new(Mutex::new(ReaderRegistry::default())),
            callback: Arc::new(Mutex::new(None)),
            thread_name_prefix,
        }
    }

    /// Install the callback that will receive every HID input report from
    /// every opened reader thread.
    pub fn set_data_callback(&self, callback: HidDataCallback) {
        if let Ok(mut slot) = self.callback.lock() {
            *slot = Some(callback);
        }
    }

    /// Stop all reader threads (e.g. when device disappears).
    pub fn stop_all(&self) {
        if let Ok(mut reg) = self.registry.lock() {
            for entry in reg.running.drain(..) {
                entry.stop.store(true, Ordering::SeqCst);
            }
        }
    }

    fn already_running(&self, key: &str) -> bool {
        self.registry
            .lock()
            .map(|reg| reg.running.iter().any(|entry| entry.key == key))
            .unwrap_or(false)
    }

    fn register(&self, key: String, stop: Arc<AtomicBool>) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.running.push(ReaderEntry { key, stop });
        }
    }

    pub fn hid_startup_with_filter<F>(
        &self,
        vid: u16,
        pid: u16,
        manufacturer_id: i32,
        filter_description: &str,
        mut filter: F,
    ) -> Result<(), InitDeviceError>
    where
        F: FnMut(u16, u16) -> bool,
    {
        let api = HidApi::new()
            .map_err(|e| InitDeviceError::Provider(format!("HidApi::new failed: {e}")))?;

        let mut opened = 0usize;
        for info in api
            .device_list()
            .filter(|d| d.vendor_id() == vid && d.product_id() == pid)
        {
            let path = info.path().to_string_lossy().to_string();
            let usage_page = info.usage_page();
            let usage = info.usage();

            if !filter(usage_page, usage) {
                log::debug!(
                    "HID skip interface (vid={:04X} pid={:04X} usage_page={:#06X} usage={:#06X}) due to filter: {}",
                    vid,
                    pid,
                    usage_page,
                    usage,
                    filter_description
                );
                continue;
            }

            if self.already_running(&path) {
                continue;
            }

            let device = match api.open_path(info.path()) {
                Ok(dev) => dev,
                Err(e) => {
                    log::debug!(
                        "HID open_path failed (vid={:04X} pid={:04X} usage_page={:#06X} usage={:#06X}): {e}",
                        vid,
                        pid,
                        usage_page,
                        usage,
                    );
                    continue;
                }
            };

            let _ = device.set_blocking_mode(true);
            log::info!(
                "HID opening interface: vid={:04X} pid={:04X} usage_page={:#06X} usage={:#06X} path={}",
                vid,
                pid,
                usage_page,
                usage,
                path
            );

            let stop = Arc::new(AtomicBool::new(false));
            let callback_slot = Arc::clone(&self.callback);
            let registry = Arc::clone(&self.registry);
            let stop_for_thread = Arc::clone(&stop);
            let path_for_thread = path.clone();
            let thread_name_prefix = self.thread_name_prefix;

            thread::Builder::new()
                .name(format!("{thread_name_prefix}-{:04X}-{:04X}", vid, pid))
                .spawn(move || {
                    run_reader_loop(
                        device,
                        stop_for_thread,
                        callback_slot,
                        path_for_thread.clone(),
                        usage_page,
                        usage,
                    );
                    if let Ok(mut reg) = registry.lock() {
                        reg.running.retain(|e| e.key != path_for_thread);
                    }
                })
                .map_err(|e| InitDeviceError::Provider(format!("spawn HID reader failed: {e}")))?;

            self.register(path, stop);
            opened += 1;
        }

        if opened == 0 {
            return Err(InitDeviceError::Provider(format!(
                "No HID interfaces opened for VID_{:04X} PID_{:04X} (manufacturer_id={}, filter={})",
                vid, pid, manufacturer_id, filter_description
            )));
        }

        log::info!(
            "HID starter opened {} interface(s) for VID_{:04X} PID_{:04X} (manufacturer_id={}, filter={})",
            opened,
            vid,
            pid,
            manufacturer_id,
            filter_description
        );
        Ok(())
    }
}

impl Default for SharedHidStarter {
    fn default() -> Self {
        Self::new("hid-reader")
    }
}

impl HidStarter for SharedHidStarter {
    fn hid_startup(&self, vid: u16, pid: u16, manufacturer_id: i32) -> Result<(), InitDeviceError> {
        self.hid_startup_with_filter(vid, pid, manufacturer_id, "all-interfaces", |_, _| true)
    }
}

fn run_reader_loop(
    device: HidDevice,
    stop: Arc<AtomicBool>,
    callback_slot: Arc<Mutex<Option<HidDataCallback>>>,
    path: String,
    usage_page: u16,
    usage: u16,
) {
    log::info!(
        "HID reader started: usage_page={:#06X} usage={:#06X} path={}",
        usage_page,
        usage,
        path
    );

    let mut buf = [0u8; 128];
    let mut consecutive_errors = 0u32;
    let mut last_error_log: Option<String> = None;
    let label = short_path_label(&path);

    while !stop.load(Ordering::SeqCst) {
        match device.read_timeout(&mut buf, 200) {
            Ok(0) => {
                consecutive_errors = 0;
            }
            Ok(n) => {
                consecutive_errors = 0;
                let payload = normalize_hid_report(&buf[..n]);
                log::debug!(
                    "HID frame [{label}] report_id={:#04X} len={} bytes: {}",
                    buf[0],
                    payload.len(),
                    hex_preview(payload, 16)
                );
                let cb_opt = callback_slot.lock().ok().and_then(|g| g.clone());
                if let Some(cb) = cb_opt {
                    cb(payload);
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                let msg = e.to_string();
                if last_error_log.as_deref() != Some(msg.as_str()) {
                    log::info!("HID read error [{label}]: {msg}");
                    last_error_log = Some(msg);
                }
                if consecutive_errors > 50 {
                    log::warn!("HID reader giving up after repeated errors [{label}]: {path}");
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    log::info!("HID reader stopped: {}", path);
}

fn normalize_hid_report(raw: &[u8]) -> &[u8] {
    if raw.len() <= 1 {
        return raw;
    }

    if raw_payload_is_aligned(raw) {
        return raw;
    }

    let stripped = &raw[1..];
    if payload_is_aligned(stripped) {
        return stripped;
    }

    // The original Windows path always strips the leading report-id byte before
    // parsing. Keep that as the fallback for platforms where hidapi exposes the
    // report id but the frame is not one of the opcodes we recognise yet.
    stripped
}

fn raw_payload_is_aligned(data: &[u8]) -> bool {
    payload_is_aligned(data)
}

fn payload_is_aligned(data: &[u8]) -> bool {
    if data.first() == Some(&0x3C) {
        return true;
    }

    if data.len() > 4 && is_known_opcode(data[2], data[3]) {
        return true;
    }

    false
}

fn is_known_opcode(opcode: u8, subcode: u8) -> bool {
    matches!(
        (opcode, subcode),
        (32, 1)
            | (32, 3)
            | (32, 4)
            | (34, 1)
            | (34, 3)
            | (34, 4)
            | (35, 3)
            | (35, 4)
            | (48, 1)
            | (48, 3)
            | (48, 4)
    )
}

fn short_path_label(path: &str) -> String {
    if path.ends_with("\\KBD") {
        return "KBD".to_string();
    }

    let upper = path.to_ascii_uppercase();
    let mi = upper
        .split('#')
        .find(|seg| seg.starts_with("VID_"))
        .and_then(|seg| seg.split('&').find(|tok| tok.starts_with("MI_")))
        .unwrap_or("");
    let col = upper
        .split('#')
        .find(|seg| seg.starts_with("VID_"))
        .and_then(|seg| seg.split('&').find(|tok| tok.starts_with("COL")))
        .unwrap_or("");
    match (mi.is_empty(), col.is_empty()) {
        (true, true) => "?".to_string(),
        (false, true) => mi.to_string(),
        (true, false) => col.to_string(),
        (false, false) => format!("{mi}/{col}"),
    }
}

fn hex_preview(data: &[u8], max: usize) -> String {
    let take = data.len().min(max);
    let mut out = String::with_capacity(take * 3);
    for (i, b) in data[..take].iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:02X}", b));
    }
    if data.len() > take {
        out.push_str(" ..");
    }
    out
}

/// Helper that wraps a `MoserHidStartupHandler` + `MoserHost` so the reader
/// thread callback can dispatch into the parser without each caller writing
/// the locking boilerplate.
pub struct MoserDispatcher<H: MoserHost + Send + 'static> {
    handler: Mutex<MoserHidStartupHandler>,
    host: Mutex<H>,
}

impl<H: MoserHost + Send + 'static> MoserDispatcher<H> {
    pub fn new(config: HandlerConfig, host: H) -> Self {
        Self {
            handler: Mutex::new(MoserHidStartupHandler {
                config,
                ..Default::default()
            }),
            host: Mutex::new(host),
        }
    }

    pub fn dispatch(&self, data: &[u8]) {
        let mut handler = match self.handler.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut host = match self.host.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        log::debug!(
            "MoserDispatcher::dispatch len={} mfr={} mode={:?}",
            data.len(),
            handler.config.manufacturer,
            handler.config.mouse_connection_mode
        );
        if let Err(_e) = handler.data_received(data.to_vec(), &mut *host) {
            // Host's Error type is opaque to us; the host itself is expected to log details.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_known_opcode, normalize_hid_report};

    #[test]
    fn keeps_payload_when_frame_already_matches_audio_marker() {
        let raw = [0x3C, 0x01, 0x02, 0x03];
        assert_eq!(normalize_hid_report(&raw), &raw);
    }

    #[test]
    fn strips_report_id_when_payload_starts_after_first_byte() {
        let raw = [0x05, 0x3C, 0x11, 0x22, 0x33];
        assert_eq!(normalize_hid_report(&raw), &raw[1..]);
    }

    #[test]
    fn strips_report_id_for_numbered_button_reports() {
        let raw = [0xC0, 0x00, 0x02, 0x20, 0x04, 0x00];
        assert_eq!(normalize_hid_report(&raw), &raw[1..]);
    }

    #[test]
    fn keeps_payload_when_button_opcode_is_already_aligned() {
        let raw = [0x00, 0x02, 0x20, 0x04, 0x00];
        assert_eq!(normalize_hid_report(&raw), &raw);
    }

    #[test]
    fn recognises_known_moser_opcodes() {
        assert!(is_known_opcode(32, 1));
        assert!(is_known_opcode(35, 4));
        assert!(!is_known_opcode(0x99, 0x88));
    }
}
