use anyhow::Result;
use log::info;
use std::path::Path;

use vad_rs::Vad;

use super::{VadFrame, VoiceActivityDetector};
use crate::audio_toolkit::constants;

const SILERO_FRAME_MS: u32 = 30;
const SILERO_FRAME_SAMPLES: usize =
    (constants::WHISPER_SAMPLE_RATE * SILERO_FRAME_MS / 1000) as usize;

pub struct SileroVad {
    engine: Vad,
    threshold: f32,
}

impl SileroVad {
    pub fn new<P: AsRef<Path>>(model_path: P, threshold: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&threshold) {
            anyhow::bail!("threshold must be between 0.0 and 1.0");
        }

        let path = model_path.as_ref();
        info!(
            "SileroVad: loading model from '{}' (threshold={})",
            path.display(),
            threshold
        );

        let metadata = std::fs::metadata(path)
            .map_err(|e| anyhow::anyhow!("Failed to stat VAD model '{}': {}", path.display(), e))?;
        let file_size_mb = metadata.len() as f64 / 1_048_576.0;
        info!(
            "SileroVad: model file size = {:.2} MB",
            file_size_mb
        );

        let engine = Vad::new(path, constants::WHISPER_SAMPLE_RATE as usize)
            .map_err(|e| anyhow::anyhow!("Failed to create VAD: {e}"))?;

        info!("SileroVad: model loaded and ready");

        Ok(Self {
            engine,
            threshold,
        })
    }
}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        if frame.len() != SILERO_FRAME_SAMPLES {
            anyhow::bail!(
                "expected {SILERO_FRAME_SAMPLES} samples, got {}",
                frame.len()
            );
        }

        let result = self
            .engine
            .compute(frame)
            .map_err(|e| anyhow::anyhow!("Silero VAD error: {e}"))?;

        if result.prob > self.threshold {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn reset(&mut self) {
        // Clear the Silero LSTM hidden/cell state so a new session doesn't
        // inherit recurrent context from the previous recording.
        self.engine.reset();
    }
}
