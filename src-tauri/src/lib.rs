mod actions;
mod commands;
mod managers;
mod settings;
mod shortcut;
mod utils;

use managers::audio::AudioRecordingManager;
use managers::model::ModelManager;
use managers::transcription::TranscriptionManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Emitter;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[derive(Default)]
struct ShortcutToggleStates {
    // Map: shortcut_binding_id -> is_active
    active_toggles: HashMap<String, bool>,
}

type ManagedToggleState = Mutex<ShortcutToggleStates>;

#[tauri::command]
fn trigger_update_check(app: AppHandle) -> Result<(), String> {
    app.emit("check-for-updates", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(Mutex::new(ShortcutToggleStates::default()))
        .setup(move |app| {
            let version = env!("CARGO_PKG_VERSION");
            let version_label = format!("Handy v{}", version);
            let version_i = MenuItem::with_id(app, "version", &version_label, false, None::<&str>)?;

            // Platform-specific accelerators
            #[cfg(target_os = "macos")]
            let settings_accelerator = Some("Cmd+,");
            #[cfg(not(target_os = "macos"))]
            let settings_accelerator = Some("Ctrl+,");

            #[cfg(target_os = "macos")]
            let quit_accelerator = Some("Cmd+Q");
            #[cfg(not(target_os = "macos"))]
            let quit_accelerator = Some("Ctrl+Q");

            let settings_i =
                MenuItem::with_id(app, "settings", "Settings...", true, settings_accelerator)?;
            let check_updates_i = MenuItem::with_id(
                app,
                "check_updates",
                "Check for Updates...",
                true,
                None::<&str>,
            )?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, quit_accelerator)?;
            let menu = Menu::with_items(
                app,
                &[
                    &version_i,
                    &PredefinedMenuItem::separator(app)?,
                    &settings_i,
                    &check_updates_i,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_i,
                ],
            )?;
            let tray = TrayIconBuilder::new()
                .icon(Image::from_path(app.path().resolve(
                    "resources/tray_idle.png",
                    tauri::path::BaseDirectory::Resource,
                )?)?)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(settings_window) = app.get_webview_window("main") {
                            // First, ensure the window is visible
                            if let Err(e) = settings_window.show() {
                                eprintln!("Failed to show window: {}", e);
                            }
                            // Then, bring it to the front and give it focus
                            if let Err(e) = settings_window.set_focus() {
                                eprintln!("Failed to focus window: {}", e);
                            }
                            // Optional: On macOS, ensure the app becomes active if it was an accessory
                            #[cfg(target_os = "macos")]
                            {
                                if let Err(e) =
                                    app.set_activation_policy(tauri::ActivationPolicy::Regular)
                                {
                                    eprintln!("Failed to set activation policy to Regular: {}", e);
                                }
                            }
                        } else {
                            eprintln!("Main window not found");
                        }
                    }
                    "check_updates" => {
                        let _ = app.emit("check-for-updates", ());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            app.manage(tray);

            // Get the autostart manager
            let autostart_manager = app.autolaunch();
            // Enable autostart
            let _ = autostart_manager.enable();

            let recording_manager = Arc::new(
                AudioRecordingManager::new(app).expect("Failed to initialize recording manager"),
            );
            let model_manager =
                Arc::new(ModelManager::new(&app).expect("Failed to initialize model manager"));
            let transcription_manager = Arc::new(
                TranscriptionManager::new(&app, model_manager.clone())
                    .expect("Failed to initialize transcription manager"),
            );

            // Add managers to Tauri's managed state
            app.manage(recording_manager.clone());
            app.manage(model_manager.clone());
            app.manage(transcription_manager.clone());

            shortcut::init_shortcuts(app);

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _res = window.hide();
                #[cfg(target_os = "macos")]
                {
                    let res = window
                        .app_handle()
                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                    if let Err(e) = res {
                        println!("Failed to set activation policy: {}", e);
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            shortcut::change_binding,
            shortcut::reset_binding,
            shortcut::change_ptt_setting,
            shortcut::change_audio_feedback_setting,
            shortcut::change_translate_to_english_setting,
            trigger_update_check,
            commands::models::get_available_models,
            commands::models::get_model_info,
            commands::models::download_model,
            commands::models::delete_model,
            commands::models::set_active_model,
            commands::models::get_current_model,
            commands::models::get_transcription_model_status,
            commands::models::is_model_loading,
            commands::models::has_any_models_available,
            commands::models::has_any_models_or_downloads,
            commands::models::get_recommended_first_model,
            commands::audio::update_microphone_mode,
            commands::audio::get_microphone_mode
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
