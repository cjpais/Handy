use super::{VadFrame, VadTailReport, VoiceActivityDetector};
use anyhow::Result;
use std::collections::VecDeque;

/// One pre-roll buffer slot. `emitted` and `voiced` exist only to power the
/// end-of-recording `tail_report()` diagnostic; they never affect emission.
struct BufferedFrame {
    samples: Vec<f32>,
    /// The inner VAD's speech probability, passed on with the samples.
    prob: f32,
    emitted: bool,
    voiced: bool,
}

pub struct SmoothedVad {
    inner_vad: Box<dyn VoiceActivityDetector>,
    prefill_frames: usize,
    hangover_frames: usize,
    onset_frames: usize,

    frame_buffer: VecDeque<BufferedFrame>,
    hangover_counter: usize,
    onset_counter: usize,
    in_speech: bool,

    temp_out: Vec<f32>,
    temp_probs: Vec<f32>,
}

impl SmoothedVad {
    pub fn new(
        inner_vad: Box<dyn VoiceActivityDetector>,
        prefill_frames: usize,
        hangover_frames: usize,
        onset_frames: usize,
    ) -> Self {
        Self {
            inner_vad,
            prefill_frames,
            hangover_frames,
            onset_frames,
            frame_buffer: VecDeque::new(),
            hangover_counter: 0,
            onset_counter: 0,
            in_speech: false,
            temp_out: Vec::new(),
            temp_probs: Vec::new(),
        }
    }

    fn mark_last_emitted(&mut self) {
        if let Some(frame) = self.frame_buffer.back_mut() {
            frame.emitted = true;
        }
    }

    /// The current frame, kept as speech on its own.
    fn emit_current<'a>(&'a mut self, frame: &'a [f32]) -> VadFrame<'a> {
        self.mark_last_emitted();
        let prob = &self
            .frame_buffer
            .back()
            .expect("the current frame is buffered")
            .prob;
        VadFrame::Speech {
            samples: frame,
            probs: std::slice::from_ref(prob),
        }
    }
}

impl VoiceActivityDetector for SmoothedVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        // 1. Delegate to the wrapped VAD
        let (is_voice, prob) = {
            let decision = self.inner_vad.push_frame(frame)?;
            (decision.is_speech(), decision.frame_prob())
        };

        // 2. Buffer every incoming frame for possible pre-roll
        self.frame_buffer.push_back(BufferedFrame {
            samples: frame.to_vec(),
            prob,
            emitted: false,
            voiced: is_voice,
        });
        while self.frame_buffer.len() > self.prefill_frames + 1 {
            self.frame_buffer.pop_front();
        }

        match (self.in_speech, is_voice) {
            // Potential start of speech - need to accumulate onset frames
            (false, true) => {
                self.onset_counter += 1;
                if self.onset_counter >= self.onset_frames {
                    // We have enough consecutive voice frames to trigger speech
                    self.in_speech = true;
                    self.hangover_counter = self.hangover_frames;
                    self.onset_counter = 0; // Reset for next time

                    // Collect prefill + current frame
                    self.temp_out.clear();
                    self.temp_probs.clear();
                    for buffered in self.frame_buffer.iter_mut().filter(|f| !f.emitted) {
                        self.temp_out.extend(buffered.samples.iter());
                        self.temp_probs.push(buffered.prob);
                        buffered.emitted = true;
                    }
                    Ok(VadFrame::Speech {
                        samples: &self.temp_out,
                        probs: &self.temp_probs,
                    })
                } else {
                    // Not enough frames yet, still silence
                    Ok(VadFrame::Noise { prob })
                }
            }

            // Ongoing Speech
            (true, true) => {
                self.hangover_counter = self.hangover_frames;
                Ok(self.emit_current(frame))
            }

            // End of Speech or interruption during onset phase
            (true, false) => {
                if self.hangover_counter > 0 {
                    self.hangover_counter -= 1;
                    Ok(self.emit_current(frame))
                } else {
                    self.in_speech = false;
                    Ok(VadFrame::Noise { prob })
                }
            }

            // Silence or broken onset sequence
            (false, false) => {
                self.onset_counter = 0; // Reset onset counter on silence
                Ok(VadFrame::Noise { prob })
            }
        }
    }

    fn frame_samples(&self) -> usize {
        self.inner_vad.frame_samples()
    }

    fn set_hangover_frames(&mut self, frames: usize) {
        self.hangover_frames = frames;
    }

    /// Trailing run of withheld frames plus smoothing state. Interior
    /// withheld frames (before already-emitted speech) are not counted.
    fn tail_report(&self) -> Option<VadTailReport> {
        let mut withheld_frames = 0;
        let mut withheld_voiced_frames = 0;
        for frame in self
            .frame_buffer
            .iter()
            .rev()
            .take_while(|frame| !frame.emitted)
        {
            withheld_frames += 1;
            if frame.voiced {
                withheld_voiced_frames += 1;
            }
        }

        Some(VadTailReport {
            withheld_frames,
            withheld_voiced_frames,
            in_speech: self.in_speech,
            onset_counter: self.onset_counter,
            hangover_counter: self.hangover_counter,
        })
    }

    fn reset(&mut self) {
        self.inner_vad.reset();
        self.frame_buffer.clear();
        self.hangover_counter = 0;
        self.onset_counter = 0;
        self.in_speech = false;
        self.temp_out.clear();
        self.temp_probs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inner VAD that replays a scripted voice/no-voice sequence.
    struct ScriptedVad {
        script: VecDeque<bool>,
    }

    impl ScriptedVad {
        fn new(script: &[bool]) -> Self {
            Self {
                script: script.iter().copied().collect(),
            }
        }
    }

    impl VoiceActivityDetector for ScriptedVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            if self.script.pop_front().unwrap_or(false) {
                Ok(VadFrame::Speech {
                    samples: frame,
                    probs: &[0.9],
                })
            } else {
                Ok(VadFrame::Noise { prob: 0.1 })
            }
        }

        fn frame_samples(&self) -> usize {
            4
        }
    }

    fn frame(value: f32) -> Vec<f32> {
        vec![value; 4]
    }

    fn smoothed(script: &[bool], onset_frames: usize) -> SmoothedVad {
        SmoothedVad::new(Box::new(ScriptedVad::new(script)), 3, 2, onset_frames)
    }

    #[test]
    fn tail_report_counts_withheld_onset_tail() {
        // One voiced frame at the end: onset (2 frames) never confirms, so
        // both trailing frames are withheld and one of them is voiced —
        // consistent with a final word cut off at the stop boundary.
        let mut vad = smoothed(&[false, true], 2);
        assert!(!vad.push_frame(&frame(0.1)).unwrap().is_speech());
        assert!(!vad.push_frame(&frame(0.9)).unwrap().is_speech());

        let report = vad.tail_report().expect("smoothed VAD always reports");
        assert_eq!(report.withheld_frames, 2);
        assert_eq!(report.withheld_voiced_frames, 1);
        assert_eq!(report.onset_counter, 1);
        assert!(!report.in_speech);
    }

    #[test]
    fn tail_report_counts_only_trailing_run() {
        // Speech emitted through hangover, then silence past the hangover is
        // withheld. Only the trailing run counts — frames older than an
        // emitted frame are excluded.
        let mut vad = smoothed(&[true, true, false, false, false, false], 2);
        for v in [0.1, 0.2, 0.3, 0.4] {
            let _ = vad.push_frame(&frame(v)).unwrap();
        }
        assert!(!vad.push_frame(&frame(0.5)).unwrap().is_speech()); // past hangover
        assert!(!vad.push_frame(&frame(0.6)).unwrap().is_speech());

        let report = vad.tail_report().unwrap();
        assert_eq!(report.withheld_frames, 2);
        assert_eq!(report.withheld_voiced_frames, 0);
    }

    #[test]
    fn speech_carries_one_probability_per_frame() {
        // The onset releases the withheld pre-roll with each frame's own
        // probability; a hangover frame carries its (low) probability too.
        let mut vad = smoothed(&[false, true, true, false], 2);
        assert!(matches!(
            vad.push_frame(&frame(0.1)).unwrap(),
            VadFrame::Noise { prob } if prob == 0.1
        ));
        assert!(!vad.push_frame(&frame(0.2)).unwrap().is_speech());
        match vad.push_frame(&frame(0.3)).unwrap() {
            VadFrame::Speech { samples, probs } => {
                assert_eq!(samples.len(), 3 * 4);
                assert_eq!(probs, &[0.1, 0.9, 0.9]);
            }
            VadFrame::Noise { .. } => panic!("onset should release speech"),
        }
        match vad.push_frame(&frame(0.4)).unwrap() {
            VadFrame::Speech { samples, probs } => {
                assert_eq!(samples, &frame(0.4)[..]);
                assert_eq!(probs, &[0.1]);
            }
            VadFrame::Noise { .. } => panic!("hangover should keep the frame"),
        }
    }
}
