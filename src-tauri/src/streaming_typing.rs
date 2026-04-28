//! Real-time streaming voice typing.
//!
//! While the user is recording, this module periodically snapshots the audio
//! captured so far, runs it through the loaded Whisper model, diffs the
//! result against what has already been typed, and emits the delta directly
//! into the focused window via `enigo`. On stop, the consumer of the recorder
//! still gets the final samples for the canonical (full-quality) pass and
//! history persistence — this module only handles "show text as-it-arrives".
//!
//! Strategy:
//!   * Sliding-window re-transcription (transcribe-rs has no native streaming
//!     API — `supports_streaming: false`). We re-transcribe the entire audio
//!     so far every 500 ms.
//!   * Aggressive diff with backspace correction: longest common prefix
//!     between previous output and new output is kept; the diverging tail of
//!     the previous output is erased with backspaces; the new tail is typed.
//!     This produces flicker on word revisions but stays in sync with what
//!     Whisper currently believes the user said.
//!   * Skips very short snapshots (< 1.0 s) because Whisper produces garbage
//!     on tiny clips and we'd just have to backspace it all anyway.

use crate::input::EnigoState;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;
use enigo::{Direction, Key, Keyboard};
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 16 kHz mono samples needed before we attempt the first streaming snapshot.
const MIN_SAMPLES_FOR_FIRST_PASS: usize = 16_000; // 1.0 s
/// Cooldown between streaming Whisper passes. Whisper-medium runs ~600 ms
/// for short clips on this machine; 500 ms keeps the loop responsive without
/// permanently saturating the model lock.
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(500);

/// Handle returned by [`spawn`]. Drop it (or call `stop`) to halt streaming.
pub struct StreamingTypingHandle {
    stop_flag: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    /// Text that streaming has already typed into the window. Read by the
    /// final paste path so it knows how much (if any) to append.
    typed_so_far: Arc<Mutex<String>>,
}

impl StreamingTypingHandle {
    pub fn stop(mut self) -> String {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
        self.typed_so_far.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl Drop for StreamingTypingHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

/// Spawn the streaming typing thread for one recording session. The thread
/// exits when the returned handle is dropped or `stop()` is called.
pub fn spawn(app: AppHandle) -> StreamingTypingHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let typed_so_far = Arc::new(Mutex::new(String::new()));

    let stop_for_thread = Arc::clone(&stop_flag);
    let typed_for_thread = Arc::clone(&typed_so_far);

    let join = thread::Builder::new()
        .name("streaming-typer".into())
        .spawn(move || run_loop(app, stop_for_thread, typed_for_thread))
        .ok();

    StreamingTypingHandle {
        stop_flag,
        join,
        typed_so_far,
    }
}

fn run_loop(app: AppHandle, stop: Arc<AtomicBool>, typed_so_far: Arc<Mutex<String>>) {
    info!("[streaming] thread started");

    // Wait until enough audio is buffered for the first pass.
    let rm = match app.try_state::<Arc<AudioRecordingManager>>() {
        Some(s) => s.inner().clone(),
        None => {
            warn!("[streaming] AudioRecordingManager missing — aborting");
            return;
        }
    };
    let tm = match app.try_state::<Arc<TranscriptionManager>>() {
        Some(s) => s.inner().clone(),
        None => {
            warn!("[streaming] TranscriptionManager missing — aborting");
            return;
        }
    };

    // Pre-build the OpenCC converter once per session so we don't pay the
    // load cost on every snapshot. None when no zh-variant conversion is
    // requested in settings; matches actions::maybe_convert_chinese_variant.
    let opencc_converter: Option<OpenCC> = {
        let settings = get_settings(&app);
        let cfg = match settings.selected_language.as_str() {
            "zh-Hans" => Some(BuiltinConfig::Tw2sp), // Traditional → Simplified
            "zh-Hant" => Some(BuiltinConfig::S2twp), // Simplified → Traditional
            _ => None,
        };
        cfg.and_then(|c| match OpenCC::from_config(c) {
            Ok(conv) => Some(conv),
            Err(e) => {
                warn!("[streaming] OpenCC init failed: {e} — running without zh conversion");
                None
            }
        })
    };

    // Block until we have at least 1 s of audio, then start the cadence.
    let wait_start = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            info!("[streaming] stopped before first pass");
            return;
        }
        if rm.snapshot_live_audio().len() >= MIN_SAMPLES_FOR_FIRST_PASS {
            break;
        }
        if wait_start.elapsed() > Duration::from_secs(15) {
            warn!("[streaming] gave up waiting for audio after 15 s");
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let mut last_pass_done = Instant::now() - SNAPSHOT_INTERVAL;
    while !stop.load(Ordering::Relaxed) {
        // Sleep in small slices so stop() takes effect quickly.
        let elapsed = last_pass_done.elapsed();
        if elapsed < SNAPSHOT_INTERVAL {
            thread::sleep((SNAPSHOT_INTERVAL - elapsed).min(Duration::from_millis(50)));
            continue;
        }

        let samples = rm.snapshot_live_audio();
        if samples.is_empty() {
            last_pass_done = Instant::now();
            continue;
        }

        let pass_start = Instant::now();
        let raw_text = match tm.transcribe(samples) {
            Ok(t) => t.trim().to_string(),
            Err(e) => {
                debug!("[streaming] transcribe error (ignored): {e}");
                last_pass_done = Instant::now();
                continue;
            }
        };
        // Normalize to the user's preferred Chinese variant before diffing.
        // Whisper-zh randomly emits Simplified or Traditional regardless of
        // language=zh; without this, the variant would flicker on screen.
        let new_text = match &opencc_converter {
            Some(conv) => conv.convert(&raw_text),
            None => raw_text.clone(),
        };
        debug!(
            "[streaming] pass {:?} → {:?}",
            pass_start.elapsed(),
            preview(&new_text, 60)
        );

        if new_text.is_empty() {
            last_pass_done = Instant::now();
            continue;
        }

        // Diff against what we already typed and emit the delta.
        emit_diff(&app, &typed_so_far, &new_text);
        last_pass_done = Instant::now();
    }

    info!("[streaming] thread exited");
}

/// Compare `new_text` against the already-typed buffer and apply a
/// (backspace × N) + (type tail) update so the focused window reflects the
/// latest Whisper output. Updates `typed_so_far` to match what's now on
/// screen.
fn emit_diff(app: &AppHandle, typed_so_far: &Arc<Mutex<String>>, new_text: &str) {
    let mut current = typed_so_far.lock().unwrap();

    // Common prefix (in chars, not bytes — Chinese text is multi-byte).
    let common_chars = current
        .chars()
        .zip(new_text.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let current_chars: Vec<char> = current.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();
    let backspaces = current_chars.len() - common_chars;
    let to_type: String = new_chars[common_chars..].iter().collect();

    if backspaces == 0 && to_type.is_empty() {
        return;
    }

    info!(
        "[streaming] live diff: backspace×{backspaces}, append={:?} (was={:?})",
        preview(&to_type, 40),
        preview(&current, 40)
    );

    let enigo_state = match app.try_state::<EnigoState>() {
        Some(s) => s,
        None => {
            warn!("[streaming] EnigoState missing — cannot emit diff");
            return;
        }
    };
    let mut enigo = match enigo_state.0.lock() {
        Ok(g) => g,
        Err(_) => {
            warn!("[streaming] enigo mutex poisoned");
            return;
        }
    };

    for _ in 0..backspaces {
        if let Err(e) = enigo.key(Key::Backspace, Direction::Click) {
            warn!("[streaming] backspace failed: {e:?}");
            return;
        }
    }
    if !to_type.is_empty() {
        if let Err(e) = enigo.text(&to_type) {
            warn!("[streaming] text() failed: {e:?}");
            return;
        }
    }

    // Update mirror.
    let mut new_string = String::with_capacity(common_chars + to_type.len());
    new_string.extend(current_chars[..common_chars].iter());
    new_string.push_str(&to_type);
    *current = new_string;
}

/// Reconcile the on-screen text (which streaming typed character-by-character
/// from a series of sliding-window passes) against the canonical, full-quality
/// transcription emitted at the end. Backspaces over the diverging suffix and
/// types the new tail. Idempotent when `already_typed == final_text`.
pub fn reconcile(app: &AppHandle, already_typed: &str, final_text: &str) -> Result<(), String> {
    let common_chars = already_typed
        .chars()
        .zip(final_text.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let typed_chars: Vec<char> = already_typed.chars().collect();
    let final_chars: Vec<char> = final_text.chars().collect();
    let backspaces = typed_chars.len() - common_chars;
    let to_type: String = final_chars[common_chars..].iter().collect();

    if backspaces == 0 && to_type.is_empty() {
        debug!("[streaming] reconcile: no-op");
        return Ok(());
    }
    info!(
        "[streaming] reconcile: backspace×{backspaces}, append={:?}",
        preview(&to_type, 60)
    );

    let enigo_state = app
        .try_state::<EnigoState>()
        .ok_or_else(|| "EnigoState missing".to_string())?;
    let mut enigo = enigo_state
        .0
        .lock()
        .map_err(|e| format!("enigo lock failed: {e}"))?;

    for _ in 0..backspaces {
        enigo
            .key(Key::Backspace, Direction::Click)
            .map_err(|e| format!("backspace failed: {e:?}"))?;
    }
    if !to_type.is_empty() {
        enigo
            .text(&to_type)
            .map_err(|e| format!("text() failed: {e:?}"))?;
    }
    Ok(())
}

fn preview(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let head: String = chars[..max_chars].iter().collect();
        format!("{head}…")
    }
}
