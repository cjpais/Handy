use std::sync::{Arc, Mutex};
use tauri::{AppHandle, LogicalPosition, WebviewUrl, WebviewWindowBuilder};

pub struct VoiceActivityWindowManager {
    window_handle: Arc<Mutex<Option<tauri::WebviewWindow>>>,
    app_handle: AppHandle,
}

impl VoiceActivityWindowManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            window_handle: Arc::new(Mutex::new(None)),
            app_handle,
        }
    }

    pub fn show_window(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut window_guard = self.window_handle.lock().unwrap();

        // If window doesn't exist, create it
        if window_guard.is_none() {
            let window = WebviewWindowBuilder::new(
                &self.app_handle,
                "voice-activity-indicator",
                WebviewUrl::App("voice-activity.html".into()),
            )
            .title("Voice Activity Indicator")
            .inner_size(160.0, 50.0)
            .min_inner_size(160.0, 50.0)
            .max_inner_size(160.0, 50.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false) // Start hidden, we'll show it after positioning
            .build()?;

            // Position window at bottom center of screen
            self.position_window_bottom_center(&window)?;

            // Now show the window
            window.show()?;

            *window_guard = Some(window);
        } else if let Some(window) = window_guard.as_ref() {
            // Window exists, just show it
            window.show()?;
        }

        Ok(())
    }

    pub fn hide_window(&self) -> Result<(), Box<dyn std::error::Error>> {
        let window_guard = self.window_handle.lock().unwrap();

        if let Some(window) = window_guard.as_ref() {
            window.hide()?;
        }

        Ok(())
    }

    pub fn close_window(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut window_guard = self.window_handle.lock().unwrap();

        if let Some(window) = window_guard.take() {
            window.close()?;
        }

        Ok(())
    }

    fn position_window_bottom_center(
        &self,
        window: &tauri::WebviewWindow,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get the primary monitor size
        if let Some(monitor) = window.primary_monitor()? {
            let monitor_size = monitor.size();
            let scale_factor = monitor.scale_factor();

            // Calculate logical screen dimensions
            let screen_width = monitor_size.width as f64 / scale_factor;
            let screen_height = monitor_size.height as f64 / scale_factor;

            // Window dimensions
            let window_width = 160.0;
            let window_height = 50.0;

            // Position at bottom center with some margin from bottom
            let x = (screen_width - window_width) / 2.0;
            let y = screen_height - window_height - 20.0;

            window.set_position(LogicalPosition::new(x, y))?;
        }

        Ok(())
    }

    pub fn is_window_visible(&self) -> bool {
        let window_guard = self.window_handle.lock().unwrap();

        if let Some(window) = window_guard.as_ref() {
            window.is_visible().unwrap_or(false)
        } else {
            false
        }
    }
}

impl Clone for VoiceActivityWindowManager {
    fn clone(&self) -> Self {
        Self {
            window_handle: self.window_handle.clone(),
            app_handle: self.app_handle.clone(),
        }
    }
}
