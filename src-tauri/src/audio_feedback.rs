use crate::settings::{self, AppSettings};
use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, error, warn};
use rodio::OutputStreamBuilder;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy)]
pub enum SoundType {
    Start,
    Stop,
}

fn sound_type_key(sound_type: SoundType) -> &'static str {
    match sound_type {
        SoundType::Start => "start",
        SoundType::Stop => "stop",
    }
}

fn resolve_custom_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let configured_path = match sound_type {
        SoundType::Start => settings.custom_start_sound_path.as_ref(),
        SoundType::Stop => settings.custom_stop_sound_path.as_ref(),
    };

    let resolved = if let Some(path) = configured_path {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            candidate
        } else {
            app.path()
                .resolve(path, tauri::path::BaseDirectory::AppData)
                .ok()?
        }
    } else {
        app.path()
            .resolve(
                format!("custom_{}.wav", sound_type_key(sound_type)),
                tauri::path::BaseDirectory::AppData,
            )
            .ok()?
    };

    if resolved.exists() {
        Some(resolved)
    } else {
        warn!(
            "Custom {} sound does not exist at '{}'",
            sound_type_key(sound_type),
            resolved.display()
        );
        None
    }
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    if settings.sound_theme == crate::settings::SoundTheme::Custom {
        return resolve_custom_sound_path(app, settings, sound_type);
    }

    let sound_file = match sound_type {
        SoundType::Start => settings.sound_theme.to_start_path(),
        SoundType::Stop => settings.sound_theme.to_stop_path(),
    };
    app.path()
        .resolve(&sound_file, tauri::path::BaseDirectory::Resource)
        .ok()
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(app, path);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

fn play_sound_async(app: &AppHandle, path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Err(e) = play_sound_at_path(&app_handle, path.as_path()) {
            error!("Failed to play sound '{}': {}", path.display(), e);
        }
    });
}

fn play_sound_blocking(app: &AppHandle, path: &Path) {
    if let Err(e) = play_sound_at_path(app, path) {
        error!("Failed to play sound '{}': {}", path.display(), e);
    }
}

fn play_sound_at_path(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::get_settings(app);
    let volume = settings.audio_feedback_volume;
    let selected_device = settings.selected_output_device.clone();
    play_audio_file(path, selected_device, volume)
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            debug!("Using default device");
            OutputStreamBuilder::from_default_device()?
        } else {
            let host = crate::audio_toolkit::get_cpal_host();
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
                    warn!("Device '{}' not found, using default device", device_name);
                    OutputStreamBuilder::from_default_device()?
                }
            }
        }
    } else {
        debug!("Using default device");
        OutputStreamBuilder::from_default_device()?
    };

    let stream_handle = stream_builder.open_stream()?;
    let mixer = stream_handle.mixer();

    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let sink = rodio::play(mixer, buf_reader)?;
    sink.set_volume(volume);
    sink.sleep_until_end();

    Ok(())
}
