use crate::settings;
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::OutputStreamBuilder;
use std::fs::File;
use std::io::BufReader;
use std::thread;
use tauri::{AppHandle, Manager};

pub enum SoundType {
    Start,
    Stop,
}

/// Plays an audio resource from the specified directory.
fn play_sound(app: &AppHandle, resource_path: &str, base_dir: tauri::path::BaseDirectory) {
    let app_handle = app.clone();
    let resource_path = resource_path.to_string();
    let volume = settings::get_settings(app).audio_feedback_volume;

    thread::spawn(move || {
        let audio_path = match app_handle.path().resolve(&resource_path, base_dir) {
            Ok(path) => path.to_path_buf(),
            Err(e) => {
                eprintln!(
                    "Failed to resolve audio file path '{}': {}",
                    resource_path, e
                );
                return;
            }
        };

        let settings = settings::get_settings(&app_handle);
        let selected_device = settings.selected_output_device.clone();

        if let Err(e) = play_audio_file(&audio_path, selected_device, volume) {
            eprintln!("Failed to play sound '{}': {}", resource_path, e);
        }
    });
}

fn get_sound_path(app: &AppHandle, sound_type: SoundType) -> String {
    let settings = settings::get_settings(app);
    match sound_type {
        SoundType::Start => match settings.sound_theme {
            crate::settings::SoundTheme::Custom => "custom_start.wav".to_string(),
            _ => settings.sound_theme.to_start_path(),
        },
        SoundType::Stop => match settings.sound_theme {
            crate::settings::SoundTheme::Custom => "custom_stop.wav".to_string(),
            _ => settings.sound_theme.to_stop_path(),
        },
    }
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    // Only play if audio feedback is enabled
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }

    let sound_file = get_sound_path(app, sound_type);
    let base_dir = if settings.sound_theme == crate::settings::SoundTheme::Custom {
        tauri::path::BaseDirectory::AppData
    } else {
        tauri::path::BaseDirectory::Resource
    };
    play_sound(app, &sound_file, base_dir);
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    // Always play test sound, regardless of audio_feedback setting
    let settings = settings::get_settings(app);
    let sound_file = get_sound_path(app, sound_type);
    let base_dir = if settings.sound_theme == crate::settings::SoundTheme::Custom {
        tauri::path::BaseDirectory::AppData
    } else {
        tauri::path::BaseDirectory::Resource
    };
    play_sound(app, &sound_file, base_dir);
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            println!("Using default device");
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
                    eprintln!("Device '{}' not found, using default device", device_name);
                    OutputStreamBuilder::from_default_device()?
                }
            }
        }
    } else {
        println!("Using default device");
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
