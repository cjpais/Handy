//! Pause detection for streaming transcription.
//!
//! Detects sustained silence during recording to trigger intermediate transcription.

use log::debug;
use std::time::{Duration, Instant};

/// Detects speech pauses during recording.
///
/// Uses VAD results to identify sustained silence periods that indicate
/// natural pause points suitable for intermediate transcription.
pub struct PauseDetector {
    /// Number of consecutive silence frames observed
    silence_frames: u32,

    /// Threshold in frames before triggering a pause event
    pause_threshold_frames: u32,

    /// Whether we're currently in a detected pause state
    in_pause: bool,

    /// Whether speech has been detected since recording started
    has_seen_speech: bool,

    /// Timestamp of when silence started (for debugging)
    silence_start: Option<Instant>,

    /// Frame duration for calculating timing
    frame_duration_ms: u32,
}

impl PauseDetector {
    /// Create a new pause detector.
    ///
    /// # Arguments
    /// * `pause_threshold_ms` - Minimum silence duration to trigger a pause (e.g., 400ms)
    /// * `frame_duration_ms` - Duration of each VAD frame (typically 30ms)
    pub fn new(pause_threshold_ms: u32, frame_duration_ms: u32) -> Self {
        let pause_threshold_frames =
            (pause_threshold_ms as f32 / frame_duration_ms as f32).ceil() as u32;

        debug!(
            "PauseDetector created: threshold={}ms ({}frames), frame={}ms",
            pause_threshold_ms, pause_threshold_frames, frame_duration_ms
        );

        Self {
            silence_frames: 0,
            pause_threshold_frames,
            in_pause: false,
            has_seen_speech: false,
            silence_start: None,
            frame_duration_ms,
        }
    }

    /// Process a VAD result for this frame.
    ///
    /// Returns `true` if a pause was just detected (transition from speech to pause).
    /// This is a "rising edge" - it only returns true once per pause event.
    pub fn on_vad_result(&mut self, is_speech: bool) -> bool {
        if is_speech {
            // Speech detected - reset silence counter
            if self.silence_frames > 0 {
                debug!(
                    "Speech resumed after {}ms silence",
                    self.silence_frames * self.frame_duration_ms
                );
            }
            self.silence_frames = 0;
            self.silence_start = None;
            self.in_pause = false;
            self.has_seen_speech = true;
            return false;
        }

        // Silence frame
        if !self.has_seen_speech {
            // Don't count silence before any speech
            return false;
        }

        self.silence_frames += 1;

        if self.silence_start.is_none() {
            self.silence_start = Some(Instant::now());
        }

        // Check if we just crossed the threshold
        if self.silence_frames >= self.pause_threshold_frames && !self.in_pause {
            self.in_pause = true;
            let silence_duration = self
                .silence_start
                .map(|s| s.elapsed())
                .unwrap_or(Duration::ZERO);
            debug!(
                "Pause detected after {}ms of silence ({} frames)",
                silence_duration.as_millis(),
                self.silence_frames
            );
            return true;
        }

        false
    }

    /// Check if we're currently in a pause state.
    pub fn is_in_pause(&self) -> bool {
        self.in_pause
    }

    /// Check if speech has been detected since reset.
    pub fn has_seen_speech(&self) -> bool {
        self.has_seen_speech
    }

    /// Get the current silence duration in milliseconds.
    pub fn silence_duration_ms(&self) -> u32 {
        self.silence_frames * self.frame_duration_ms
    }

    /// Reset the detector state (call when starting a new recording).
    pub fn reset(&mut self) {
        self.silence_frames = 0;
        self.in_pause = false;
        self.has_seen_speech = false;
        self.silence_start = None;
        debug!("PauseDetector reset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pause_detection() {
        // 400ms threshold with 30ms frames = ~13 frames
        let mut detector = PauseDetector::new(400, 30);

        // No pause before speech
        for _ in 0..20 {
            assert!(!detector.on_vad_result(false));
        }
        assert!(!detector.has_seen_speech());

        // Speech detected
        assert!(!detector.on_vad_result(true));
        assert!(detector.has_seen_speech());

        // More speech
        assert!(!detector.on_vad_result(true));
        assert!(!detector.on_vad_result(true));

        // Start silence - not enough for pause yet
        for _ in 0..10 {
            assert!(!detector.on_vad_result(false));
        }
        assert!(!detector.is_in_pause());

        // More silence - should trigger pause
        for i in 0..5 {
            let result = detector.on_vad_result(false);
            // Should trigger around frame 13-14
            if i >= 3 {
                // At this point we've crossed ~13 frames
                break;
            }
        }

        // Eventually it should be in pause
        assert!(detector.is_in_pause());
    }

    #[test]
    fn test_pause_only_triggers_once() {
        let mut detector = PauseDetector::new(100, 30); // ~3-4 frames

        // Speech
        detector.on_vad_result(true);

        // Silence until pause triggers
        let mut triggered_count = 0;
        for _ in 0..20 {
            if detector.on_vad_result(false) {
                triggered_count += 1;
            }
        }

        // Should only trigger once
        assert_eq!(triggered_count, 1);
    }

    #[test]
    fn test_speech_resets_pause() {
        let mut detector = PauseDetector::new(100, 30);

        // Speech then pause
        detector.on_vad_result(true);
        for _ in 0..10 {
            detector.on_vad_result(false);
        }
        assert!(detector.is_in_pause());

        // Speech resumes - should exit pause
        detector.on_vad_result(true);
        assert!(!detector.is_in_pause());
        assert_eq!(detector.silence_duration_ms(), 0);
    }
}
