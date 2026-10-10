use std::sync::{mpsc, Arc};

use handy_recorder::{AudioChunk, Sink};

use crate::audio_toolkit::{
    audio::AudioVisualiser,
    constants,
    vad::{VadFrame, VoiceActivityDetector},
};

/// How 16 kHz mono frames should be filtered for one recording session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadPolicy {
    /// Bypass VAD and forward every frame.
    Disabled,
    /// Current offline-tuned VAD profile.
    Offline,
    /// VAD profile with a longer post-speech tail for streaming-capable models.
    Streaming,
}

/// A single VAD engine plus the two hangover-tail lengths its smoothing wrapper
/// should use. The offline and streaming policies are never active
/// concurrently, so one detector is reconfigured per recording rather than
/// kept as two resident engines.
pub struct CaptureVad {
    detector: Box<dyn VoiceActivityDetector>,
    offline_hangover_frames: usize,
    streaming_hangover_frames: usize,
}

impl CaptureVad {
    pub fn new(
        detector: Box<dyn VoiceActivityDetector>,
        offline_hangover_frames: usize,
        streaming_hangover_frames: usize,
    ) -> Self {
        assert!(
            detector.frame_samples() > 0,
            "VAD frame size must be non-zero"
        );
        Self {
            detector,
            offline_hangover_frames,
            streaming_hangover_frames,
        }
    }

    /// Samples per detector frame; the recorder delivers chunks of this size.
    pub fn frame_samples(&self) -> usize {
        self.detector.frame_samples()
    }

    /// Post-speech hangover tail (in detector frames) for the given policy.
    /// `Disabled` never reaches the detector, so it maps to the offline value.
    fn hangover_for(&self, policy: VadPolicy) -> usize {
        match policy {
            VadPolicy::Streaming => self.streaming_hangover_frames,
            VadPolicy::Offline | VadPolicy::Disabled => self.offline_hangover_frames,
        }
    }
}

/// Receives spectrum levels for the recording overlay.
pub type LevelCallback = Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>;
/// Receives each 16 kHz mono frame that passes the recording's VAD policy, in
/// order. Used to feed live streaming transcription as audio arrives.
pub type AudioFrameCallback = Arc<dyn Fn(&[f32]) + Send + Sync + 'static>;

/// What a finished recording hands back.
pub struct CapturedAudio {
    /// 16 kHz mono samples that passed the VAD policy.
    pub samples: Vec<f32>,
    /// The detector the recording borrowed, for the next one.
    pub vad: CaptureVad,
}

/// One recording's consumer of 16 kHz mono audio from handy-recorder. Runs on
/// the recorder's delivery thread, never the real-time audio thread, so it
/// can run VAD inline. Chunks are exactly one VAD frame long.
pub struct CaptureSink {
    vad: CaptureVad,
    policy: VadPolicy,
    visualizer: AudioVisualiser,
    on_levels: LevelCallback,
    on_audio: AudioFrameCallback,
    /// Signalled on the first chunk: the device is delivering audio. Silence
    /// counts; readiness is not speech.
    ready: Option<mpsc::Sender<()>>,
    samples: Vec<f32>,
}

/// Spectrum window at 16 kHz: 32 ms, near the overlay's ~30 Hz refresh.
const VISUALIZER_WINDOW: usize = 512;
const VISUALIZER_BUCKETS: usize = 16;

impl CaptureSink {
    /// A sink for one recording. Resets the detector and applies the policy's
    /// hangover tail, so no state carries over from the previous recording.
    pub fn new(
        mut vad: CaptureVad,
        policy: VadPolicy,
        on_levels: LevelCallback,
        on_audio: AudioFrameCallback,
        ready: mpsc::Sender<()>,
    ) -> Self {
        if policy != VadPolicy::Disabled {
            let hangover = vad.hangover_for(policy);
            vad.detector.set_hangover_frames(hangover);
            vad.detector.reset();
        }
        Self {
            vad,
            policy,
            visualizer: AudioVisualiser::new(
                constants::WHISPER_SAMPLE_RATE,
                VISUALIZER_WINDOW,
                VISUALIZER_BUCKETS,
                400.0,
                4000.0,
            ),
            on_levels,
            on_audio,
            ready: Some(ready),
            samples: Vec::new(),
        }
    }

    /// The detector, from a sink that never recorded (its start failed).
    pub fn into_vad(self) -> CaptureVad {
        self.vad
    }

    /// Hands back the recording and the detector. Logs what VAD still held
    /// back when the recording stopped.
    pub fn finish(self) -> CapturedAudio {
        // Diagnostic for audio still withheld when capture stopped; it is not
        // conclusive in either direction.
        if self.policy != VadPolicy::Disabled {
            if let Some(report) = self.vad.detector.tail_report() {
                log::debug!(
                    "VAD at stop: withheld tail {} frames (~{}ms, {} voiced), in_speech={}, onset_counter={}, hangover_counter={}",
                    report.withheld_frames,
                    report.withheld_frames * self.vad.frame_samples() * 1000
                        / constants::WHISPER_SAMPLE_RATE as usize,
                    report.withheld_voiced_frames,
                    report.in_speech,
                    report.onset_counter,
                    report.hangover_counter
                );
            }
        }
        CapturedAudio {
            samples: self.samples,
            vad: self.vad,
        }
    }

    fn emit(samples: &mut Vec<f32>, on_audio: &AudioFrameCallback, frame: &[f32]) {
        samples.extend_from_slice(frame);
        on_audio(frame);
    }
}

impl Sink for CaptureSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(());
        }

        // Mono by construction: the recorder mixes down or picks one channel.
        let real = &chunk.samples[..chunk.valid_frames];
        if let Some(levels) = self.visualizer.feed(real) {
            (self.on_levels)(levels);
        }

        if self.policy == VadPolicy::Disabled {
            Self::emit(&mut self.samples, &self.on_audio, real);
            return;
        }

        // The detector needs whole frames, so the final chunk goes in with its
        // zero padding; at most one frame of silence reaches the recording.
        match self
            .vad
            .detector
            .push_frame(chunk.samples)
            .unwrap_or(VadFrame::Speech(chunk.samples))
        {
            VadFrame::Speech(frame) => Self::emit(&mut self.samples, &self.on_audio, frame),
            VadFrame::Noise => {}
        }
    }
}

#[cfg(test)]
mod tests;
