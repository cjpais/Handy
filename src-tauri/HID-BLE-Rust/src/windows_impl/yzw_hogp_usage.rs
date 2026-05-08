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

#[derive(Clone, Default)]
pub struct WindowsHidStarter {
    registry: Arc<Mutex<ReaderRegistry>>,
    callback: Arc<Mutex<Option<HidDataCallback>>>,
}

impl WindowsHidStarter {
    pub fn new() -> Self {
        Self::default()
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
}

impl HidStarter for WindowsHidStarter {
    fn hid_startup(&self, vid: u16, pid: u16, manufacturer_id: i32) -> Result<(), InitDeviceError> {
        let api = HidApi::new()
            .map_err(|e| InitDeviceError::Provider(format!("HidApi::new failed: {e}")))?;

        // The receiver exposes several HID collections (mouse, keyboard, vendor).
        // The voice / button reports live on the vendor collection. We open every
        // matching path and let the parser filter — opening read-only is cheap and
        // the OS allows multiple readers per HID collection.
        let mut opened = 0usize;
        for info in api
            .device_list()
            .filter(|d| d.vendor_id() == vid && d.product_id() == pid)
        {
            let path = match info.path().to_str() {
                Ok(p) => p.to_string(),
                Err(_) => continue,
            };

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
                        info.usage_page(),
                        info.usage(),
                    );
                    continue;
                }
            };

            let _ = device.set_blocking_mode(true);

            let stop = Arc::new(AtomicBool::new(false));
            let callback_slot = Arc::clone(&self.callback);
            let registry = Arc::clone(&self.registry);
            let stop_for_thread = Arc::clone(&stop);
            let path_for_thread = path.clone();
            let usage_page = info.usage_page();
            let usage = info.usage();

            thread::Builder::new()
                .name(format!("hid-reader-{:04X}-{:04X}", vid, pid))
                .spawn(move || {
                    run_reader_loop(
                        device,
                        stop_for_thread,
                        callback_slot,
                        path_for_thread.clone(),
                        usage_page,
                        usage,
                    );
                    // Self-cleanup so we can re-open later if device reappears.
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
                "No HID interfaces opened for VID_{:04X} PID_{:04X} (manufacturer_id={})",
                vid, pid, manufacturer_id
            )));
        }

        log::info!(
            "HID starter opened {} interface(s) for VID_{:04X} PID_{:04X} (manufacturer_id={})",
            opened,
            vid,
            pid,
            manufacturer_id
        );
        Ok(())
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
    // Short label like "MI_02" or "Col04" to keep per-frame logs readable.
    let label = short_path_label(&path);

    while !stop.load(Ordering::SeqCst) {
        match device.read_timeout(&mut buf, 200) {
            Ok(0) => {
                consecutive_errors = 0;
            }
            Ok(n) => {
                consecutive_errors = 0;
                // hidapi delivers the leading Report ID as buf[0] for numbered
                // reports (matches Win32 ReadFile behaviour). The original C#
                // implementation strips this byte before parsing — see
                // Hid.cs ReadCompleted: `reportData = readBuff[1..]`. Mirror
                // that here so opcode indexes (idx1=2, idx2=3) and the audio
                // marker (data[0]==0x3C) line up exactly.
                let payload: Vec<u8> = if n > 1 {
                    buf[1..n].to_vec()
                } else {
                    buf[..n].to_vec()
                };
                log::debug!(
                    "HID frame [{label}] report_id={:#04X} len={} bytes: {}",
                    buf[0],
                    payload.len(),
                    hex_preview(&payload, 16)
                );
                let cb_opt = callback_slot.lock().ok().and_then(|g| g.clone());
                if let Some(cb) = cb_opt {
                    cb(&payload);
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                let msg = e.to_string();
                // Only log once per unique error string to avoid log spam, but
                // keep it at info so we can see why a reader died.
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

/// Build a short label like "MI_02" or "Col04" or "KBD" out of a HID path so
/// per-frame log lines stay readable.
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
