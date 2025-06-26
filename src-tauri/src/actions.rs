use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::utils;
use crate::utils::change_tray_icon;
use crate::utils::TrayIconState;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action
struct TranscribeAction;

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let binding_id = binding_id.to_string();
        change_tray_icon(app, TrayIconState::Recording);

        let rm = app.state::<Arc<AudioRecordingManager>>();
        rm.try_start_recording(&binding_id);
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());

        change_tray_icon(app, TrayIconState::Idle);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task

        tauri::async_runtime::spawn(async move {
            let binding_id = binding_id.clone(); // Clone for the inner async task
            if let Some(samples) = rm.stop_recording(&binding_id) {
                match tm.transcribe(samples) {
                    Ok(transcription) => {
                        println!("Transcription Result: {}", transcription);
                        if !transcription.is_empty() {
                            let transcription_clone = transcription.clone();
                            let ah_clone = ah.clone();
                            ah.run_on_main_thread(move || {
                                match utils::paste(transcription_clone, ah_clone) {
                                    Ok(()) => println!("Text pasted successfully"),
                                    Err(e) => eprintln!("Failed to paste transcription: {}", e),
                                }
                            })
                            .unwrap_or_else(|e| {
                                eprintln!("Failed to run paste on main thread: {:?}", e);
                            });
                        }
                    }
                    Err(err) => println!("Global Shortcut Transcription error: {}", err),
                }
            }
        });
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
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});
