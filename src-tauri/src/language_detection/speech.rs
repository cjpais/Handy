//! The speech the language detector hears: silence cut out by VAD, so a
//! window of N seconds is N seconds of speech.

use anyhow::Result;

use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};

/// The first `max_samples` of speech in `audio`, as `vad` keeps it.
pub fn speech_head(
    vad: &mut dyn VoiceActivityDetector,
    audio: &[f32],
    max_samples: usize,
) -> Result<Vec<f32>> {
    let mut speech = Vec::with_capacity(max_samples.min(audio.len()));
    for frame in audio.chunks_exact(vad.frame_samples()) {
        if let VadFrame::Speech(kept) = vad.push_frame(frame)? {
            speech.extend_from_slice(kept);
            if speech.len() >= max_samples {
                break;
            }
        }
    }
    speech.truncate(max_samples);
    Ok(speech)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames whose first sample is positive are speech.
    struct SignVad;

    impl VoiceActivityDetector for SignVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            Ok(if frame[0] > 0.0 {
                VadFrame::Speech(frame)
            } else {
                VadFrame::Noise
            })
        }

        fn frame_samples(&self) -> usize {
            2
        }
    }

    #[test]
    fn keeps_only_speech_up_to_the_window() {
        let audio = [0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(
            speech_head(&mut SignVad, &audio, 3).unwrap(),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(
            speech_head(&mut SignVad, &audio, 100).unwrap(),
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );
        assert!(speech_head(&mut SignVad, &[0.0; 6], 4).unwrap().is_empty());
    }
}
