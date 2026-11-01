//! A verified paste destination captured before the overlay accepts keyboard focus.

#[derive(Clone, Copy)]
pub struct PasteTarget {
    #[cfg(target_os = "windows")]
    handle: isize,
    #[cfg(target_os = "windows")]
    process_id: u32,
}

pub fn supported() -> bool {
    cfg!(target_os = "windows")
}

impl PasteTarget {
    #[cfg(target_os = "windows")]
    pub fn capture(overlay: &tauri::WebviewWindow) -> Result<Self, String> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() || overlay.hwnd().map_err(|_| "Overlay is unavailable")? == hwnd {
            return Err("Choose the destination app before editing".into());
        }
        let mut process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        }
        if process_id == 0 {
            return Err("Could not identify the destination app".into());
        }
        log::debug!(
            "Review destination captured: process={process_id}, window={}",
            hwnd.0 as isize
        );
        Ok(Self {
            handle: hwnd.0 as isize,
            process_id,
        })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn capture(_: &tauri::WebviewWindow) -> Result<Self, String> {
        Err("Live editing is currently available on Windows".into())
    }

    #[cfg(target_os = "windows")]
    fn hwnd(self) -> windows::Win32::Foundation::HWND {
        windows::Win32::Foundation::HWND(self.handle as *mut std::ffi::c_void)
    }

    #[cfg(target_os = "windows")]
    fn valid(self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(self.hwnd(), Some(&mut pid));
        }
        unsafe { IsWindow(Some(self.hwnd())).as_bool() && pid == self.process_id }
    }

    #[cfg(target_os = "windows")]
    pub fn restore(self, app: &tauri::AppHandle) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::{
            IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        if !self.valid() {
            return Err("The original destination window has closed".into());
        }
        // The GUI thread owns the foreground overlay and its input queue.
        // A worker thread cannot reliably transfer that foreground privilege.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let before = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            let accepted = unsafe {
                if IsIconic(self.hwnd()).as_bool() {
                    let _ = ShowWindow(self.hwnd(), SW_RESTORE);
                }
                SetForegroundWindow(self.hwnd()).as_bool()
            };
            log::debug!(
                "Review activation: previous={}, destination={}, accepted={accepted}",
                before.0 as isize,
                self.handle
            );
            let _ = tx.send(());
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "Destination activation timed out; draft retained".to_string())?;
        // Cross-thread activation is asynchronous: SetForegroundWindow can
        // return while the target is still processing WM_ACTIVATE. Wait for
        // the actual foreground identity, never assume the request succeeded.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            if self.check_focused().is_ok() {
                return Ok(());
            }
            if !self.valid() {
                return Err("The original destination window has closed".into());
            }
            if std::time::Instant::now() >= deadline {
                return self.check_focused();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn restore(self, _: &tauri::AppHandle) -> Result<(), String> {
        Err("Destination restoration is unavailable".into())
    }

    #[cfg(target_os = "windows")]
    pub fn check_focused(self) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        if self.valid() && unsafe { GetForegroundWindow() } == self.hwnd() {
            Ok(())
        } else {
            Err("Could not return to the original destination; draft retained".into())
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn check_focused(self) -> Result<(), String> {
        Err("Destination verification is unavailable".into())
    }
}
