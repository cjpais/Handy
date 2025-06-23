use enigo::Enigo;
use enigo::Keyboard;
use enigo::Settings;
use tauri::image::Image;
use tauri::tray::TrayIcon;
use tauri::AppHandle;
use tauri::Manager;

pub fn paste(text: String, _: AppHandle) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;

    enigo
        .text(&text)
        .map_err(|e| format!("Failed to paste text: {}", e))?;

    Ok(())
}

pub enum TrayIconState {
    Idle,
    Recording,
}

pub fn change_tray_icon(app: &AppHandle, icon: TrayIconState) {
    let tray = app.state::<TrayIcon>();

    let icon_path = match icon {
        TrayIconState::Idle => "resources/tray_idle.png",
        TrayIconState::Recording => "resources/tray_recording.png",
    };

    let _ = tray.set_icon(Some(
        Image::from_path(
            app.path()
                .resolve(icon_path, tauri::path::BaseDirectory::Resource)
                .expect("failed to resolve"),
        )
        .expect("failed to set icon"),
    ));
}
