use crate::settings;

use enigo::Enigo;
use enigo::Key;
use enigo::Keyboard;
use enigo::Settings;

use cpal::traits::{DeviceTrait, HostTrait};
use log::debug;
use rodio::OutputStreamBuilder;
use std::fs::File;
use std::io::BufReader;
use std::thread;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIcon;
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Sends a paste command (Cmd+V or Ctrl+V) using platform-specific virtual key codes.
/// This ensures the paste works regardless of keyboard layout (e.g., Russian, AZERTY, DVORAK).
fn send_paste() -> Result<(), String> {
    // Platform-specific key definitions
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, Key::Other(9));
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56)); // VK_V
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    // Press modifier + V
    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    enigo
        .key(v_key_code, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press V key: {}", e))?;

    // Release V + modifier (reverse order)
    enigo
        .key(v_key_code, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release V key: {}", e))?;
    enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e))?;

    Ok(())
}

pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let clipboard = app_handle.clipboard();

    // get the current clipboard content
    let clipboard_content = clipboard.read_text().unwrap_or_default();

    clipboard
        .write_text(&text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    // small delay to ensure the clipboard content has been written to
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_paste()?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    // restore the clipboard
    clipboard
        .write_text(&clipboard_content)
        .map_err(|e| format!("Failed to restore clipboard: {}", e))?;

    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrayIconState {
    Idle,
    Recording,
    Transcribing,
}

pub fn change_tray_icon(app: &AppHandle, icon: TrayIconState) {
    let tray = app.state::<TrayIcon>();

    let icon_path = match icon {
        TrayIconState::Idle => "resources/tray_idle.png",
        TrayIconState::Recording => "resources/tray_recording.png",
        TrayIconState::Transcribing => "resources/tray_transcribing.png",
    };

    let _ = tray.set_icon(Some(
        Image::from_path(
            app.path()
                .resolve(icon_path, tauri::path::BaseDirectory::Resource)
                .expect("failed to resolve"),
        )
        .expect("failed to set icon"),
    ));

    // Update menu based on state
    update_tray_menu(app, &icon);
}

/// Centralized cancellation function that can be called from anywhere in the app.
/// Handles cancelling both recording and transcription operations and updates UI state.
pub fn cancel_current_operation(app: &AppHandle) {
    use crate::actions::ACTION_MAP;
    use crate::managers::audio::AudioRecordingManager;
    use crate::ManagedToggleState;
    use std::sync::Arc;

    println!("Initiating operation cancellation...");

    // First, reset all shortcut toggle states and call stop actions
    // This is critical for non-push-to-talk mode where shortcuts toggle on/off
    let toggle_state_manager = app.state::<ManagedToggleState>();
    if let Ok(mut states) = toggle_state_manager.lock() {
        // For each currently active toggle, call its stop action and reset state
        let active_bindings: Vec<String> = states
            .active_toggles
            .iter()
            .filter(|(_, &is_active)| is_active)
            .map(|(binding_id, _)| binding_id.clone())
            .collect();

        for binding_id in active_bindings {
            println!("Stopping active action for binding: {}", binding_id);

            // Call the action's stop method to ensure proper cleanup
            if let Some(action) = ACTION_MAP.get(&binding_id) {
                action.stop(app, &binding_id, "cancelled");
            }

            // Reset the toggle state
            if let Some(is_active) = states.active_toggles.get_mut(&binding_id) {
                *is_active = false;
            }
        }
    } else {
        eprintln!("Warning: Failed to lock toggle state manager during cancellation");
    }

    // Cancel any ongoing recording
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    audio_manager.cancel_recording();

    // Update tray icon and menu to idle state
    change_tray_icon(app, TrayIconState::Idle);

    println!("Operation cancellation completed - returned to idle state");
}

pub fn update_tray_menu(app: &AppHandle, state: &TrayIconState) {
    // Platform-specific accelerators
    #[cfg(target_os = "macos")]
    let (settings_accelerator, quit_accelerator) = (Some("Cmd+,"), Some("Cmd+Q"));
    #[cfg(not(target_os = "macos"))]
    let (settings_accelerator, quit_accelerator) = (Some("Ctrl+,"), Some("Ctrl+Q"));

    // Create common menu items
    let version_label = format!("Handy v{}", env!("CARGO_PKG_VERSION"));
    let version_i = MenuItem::with_id(app, "version", &version_label, false, None::<&str>)
        .expect("failed to create version item");
    let settings_i = MenuItem::with_id(app, "settings", "Settings...", true, settings_accelerator)
        .expect("failed to create settings item");
    let check_updates_i = MenuItem::with_id(
        app,
        "check_updates",
        "Check for Updates...",
        true,
        None::<&str>,
    )
    .expect("failed to create check updates item");
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, quit_accelerator)
        .expect("failed to create quit item");
    let separator = || PredefinedMenuItem::separator(app).expect("failed to create separator");

    let menu = match state {
        TrayIconState::Recording | TrayIconState::Transcribing => {
            let cancel_i = MenuItem::with_id(app, "cancel", "Cancel", true, None::<&str>)
                .expect("failed to create cancel item");
            Menu::with_items(
                app,
                &[
                    &version_i,
                    &separator(),
                    &cancel_i,
                    &separator(),
                    &settings_i,
                    &check_updates_i,
                    &separator(),
                    &quit_i,
                ],
            )
            .expect("failed to create menu")
        }
        TrayIconState::Idle => Menu::with_items(
            app,
            &[
                &version_i,
                &separator(),
                &settings_i,
                &check_updates_i,
                &separator(),
                &quit_i,
            ],
        )
        .expect("failed to create menu"),
    };

    let tray = app.state::<TrayIcon>();
    let _ = tray.set_menu(Some(menu));
    let _ = tray.set_icon_as_template(true);
}

/// Plays an audio resource from the resources directory.
/// Checks if audio feedback is enabled in settings before playing.
pub fn play_sound(app: &AppHandle, resource_path: &str) {
    // Check if audio feedback is enabled
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }

    let app_handle = app.clone();
    let resource_path = resource_path.to_string();

    // Spawn a new thread to play the audio without blocking the main thread
    thread::spawn(move || {
        // Get the path to the audio file in resources
        let audio_path = match app_handle
            .path()
            .resolve(&resource_path, tauri::path::BaseDirectory::Resource)
        {
            Ok(path) => path,
            Err(e) => {
                eprintln!(
                    "Failed to resolve audio file path '{}': {}",
                    resource_path, e
                );
                return;
            }
        };

        // Get the selected output device from settings
        let settings = settings::get_settings(&app_handle);
        let selected_device = settings.selected_output_device.clone();

        // Try to play the audio file
        if let Err(e) = play_audio_file(&audio_path, selected_device) {
            eprintln!("Failed to play sound '{}': {}", resource_path, e);
        }
    });
}

/// Convenience function to play the recording start sound
pub fn play_recording_start_sound(app: &AppHandle) {
    play_sound(app, "resources/rec_start.wav");
}

/// Convenience function to play the recording stop sound
pub fn play_recording_stop_sound(app: &AppHandle) {
    play_sound(app, "resources/rec_stop.wav");
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            println!("Using default device");
            // Use default device
            OutputStreamBuilder::from_default_device()?
        } else {
            // Try to find the device by name
            let host = cpal::default_host();
            let devices = host.output_devices()?;

            let mut found_device = None;
            for device in devices {
                if device.name()? == device_name {
                    found_device = Some(device);
                    break;
                }
            }

            match found_device {
                Some(device) => OutputStreamBuilder::from_device(device)?,
                None => {
                    eprintln!("Device '{}' not found, using default device", device_name);
                    OutputStreamBuilder::from_default_device()?
                }
            }
        }
    } else {
        println!("Using default device");
        // Use default device
        OutputStreamBuilder::from_default_device()?
    };

    let stream_handle = stream_builder.open_stream()?;
    let mixer = stream_handle.mixer();

    // Load the audio file
    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let sink = rodio::play(mixer, buf_reader)?;
    sink.sleep_until_end();

    Ok(())
}

/* ──────────────────────────────────────────────────────────────── */
/*                           OVERLAY MANAGEMENT                      */
/* ──────────────────────────────────────────────────────────────── */

/// Creates the recording overlay window and keeps it hidden by default
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // Get work area dimensions for positioning (respects taskbars, docks, etc.)
    if let Ok(monitors) = app_handle.primary_monitor() {
        if let Some(monitor) = monitors {
            const OVERLAY_WIDTH: f64 = 172.0;
            const OVERLAY_HEIGHT: f64 = 40.0;

            // Platform-specific bottom offset
            #[cfg(target_os = "windows")]
            const OVERLAY_BOTTOM_OFFSET: f64 = 46.0;
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            const OVERLAY_BOTTOM_OFFSET: f64 = 12.0;

            let work_area = monitor.work_area();
            let scale = monitor.scale_factor();
            let work_area_width = work_area.size.width as f64 / scale;
            let work_area_height = work_area.size.height as f64 / scale;
            let work_area_x = work_area.position.x as f64 / scale;
            let work_area_y = work_area.position.y as f64 / scale;

            // Position at bottom center of work area
            let x = work_area_x + (work_area_width - OVERLAY_WIDTH) / 2.0;
            let y = work_area_y + work_area_height - OVERLAY_BOTTOM_OFFSET;

            match WebviewWindowBuilder::new(
                app_handle,
                "recording_overlay",
                tauri::WebviewUrl::App("src/overlay/index.html".into()),
            )
            .title("Recording")
            .position(x, y)
            .resizable(false)
            .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
            .shadow(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .accept_first_mouse(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .transparent(true)
            .focused(false)
            .visible(false) // Start hidden
            .build()
            {
                Ok(_window) => {
                    debug!("Recording overlay window created successfully (hidden)");
                }
                Err(e) => {
                    debug!("Failed to create recording overlay window: {}", e);
                }
            }
        }
    }
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    // Check if show_overlay is enabled in settings
    let settings = settings::get_settings(app_handle);
    if !settings.show_overlay {
        return;
    }

    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.show();
        // Emit event to trigger fade-in animation with recording state
        let _ = overlay_window.emit("show-overlay", "recording");
    }
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    // Check if show_overlay is enabled in settings
    let settings = settings::get_settings(app_handle);
    if !settings.show_overlay {
        return;
    }

    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.show();
        // Emit event to switch to transcribing state
        let _ = overlay_window.emit("show-overlay", "transcribing");
    }
}

/// Hides the recording overlay window with fade-out animation
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    // Check if show_overlay is enabled in settings - if disabled, the overlay shouldn't be shown anyway
    // but we still want to hide it in case the setting was changed while recording
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Emit event to trigger fade-out animation
        let _ = overlay_window.emit("hide-overlay", ());
        // Hide the window after a short delay to allow animation to complete
        let window_clone = overlay_window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = window_clone.hide();
        });
    }
}

pub fn emit_levels(app_handle: &AppHandle, levels: &Vec<f32>) {
    // emit levels to main app
    let _ = app_handle.emit("mic-level", levels);

    // also emit to the recording overlay if it's open
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("mic-level", levels);
    }
}
