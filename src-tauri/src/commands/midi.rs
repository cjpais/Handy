use log::{error, info};
use tauri::{command, AppHandle, Manager, State};

use crate::managers::midi::{MidiManager, MidiRuntimeConfig};
use crate::settings::{get_settings, write_settings};

#[command]
#[specta::specta]
pub fn get_midi_ports(midi_manager: State<'_, MidiManager>) -> Result<Vec<String>, String> {
    match midi_manager.get_ports() {
        Ok(ports) => Ok(ports),
        Err(err) => {
            error!("Failed to enumerate MIDI ports: {}", err);
            Ok(Vec::new())
        }
    }
}

#[command]
#[specta::specta]
pub fn set_midi_binding_mode(
    midi_manager: State<'_, MidiManager>,
    binding: bool,
) -> Result<(), String> {
    info!("Setting MIDI binding mode: {}", binding);
    midi_manager
        .set_binding_mode(binding)
        .map_err(|err| err.to_string())
}

#[command]
#[specta::specta]
pub fn update_midi_settings(
    app: AppHandle,
    enabled: bool,
    device_name: Option<String>,
    trigger: Option<Vec<u8>>,
) -> Result<(), String> {
    let midi_manager = app.state::<MidiManager>();
    let current_settings = get_settings(&app);
    let requested_device_name = device_name.clone();
    let effective_device_name = if enabled {
        requested_device_name
            .clone()
            .or_else(|| current_settings.midi_device_name.clone())
            .ok_or_else(|| "Cannot enable MIDI without a selected device".to_string())?
    } else {
        String::new()
    };

    if enabled {
        midi_manager
            .connect(&effective_device_name)
            .map_err(|err| {
                error!(
                    "Failed to connect MIDI device '{}': {}",
                    effective_device_name, err
                );
                err.to_string()
            })?;
    } else {
        midi_manager.disconnect().map_err(|err| err.to_string())?;
    }

    midi_manager.update_runtime_config(MidiRuntimeConfig {
        enabled,
        trigger: trigger.clone(),
        push_to_talk: current_settings.push_to_talk,
    });

    let mut updated_settings = current_settings;
    updated_settings.midi_enabled = enabled;
    updated_settings.midi_device_name = if enabled {
        Some(effective_device_name)
    } else {
        requested_device_name
    };
    updated_settings.midi_trigger = trigger;
    write_settings(&app, updated_settings);

    Ok(())
}
