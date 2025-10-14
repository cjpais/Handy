use crate::audio_feedback;
use crate::audio_toolkit::audio::{list_input_devices, list_output_devices};
use crate::managers::audio::{AudioRecordingManager, MicrophoneMode};
use crate::settings::{get_settings, write_settings};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri_plugin_dialog::DialogExt;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn upload_custom_sound(app: AppHandle, sound_type: String) {
    let app_handle = app.clone();
    app.dialog()
        .file()
        .add_filter("Audio", &["wav"])
        .pick_file(move |file_path| {
            if let Some(source_path) = file_path {
                let dest_path = app_handle
                    .path()
                    .resolve(
                        format!("resources/custom_{}.wav", sound_type),
                        tauri::path::BaseDirectory::Resource,
                    )
                    .unwrap();

                if let Some(path) = source_path.as_path() {
                    std::fs::copy(path, dest_path).unwrap();
                }
            }
        });
}

#[derive(Serialize)]
pub struct CustomSounds {
    start: bool,
    stop: bool,
}

#[tauri::command]
pub fn check_custom_sounds(app: AppHandle) -> CustomSounds {
    let start_path = app
        .path()
        .resolve("resources/custom_start.wav", tauri::path::BaseDirectory::Resource)
        .unwrap();
    let stop_path = app
        .path()
        .resolve("resources/custom_stop.wav", tauri::path::BaseDirectory::Resource)
        .unwrap();

    CustomSounds {
        start: start_path.exists(),
        stop: stop_path.exists(),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioDevice {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

#[tauri::command]
pub fn update_microphone_mode(app: AppHandle, always_on: bool) -> Result<(), String> {
    // Update settings
    let mut settings = get_settings(&app);
    settings.always_on_microphone = always_on;
    write_settings(&app, settings);

    // Update the audio manager mode
    let rm = app.state::<Arc<AudioRecordingManager>>();
    let new_mode = if always_on {
        MicrophoneMode::AlwaysOn
    } else {
        MicrophoneMode::OnDemand
    };

    rm.update_mode(new_mode)
        .map_err(|e| format!("Failed to update microphone mode: {}", e))
}

#[tauri::command]
pub fn get_microphone_mode(app: AppHandle) -> Result<bool, String> {
    let settings = get_settings(&app);
    Ok(settings.always_on_microphone)
}

#[tauri::command]
pub fn get_available_microphones() -> Result<Vec<AudioDevice>, String> {
    let devices =
        list_input_devices().map_err(|e| format!("Failed to list audio devices: {}", e))?;

    let mut result = vec![AudioDevice {
        index: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    }];

    result.extend(devices.into_iter().map(|d| AudioDevice {
        index: d.index,
        name: d.name,
        is_default: false, // The explicit default is handled separately
    }));

    Ok(result)
}

#[tauri::command]
pub fn set_selected_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);

    // Update the audio manager to use the new device
    let rm = app.state::<Arc<AudioRecordingManager>>();
    rm.update_selected_device()
        .map_err(|e| format!("Failed to update selected device: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn get_selected_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_microphone
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
pub fn get_available_output_devices() -> Result<Vec<AudioDevice>, String> {
    let devices =
        list_output_devices().map_err(|e| format!("Failed to list output devices: {}", e))?;

    let mut result = vec![AudioDevice {
        index: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    }];

    result.extend(devices.into_iter().map(|d| AudioDevice {
        index: d.index,
        name: d.name,
        is_default: false, // The explicit default is handled separately
    }));

    Ok(result)
}

#[tauri::command]
pub fn set_selected_output_device(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_output_device = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
pub fn get_selected_output_device(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_output_device
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
pub fn play_test_sound(app: AppHandle, sound_type: String) {
    match sound_type.as_str() {
        "start" => audio_feedback::play_recording_start_sound(&app),
        "stop" => audio_feedback::play_recording_stop_sound(&app),
        _ => eprintln!("Unknown sound type: {}", sound_type),
    }
}
