// CI-only mock VAD adapter - avoids transcribe-rs dependency.
// This file is copied over adapter.rs during CI tests.

use anyhow::Result;
use std::path::Path;

use super::{VadFrame, VoiceActivityDetector};

/// Mock SileroVad that always reports noise.
pub struct SileroVad;

impl SileroVad {
    pub fn new<P: AsRef<Path>>(_model_path: P, _threshold: f32) -> Result<Self> {
        Ok(Self)
    }
}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, _frame: &'a [f32]) -> Result<VadFrame<'a>> {
        Ok(VadFrame::Noise)
    }

    fn reset(&mut self) {}
}

/// Mock SmoothedVad that always reports noise.
pub struct SmoothedVad;

impl SmoothedVad {
    pub fn new(
        _inner_vad: Box<dyn VoiceActivityDetector>,
        _prefill_frames: usize,
        _hangover_frames: usize,
        _onset_frames: usize,
    ) -> Self {
        Self
    }
}

impl VoiceActivityDetector for SmoothedVad {
    fn push_frame<'a>(&'a mut self, _frame: &'a [f32]) -> Result<VadFrame<'a>> {
        Ok(VadFrame::Noise)
    }

    fn reset(&mut self) {}
}
