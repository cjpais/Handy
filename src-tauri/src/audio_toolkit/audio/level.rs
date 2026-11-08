//! Whether a take carried any signal at all, judged on the raw capture
//! before VAD. A device that delivers only (near-)zero samples, such as an
//! output monitor picked as the default source or a muted input, looks the
//! same downstream as speech the model didn't understand.

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;

/// At or below this peak the take holds no usable signal. Room noise on a
/// working microphone sits well above it.
pub const SILENT_PEAK_DBFS: f32 = -60.0;

/// Shorter takes are accidental taps, handled elsewhere; they say nothing
/// about the microphone.
pub const MIN_SILENT_TAKE_SAMPLES: usize = WHISPER_SAMPLE_RATE as usize;

/// Peak level of a take, fed chunk by chunk.
#[derive(Clone, Copy, Debug, Default)]
pub struct TakeLevel {
    peak: f32,
    samples: usize,
}

impl TakeLevel {
    pub fn feed(&mut self, samples: &[f32]) {
        self.peak = samples
            .iter()
            .fold(self.peak, |peak, sample| peak.max(sample.abs()));
        self.samples += samples.len();
    }

    /// Peak in dBFS; negative infinity when every sample was zero.
    pub fn peak_dbfs(&self) -> f32 {
        20.0 * self.peak.log10()
    }

    pub fn seconds(&self) -> f32 {
        self.samples as f32 / WHISPER_SAMPLE_RATE as f32
    }

    /// At least a second long and never louder than [`SILENT_PEAK_DBFS`].
    pub fn is_silent(&self) -> bool {
        self.samples >= MIN_SILENT_TAKE_SAMPLES && self.peak_dbfs() <= SILENT_PEAK_DBFS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_of(samples: &[f32]) -> TakeLevel {
        let mut level = TakeLevel::default();
        level.feed(samples);
        level
    }

    /// A 440 Hz tone with the given peak.
    fn tone(dbfs: f32, seconds: f32) -> Vec<f32> {
        let amplitude = 10f32.powf(dbfs / 20.0);
        let n = (seconds * WHISPER_SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / WHISPER_SAMPLE_RATE as f32;
                amplitude * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn all_zero_take_is_silent() {
        assert!(level_of(&vec![0.0; 20_000]).is_silent());
    }

    #[test]
    fn noise_at_minus_70_dbfs_is_silent() {
        // Alternating signs, like low-level hiss.
        let amplitude = 10f32.powf(-70.0 / 20.0);
        let noise: Vec<f32> = (0..20_000)
            .map(|i| {
                if i % 3 == 0 {
                    amplitude
                } else {
                    -amplitude * 0.5
                }
            })
            .collect();
        assert!(level_of(&noise).is_silent());
    }

    #[test]
    fn tone_at_minus_40_dbfs_is_not_silent() {
        assert!(!level_of(&tone(-40.0, 1.5)).is_silent());
    }

    #[test]
    fn half_second_of_zeros_is_not_flagged() {
        assert!(!level_of(&vec![0.0; 8_000]).is_silent());
    }

    #[test]
    fn empty_take_is_not_flagged() {
        assert!(!level_of(&[]).is_silent());
    }

    #[test]
    fn chunks_add_up() {
        let mut level = TakeLevel::default();
        level.feed(&[0.0; 8_000]);
        level.feed(&[0.0; 8_000]);
        assert!(level.is_silent());
        level.feed(&[0.5]);
        assert!(!level.is_silent());
    }
}
