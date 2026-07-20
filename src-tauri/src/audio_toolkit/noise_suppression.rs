// Noise suppression module using nnnoiseless (pure Rust RNNoise implementation).
//
// Purpose: Pre-VAD noise suppression for improved speech detection accuracy
// in noisy environments. Operates on 16kHz audio in 480-sample (30ms) frames,
// which aligns with the VAD frame size.
//
// The nnnoiseless library is a Rust port of Xiph's RNNoise neural network
// denoiser. It processes audio frame-by-frame and applies a learned filter
// that reduces stationary and non-stationary noise while preserving speech.
//
// Scope: This module wraps the library for optional use in the audio pipeline.
// Noise suppression is computationally expensive (~5-10% CPU), so it's disabled
// by default and configurable via settings.
//
// Dependencies: nnnoiseless crate (pure Rust, no C dependencies)
// Side effects: None — stateless outside the DenoiseState buffer

use nnnoiseless::DenoiseState;

use crate::settings::NoiseSuppressionLevel;

/// Frame size for nnnoiseless processing.
/// At 16kHz sample rate, 480 samples = 30ms, matching the VAD frame size.
pub const NOISE_SUPPRESSION_FRAME_SIZE: usize = DenoiseState::FRAME_SIZE;

/// Noise suppression level configuration.
/// Maps to gain thresholds that control how aggressively noise is suppressed.
impl NoiseSuppressionLevel {
    /// Returns the minimum gain floor for the noise suppressor.
    /// Higher values = less aggressive suppression (more noise preserved),
    /// lower values = more aggressive suppression (more noise removed, possibly
    /// some speech artifacts).
    pub fn gain_floor(&self) -> f32 {
        match self {
            // Low: subtle suppression, minimal artifacts
            NoiseSuppressionLevel::Low => 0.3,
            // Medium: balanced suppression (RNNoise default range)
            NoiseSuppressionLevel::Medium => 0.15,
            // High: aggressive suppression, may introduce slight artifacts
            NoiseSuppressionLevel::High => 0.05,
        }
    }
}

/// Wraps the nnnoiseless denoiser for use in the audio pipeline.
pub struct NoiseSuppressor {
    denoise_state: Box<DenoiseState<'static>>,
    level: NoiseSuppressionLevel,
    /// Whether the first frame has been processed (first output should be discarded
    /// due to fade-in artifacts from the neural network).
    first_frame: bool,
    /// Reusable output buffer to avoid per-frame allocation.
    output_buf: [f32; NOISE_SUPPRESSION_FRAME_SIZE],
}

impl NoiseSuppressor {
    /// Create a new noise suppressor with the given suppression level.
    pub fn new(level: NoiseSuppressionLevel) -> Self {
        Self {
            denoise_state: DenoiseState::new(),
            level,
            first_frame: true,
            output_buf: [0.0f32; NOISE_SUPPRESSION_FRAME_SIZE],
        }
    }

    /// Process a single frame of audio samples through the noise suppressor.
    ///
    /// The input must be exactly `NOISE_SUPPRESSION_FRAME_SIZE` (480) samples.
    /// Returns the denoised samples. The first call returns an empty Vec
    /// (discarded due to fade-in artifacts); subsequent calls return 480 samples.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        if samples.len() != NOISE_SUPPRESSION_FRAME_SIZE {
            log::warn!(
                "Noise suppression: expected {} samples, got {}. Skipping.",
                NOISE_SUPPRESSION_FRAME_SIZE,
                samples.len()
            );
            return samples.to_vec();
        }

        // Scale from [-1.0, 1.0] normalized to i16 range that nnnoiseless expects
        let i16_scale: f32 = 32768.0;
        let mut input_scaled = [0.0f32; NOISE_SUPPRESSION_FRAME_SIZE];
        for (i, &s) in samples.iter().enumerate() {
            input_scaled[i] = s * i16_scale;
        }

        // Process through the denoiser
        let _vad_prob = self
            .denoise_state
            .process_frame(&mut self.output_buf[..], &input_scaled[..]);

        // Discard first frame output (fade-in artifacts)
        if self.first_frame {
            self.first_frame = false;
            // Return the original samples for the first frame
            return samples.to_vec();
        }

        // Scale back from i16 range to normalized [-1.0, 1.0]
        let gain_floor = self.level.gain_floor();
        let mut result = Vec::with_capacity(NOISE_SUPPRESSION_FRAME_SIZE);
        for (i, &s) in self.output_buf.iter().enumerate() {
            let denoised = s / i16_scale;
            // Apply gain floor to prevent over-suppression
            let original = samples[i];
            let gain = if original.abs() > 1e-6 {
                (denoised / original).abs()
            } else {
                1.0
            };
            let clamped_gain = gain.max(gain_floor);
            result.push(original * clamped_gain);
        }

        result
    }

    /// Process a frame and return the VAD probability from the denoiser.
    /// Returns (denoised_samples, vad_probability).
    pub fn process_with_vad(&mut self, samples: &[f32]) -> (Vec<f32>, f32) {
        if samples.len() != NOISE_SUPPRESSION_FRAME_SIZE {
            return (samples.to_vec(), 0.0);
        }

        let i16_scale: f32 = 32768.0;
        let mut input_scaled = [0.0f32; NOISE_SUPPRESSION_FRAME_SIZE];
        for (i, &s) in samples.iter().enumerate() {
            input_scaled[i] = s * i16_scale;
        }

        let vad_prob = self
            .denoise_state
            .process_frame(&mut self.output_buf[..], &input_scaled[..]);

        if self.first_frame {
            self.first_frame = false;
            return (samples.to_vec(), vad_prob);
        }

        let gain_floor = self.level.gain_floor();
        let mut result = Vec::with_capacity(NOISE_SUPPRESSION_FRAME_SIZE);
        for (i, &s) in self.output_buf.iter().enumerate() {
            let denoised = s / i16_scale;
            let original = samples[i];
            let gain = if original.abs() > 1e-6 {
                (denoised / original).abs()
            } else {
                1.0
            };
            let clamped_gain = gain.max(gain_floor);
            result.push(original * clamped_gain);
        }

        (result, vad_prob)
    }

    /// Reset the denoiser state. Should be called when starting a new recording.
    pub fn reset(&mut self) {
        self.denoise_state = DenoiseState::new();
        self.first_frame = true;
    }

    /// Update the noise suppression level.
    pub fn set_level(&mut self, level: NoiseSuppressionLevel) {
        self.level = level;
    }

    /// Get the current noise suppression level.
    pub fn level(&self) -> NoiseSuppressionLevel {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_suppressor_creation() {
        let ns = NoiseSuppressor::new(NoiseSuppressionLevel::Medium);
        assert!(ns.first_frame);
        assert_eq!(ns.level(), NoiseSuppressionLevel::Medium);
    }

    #[test]
    fn test_noise_suppressor_process() {
        let mut ns = NoiseSuppressor::new(NoiseSuppressionLevel::Low);
        let silence: Vec<f32> = vec![0.0; NOISE_SUPPRESSION_FRAME_SIZE];
        let result = ns.process(&silence);
        assert_eq!(result.len(), NOISE_SUPPRESSION_FRAME_SIZE);
        let result2 = ns.process(&silence);
        assert_eq!(result2.len(), NOISE_SUPPRESSION_FRAME_SIZE);
    }

    #[test]
    fn test_noise_suppressor_wrong_size() {
        let mut ns = NoiseSuppressor::new(NoiseSuppressionLevel::Medium);
        let short_frame = vec![0.0; 100];
        let result = ns.process(&short_frame);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_gain_floor_values() {
        assert!(
            NoiseSuppressionLevel::Low.gain_floor() > NoiseSuppressionLevel::Medium.gain_floor()
        );
        assert!(
            NoiseSuppressionLevel::Medium.gain_floor() > NoiseSuppressionLevel::High.gain_floor()
        );
    }
}