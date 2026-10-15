use anyhow::Result;

use crate::audio_toolkit::constants;

pub const VAD_PREFILL_MS: u64 = 450;
pub const VAD_OFFLINE_HANGOVER_MS: u64 = 450;
pub const VAD_STREAMING_HANGOVER_MS: u64 = 1650;
pub const VAD_ONSET_MS: u64 = 60;

/// Convert a VAD timing duration to whole detector frames, rounding up so an
/// alternate backend never shortens Handy's onset, pre-roll, or hangover tail.
pub const fn frames_for_duration_ms(duration_ms: u64, frame_samples: usize) -> usize {
    assert!(frame_samples > 0, "VAD frame size must be non-zero");
    let numerator = duration_ms * constants::WHISPER_SAMPLE_RATE as u64;
    let denominator = frame_samples as u64 * 1000;
    numerator.div_ceil(denominator) as usize
}

pub enum VadFrame<'a> {
    /// Speech – may aggregate several frames (prefill + current + hangover),
    /// with each frame's speech probability, in order.
    Speech {
        samples: &'a [f32],
        probs: &'a [f32],
    },
    /// Non-speech (silence, noise), with the frame's speech probability.
    /// Down-stream code can ignore the audio.
    Noise { prob: f32 },
}

impl<'a> VadFrame<'a> {
    #[inline]
    pub fn is_speech(&self) -> bool {
        matches!(self, VadFrame::Speech { .. })
    }

    /// The speech probability of the frame just pushed: the last one of a
    /// speech run.
    pub fn frame_prob(&self) -> f32 {
        match self {
            VadFrame::Speech { probs, .. } => probs.last().copied().unwrap_or(0.0),
            VadFrame::Noise { prob } => *prob,
        }
    }
}

pub trait VoiceActivityDetector: Send + Sync {
    /// Primary streaming API: feed one backend-sized frame, get a keep/drop decision.
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>>;

    /// Required number of mono 16 kHz samples per prediction.
    fn frame_samples(&self) -> usize;

    fn is_voice(&mut self, frame: &[f32]) -> Result<bool> {
        Ok(self.push_frame(frame)?.is_speech())
    }

    /// Set the post-speech hangover tail (in backend-sized frames) applied to
    /// subsequent frames. Detectors without a smoothing tail can ignore this.
    fn set_hangover_frames(&mut self, _frames: usize) {}

    /// End-of-recording diagnostic snapshot, taken after the final frame.
    /// Purely observational — implementations must not change what they emit.
    /// Detectors without smoothing state return None.
    fn tail_report(&self) -> Option<VadTailReport> {
        None
    }

    fn reset(&mut self) {}

    /// The speech probability of each frame of a whole clip, the last frame
    /// zero padded, from a fresh state (so a recording's detector is reset).
    /// Frame by frame unless the detector can score a clip in one go.
    fn score_clip(&mut self, pcm: &[f32]) -> Result<Vec<f32>> {
        self.reset();
        let frame_samples = self.frame_samples();
        let mut probs = Vec::with_capacity(pcm.len().div_ceil(frame_samples));
        let mut padded = vec![0.0; frame_samples];
        for frame in pcm.chunks(frame_samples) {
            let frame = if frame.len() == frame_samples {
                frame
            } else {
                padded[..frame.len()].copy_from_slice(frame);
                &padded
            };
            probs.push(self.push_frame(frame)?.frame_prob());
        }
        Ok(probs)
    }
}

/// End-of-recording snapshot of a smoothing detector's state. Voiced frames
/// in the withheld tail suggest — but don't prove — a final word cut off at
/// the stop; a clean report doesn't rule VAD loss out either (soft trailing
/// speech can be classified as noise).
#[derive(Debug, Clone, Copy)]
pub struct VadTailReport {
    /// Trailing frames buffered but never emitted downstream.
    pub withheld_frames: usize,
    /// How many of those withheld frames the inner VAD classified as voiced.
    pub withheld_voiced_frames: usize,
    pub in_speech: bool,
    /// Voiced frames counted toward an unconfirmed speech onset.
    pub onset_counter: usize,
    pub hangover_counter: usize,
}

mod earshot;
mod silero;
mod smoothed;

pub use earshot::EarshotVad;
pub use silero::SileroVad;
pub use smoothed::SmoothedVad;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_profiles_preserve_silero_timings() {
        assert_eq!(frames_for_duration_ms(VAD_PREFILL_MS, 512), 15);
        assert_eq!(frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, 512), 15);
        assert_eq!(frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, 512), 52);
        assert_eq!(frames_for_duration_ms(VAD_ONSET_MS, 512), 2);
    }

    /// Scores each frame with the sum of its samples, so padding shows.
    struct SummingVad;

    impl VoiceActivityDetector for SummingVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            assert_eq!(frame.len(), 4, "every frame is whole");
            Ok(VadFrame::Noise {
                prob: frame.iter().sum(),
            })
        }

        fn frame_samples(&self) -> usize {
            4
        }
    }

    #[test]
    fn score_clip_scores_every_frame_padding_the_last() {
        let pcm = [1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0];
        assert_eq!(SummingVad.score_clip(&pcm).unwrap(), vec![4.0, 8.0, 6.0]);
        assert!(SummingVad.score_clip(&[]).unwrap().is_empty());
    }

    #[test]
    fn score_clip_matches_a_detector_fed_frame_by_frame() {
        // Earshot needs no model file, so the real detector runs here.
        let pcm: Vec<f32> = (0..EarshotVad::new(0.5).unwrap().frame_samples() * 6)
            .map(|i| (i as f32 * 0.05).sin() * 0.3)
            .collect();
        let probs = EarshotVad::new(0.5).unwrap().score_clip(&pcm).unwrap();
        let mut fed = EarshotVad::new(0.5).unwrap();
        let expected: Vec<f32> = pcm
            .chunks(fed.frame_samples())
            .map(|frame| fed.push_frame(frame).unwrap().frame_prob())
            .collect();
        assert_eq!(probs, expected);
    }

    #[test]
    fn duration_profiles_round_up_for_earshot_frames() {
        assert_eq!(frames_for_duration_ms(VAD_PREFILL_MS, 256), 29);
        assert_eq!(frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, 256), 29);
        assert_eq!(frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, 256), 104);
        assert_eq!(frames_for_duration_ms(VAD_ONSET_MS, 256), 4);
    }
}
