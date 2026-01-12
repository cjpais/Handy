mod actions;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod apple_intelligence;
mod audio_feedback;
pub mod audio_toolkit;
mod auth_server;
mod clipboard;
mod commands;
mod devops;
mod discord;
pub mod discord_conversation;
pub mod filler_detector;
mod helpers;
mod input;
pub mod live_coaching;
mod llm_client;
mod local_llm;
mod local_tts;
mod managers;
mod memory;
pub mod onichan;
pub mod onichan_conversation;
pub mod onichan_models;
mod overlay;
mod settings;
mod shortcut;
mod signal_handle;
mod tray;
mod tray_i18n;
mod utils;
mod vad_model;
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri_specta::{collect_commands, Builder};

use discord::DiscordManager;
use discord_conversation::DiscordConversationManager;
use env_filter::Builder as EnvFilterBuilder;
use live_coaching::LiveCoachingManager;
use local_llm::LocalLlmManager;
use local_tts::LocalTtsManager;
use managers::audio::AudioRecordingManager;
use managers::history::HistoryManager;
use managers::model::ModelManager;
use managers::transcription::TranscriptionManager;
use memory::MemoryManager;
use onichan::OnichanManager;
use onichan_models::OnichanModelManager;
#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tauri::image::Image;

use tauri::tray::TrayIconBuilder;
use tauri::Emitter;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_log::{Builder as LogBuilder, RotationStrategy, Target, TargetKind};

use crate::settings::get_settings;

// Global atomic to store the file log level filter
// We use u8 to store the log::LevelFilter as a number
pub static FILE_LOG_LEVEL: AtomicU8 = AtomicU8::new(log::LevelFilter::Debug as u8);

fn level_filter_from_u8(value: u8) -> log::LevelFilter {
    match value {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Trace,
    }
}

fn build_console_filter() -> env_filter::Filter {
    let mut builder = EnvFilterBuilder::new();

    match std::env::var("RUST_LOG") {
        Ok(spec) if !spec.trim().is_empty() => {
            if let Err(err) = builder.try_parse(&spec) {
                log::warn!(
                    "Ignoring invalid RUST_LOG value '{}': {}. Falling back to info-level console logging",
                    spec,
                    err
                );
                builder.filter_level(log::LevelFilter::Info);
            }
        }
        _ => {
            builder.filter_level(log::LevelFilter::Info);
        }
    }

    builder.build()
}

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
            log::error!("Failed to show window: {}", e);
        }
        // Then, bring it to the front and give it focus
        if let Err(e) = main_window.set_focus() {
            log::error!("Failed to focus window: {}", e);
        }
        // Optional: On macOS, ensure the app becomes active if it was an accessory
        #[cfg(target_os = "macos")]
        {
            if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
                log::error!("Failed to set activation policy to Regular: {}", e);
            }
        }
    } else {
        log::error!("Main window not found.");
    }
}

fn initialize_core_logic(app_handle: &AppHandle) {
    // Initialize the input state (Enigo singleton for keyboard/mouse simulation)
    let enigo_state = input::EnigoState::new().expect("Failed to initialize input state (Enigo)");
    app_handle.manage(enigo_state);

    // Initialize the managers
    let recording_manager = Arc::new(
        AudioRecordingManager::new(app_handle).expect("Failed to initialize recording manager"),
    );
    let model_manager =
        Arc::new(ModelManager::new(app_handle).expect("Failed to initialize model manager"));
    let transcription_manager = Arc::new(
        TranscriptionManager::new(app_handle, model_manager.clone())
            .expect("Failed to initialize transcription manager"),
    );
    let history_manager =
        Arc::new(HistoryManager::new(app_handle).expect("Failed to initialize history manager"));

    // Initialize live coaching manager
    let live_coaching_manager = Arc::new(LiveCoachingManager::new(
        app_handle,
        recording_manager.clone(),
        transcription_manager.clone(),
    ));

    // Initialize Onichan manager
    let onichan_manager = Arc::new(OnichanManager::new(app_handle));

    // Initialize Onichan model manager
    let onichan_model_manager = Arc::new(
        OnichanModelManager::new(app_handle).expect("Failed to initialize onichan model manager"),
    );

    // Initialize local LLM and TTS managers
    // The sidecars are bundled with the app to avoid library version conflicts
    // Get the platform-specific sidecar paths
    let target_triple = if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown"
    };

    // In dev mode, sidecars are in src-tauri/<sidecar>/<name>-<target>
    // In prod, Tauri bundles them in the resources directory
    let llm_sidecar_path = if cfg!(debug_assertions) {
        // In dev, look for the sidecar in the llm-sidecar directory
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join(format!("llm-sidecar/llm-sidecar-{}", target_triple))
    } else {
        app_handle
            .path()
            .resource_dir()
            .expect("Failed to get resource dir")
            .join(format!("llm-sidecar-{}", target_triple))
    };
    log::info!("LLM sidecar path: {:?}", llm_sidecar_path);
    let local_llm_manager = Arc::new(LocalLlmManager::new(llm_sidecar_path));

    // TTS sidecar path - similar pattern
    let tts_sidecar_path = if cfg!(debug_assertions) {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join(format!("tts-sidecar/tts-sidecar-{}", target_triple))
    } else {
        app_handle
            .path()
            .resource_dir()
            .expect("Failed to get resource dir")
            .join(format!("tts-sidecar-{}", target_triple))
    };
    log::info!("TTS sidecar path: {:?}", tts_sidecar_path);
    let local_tts_manager = Arc::new(LocalTtsManager::new(tts_sidecar_path));

    // Discord sidecar path
    let discord_sidecar_path = if cfg!(debug_assertions) {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join(format!("discord-sidecar/discord-sidecar-{}", target_triple))
    } else {
        app_handle
            .path()
            .resource_dir()
            .expect("Failed to get resource dir")
            .join(format!("discord-sidecar-{}", target_triple))
    };
    log::info!("Discord sidecar path: {:?}", discord_sidecar_path);
    let discord_manager = Arc::new(DiscordManager::new(discord_sidecar_path));

    // Memory sidecar path for long-term conversation memory
    let memory_sidecar_path = if cfg!(debug_assertions) {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join(format!("memory-sidecar/memory-sidecar-{}", target_triple))
    } else {
        app_handle
            .path()
            .resource_dir()
            .expect("Failed to get resource dir")
            .join(format!("memory-sidecar-{}", target_triple))
    };
    log::info!("Memory sidecar path: {:?}", memory_sidecar_path);
    let memory_manager = Arc::new(MemoryManager::new(memory_sidecar_path));

    // Wire up the LLM, TTS, and Memory managers to the Onichan manager for local processing
    onichan_manager.set_llm_manager(local_llm_manager.clone());
    onichan_manager.set_tts_manager(local_tts_manager.clone());
    onichan_manager.set_memory_manager(memory_manager.clone());

    // Initialize Onichan conversation manager for continuous listening mode
    let onichan_conversation_manager =
        Arc::new(onichan_conversation::OnichanConversationManager::new(
            app_handle,
            transcription_manager.clone(),
            onichan_manager.clone(),
        ));

    // Initialize Discord conversation manager for Discord voice integration
    let discord_conversation_manager = Arc::new(DiscordConversationManager::new(
        app_handle,
        transcription_manager.clone(),
        onichan_manager.clone(),
        discord_manager.clone(),
    ));
    // Wire up memory manager for storing Discord transcripts with user IDs
    discord_conversation_manager.set_memory_manager(memory_manager.clone());

    // Initialize auth manager for OAuth
    let auth_manager = Arc::new(commands::auth::AuthManager::new());

    // Add managers to Tauri's managed state
    app_handle.manage(recording_manager.clone());
    app_handle.manage(model_manager.clone());
    app_handle.manage(transcription_manager.clone());
    app_handle.manage(history_manager.clone());
    app_handle.manage(live_coaching_manager.clone());
    app_handle.manage(onichan_manager.clone());
    app_handle.manage(onichan_conversation_manager.clone());
    app_handle.manage(onichan_model_manager.clone());
    app_handle.manage(local_llm_manager.clone());
    app_handle.manage(local_tts_manager.clone());
    app_handle.manage(discord_manager.clone());
    app_handle.manage(discord_conversation_manager.clone());
    app_handle.manage(memory_manager.clone());
    app_handle.manage(auth_manager.clone());

    // Initialize the shortcuts
    shortcut::init_shortcuts(app_handle);

    #[cfg(unix)]
    let signals = Signals::new(&[SIGUSR2]).unwrap();
    // Set up SIGUSR2 signal handler for toggling transcription
    #[cfg(unix)]
    signal_handle::setup_signal_handler(app_handle.clone(), signals);

    // Apply macOS Accessory policy if starting hidden
    #[cfg(target_os = "macos")]
    {
        let settings = settings::get_settings(app_handle);
        if settings.start_hidden {
            let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    }
    // Get the current theme to set the appropriate initial icon
    let initial_theme = tray::get_current_theme(app_handle);

    // Choose the appropriate initial icon based on theme
    let initial_icon_path = tray::get_icon_path(initial_theme, tray::TrayIconState::Idle);

    let tray = TrayIconBuilder::new()
        .icon(
            Image::from_path(
                app_handle
                    .path()
                    .resolve(initial_icon_path, tauri::path::BaseDirectory::Resource)
                    .unwrap(),
            )
            .unwrap(),
        )
        .show_menu_on_left_click(true)
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                show_main_window(app);
            }
            "check_updates" => {
                let settings = settings::get_settings(app);
                if settings.update_checks_enabled {
                    show_main_window(app);
                    let _ = app.emit("check-for-updates", ());
                }
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
        .build(app_handle)
        .unwrap();
    app_handle.manage(tray);

    // Initialize tray menu with idle state
    utils::update_tray_menu(app_handle, &utils::TrayIconState::Idle, None);

    // Get the autostart manager and configure based on user setting
    let autostart_manager = app_handle.autolaunch();
    let settings = settings::get_settings(&app_handle);

    if settings.autostart_enabled {
        // Enable autostart if user has opted in
        let _ = autostart_manager.enable();
    } else {
        // Disable autostart if user has opted out
        let _ = autostart_manager.disable();
    }

    // Create the recording overlay window (hidden by default)
    utils::create_recording_overlay(app_handle);
}

#[tauri::command]
#[specta::specta]
fn trigger_update_check(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    if !settings.update_checks_enabled {
        return Ok(());
    }
    app.emit("check-for-updates", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Parse console logging directives from RUST_LOG, falling back to info-level logging
    // when the variable is unset
    let console_filter = build_console_filter();

    let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        shortcut::change_binding,
        shortcut::reset_binding,
        shortcut::change_ptt_setting,
        shortcut::change_audio_feedback_setting,
        shortcut::change_audio_feedback_volume_setting,
        shortcut::change_sound_theme_setting,
        shortcut::change_start_hidden_setting,
        shortcut::change_autostart_setting,
        shortcut::change_translate_to_english_setting,
        shortcut::change_selected_language_setting,
        shortcut::change_overlay_position_setting,
        shortcut::change_debug_mode_setting,
        shortcut::change_word_correction_threshold_setting,
        shortcut::change_paste_method_setting,
        shortcut::change_clipboard_handling_setting,
        shortcut::change_post_process_enabled_setting,
        shortcut::change_post_process_base_url_setting,
        shortcut::change_post_process_api_key_setting,
        shortcut::change_post_process_model_setting,
        shortcut::set_post_process_provider,
        shortcut::fetch_post_process_models,
        shortcut::add_post_process_prompt,
        shortcut::update_post_process_prompt,
        shortcut::delete_post_process_prompt,
        shortcut::set_post_process_selected_prompt,
        shortcut::update_custom_words,
        shortcut::suspend_binding,
        shortcut::resume_binding,
        shortcut::change_mute_while_recording_setting,
        shortcut::change_append_trailing_space_setting,
        shortcut::change_app_language_setting,
        shortcut::change_update_checks_setting,
        shortcut::change_filler_detection_setting,
        shortcut::change_filler_output_mode_setting,
        shortcut::update_custom_filler_words,
        shortcut::change_show_filler_overlay_setting,
        shortcut::set_active_ui_section,
        shortcut::change_onichan_silence_threshold_setting,
        trigger_update_check,
        commands::cancel_operation,
        commands::get_app_dir_path,
        commands::get_app_settings,
        commands::get_default_settings,
        commands::get_log_dir_path,
        commands::set_log_level,
        commands::open_recordings_folder,
        commands::open_log_dir,
        commands::open_app_data_dir,
        commands::check_apple_intelligence_available,
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
        commands::audio::play_test_sound,
        commands::audio::check_custom_sounds,
        commands::audio::set_clamshell_microphone,
        commands::audio::get_clamshell_microphone,
        commands::audio::is_recording,
        commands::transcription::set_model_unload_timeout,
        commands::transcription::get_model_load_status,
        commands::transcription::unload_model_manually,
        commands::history::get_history_entries,
        commands::history::toggle_history_entry_saved,
        commands::history::get_audio_file_path,
        commands::history::delete_history_entry,
        commands::history::update_history_limit,
        commands::history::update_recording_retention_period,
        commands::reset_app_data,
        commands::onichan::onichan_enable,
        commands::onichan::onichan_disable,
        commands::onichan::onichan_is_active,
        commands::onichan::onichan_get_mode,
        commands::onichan::onichan_set_mode,
        commands::onichan::onichan_process_input,
        commands::onichan::onichan_speak,
        commands::onichan::onichan_clear_history,
        commands::onichan::onichan_get_history,
        commands::onichan::get_onichan_models,
        commands::onichan::get_onichan_llm_models,
        commands::onichan::get_onichan_tts_models,
        commands::onichan::download_onichan_model,
        commands::onichan::delete_onichan_model,
        commands::onichan::load_local_llm,
        commands::onichan::unload_local_llm,
        commands::onichan::is_local_llm_loaded,
        commands::onichan::get_local_llm_model_name,
        commands::onichan::local_llm_chat,
        commands::onichan::load_local_tts,
        commands::onichan::unload_local_tts,
        commands::onichan::is_local_tts_loaded,
        commands::onichan::local_tts_speak,
        commands::onichan::onichan_start_conversation,
        commands::onichan::onichan_stop_conversation,
        commands::onichan::onichan_is_conversation_running,
        commands::discord::discord_has_token,
        commands::discord::discord_get_token,
        commands::discord::discord_set_token,
        commands::discord::discord_clear_token,
        commands::discord::discord_connect_with_stored_token,
        commands::discord::discord_get_status,
        commands::discord::discord_get_guilds,
        commands::discord::discord_get_channels,
        commands::discord::discord_connect,
        commands::discord::discord_disconnect,
        commands::discord::discord_speak,
        commands::discord::discord_start_conversation,
        commands::discord::discord_stop_conversation,
        commands::discord::discord_is_conversation_running,
        commands::supabase::get_supabase_url,
        commands::supabase::set_supabase_url,
        commands::supabase::get_supabase_anon_key,
        commands::supabase::get_supabase_anon_key_raw,
        commands::supabase::has_supabase_anon_key,
        commands::supabase::set_supabase_anon_key,
        commands::supabase::clear_supabase_credentials,
        commands::auth::auth_start_server,
        commands::auth::auth_stop_server,
        commands::auth::auth_save_session,
        commands::auth::auth_get_session,
        commands::auth::auth_get_user,
        commands::auth::auth_logout,
        commands::auth::auth_is_authenticated,
        commands::auth::auth_get_access_token,
        commands::memory::get_memory_status,
        commands::memory::query_all_memories,
        commands::memory::get_memory_count,
        commands::memory::clear_all_memories,
        commands::memory::cleanup_old_memories,
        commands::memory::list_embedding_models,
        commands::memory::load_embedding_model,
        commands::memory::get_current_embedding_model,
        commands::memory::stop_memory_sidecar,
        commands::memory::browse_recent_memories,
        commands::memory::list_memory_users,
        commands::devops::check_devops_dependencies,
        commands::devops::list_tmux_sessions,
        commands::devops::get_tmux_session_metadata,
        commands::devops::create_tmux_session,
        commands::devops::kill_tmux_session,
        commands::devops::get_tmux_session_output,
        commands::devops::send_tmux_command,
        commands::devops::recover_tmux_sessions,
        commands::devops::is_tmux_running,
        commands::devops::list_git_worktrees,
        commands::devops::get_git_worktree_info,
        commands::devops::check_worktree_collision,
        commands::devops::create_git_worktree,
        commands::devops::create_git_worktree_existing_branch,
        commands::devops::remove_git_worktree,
        commands::devops::prune_git_worktrees,
        commands::devops::get_git_repo_root,
        commands::devops::get_git_default_branch,
        commands::devops::check_gh_auth,
        commands::devops::list_github_issues,
        commands::devops::get_github_issue,
        commands::devops::get_github_issue_with_agent,
        commands::devops::create_github_issue,
        commands::devops::comment_on_github_issue,
        commands::devops::assign_agent_to_issue,
        commands::devops::list_github_issue_comments,
        commands::devops::update_github_issue_labels,
        commands::devops::close_github_issue,
        commands::devops::reopen_github_issue,
        commands::devops::list_github_prs,
        commands::devops::get_github_pr,
        commands::devops::get_github_pr_status,
        commands::devops::create_github_pr,
        commands::devops::merge_github_pr,
        commands::devops::close_github_pr,
        commands::devops::spawn_agent,
        commands::devops::list_agent_statuses,
        commands::devops::cleanup_agent,
        commands::devops::create_pr_from_agent,
        commands::devops::complete_agent_work,
        commands::devops::check_and_cleanup_merged_pr,
        commands::devops::get_current_machine_id,
        commands::devops::list_local_agent_statuses,
        commands::devops::list_remote_agent_statuses,
        helpers::clamshell::is_laptop,
        vad_model::is_vad_model_ready,
        vad_model::download_vad_model_if_needed,
    ]);

    #[cfg(debug_assertions)] // <- Only export on non-release builds
    specta_builder
        .export(
            Typescript::default().bigint(BigIntExportBehavior::Number),
            "../src/bindings.ts",
        )
        .expect("Failed to export typescript bindings");

    let mut builder = tauri::Builder::default().plugin(
        LogBuilder::new()
            .level(log::LevelFilter::Trace) // Set to most verbose level globally
            .max_file_size(500_000)
            .rotation_strategy(RotationStrategy::KeepOne)
            .clear_targets()
            .targets([
                // Console output respects RUST_LOG environment variable
                Target::new(TargetKind::Stdout).filter({
                    let console_filter = console_filter.clone();
                    move |metadata| console_filter.enabled(metadata)
                }),
                // File logs respect the user's settings (stored in FILE_LOG_LEVEL atomic)
                Target::new(TargetKind::LogDir {
                    file_name: Some("kbve".into()),
                })
                .filter(|metadata| {
                    let file_level = FILE_LOG_LEVEL.load(Ordering::Relaxed);
                    metadata.level() <= level_filter_from_u8(file_level)
                }),
            ])
            .build(),
    );

    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    builder
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
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(Mutex::new(ShortcutToggleStates::default()))
        .setup(move |app| {
            let settings = get_settings(&app.handle());
            let tauri_log_level: tauri_plugin_log::LogLevel = settings.log_level.into();
            let file_log_level: log::Level = tauri_log_level.into();
            // Store the file log level in the atomic for the filter to use
            FILE_LOG_LEVEL.store(file_log_level.to_level_filter() as u8, Ordering::Relaxed);
            let app_handle = app.handle().clone();

            initialize_core_logic(&app_handle);

            // Show main window only if not starting hidden
            if !settings.start_hidden {
                if let Some(main_window) = app_handle.get_webview_window("main") {
                    main_window.show().unwrap();
                    main_window.set_focus().unwrap();
                }
            }

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
                        log::error!("Failed to set activation policy: {}", e);
                    }
                }
            }
            tauri::WindowEvent::ThemeChanged(theme) => {
                log::info!("Theme changed to: {:?}", theme);
                // Update tray icon to match new theme, maintaining idle state
                utils::change_tray_icon(&window.app_handle(), utils::TrayIconState::Idle);
            }
            _ => {}
        })
        .invoke_handler(specta_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
