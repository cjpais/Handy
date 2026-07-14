use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, error, warn};
use rodio::OutputStreamBuilder;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub enum SoundType {
    Start,
    Stop,
}

/// How long a caller that needs the chime to finish (to sequence muting
/// after it) is allowed to wait. If the audio stack is wedged, callers
/// proceed without sound instead of hanging the transcription pipeline.
const BLOCKING_PLAY_TIMEOUT: Duration = Duration::from_secs(3);

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

enum Request {
    /// Ensure the output stream for `device` exists (startup pre-warm).
    Warm {
        device: Option<String>,
    },
    Play {
        path: PathBuf,
        device: Option<String>,
        volume: f32,
        done: Option<mpsc::Sender<()>>,
    },
}

static PLAYER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();

fn player() -> &'static mpsc::Sender<Request> {
    PLAYER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("audio-feedback".into())
            .spawn(move || playback_worker(rx))
            .expect("failed to spawn audio feedback thread");
        tx
    })
}

/// Pre-warm the output stream at startup, while no transcription is running.
/// Opening the stream is the WASAPI call that can wedge when it races other
/// audio session activity, taking the rest of the audio stack (including the
/// microphone stream shutdown on the transcription path) down with it — so it
/// happens once here and on device change, never per transcription.
pub fn init(app: &AppHandle) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    let _ = player().send(Request::Warm {
        device: settings.selected_output_device.clone(),
    });
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        send_play(&settings, path, None);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        wait_for_play(&settings, path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        wait_for_play(&settings, path);
    }
}

fn send_play(settings: &AppSettings, path: PathBuf, done: Option<mpsc::Sender<()>>) {
    let _ = player().send(Request::Play {
        path,
        device: settings.selected_output_device.clone(),
        volume: settings.audio_feedback_volume,
        done,
    });
}

fn wait_for_play(settings: &AppSettings, path: PathBuf) {
    let (tx, rx) = mpsc::channel();
    send_play(settings, path, Some(tx));
    if rx.recv_timeout(BLOCKING_PLAY_TIMEOUT).is_err() {
        warn!(
            "Audio feedback did not finish within {:?}; continuing without it",
            BLOCKING_PLAY_TIMEOUT
        );
    }
}

/// If a chime hasn't finished within this bound, its output stream is
/// considered a zombie (frozen audio clock after a device state change —
/// observed with wireless headsets) and gets scrapped.
const PLAYBACK_STALL_TIMEOUT: Duration = Duration::from_secs(5);

fn playback_worker(rx: mpsc::Receiver<Request>) {
    // The output stream is created once and kept open across transcriptions.
    // Recreating it per chime is what raced concurrent WASAPI session state
    // and could deadlock the whole audio stack, mic stream shutdown included.
    let mut cached: Option<(Option<String>, rodio::OutputStream)> = None;

    while let Ok(req) = rx.recv() {
        match req {
            Request::Warm { device } => {
                ensure_stream(&mut cached, device);
            }
            Request::Play {
                path,
                device,
                volume,
                done,
            } => {
                if let Some((_, stream)) = ensure_stream(&mut cached, device) {
                    if let Err(e) = play_on_stream(stream, &path, volume) {
                        error!(
                            "Failed to play sound '{}': {}; scrapping output stream",
                            path.display(),
                            e
                        );
                        scrap_stream(&mut cached);
                    }
                }
                if let Some(done) = done {
                    let _ = done.send(());
                }
            }
        }
    }
}

/// Dispose of the cached stream on a throwaway thread. Dropping a cpal
/// stream whose device wedged can block indefinitely — that must park a
/// disposable thread, never this worker.
fn scrap_stream(cached: &mut Option<(Option<String>, rodio::OutputStream)>) {
    if let Some((_, stream)) = cached.take() {
        thread::spawn(move || drop(stream));
    }
}

fn ensure_stream(
    cached: &mut Option<(Option<String>, rodio::OutputStream)>,
    device: Option<String>,
) -> Option<&(Option<String>, rodio::OutputStream)> {
    let stale = cached.as_ref().map(|(d, _)| d != &device).unwrap_or(true);
    if stale {
        scrap_stream(cached);
        match create_stream(device.as_deref()) {
            Ok(stream) => *cached = Some((device, stream)),
            Err(e) => error!("Failed to open audio feedback output stream: {}", e),
        }
    }
    cached.as_ref()
}

fn create_stream(
    device_name: Option<&str>,
) -> Result<rodio::OutputStream, Box<dyn std::error::Error>> {
    let stream_builder = if let Some(name) = device_name.filter(|n| *n != "Default") {
        let host = crate::audio_toolkit::get_cpal_host();
        let devices = host.output_devices()?;

        let mut found_device = None;
        for device in devices {
            if device.name()? == name {
                found_device = Some(device);
                break;
            }
        }

        match found_device {
            Some(device) => OutputStreamBuilder::from_device(device)?,
            None => {
                warn!("Device '{}' not found, using default device", name);
                OutputStreamBuilder::from_default_device()?
            }
        }
    } else {
        debug!("Using default device");
        OutputStreamBuilder::from_default_device()?
    };

    Ok(stream_builder.open_stream()?)
}

fn play_on_stream(
    stream: &rodio::OutputStream,
    path: &Path,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let sink = rodio::play(stream.mixer(), BufReader::new(file))?;
    sink.set_volume(volume);
    // Bounded wait instead of sleep_until_end(): a stream whose device
    // changed state underneath it stops consuming samples without erroring,
    // and an unbounded wait would wedge the worker on it forever.
    let started = std::time::Instant::now();
    while !sink.empty() {
        if started.elapsed() > PLAYBACK_STALL_TIMEOUT {
            sink.stop();
            return Err(format!(
                "playback did not finish within {:?} (zombie output stream?)",
                PLAYBACK_STALL_TIMEOUT
            )
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
