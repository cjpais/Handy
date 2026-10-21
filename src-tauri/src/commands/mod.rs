pub mod audio;
pub mod history;
pub mod models;
pub mod transcription;

use crate::settings::{
    get_settings, update_checks_forced_disabled, write_settings, AppSettings, LogLevel,
};
use crate::utils::cancel_current_operation;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
#[specta::specta]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}

#[tauri::command]
#[specta::specta]
pub fn is_portable() -> bool {
    crate::portable::is_portable()
}

#[tauri::command]
#[specta::specta]
pub fn is_update_checks_locked() -> bool {
    update_checks_forced_disabled()
}

#[tauri::command]
#[specta::specta]
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_app_settings(app: AppHandle) -> Result<AppSettings, String> {
    Ok(get_settings(&app))
}

#[tauri::command]
#[specta::specta]
pub fn get_default_settings() -> Result<AppSettings, String> {
    Ok(crate::settings::get_default_settings())
}

#[tauri::command]
#[specta::specta]
pub fn get_log_dir_path(app: AppHandle) -> Result<String, String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    Ok(log_dir.to_string_lossy().to_string())
}

#[specta::specta]
#[tauri::command]
pub fn set_log_level(app: AppHandle, level: LogLevel) -> Result<(), String> {
    let tauri_log_level: tauri_plugin_log::LogLevel = level.into();
    let log_level: log::Level = tauri_log_level.into();
    // Update the file log level atomic so the filter picks up the new level
    crate::FILE_LOG_LEVEL.store(
        log_level.to_level_filter() as u8,
        std::sync::atomic::Ordering::Relaxed,
    );

    let mut settings = get_settings(&app);
    settings.log_level = level;
    write_settings(&app, settings);

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_recordings_folder(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let recordings_dir = app_data_dir.join("recordings");

    let path = recordings_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open recordings folder: {}", e))?;

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    let path = log_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open log directory: {}", e))?;

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_app_data_dir(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let path = app_data_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open app data directory: {}", e))?;

    Ok(())
}

/// Check if Apple Intelligence is available on this device.
/// Called by the frontend when the user selects Apple Intelligence provider.
#[specta::specta]
#[tauri::command]
pub fn check_apple_intelligence_available() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        crate::apple_intelligence::check_apple_intelligence_availability()
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

/// Try to initialize Enigo (keyboard/mouse simulation).
/// On macOS, this will return an error if accessibility permissions are not granted.
#[specta::specta]
#[tauri::command]
pub fn initialize_enigo(app: AppHandle) -> Result<(), String> {
    use crate::input::EnigoState;

    // Check if already initialized
    if app.try_state::<EnigoState>().is_some() {
        log::debug!("Enigo already initialized");
        return Ok(());
    }

    // Try to initialize
    match EnigoState::new() {
        Ok(enigo_state) => {
            app.manage(enigo_state);
            log::info!("Enigo initialized successfully after permission grant");
            Ok(())
        }
        Err(e) => {
            if cfg!(target_os = "macos") {
                log::warn!(
                    "Failed to initialize Enigo: {} (accessibility permissions may not be granted)",
                    e
                );
            } else {
                log::warn!("Failed to initialize Enigo: {}", e);
            }
            Err(format!("Failed to initialize input system: {}", e))
        }
    }
}

/// Marker state to track if shortcuts have been initialized.
pub struct ShortcutsInitialized;

/// Initialize keyboard shortcuts.
/// On macOS, this should be called after accessibility permissions are granted.
/// This is idempotent - calling it multiple times is safe.
#[specta::specta]
#[tauri::command]
pub fn initialize_shortcuts(app: AppHandle) -> Result<(), String> {
    // Check if already initialized
    if app.try_state::<ShortcutsInitialized>().is_some() {
        log::debug!("Shortcuts already initialized");
        return Ok(());
    }

    // Initialize shortcuts
    crate::shortcut::init_shortcuts(&app);

    // Mark as initialized before reconciling the macOS Secure Input fallback.
    app.manage(ShortcutsInitialized);
    crate::secure_input::reconcile_fallback(&app);

    log::info!("Shortcuts initialized successfully");
    Ok(())
}

/// Mark onboarding as finished. Onboarding calls this when the person closes
/// its last screen; activating a model no longer counts as finishing, so
/// someone who quits midway comes back to the rest of the flow.
#[specta::specta]
#[tauri::command]
pub fn complete_onboarding(app: AppHandle) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.onboarding_completed = true;
    crate::settings::write_settings(&app, settings);
    close_practice_note(app)
}

/// When Handy's window closes it only hides, so the screen showing doesn't
/// unmount: undo what onboarding's screens hold while they're up (the
/// shortcuts Demo turns off, the practice note).
pub fn on_main_window_hidden(app: &AppHandle) {
    if app.try_state::<ShortcutsInitialized>().is_some() {
        crate::shortcut::resume_all_shortcuts(app);
    }
    if let Err(e) = close_practice_note(app.clone()) {
        log::warn!("Failed to close the practice note: {e}");
    }
}

/// Close the main window as if the person closed it: it hides, and Handy keeps
/// running in the tray (onboarding's last screen ends this way).
#[specta::specta]
#[tauri::command]
pub fn close_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The window label of onboarding's practice note.
pub const PRACTICE_NOTE_LABEL: &str = "practice_note";

/// Whether the practice note should be open: a close that lands while an open
/// is still building the window wins once the build finishes.
static PRACTICE_NOTE_WANTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Open onboarding's practice note: a small blank note beside Handy's window
/// to dictate into on the Try it screen, placed where the design puts it
/// (over the right of Handy's window, a little lower), kept on screen.
///
/// Async because building a window from a synchronous command deadlocks on
/// Windows (WebView2).
#[specta::specta]
#[tauri::command]
pub async fn open_practice_note(app: AppHandle, title: String) -> Result<(), String> {
    // Async commands can overlap (Try it re-runs its effect): one at a time,
    // so two opens can't both build the window.
    static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _opening = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    PRACTICE_NOTE_WANTED.store(true, std::sync::atomic::Ordering::SeqCst);

    const WIDTH: f64 = 430.0;
    // The note's page; on macOS its title bar is inside the window's height.
    let height = 272.0 + crate::utils::title_bar_height();
    // From the top left of Handy's content to the note's top left.
    const OFFSET: (f64, f64) = (450.0, 74.0);

    if let Some(note) = app.get_webview_window(PRACTICE_NOTE_LABEL) {
        note.set_title(&title).map_err(|e| e.to_string())?;
        note.show().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        PRACTICE_NOTE_LABEL,
        tauri::WebviewUrl::App("src/note/index.html".into()),
    )
    .title(title)
    .inner_size(WIDTH, height)
    .resizable(true)
    .maximizable(false)
    .minimizable(false)
    .focused(false);

    if let Some(main) = app.get_webview_window("main") {
        let scale = main.scale_factor().map_err(|e| e.to_string())?;
        let origin = main
            .inner_position()
            .map_err(|e| e.to_string())?
            .to_logical::<f64>(scale);
        let page_top = origin.y + crate::utils::title_bar_height();
        let (mut x, mut y) = (origin.x + OFFSET.0, page_top + OFFSET.1);
        if let Ok(Some(monitor)) = main.current_monitor() {
            let area = monitor.work_area();
            let left = area.position.x as f64 / scale;
            let top = area.position.y as f64 / scale;
            let right = left + area.size.width as f64 / scale;
            let bottom = top + area.size.height as f64 / scale;
            x = x.min(right - WIDTH - 16.0).max(left);
            y = y.min(bottom - height - 48.0).max(top);
        }
        builder = builder.position(x, y);
    }

    let note = builder.build().map_err(|e| e.to_string())?;
    if !PRACTICE_NOTE_WANTED.load(std::sync::atomic::Ordering::SeqCst) {
        note.destroy().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sent when the practice note has gone.
pub const PRACTICE_NOTE_CLOSED_EVENT: &str = "practice-note-closed";

/// Close onboarding's practice note, if it's open.
#[specta::specta]
#[tauri::command]
pub fn close_practice_note(app: AppHandle) -> Result<(), String> {
    PRACTICE_NOTE_WANTED.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(note) = app.get_webview_window(PRACTICE_NOTE_LABEL) {
        note.destroy().map_err(|e| e.to_string())?;
    }
    Ok(())
}
