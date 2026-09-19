//! Hands-free continuous capture: a Silero-VAD-driven loop that auto-segments
//! utterances with no shortcut press, transcribes each one, and routes it through
//! [`crate::actions::route_hands_free_utterance`] (capture-all to history + wake-word
//! gated paste).
//!
//! Design (V0):
//! - The recorder is opened with a speech-frame callback (see
//!   [`AudioRecorder::with_speech_frame_callback`]). That callback feeds the
//!   [`HandsFreeSegmenter`], which accumulates speech frames into an utterance buffer
//!   and, after a run of trailing silence, finalizes the utterance.
//! - Finalized utterances are sent over a channel to a single worker thread so
//!   transcription is serialized (one utterance at a time) and never blocks the audio
//!   callback. This mirrors the wake-word path's "serialize through one thread" rule.
//! - Acoustic wake-word models are intentionally NOT used in V0; the wake word is
//!   gated on the transcript prefix in `route_hands_free_utterance`.

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use log::{debug, error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

/// One 30ms frame at 16kHz.
const FRAME_SAMPLES: usize = (WHISPER_SAMPLE_RATE as usize) * 30 / 1000; // 480

/// Number of consecutive silence frames that end an utterance (~0.6s).
const SILENCE_FRAMES_TO_FINALIZE: usize = 20;

/// Minimum speech samples for an utterance to be worth transcribing (~0.3s).
const MIN_UTTERANCE_SAMPLES: usize = (WHISPER_SAMPLE_RATE as usize) * 3 / 10;

/// Hard cap on a single utterance buffer (~30s) to bound memory if VAD never
/// reports silence (e.g. continuous background noise).
const MAX_UTTERANCE_SAMPLES: usize = (WHISPER_SAMPLE_RATE as usize) * 30;

/// Accumulates VAD-classified frames into utterances. Cheap to drive from the audio
/// callback: it only buffers and, on utterance end, hands a finished buffer to the
/// worker channel.
struct HandsFreeSegmenter {
    buffer: Vec<f32>,
    silence_run: usize,
    have_speech: bool,
}

impl HandsFreeSegmenter {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(WHISPER_SAMPLE_RATE as usize * 5),
            silence_run: 0,
            have_speech: false,
        }
    }

    /// Feed one frame. `Some(frame)` is a speech frame, `None` is silence/noise.
    /// Returns a finished utterance (16kHz mono f32) when one is finalized.
    fn push(&mut self, frame: Option<&[f32]>) -> Option<Vec<f32>> {
        match frame {
            Some(samples) => {
                self.have_speech = true;
                self.silence_run = 0;
                self.buffer.extend_from_slice(samples);
                if self.buffer.len() >= MAX_UTTERANCE_SAMPLES {
                    return self.finalize();
                }
                None
            }
            None => {
                if !self.have_speech {
                    // No speech captured yet; nothing to finalize.
                    return None;
                }
                self.silence_run += 1;
                if self.silence_run >= SILENCE_FRAMES_TO_FINALIZE {
                    return self.finalize();
                }
                None
            }
        }
    }

    /// Take the current buffer as a finished utterance and reset state. Returns `None`
    /// if the buffer is too short to be worth transcribing.
    fn finalize(&mut self) -> Option<Vec<f32>> {
        self.silence_run = 0;
        self.have_speech = false;
        if self.buffer.len() < MIN_UTTERANCE_SAMPLES {
            self.buffer.clear();
            return None;
        }
        Some(std::mem::take(&mut self.buffer))
    }
}

/// Owns the hands-free worker channel + paused flag. Held by the
/// [`AudioRecordingManager`] for the lifetime of the app.
pub struct HandsFreeManager {
    /// Sends finished utterances to the worker thread. `None` until started.
    tx: Mutex<Option<Sender<Vec<f32>>>>,
    /// When false, the loop is running; when true, segmentation is suspended.
    paused: Arc<AtomicBool>,
    /// Whether the hands-free loop is currently active.
    running: Arc<AtomicBool>,
    segmenter: Arc<Mutex<HandsFreeSegmenter>>,
}

impl HandsFreeManager {
    pub fn new() -> Self {
        Self {
            tx: Mutex::new(None),
            paused: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            segmenter: Arc::new(Mutex::new(HandsFreeSegmenter::new())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Toggle pause. Returns the new paused state. While paused, incoming frames are
    /// dropped and the in-flight utterance buffer is reset.
    pub fn toggle_pause(&self) -> bool {
        let now = !self.paused.fetch_xor(true, Ordering::Relaxed);
        if now {
            // Just paused: discard any partial utterance.
            *self.segmenter.lock().unwrap() = HandsFreeSegmenter::new();
        }
        info!("Hands-free: paused={}", now);
        now
    }

    /// Start the worker thread. Idempotent. The audio callback is wired separately via
    /// [`HandsFreeManager::on_speech_frame`].
    pub fn start(&self, app: &AppHandle) {
        if self.running.swap(true, Ordering::Relaxed) {
            debug!("Hands-free: already running");
            return;
        }
        self.paused.store(false, Ordering::Relaxed);
        *self.segmenter.lock().unwrap() = HandsFreeSegmenter::new();

        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        *self.tx.lock().unwrap() = Some(tx);

        // Warm the transcription model so the first utterance is fast.
        if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
            tm.initiate_model_load();
        }

        let app = app.clone();
        std::thread::spawn(move || {
            info!("Hands-free worker started");
            while let Ok(utterance) = rx.recv() {
                process_utterance(&app, utterance);
            }
            info!("Hands-free worker exited");
        });
    }

    /// Stop the worker thread and reset state.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        // Dropping the sender closes the channel and ends the worker loop.
        *self.tx.lock().unwrap() = None;
        *self.segmenter.lock().unwrap() = HandsFreeSegmenter::new();
        info!("Hands-free: stopped");
    }

    /// Called from the recorder's speech-frame callback for every VAD-classified frame.
    /// Cheap: buffers and, on utterance end, ships the buffer to the worker.
    pub fn on_speech_frame(&self, frame: Option<&[f32]>) {
        if !self.running.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) {
            return;
        }
        let finished = self.segmenter.lock().unwrap().push(frame);
        if let Some(utterance) = finished {
            if let Some(tx) = self.tx.lock().unwrap().as_ref() {
                if tx.send(utterance).is_err() {
                    error!("Hands-free: worker channel closed, dropping utterance");
                }
            }
        }
    }
}

impl Default for HandsFreeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Transcribe one utterance and route it (history + wake-word gated paste). Runs on the
/// dedicated worker thread, so it may block on transcription.
fn process_utterance(app: &AppHandle, samples: Vec<f32>) {
    let _ = FRAME_SAMPLES; // documented frame size; kept for clarity.
    debug!("Hands-free: processing utterance ({} samples)", samples.len());

    let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() else {
        error!("Hands-free: TranscriptionManager unavailable");
        return;
    };
    let Some(hm) = app.try_state::<Arc<HistoryManager>>() else {
        error!("Hands-free: HistoryManager unavailable");
        return;
    };

    // Persist the WAV so a history row can reference it (capture-all).
    let file_name = format!("handy-handsfree-{}.wav", chrono::Utc::now().timestamp_millis());
    let wav_path = hm.recordings_dir().join(&file_name);
    let sample_count = samples.len();
    let wav_saved = match crate::audio_toolkit::save_wav_file(&wav_path, &samples) {
        Ok(()) => match crate::audio_toolkit::verify_wav_file(&wav_path, sample_count) {
            Ok(()) => true,
            Err(e) => {
                error!("Hands-free: WAV verification failed: {}", e);
                false
            }
        },
        Err(e) => {
            error!("Hands-free: failed to save WAV: {}", e);
            false
        }
    };

    let transcription = match tm.transcribe(samples) {
        Ok(t) => t,
        Err(e) => {
            error!("Hands-free: transcription error: {}", e);
            return;
        }
    };

    let trimmed = transcription.trim();
    if trimmed.is_empty() {
        debug!("Hands-free: empty transcription, skipping");
        return;
    }

    // Route on the async runtime (paste + post-process helpers are async).
    let app_for_route = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::actions::route_hands_free_utterance(
            &app_for_route,
            transcription,
            file_name,
            wav_saved,
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(n: usize) -> Vec<f32> {
        vec![0.1f32; n]
    }

    #[test]
    fn segmenter_finalizes_after_silence() {
        let mut seg = HandsFreeSegmenter::new();
        // Enough speech to exceed the minimum.
        let frame = speech(FRAME_SAMPLES);
        let needed = (MIN_UTTERANCE_SAMPLES / FRAME_SAMPLES) + 2;
        for _ in 0..needed {
            assert!(seg.push(Some(&frame)).is_none());
        }
        // Silence shorter than threshold does not finalize.
        for _ in 0..(SILENCE_FRAMES_TO_FINALIZE - 1) {
            assert!(seg.push(None).is_none());
        }
        // The threshold-crossing silence frame finalizes.
        let out = seg.push(None).expect("utterance finalized");
        assert!(out.len() >= MIN_UTTERANCE_SAMPLES);
    }

    #[test]
    fn segmenter_drops_too_short_utterance() {
        let mut seg = HandsFreeSegmenter::new();
        let frame = speech(FRAME_SAMPLES);
        // One short speech frame, well under the minimum.
        assert!(seg.push(Some(&frame)).is_none());
        for _ in 0..SILENCE_FRAMES_TO_FINALIZE {
            // Should never produce an utterance (too short).
            assert!(seg.push(None).is_none());
        }
    }

    #[test]
    fn silence_without_speech_does_nothing() {
        let mut seg = HandsFreeSegmenter::new();
        for _ in 0..(SILENCE_FRAMES_TO_FINALIZE * 3) {
            assert!(seg.push(None).is_none());
        }
    }
}
