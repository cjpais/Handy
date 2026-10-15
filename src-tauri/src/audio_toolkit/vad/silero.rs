use anyhow::Result;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};

use transcribe_cpp::{Backend, BackendMask, Model, ModelOptions, VadSession, VadSessionOptions};

use super::{check_whole_frames, SpeechScorer, VadFrame, VoiceActivityDetector};
use crate::audio_toolkit::constants;

pub struct SileroVad {
    // `VadSession` is Send but not Sync; the trait requires both. Only ever
    // reached through `get_mut`, so it is never locked and never poisoned.
    session: Mutex<VadSession>,
    frame_samples: usize,
    threshold: f32,
}

/// Register transcribe.cpp's CPU backends once per process. The allowed mask
/// is fixed at first registration, so it must never include a GPU here.
fn init_cpu_backends() -> Result<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        // Route native/ggml diagnostics into Handy's log; must precede any
        // native call.
        transcribe_cpp::init_logging();
        transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
            .map_err(|e| e.to_string())
    })
    .clone()
    .map_err(|e| anyhow::anyhow!("Failed to initialize transcribe.cpp CPU backend: {e}"))
}

impl SileroVad {
    pub fn new<P: AsRef<Path>>(model_path: P, threshold: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&threshold) {
            anyhow::bail!("threshold must be between 0.0 and 1.0");
        }

        init_cpu_backends()?;
        let model = Model::load_with(
            model_path.as_ref(),
            &ModelOptions {
                backend: Backend::Cpu,
                device: None,
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to load VAD model: {e}"))?;

        let info = model
            .vad_info()
            .map_err(|e| anyhow::anyhow!("Model does not support VAD: {e}"))?;
        if info.sample_rate != constants::WHISPER_SAMPLE_RATE as i32 || info.frame_samples <= 0 {
            anyhow::bail!(
                "unsupported VAD model: {} Hz, {} samples per frame",
                info.sample_rate,
                info.frame_samples
            );
        }

        // The session keeps the native model alive on its own.
        let session = model
            .vad_session_with(&VadSessionOptions { n_threads: 1 })
            .map_err(|e| anyhow::anyhow!("Failed to create VAD session: {e}"))?;

        Ok(Self {
            session: Mutex::new(session),
            frame_samples: info.frame_samples as usize,
            threshold,
        })
    }
}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        if frame.len() != self.frame_samples {
            anyhow::bail!(
                "expected {} samples, got {}",
                self.frame_samples,
                frame.len()
            );
        }
        if self.score(frame)?[0] > self.threshold {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    fn reset(&mut self) {
        // Clear the Silero LSTM state and frame context so a new session
        // doesn't inherit recurrent context from the previous recording.
        self.session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .stream_reset();
    }
}

impl SpeechScorer for SileroVad {
    fn score(&mut self, frames: &[f32]) -> Result<Vec<f32>> {
        check_whole_frames(frames, self.frame_samples)?;
        let result = self
            .session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .stream_feed(frames)
            .map_err(|e| anyhow::anyhow!("Silero VAD error: {e}"))?;

        // Whole frames in must score exactly that many frames; anything else
        // means the stream is out of step with the caller's framing.
        let expected = frames.len() / self.frame_samples;
        if result.probs.len() != expected {
            anyhow::bail!(
                "Silero VAD returned {} probabilities for {expected} frames",
                result.probs.len()
            );
        }
        Ok(result.probs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the bundled model (see AGENTS.md); run with `--ignored`.
    #[test]
    #[ignore]
    fn scores_the_same_in_pieces_and_frame_by_frame() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/models/silero-vad-v6.2-F32.gguf"
        );
        let mut vad = SileroVad::new(path, 0.3).unwrap();
        let frame = vad.frame_samples();
        let pcm: Vec<f32> = (0..frame * 600)
            .map(|i| (i as f32 * 0.013).sin() * (i as f32 * 0.0007).cos() * 0.2)
            .collect();
        let whole = vad.score(&pcm).unwrap();
        assert_eq!(whole.len(), 600);

        let mut pieces = SileroVad::new(path, 0.3).unwrap();
        let mut scored = pieces.score(&pcm[..frame * 250]).unwrap();
        scored.extend(pieces.score(&pcm[frame * 250..]).unwrap());
        assert_eq!(scored, whole);

        let mut by_frame = SileroVad::new(path, 0.3).unwrap();
        let fed: Vec<f32> = pcm
            .chunks(frame)
            .flat_map(|chunk| by_frame.score(chunk).unwrap())
            .collect();
        assert_eq!(fed, whole);

        assert!(vad.score(&pcm[..frame + 1]).is_err());
    }
}
