use crate::audio_toolkit::audio::output_device_name;
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use log::{debug, error, warn};
use rodio::cpal::traits::HostTrait;
use rodio::source::SineWave;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Range accepted for the recording tone's pitch, so a hand-edited settings
/// file can't ask for an inaudible or out-of-range sine.
const RECORDING_TONE_MIN_HZ: f32 = 20.0;
const RECORDING_TONE_MAX_HZ: f32 = 20_000.0;
/// Amplitude at 100% on the tone volume slider. The tone only needs to be
/// noticeable, so even the top of the slider stays well below full scale.
const RECORDING_TONE_MAX_AMPLITUDE: f32 = 0.25;
/// Most extra gain given to low pitches (see `loudness_boost`).
const RECORDING_TONE_MAX_BOOST_DB: f64 = 30.0;
/// Hard ceiling after the low-pitch boost, well short of clipping.
const RECORDING_TONE_AMPLITUDE_LIMIT: f32 = 0.5;
/// Fade in and out so starting and stopping the tone doesn't click.
const RECORDING_TONE_FADE: Duration = Duration::from_millis(50);
const RECORDING_TONE_FADE_STEPS: u32 = 10;
/// How often the tone checks whether recording is still going on.
const RECORDING_TONE_POLL: Duration = Duration::from_millis(20);

pub enum SoundType {
    Start,
    Stop,
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let sound_file = get_sound_path(settings, sound_type);
    let base_dir = get_sound_base_dir(settings);
    match base_dir {
        tauri::path::BaseDirectory::AppData => {
            crate::portable::resolve_app_data(app, &sound_file).ok()
        }
        _ => app.path().resolve(&sound_file, base_dir).ok(),
    }
}

fn get_sound_path(settings: &AppSettings, sound_type: SoundType) -> String {
    match (settings.sound_theme, sound_type) {
        (SoundTheme::Custom, SoundType::Start) => "custom_start.wav".to_string(),
        (SoundTheme::Custom, SoundType::Stop) => "custom_stop.wav".to_string(),
        (_, SoundType::Start) => settings.sound_theme.to_start_path(),
        (_, SoundType::Stop) => settings.sound_theme.to_stop_path(),
    }
}

fn get_sound_base_dir(settings: &AppSettings) -> tauri::path::BaseDirectory {
    match settings.sound_theme {
        SoundTheme::Custom => tauri::path::BaseDirectory::AppData,
        _ => tauri::path::BaseDirectory::Resource,
    }
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

/// Plays a quiet continuous tone for as long as `is_active` returns true, so
/// the user can tell at any moment that Handy is still recording, not only
/// at the start and stop chimes. Blocks the calling thread.
pub fn play_recording_tone_while(app: &AppHandle, is_active: impl Fn() -> bool) {
    let settings = settings::get_settings(app);
    if !settings.recording_tone {
        return;
    }
    let frequency = settings
        .recording_tone_frequency
        .clamp(RECORDING_TONE_MIN_HZ, RECORDING_TONE_MAX_HZ);
    // Square the slider value so the quiet end, where the tone is meant to
    // sit, gets most of the slider's travel.
    let volume = settings.recording_tone_volume.clamp(0.0, 1.0);
    let amplitude = (volume * volume * RECORDING_TONE_MAX_AMPLITUDE * loudness_boost(frequency))
        .min(RECORDING_TONE_AMPLITUDE_LIMIT);
    if let Err(e) = play_tone_while(
        settings.selected_output_device,
        frequency,
        amplitude,
        is_active,
    ) {
        error!("Failed to play recording tone: {}", e);
    }
}

/// Gain that makes a low tone sound about as loud as a 1 kHz tone at the same
/// slider position. The ear is far less sensitive to low pitches (about 19 dB
/// at 100 Hz, 30 dB at 50 Hz), so without it the quiet levels the tone is
/// meant for make the low notes inaudible. This is the inverse of the
/// A-weighting curve (IEC 61672-1), which approximates that sensitivity; it
/// only ever boosts, and is capped so a deep note stays sane.
fn loudness_boost(frequency: f32) -> f32 {
    let f2 = f64::from(frequency).powi(2);
    let ra = 12194.0_f64.powi(2) * f2 * f2
        / ((f2 + 20.6_f64.powi(2))
            * ((f2 + 107.7_f64.powi(2)) * (f2 + 737.9_f64.powi(2))).sqrt()
            * (f2 + 12194.0_f64.powi(2)));
    let a_weighting_db = 20.0 * ra.log10() + 2.0;
    let boost_db = (-a_weighting_db).clamp(0.0, RECORDING_TONE_MAX_BOOST_DB);
    10.0_f64.powf(boost_db / 20.0) as f32
}

fn play_tone_while(
    selected_device: Option<String>,
    frequency: f32,
    amplitude: f32,
    is_active: impl Fn() -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !is_active() {
        return Ok(());
    }
    let device_sink = open_output_sink(selected_device)?;
    let player = Player::connect_new(device_sink.mixer());
    player.set_volume(amplitude);
    player.append(SineWave::new(frequency).fade_in(RECORDING_TONE_FADE));

    while is_active() {
        thread::sleep(RECORDING_TONE_POLL);
    }

    let step = RECORDING_TONE_FADE / RECORDING_TONE_FADE_STEPS;
    for i in (0..RECORDING_TONE_FADE_STEPS).rev() {
        player.set_volume(amplitude * i as f32 / RECORDING_TONE_FADE_STEPS as f32);
        thread::sleep(step);
    }
    player.stop();

    Ok(())
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
    let device_sink = open_output_sink(selected_device)?;
    let mixer = device_sink.mixer();

    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let player = rodio::play(mixer, buf_reader)?;
    player.set_volume(volume);
    player.sleep_until_end();

    Ok(())
}

fn open_output_sink(
    selected_device: Option<String>,
) -> Result<MixerDeviceSink, Box<dyn std::error::Error>> {
    let sink_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            debug!("Using default device");
            DeviceSinkBuilder::from_default_device()?
        } else {
            let host = crate::audio_toolkit::get_cpal_host();
            let found_device = host
                .output_devices()?
                .find(|device| output_device_name(device).as_deref() == Some(&device_name));

            match found_device {
                Some(device) => DeviceSinkBuilder::from_device(device)?,
                None => {
                    warn!("Device '{}' not found, using default device", device_name);
                    DeviceSinkBuilder::from_default_device()?
                }
            }
        }
    } else {
        debug!("Using default device");
        DeviceSinkBuilder::from_default_device()?
    };

    Ok(sink_builder.open_stream()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boost_db(frequency: f32) -> f64 {
        20.0 * f64::from(loudness_boost(frequency)).log10()
    }

    #[test]
    fn loudness_boost_follows_a_weighting() {
        // IEC 61672-1 table values: 0 dB at 1 kHz, -19.1 dB at 100 Hz,
        // -8.6 dB at 250 Hz.
        assert!(boost_db(1000.0).abs() < 0.1);
        assert!((boost_db(100.0) - 19.1).abs() < 0.2);
        assert!((boost_db(250.0) - 8.6).abs() < 0.2);
    }

    #[test]
    fn loudness_boost_never_cuts_and_is_capped() {
        // A-weighting is slightly positive around 2-5 kHz; never cut there.
        assert!(boost_db(3000.0) >= 0.0);
        assert!((boost_db(20.0) - RECORDING_TONE_MAX_BOOST_DB).abs() < 0.01);
    }
}
