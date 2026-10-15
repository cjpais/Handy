use super::{AudioFrameCallback, CaptureSink, CaptureVad, FrameScores, LevelCallback, VadPolicy};
use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
use handy_recorder::{AudioChunk, Sink};
use std::sync::{mpsc, Arc, Mutex};

/// Keeps frames whose first sample is non-zero, scoring each frame with its
/// first sample, and records how the sink configured it.
#[derive(Default)]
struct ScriptedVad {
    frame_samples: usize,
    hangover: Arc<Mutex<Option<usize>>>,
    resets: Arc<Mutex<usize>>,
}

impl VoiceActivityDetector for ScriptedVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
        Ok(if frame[0] != 0.0 {
            VadFrame::Speech {
                samples: frame,
                probs: std::slice::from_ref(&frame[0]),
            }
        } else {
            VadFrame::Noise { prob: frame[0] }
        })
    }

    fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    fn set_hangover_frames(&mut self, frames: usize) {
        *self.hangover.lock().unwrap() = Some(frames);
    }

    fn reset(&mut self) {
        *self.resets.lock().unwrap() += 1;
    }
}

const FRAME: usize = 4;

/// One delivery's scores and frame size; `None` when unscored.
type Delivered = Option<(Vec<f32>, usize)>;

struct Harness {
    sink: CaptureSink,
    ready: mpsc::Receiver<()>,
    fed: Arc<Mutex<Vec<f32>>>,
    scores: Arc<Mutex<Vec<Delivered>>>,
    hangover: Arc<Mutex<Option<usize>>>,
    resets: Arc<Mutex<usize>>,
}

fn harness(policy: VadPolicy) -> Harness {
    let vad = ScriptedVad {
        frame_samples: FRAME,
        ..Default::default()
    };
    let hangover = Arc::clone(&vad.hangover);
    let resets = Arc::clone(&vad.resets);
    let fed = Arc::new(Mutex::new(Vec::new()));
    let scores = Arc::new(Mutex::new(Vec::new()));
    let on_audio: AudioFrameCallback = {
        let fed = Arc::clone(&fed);
        let scores = Arc::clone(&scores);
        Arc::new(
            move |frame: &[f32], frame_scores: Option<FrameScores<'_>>| {
                fed.lock().unwrap().extend_from_slice(frame);
                scores
                    .lock()
                    .unwrap()
                    .push(frame_scores.map(|s| (s.probs.to_vec(), s.frame_samples)));
            },
        )
    };
    let on_levels: LevelCallback = Arc::new(|_| {});
    let (ready_tx, ready) = mpsc::channel();
    let sink = CaptureSink::new(
        CaptureVad::new(Box::new(vad), 10, 20),
        policy,
        on_levels,
        on_audio,
        ready_tx,
    );
    Harness {
        sink,
        ready,
        fed,
        scores,
        hangover,
        resets,
    }
}

fn chunk(samples: &[f32], valid_frames: usize) -> AudioChunk<'_> {
    AudioChunk {
        samples,
        sample_rate: 16_000,
        channels: 1,
        valid_frames,
    }
}

#[test]
fn readiness_fires_once_on_the_first_chunk() {
    let mut h = harness(VadPolicy::Disabled);
    assert!(h.ready.try_recv().is_err());

    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));

    assert!(h.ready.try_recv().is_ok());
    // The sender is gone after the first chunk, so a second signal can't come.
    assert_eq!(h.ready.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn readiness_waiter_wakes_when_a_recording_ends_without_audio() {
    let h = harness(VadPolicy::Offline);
    let _ = h.sink.finish();
    assert!(h.ready.recv().is_err());
}

#[test]
fn disabled_vad_keeps_every_real_sample_and_drops_padding() {
    let mut h = harness(VadPolicy::Disabled);
    h.sink.process_chunk(chunk(&[1.0, 0.0, 2.0, 0.0], FRAME));
    // Final chunk: one real frame, three of zero padding.
    h.sink.process_chunk(chunk(&[3.0, 0.0, 0.0, 0.0], 1));

    let expected = vec![1.0, 0.0, 2.0, 0.0, 3.0];
    assert_eq!(*h.fed.lock().unwrap(), expected);
    // Every frame is still scored, the padded final one included.
    assert_eq!(
        *h.scores.lock().unwrap(),
        vec![Some((vec![1.0], FRAME)), Some((vec![3.0], FRAME))]
    );
    assert_eq!(h.sink.finish().samples, expected);
}

#[test]
fn disabled_vad_keeps_silence_with_its_score() {
    let mut h = harness(VadPolicy::Disabled);
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    assert_eq!(*h.fed.lock().unwrap(), vec![0.0; FRAME]);
    assert_eq!(*h.scores.lock().unwrap(), vec![Some((vec![0.0], FRAME))]);
}

#[test]
fn vad_keeps_speech_frames_only() {
    let mut h = harness(VadPolicy::Offline);
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[2.0; FRAME], FRAME));

    let expected = [[1.0; FRAME], [2.0; FRAME]].concat();
    assert_eq!(*h.fed.lock().unwrap(), expected);
    assert_eq!(
        *h.scores.lock().unwrap(),
        vec![Some((vec![1.0], FRAME)), Some((vec![2.0], FRAME))],
        "kept speech carries its scores"
    );
    assert_eq!(h.sink.finish().samples, expected);
}

#[test]
fn each_recording_resets_the_detector_with_its_policys_hangover() {
    let offline = harness(VadPolicy::Offline);
    assert_eq!(*offline.hangover.lock().unwrap(), Some(10));
    assert_eq!(*offline.resets.lock().unwrap(), 1);

    let streaming = harness(VadPolicy::Streaming);
    assert_eq!(*streaming.hangover.lock().unwrap(), Some(20));
    assert_eq!(*streaming.resets.lock().unwrap(), 1);

    // VAD off still scores, so it starts from a clean detector too.
    let disabled = harness(VadPolicy::Disabled);
    assert_eq!(*disabled.resets.lock().unwrap(), 1);
}

#[test]
fn finish_returns_the_detector_for_the_next_recording() {
    let h = harness(VadPolicy::Offline);
    let captured = h.sink.finish();
    assert_eq!(captured.vad.frame_samples(), FRAME);
    assert_eq!(captured.vad.hangover_for(VadPolicy::Streaming), 20);
}
