use anyhow::Result;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};

use transcribe_cpp::{Backend, BackendMask, Model, ModelOptions, VadSession, VadSessionOptions};

use super::{VadFrame, VoiceActivityDetector};
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

        let result = self
            .session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .stream_feed(frame)
            .map_err(|e| anyhow::anyhow!("Silero VAD error: {e}"))?;

        // A whole frame in must score exactly one frame; anything else means
        // the stream is out of step with the recorder's framing.
        let [prob] = result.probs[..] else {
            anyhow::bail!(
                "Silero VAD returned {} probabilities for one frame",
                result.probs.len()
            );
        };

        if prob > self.threshold {
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
