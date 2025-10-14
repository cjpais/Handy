use crate::audio_feedback::{play_recording_start_sound, play_recording_stop_sound};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::audio_backup::AudioBackupManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::overlay::{show_recording_overlay, show_transcribing_overlay, show_polishing_overlay};
use crate::settings::get_settings;
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils;
use log::{debug, error};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action
struct TranscribeAction;

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Clean up old backup files when starting a new recording
        if let Some(abm) = app.try_state::<Arc<AudioBackupManager>>() {
            if let Err(e) = abm.cleanup_old_backups() {
                error!("Failed to cleanup old backup files: {}", e);
            }
        }

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        tm.initiate_model_load();

        let binding_id = binding_id.to_string();
        change_tray_icon(app, TrayIconState::Recording);
        show_recording_overlay(app);

        let rm = app.state::<Arc<AudioRecordingManager>>();

        // Get the microphone mode to determine audio feedback timing
        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;
        debug!("Microphone mode - always_on: {}", is_always_on);

        if is_always_on {
            // Always-on mode: Play audio feedback immediately
            debug!("Always-on mode: Playing audio feedback immediately");
            play_recording_start_sound(app);
            let recording_started = rm.try_start_recording(&binding_id);
            debug!("Recording started: {}", recording_started);
        } else {
            // On-demand mode: Start recording first, then play audio feedback
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            if rm.try_start_recording(&binding_id) {
                debug!("Recording started in {:?}", recording_start_time.elapsed());
                // Small delay to ensure microphone stream is active
                let app_clone = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    debug!("Playing delayed audio feedback");
                    play_recording_start_sound(&app_clone);
                });
            } else {
                debug!("Failed to start recording");
            }
        }

        debug!(
            "TranscribeAction::start completed in {:?}",
            start_time.elapsed()
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());
        let abm = Arc::clone(&app.state::<Arc<AudioBackupManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);
        show_transcribing_overlay(app);

        // Play audio feedback for recording stop
        play_recording_stop_sound(app);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task

        tauri::async_runtime::spawn(async move {
            let binding_id = binding_id.clone(); // Clone for the inner async task
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                // Save backup audio before transcription
                if let Err(e) = abm.save_backup_audio(&samples).await {
                    error!("Failed to save backup audio: {}", e);
                }

                let transcription_time = Instant::now();
                let samples_clone = samples.clone(); // Clone for history saving
                match tm.transcribe(samples) {
                    Ok(transcription) => {
                        debug!(
                            "Transcription completed in {:?}: '{}'",
                            transcription_time.elapsed(),
                            transcription
                        );
                        if !transcription.is_empty() {
                            // Save to history
                            let hm_clone = Arc::clone(&hm);
                            let transcription_for_history = transcription.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = hm_clone
                                    .save_transcription(samples_clone, transcription_for_history)
                                    .await
                                {
                                    error!("Failed to save transcription to history: {}", e);
                                }
                            });
                            let transcription_clone = transcription.clone();
                            let ah_clone = ah.clone();
                            let paste_time = Instant::now();
                            ah.run_on_main_thread(move || {
                                // Check if auto polish is enabled and there are active polish rules
                                let settings = get_settings(&ah_clone);
                                let should_auto_polish = settings.auto_polish && 
                                    !settings.polish_rules.is_empty() && 
                                    settings.polish_rules.iter().any(|rule| rule.enabled);
                                
                                if should_auto_polish {
                                    // If auto polish is enabled, paste and select text for polishing
                                    match utils::paste_and_select(transcription_clone.clone(), ah_clone.clone()) {
                                        Ok(()) => {
                                            debug!(
                                                "Text pasted and selected successfully for polishing in {:?}",
                                                paste_time.elapsed()
                                            );
                                            
                                            // Show polishing overlay
                                            show_polishing_overlay(&ah_clone);
                                            
                                            let ah_for_polish = ah_clone.clone();
                                            let text_for_polish = transcription_clone.clone();
                                            tauri::async_runtime::spawn(async move {
                                                // Small delay to ensure text is selected
                                                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                                                
                                                // Apply polish rules
                                                let settings = get_settings(&ah_for_polish);
                                                match crate::audio_toolkit::text::apply_polish_rules_with_error(&text_for_polish, &settings.polish_rules).await {
                                                    Ok(polished_text) => {
                                                        // Paste the polished text back
                                                        if let Err(e) = utils::paste(polished_text, ah_for_polish.clone()) {
                                                            error!("Failed to paste polished text: {}", e);
                                                            // Show error notification
                                                            let _ = ah_for_polish.emit("polish-error", format!("Failed to paste polished text: {}", e));
                                                        }
                                                    },
                                                    Err(e) => {
                                                        error!("Failed to apply polish rules: {}", e);
                                                        // Show error notification
                                                        let _ = ah_for_polish.emit("polish-error", format!("Polish failed: {}", e));
                                                    }
                                                }
                                                
                                                // Hide polishing overlay
                                                utils::hide_recording_overlay(&ah_for_polish);
                                            });
                                        },
                                        Err(e) => {
                                            eprintln!("Failed to paste and select transcription for polishing: {}", e);
                                            // Show error notification
                                            let _ = ah_clone.emit("polish-error", format!("Failed to prepare text for polishing: {}", e));
                                        }
                                    }
                                } else {
                                    // If no auto polish, just paste without selecting
                                    match utils::paste(transcription_clone.clone(), ah_clone.clone()) {
                                        Ok(()) => {
                                            debug!(
                                                "Text pasted successfully (no selection) in {:?}",
                                                paste_time.elapsed()
                                            );
                                        },
                                        Err(e) => eprintln!("Failed to paste transcription: {}", e),
                                    }
                                }
                                // Hide the overlay after transcription is complete
                                utils::hide_recording_overlay(&ah_clone);
                                change_tray_icon(&ah_clone, TrayIconState::Idle);
                            })
                            .unwrap_or_else(|e| {
                                eprintln!("Failed to run paste on main thread: {:?}", e);
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                            });
                        } else {
                            utils::hide_recording_overlay(&ah);
                            change_tray_icon(&ah, TrayIconState::Idle);
                        }
                    }
                    Err(err) => {
                        debug!("Global Shortcut Transcription error: {}", err);
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                utils::hide_recording_overlay(&ah);
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!(
            "TranscribeAction::stop completed in {:?}",
            stop_time.elapsed()
        );
    }
}

// Polish Action
struct PolishAction;

impl ShortcutAction for PolishAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        debug!("PolishAction::start called for binding: {}", binding_id);
        
        // Show polishing overlay and change tray icon
        change_tray_icon(app, TrayIconState::Transcribing);
        show_polishing_overlay(app);
        
        // Get selected text from clipboard and apply polish
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            // Use Ctrl+C to copy selected text first
            if let Err(e) = utils::copy_selected_text(&app_clone) {
                error!("Failed to copy selected text: {}", e);
                let _ = app_clone.emit("polish-error", format!("Failed to copy selected text: {}", e));
                utils::hide_recording_overlay(&app_clone);
                change_tray_icon(&app_clone, TrayIconState::Idle);
                return;
            }
            
            // Small delay to ensure clipboard is updated
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            // Get clipboard content
            let clipboard_content = app_clone.clipboard().read_text().unwrap_or_default();
            
            if clipboard_content.is_empty() {
                error!("No text selected or clipboard is empty");
                let _ = app_clone.emit("polish-error", "No text selected or clipboard is empty".to_string());
                utils::hide_recording_overlay(&app_clone);
                change_tray_icon(&app_clone, TrayIconState::Idle);
                return;
            }
            
            // Apply polish rules
            let settings = get_settings(&app_clone);
            match crate::audio_toolkit::text::apply_polish_rules_with_error(&clipboard_content, &settings.polish_rules).await {
                Ok(polished_text) => {
                    // Paste the polished text back
                    if let Err(e) = utils::paste(polished_text, app_clone.clone()) {
                        error!("Failed to paste polished text: {}", e);
                        let _ = app_clone.emit("polish-error", format!("Failed to paste polished text: {}", e));
                    }
                },
                Err(e) => {
                    error!("Failed to apply polish rules: {}", e);
                    let _ = app_clone.emit("polish-error", format!("Polish failed: {}", e));
                }
            }
            
            // Hide overlay and reset tray icon
            utils::hide_recording_overlay(&app_clone);
            change_tray_icon(&app_clone, TrayIconState::Idle);
        });
    }

    fn stop(&self, _app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        debug!("PolishAction::stop called for binding: {}", binding_id);
        // Polish action is instantaneous, no stop action needed
    }
}

// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        println!(
            "Shortcut ID '{}': Started - {} (App: {})", // Changed "Pressed" to "Started" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        println!(
            "Shortcut ID '{}': Stopped - {} (App: {})", // Changed "Released" to "Stopped" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }
}

// Static Action Map
pub static ACTION_MAP: Lazy<HashMap<String, Arc<dyn ShortcutAction>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "transcribe".to_string(),
        Arc::new(TranscribeAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "polish".to_string(),
        Arc::new(PolishAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});
