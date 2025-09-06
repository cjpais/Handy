mod actions;
mod audio_feedback;
pub mod audio_toolkit;
mod clipboard;
mod commands;
mod managers;
mod overlay;
mod settings;
mod shortcut;
mod tray;
mod utils;

use managers::audio::AudioRecordingManager;
use managers::history::HistoryManager;
use managers::model::ModelManager;
use managers::transcription::TranscriptionManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::image::Image;

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

fn show_main_window(app: &AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        // First, ensure the window is visible
        if let Err(e) = main_window.show() {
            eprintln!("Failed to show window: {}", e);
        }
        // Then, bring it to the front and give it focus
        if let Err(e) = main_window.set_focus() {
            eprintln!("Failed to focus window: {}", e);
        }
        // Optional: On macOS, ensure the app becomes active if it was an accessory
        #[cfg(target_os = "macos")]
        {
            if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
                eprintln!("Failed to set activation policy to Regular: {}", e);
            }
        }
    } else {
        eprintln!("Main window not found");
    }
}

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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations(
                    "sqlite:history.db",
                    managers::history::HistoryManager::get_migrations(),
                )
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(Mutex::new(ShortcutToggleStates::default()))
        .setup(move |app| {
            // Apply macOS Accessory policy early if starting hidden
            #[cfg(target_os = "macos")]
            {
                let settings = settings::get_settings(&app.handle());
                if settings.start_hidden {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            // Get the current theme to set the appropriate initial icon
            let initial_theme = tray::get_current_theme(&app.handle());

            // Choose the appropriate initial icon based on theme
            let initial_icon_path = tray::get_icon_path(initial_theme, tray::TrayIconState::Idle);

            let tray = TrayIconBuilder::new()
                .icon(Image::from_path(app.path().resolve(
                    initial_icon_path,
                    tauri::path::BaseDirectory::Resource,
                )?)?)
                .show_menu_on_left_click(true)
                .icon_as_template(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        show_main_window(app);
                    }
                    "check_updates" => {
                        show_main_window(app);
                        let _ = app.emit("check-for-updates", ());
                    }
                    "cancel" => {
                        use crate::utils::cancel_current_operation;

                        // Use centralized cancellation that handles all operations
                        cancel_current_operation(app);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            app.manage(tray);

            // Initialize tray menu with idle state
            utils::update_tray_menu(&app.handle(), &utils::TrayIconState::Idle);

            // Get the autostart manager
            let autostart_manager = app.autolaunch();
            // Enable autostart
            let _ = autostart_manager.enable();

            // Window is configured to start hidden to avoid flicker.
            // If user didn't choose Start Hidden, show it now.
            let settings = settings::get_settings(&app.handle());
            if settings.start_hidden {
                if let Some(main_window) = app.get_webview_window("main") {
                    let _ = main_window.hide();
                }
            } else {
                show_main_window(&app.handle());
            }

            let recording_manager = Arc::new(
                AudioRecordingManager::new(app).expect("Failed to initialize recording manager"),
            );
            let model_manager =
                Arc::new(ModelManager::new(&app).expect("Failed to initialize model manager"));
            let transcription_manager = Arc::new(
                TranscriptionManager::new(&app, model_manager.clone())
                    .expect("Failed to initialize transcription manager"),
            );
            let history_manager =
                Arc::new(HistoryManager::new(&app).expect("Failed to initialize history manager"));

            // Add managers to Tauri's managed state
            app.manage(recording_manager.clone());
            app.manage(model_manager.clone());
            app.manage(transcription_manager.clone());
            app.manage(history_manager.clone());

            // Create the recording overlay window (hidden by default)
            utils::create_recording_overlay(&app.handle());

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
            tauri::WindowEvent::ThemeChanged(theme) => {
                println!("Theme changed to: {:?}", theme);
                // Update tray icon to match new theme, maintaining idle state
                utils::change_tray_icon(&window.app_handle(), utils::TrayIconState::Idle);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            shortcut::change_binding,
            shortcut::reset_binding,
            shortcut::change_ptt_setting,
            shortcut::change_audio_feedback_setting,
            shortcut::change_start_hidden_setting,
            shortcut::change_translate_to_english_setting,
            shortcut::change_selected_language_setting,
            shortcut::change_overlay_position_setting,
            shortcut::change_debug_mode_setting,
            shortcut::change_word_correction_threshold_setting,
            shortcut::update_custom_words,
            shortcut::suspend_binding,
            shortcut::resume_binding,
            trigger_update_check,
            commands::cancel_operation,
            commands::get_app_dir_path,
            commands::models::get_available_models,
            commands::models::get_model_info,
            commands::models::download_model,
            commands::models::delete_model,
            commands::models::cancel_download,
            commands::models::set_active_model,
            commands::models::get_current_model,
            commands::models::get_transcription_model_status,
            commands::models::is_model_loading,
            commands::models::has_any_models_available,
            commands::models::has_any_models_or_downloads,
            commands::models::get_recommended_first_model,
            commands::audio::update_microphone_mode,
            commands::audio::get_microphone_mode,
            commands::audio::get_available_microphones,
            commands::audio::set_selected_microphone,
            commands::audio::get_selected_microphone,
            commands::audio::get_available_output_devices,
            commands::audio::set_selected_output_device,
            commands::audio::get_selected_output_device,
            commands::transcription::set_model_unload_timeout,
            commands::transcription::get_model_load_status,
            commands::transcription::unload_model_manually,
            commands::history::get_history_entries,
            commands::history::toggle_history_entry_saved,
            commands::history::get_audio_file_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
