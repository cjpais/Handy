use crate::audio_toolkit::audio::output_device_name;
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use log::{debug, error, warn};
use rodio::cpal::traits::HostTrait;
use rodio::cpal::ErrorKind;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

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

const DEFAULT_DEVICE: &str = "Default";
/// Extra time allowed past a sound's own length before the wait gives up.
const PLAYBACK_MARGIN: Duration = Duration::from_secs(1);
/// Wait used when the decoder cannot tell how long the sound is.
const UNKNOWN_LENGTH_WAIT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// A stream with no chime for this long is closed, so Handy does not hold an
/// output stream (and keep the device awake) while it sits idle.
const IDLE_CLOSE_AFTER: Duration = Duration::from_secs(30);

/// One output stream shared by every feedback sound.
///
/// Opening a fresh stream per chime left a playback stream behind on each
/// play under PipeWire (#1973), and on Windows a per-chime WASAPI stream could
/// stall with a frozen clock so the playing thread never returned (#1712).
/// The stream is opened on the first chime and reused until the configured
/// device changes, the stream reports a fatal error, a sound fails to finish
/// in time, or no chime has played for `IDLE_CLOSE_AFTER`.
struct CachedOutput {
    device: String,
    sink: MixerDeviceSink,
    /// Set from cpal's error callback when the stream can no longer play.
    failed: Arc<AtomicBool>,
    /// Identifies this stream, so an idle watcher never closes a newer one.
    generation: u64,
    /// When the last chime on this stream finished.
    last_used: Instant,
}

/// Held for the length of one sound, so overlapping chimes play one after the
/// other. Every hold is bounded by `playback_deadline`.
static OUTPUT: Mutex<Option<CachedOutput>> = Mutex::new(None);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Time left before an unused stream should be closed, or `None` when it has
/// been idle long enough to close now.
fn idle_time_left(last_used: Instant, now: Instant, idle_limit: Duration) -> Option<Duration> {
    let idle_for = now.saturating_duration_since(last_used);
    idle_limit
        .checked_sub(idle_for)
        .filter(|left| !left.is_zero())
}

/// Runs for the life of one cached stream and closes it once it has gone
/// unused for `IDLE_CLOSE_AFTER`. Exits as soon as the stream it watches has
/// been replaced or dropped for another reason.
fn spawn_idle_watcher(generation: u64) {
    thread::spawn(move || {
        let mut wait = IDLE_CLOSE_AFTER;
        loop {
            thread::sleep(wait);
            let mut output = OUTPUT
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let last_used = match output.as_ref() {
                Some(cached) if cached.generation == generation => cached.last_used,
                _ => return,
            };
            match idle_time_left(last_used, Instant::now(), IDLE_CLOSE_AFTER) {
                Some(left) => wait = left,
                None => {
                    debug!("closing idle feedback output stream");
                    discard(&mut output);
                    return;
                }
            }
        }
    });
}

enum PlayOutcome {
    Finished,
    StreamFailed,
    TimedOut(Duration),
}

/// Whether the cached stream has to be replaced before the next sound.
fn should_reopen(cached_device: Option<&str>, cached_failed: bool, wanted_device: &str) -> bool {
    match cached_device {
        None => true,
        Some(device) => cached_failed || device != wanted_device,
    }
}

/// How long to wait for a sound to finish before treating the stream as stuck.
fn playback_deadline(total_duration: Option<Duration>) -> Duration {
    match total_duration {
        Some(length) => length + PLAYBACK_MARGIN,
        None => UNKNOWN_LENGTH_WAIT,
    }
}

/// Stream errors after which the stream will not play again. Glitches, a
/// rerouted default device and a refused real-time priority leave it usable.
fn is_fatal_stream_error(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
    )
}

fn sink_builder_for(device_name: &str) -> Result<DeviceSinkBuilder, Box<dyn std::error::Error>> {
    if device_name == DEFAULT_DEVICE {
        debug!("Using default device");
        return Ok(DeviceSinkBuilder::from_default_device()?);
    }

    let host = crate::audio_toolkit::get_cpal_host();
    let found_device = host
        .output_devices()?
        .find(|device| output_device_name(device).as_deref() == Some(device_name));

    match found_device {
        Some(device) => Ok(DeviceSinkBuilder::from_device(device)?),
        None => {
            warn!("Device '{}' not found, using default device", device_name);
            Ok(DeviceSinkBuilder::from_default_device()?)
        }
    }
}

fn open_output(device_name: &str) -> Result<CachedOutput, Box<dyn std::error::Error>> {
    let failed = Arc::new(AtomicBool::new(false));
    let failed_flag = failed.clone();
    let mut sink = sink_builder_for(device_name)?
        .with_error_callback(move |err: rodio::cpal::Error| {
            if is_fatal_stream_error(err.kind()) {
                warn!("Feedback output stream error, will reopen: {}", err);
                failed_flag.store(true, Ordering::SeqCst);
            } else {
                debug!("Feedback output stream warning: {}", err);
            }
        })
        .open_stream()?;
    // The cache drops the sink on purpose; rodio would otherwise print a
    // warning to stderr each time.
    sink.log_on_drop(false);
    debug!("opened feedback output stream for {}", device_name);
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    // The caller holds the cache lock, so the watcher cannot look before this
    // stream is stored.
    spawn_idle_watcher(generation);
    Ok(CachedOutput {
        device: device_name.to_string(),
        sink,
        failed,
        generation,
        last_used: Instant::now(),
    })
}

/// Drops the cached stream off the calling thread. Dropping a cpal stream
/// joins its audio thread; if that stream is the one that stalled, the join
/// must not hold the cache lock or the chime caller.
fn discard(output: &mut Option<CachedOutput>) {
    if let Some(old) = output.take() {
        thread::spawn(move || drop(old));
    }
}

fn play_on(
    output: &CachedOutput,
    path: &Path,
    volume: f32,
) -> Result<PlayOutcome, Box<dyn std::error::Error>> {
    let source = Decoder::try_from(File::open(path)?)?;
    let limit = playback_deadline(source.total_duration());

    let player = Player::connect_new(output.sink.mixer());
    player.set_volume(volume);
    player.append(source);

    let deadline = Instant::now() + limit;
    loop {
        if player.empty() {
            return Ok(PlayOutcome::Finished);
        }
        if output.failed.load(Ordering::SeqCst) {
            player.stop();
            return Ok(PlayOutcome::StreamFailed);
        }
        if Instant::now() >= deadline {
            player.stop();
            return Ok(PlayOutcome::TimedOut(limit));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let device = selected_device.unwrap_or_else(|| DEFAULT_DEVICE.to_string());
    let mut output = OUTPUT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // A reused stream that fails while playing gets one retry on a fresh
    // stream, which covers the device going away between chimes.
    for attempt in 0..2 {
        let reopen = should_reopen(
            output.as_ref().map(|cached| cached.device.as_str()),
            output
                .as_ref()
                .is_some_and(|cached| cached.failed.load(Ordering::SeqCst)),
            &device,
        );
        if reopen {
            discard(&mut output);
            *output = Some(open_output(&device)?);
        }
        let cached = output
            .as_ref()
            .ok_or("feedback output stream missing after open")?;

        match play_on(cached, path, volume)? {
            PlayOutcome::Finished => {
                if let Some(cached) = output.as_mut() {
                    cached.last_used = Instant::now();
                }
                return Ok(());
            }
            PlayOutcome::StreamFailed => {
                discard(&mut output);
                if attempt == 0 && !reopen {
                    warn!("Feedback output stream failed, retrying on a fresh stream");
                    continue;
                }
                return Err("feedback output stream failed".into());
            }
            PlayOutcome::TimedOut(limit) => {
                warn!(
                    "Feedback sound '{}' did not finish within {} ms; the output stream will be reopened",
                    path.display(),
                    limit.as_millis()
                );
                discard(&mut output);
                return Ok(());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_when_nothing_is_cached() {
        assert!(should_reopen(None, false, "Default"));
    }

    #[test]
    fn reuses_a_healthy_stream_for_the_same_device() {
        assert!(!should_reopen(Some("Default"), false, "Default"));
        assert!(!should_reopen(Some("Speakers"), false, "Speakers"));
    }

    #[test]
    fn reopens_when_the_device_setting_changes() {
        assert!(should_reopen(Some("Default"), false, "Speakers"));
        assert!(should_reopen(Some("Speakers"), false, "Default"));
    }

    #[test]
    fn reopens_after_a_stream_failure() {
        assert!(should_reopen(Some("Default"), true, "Default"));
    }

    #[test]
    fn deadline_is_length_plus_margin() {
        assert_eq!(
            playback_deadline(Some(Duration::from_millis(400))),
            Duration::from_millis(1400)
        );
    }

    #[test]
    fn deadline_falls_back_when_length_is_unknown() {
        assert_eq!(playback_deadline(None), Duration::from_secs(3));
    }

    #[test]
    fn idle_stream_stays_open_until_the_limit() {
        let start = Instant::now();
        let limit = Duration::from_secs(30);
        assert_eq!(
            idle_time_left(start, start + Duration::from_secs(10), limit),
            Some(Duration::from_secs(20))
        );
        assert_eq!(idle_time_left(start, start, limit), Some(limit));
    }

    #[test]
    fn idle_stream_closes_at_or_after_the_limit() {
        let start = Instant::now();
        let limit = Duration::from_secs(30);
        assert_eq!(idle_time_left(start, start + limit, limit), None);
        assert_eq!(
            idle_time_left(start, start + Duration::from_secs(45), limit),
            None
        );
    }

    #[test]
    fn chime_after_the_watcher_woke_pushes_the_close_back() {
        // last_used later than "now" (clock read before the chime finished)
        // must not close the stream.
        let now = Instant::now();
        let limit = Duration::from_secs(30);
        assert_eq!(
            idle_time_left(now + Duration::from_secs(1), now, limit),
            Some(limit)
        );
    }

    #[test]
    fn only_unrecoverable_stream_errors_force_a_reopen() {
        assert!(!is_fatal_stream_error(ErrorKind::Xrun));
        assert!(!is_fatal_stream_error(ErrorKind::DeviceChanged));
        assert!(!is_fatal_stream_error(ErrorKind::RealtimeDenied));
        assert!(is_fatal_stream_error(ErrorKind::DeviceNotAvailable));
        assert!(is_fatal_stream_error(ErrorKind::StreamInvalidated));
        assert!(is_fatal_stream_error(ErrorKind::BackendError));
    }
}
