#[cfg(target_os = "windows")]
mod windows_session {
    use once_cell::sync::OnceCell;
    use tauri::{AppHandle, Manager};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY,
        WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_UNLOCK,
    };

    static APP_HANDLE: OnceCell<AppHandle> = OnceCell::new();

    pub fn setup_session_notifications(app: AppHandle) {
        if APP_HANDLE.set(app).is_err() {
            log::debug!("Windows session notifications already initialized");
            return;
        }

        std::thread::spawn(|| {
            if let Err(e) = run_message_loop() {
                log::warn!("Windows session notification listener stopped: {}", e);
            }
        });
    }

    fn run_message_loop() -> windows::core::Result<()> {
        let class_name = w!("HandySessionNotificationWindow");
        let window_name = w!("Handy Session Notifications");

        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: class_name,
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&wnd_class);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                window_name,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?;

            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
            log::info!("Windows session unlock notifications registered");

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = WTSUnRegisterSessionNotification(hwnd);
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_UNLOCK => {
                log::info!("Windows session unlocked; refreshing shortcuts");
                if let Some(app) = APP_HANDLE.get() {
                    if app
                        .try_state::<crate::commands::ShortcutsInitialized>()
                        .is_some()
                    {
                        crate::shortcut::refresh_shortcuts_after_resume(app);
                    } else {
                        log::debug!(
                            "Skipping session-unlock shortcut refresh; shortcuts are not initialized"
                        );
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_session::setup_session_notifications;

#[cfg(not(target_os = "windows"))]
pub fn setup_session_notifications(_app: tauri::AppHandle) {}
