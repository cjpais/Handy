//! Adapts transcribe-rs VAD types to Handy's VoiceActivityDetector trait.

use anyhow::Result;
use std::path::Path;
use transcribe_rs::vad::Vad;

use super::{VadFrame, VoiceActivityDetector};

/// Wrapper around `transcribe_rs::vad::SileroVad`.
pub struct SileroVad(transcribe_rs::vad::SileroVad);

impl SileroVad {
    pub fn new<P: AsRef<Path>>(model_path: P, threshold: f32) -> Result<Self> {
        let inner = transcribe_rs::vad::SileroVad::new(model_path, threshold)
            .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {e}"))?;
        Ok(Self(inner))
    }
}

// transcribe-rs Vad is Send but not Sync. SileroVad is only accessed
// behind &mut (single-threaded in the recorder worker), so Sync is safe.
unsafe impl Sync for SileroVad {}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        let speech = self
            .0
            .is_speech(frame)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if speech {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}

/// Wrapper around `transcribe_rs::vad::SmoothedVad` that implements
/// Handy's `VoiceActivityDetector` trait with prefill-aware output.
pub struct SmoothedVad {
    inner: transcribe_rs::vad::SmoothedVad,
    temp_out: Vec<f32>,
}

// Same reasoning as SileroVad — only accessed behind &mut in recorder worker.
unsafe impl Sync for SmoothedVad {}

impl SmoothedVad {
    pub fn new(
        inner_vad: Box<dyn VoiceActivityDetector>,
        prefill_frames: usize,
        hangover_frames: usize,
        onset_frames: usize,
    ) -> Self {
        let adapted = VadAdapter(inner_vad);
        let inner = transcribe_rs::vad::SmoothedVad::new(
            Box::new(adapted),
            prefill_frames,
            hangover_frames,
            onset_frames,
        );
        Self {
            inner,
            temp_out: Vec::new(),
        }
    }
}

impl VoiceActivityDetector for SmoothedVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        let speech = self
            .inner
            .is_speech(frame)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if speech {
            let prefill = self.inner.drain_prefill();
            if !prefill.is_empty() {
                self.temp_out.clear();
                self.temp_out.extend_from_slice(&prefill);
                self.temp_out.extend_from_slice(frame);
                Ok(VadFrame::Speech(&self.temp_out))
            } else {
                Ok(VadFrame::Speech(frame))
            }
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.temp_out.clear();
    }
}

/// Adapts a Handy `VoiceActivityDetector` to the transcribe-rs `Vad` trait.
struct VadAdapter(Box<dyn VoiceActivityDetector>);

unsafe impl Send for VadAdapter {}

impl Vad for VadAdapter {
    fn frame_size(&self) -> usize {
        480 // 30ms at 16kHz
    }

    fn is_speech(
        &mut self,
        frame: &[f32],
    ) -> std::result::Result<bool, transcribe_rs::TranscribeError> {
        self.0
            .is_voice(frame)
            .map_err(|e| transcribe_rs::TranscribeError::Inference(e.to_string()))
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}
