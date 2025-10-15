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

/// Plays an audio resource from the resources directory.
/// Checks if audio feedback is enabled in settings before playing.
pub fn play_sound(app: &AppHandle, resource_path: &str) {
    // Check if audio feedback is enabled
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }

    let app_handle = app.clone();
    let resource_path = resource_path.to_string();
    let volume = settings.audio_feedback_volume;

    // Spawn a new thread to play the audio without blocking the main thread
    thread::spawn(move || {
        // Get the path to the audio file in resources
        let audio_path = match app_handle
            .path()
            .resolve(&resource_path, tauri::path::BaseDirectory::Resource)
        {
            Ok(path) => path.to_path_buf(),
            Err(e) => {
                eprintln!(
                    "Failed to resolve audio file path '{}': {}",
                    resource_path, e
                );
                return;
            }
        };

        // Get the selected output device from settings
        let settings = settings::get_settings(&app_handle);
        let selected_device = settings.selected_output_device.clone();

        // Try to play the audio file
        if let Err(e) = play_audio_file(&audio_path, selected_device, volume) {
            eprintln!("Failed to play sound '{}': {}", resource_path, e);
        }
    });
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    let sound_file = match sound_type {
        SoundType::Start => settings.sound_theme.to_start_path(),
        SoundType::Stop => settings.sound_theme.to_stop_path(),
    };
    play_sound(app, &sound_file);
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            println!("Using default device");
            // Use default device
            OutputStreamBuilder::from_default_device()?
        } else {
            // Try to find the device by name
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
        // Use default device
        OutputStreamBuilder::from_default_device()?
    };

    let stream_handle = stream_builder.open_stream()?;
    let mixer = stream_handle.mixer();

    // Load the audio file
    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let sink = rodio::play(mixer, buf_reader)?;
    sink.set_volume(volume);
    sink.sleep_until_end();

    Ok(())
}
