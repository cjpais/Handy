use rubato::{Fft, FixedSync, Resampler};
use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use std::time::Duration;

const RESAMPLER_CHUNK_SIZE: usize = 1024;

pub struct FrameResampler {
    resampler: Option<Fft<f32>>,
    chunk_in: usize,
    in_buf: Vec<f32>,
    frame_samples: usize,
    pending: Vec<f32>,
}

impl FrameResampler {
    pub fn new(in_hz: usize, out_hz: usize, frame_dur: Duration) -> Self {
        let frame_samples = ((out_hz as f64 * frame_dur.as_secs_f64()).round()) as usize;
        assert!(frame_samples > 0, "frame duration too short");

        let resampler = (in_hz != out_hz).then(|| {
            Fft::<f32>::new(in_hz, out_hz, RESAMPLER_CHUNK_SIZE, 1, FixedSync::Input)
                .expect("Failed to create resampler")
        });

        let chunk_in = resampler
            .as_ref()
            .map(|r| r.input_frames_next())
            .unwrap_or(RESAMPLER_CHUNK_SIZE);

        Self {
            resampler,
            chunk_in,
            in_buf: Vec::with_capacity(chunk_in),
            frame_samples,
            pending: Vec::with_capacity(frame_samples),
        }
    }

    pub fn push(&mut self, mut src: &[f32], mut emit: impl FnMut(&[f32])) {
        if self.resampler.is_none() {
            self.emit_frames(src, &mut emit);
            return;
        }

        while !src.is_empty() {
            let space = self.chunk_in - self.in_buf.len();
            let take = space.min(src.len());
            self.in_buf.extend_from_slice(&src[..take]);
            src = &src[take..];

            if self.in_buf.len() == self.chunk_in {
                if let Some(out) = self.process_chunk() {
                    self.emit_frames(&out, &mut emit);
                }
                self.in_buf.clear();
            }
        }
    }

    pub fn finish(&mut self, mut emit: impl FnMut(&[f32])) {
        if self.resampler.is_some() && !self.in_buf.is_empty() {
            self.in_buf.resize(self.chunk_in, 0.0);
            if let Some(out) = self.process_chunk() {
                self.emit_frames(&out, &mut emit);
            }
        }
        self.in_buf.clear();

        if !self.pending.is_empty() {
            self.pending.resize(self.frame_samples, 0.0);
            emit(&self.pending);
            self.pending.clear();
        }
    }

    pub fn reset(&mut self) {
        self.in_buf.clear();
        self.pending.clear();
        if let Some(ref mut resampler) = self.resampler {
            let _ = resampler.reset();
        }
    }

    fn process_chunk(&mut self) -> Option<Vec<f32>> {
        let frames = self.in_buf.len();
        let input_data = self.in_buf.clone();
        let resampler = self.resampler.as_mut()?;
        let input = InterleavedSlice::new(&input_data, 1, frames).ok()?;
        match resampler.process(&input, None) {
            Ok(out) => {
                let n = out.frames();
                let mut samples = Vec::with_capacity(n);
                for f in 0..n {
                    samples.push(out.read_sample(0, f).unwrap_or(0.0));
                }
                Some(samples)
            }
            Err(e) => {
                log::warn!("[Resampler] process failed: {e}");
                None
            }
        }
    }

    fn emit_frames(&mut self, mut data: &[f32], emit: &mut impl FnMut(&[f32])) {
        while !data.is_empty() {
            let space = self.frame_samples - self.pending.len();
            let take = space.min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];

            if self.pending.len() == self.frame_samples {
                emit(&self.pending);
                self.pending.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(sample_rate: usize, freq: f64, duration_secs: f64) -> Vec<f32> {
        let n = (sample_rate as f64 * duration_secs) as usize;
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64).sin() as f32
            })
            .collect()
    }

    fn collect_output(resampler: &mut FrameResampler, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        resampler.push(input, |frame| out.extend_from_slice(frame));
        out
    }

    #[test]
    fn reset_clears_in_buf_and_pending() {
        let mut r = FrameResampler::new(48000, 16000, Duration::from_millis(30));

        let partial = vec![0.5f32; 500];
        let _ = collect_output(&mut r, &partial);

        r.reset();

        let silence = vec![0.0f32; 4096];
        let out = collect_output(&mut r, &silence);

        let max_abs = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_abs < 0.01,
            "After reset, silence input should produce near-silence output, got max_abs={}",
            max_abs
        );
    }

    #[test]
    fn reset_clears_fft_overlap_buffers() {
        let mut r = FrameResampler::new(48000, 16000, Duration::from_millis(30));

        let sine = sine_wave(48000, 1000.0, 0.5);
        let _ = collect_output(&mut r, &sine);
        r.finish(|_| {});

        r.reset();

        let silence = vec![0.0f32; 4096];
        let out = collect_output(&mut r, &silence);

        let max_abs = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_abs < 0.01,
            "FFT overlap should not leak after reset; got max_abs={} (expected near-zero)",
            max_abs
        );
    }

    #[test]
    fn reset_between_recordings_no_crosstalk() {
        let mut r = FrameResampler::new(48000, 16000, Duration::from_millis(30));

        let ramp: Vec<f32> = (0..48000).map(|i| i as f32 / 48000.0).collect();
        let out1 = collect_output(&mut r, &ramp);
        r.finish(|_| {});
        assert!(!out1.is_empty(), "Recording 1 should produce output");

        r.reset();

        let dc = vec![-0.5f32; 48000];
        let out2 = collect_output(&mut r, &dc);

        if out2.len() > 480 {
            let tail = &out2[480..];
            for (i, &s) in tail.iter().enumerate() {
                assert!(
                    (s - (-0.5)).abs() < 0.05,
                    "Recording 2 sample {} = {} (expected ~-0.5); ramp leaked through",
                    i + 480,
                    s
                );
            }
        }
    }

    #[test]
    fn reset_passthrough_mode_clears_pending() {
        let mut r = FrameResampler::new(16000, 16000, Duration::from_millis(30));

        let partial = vec![1.0f32; 200];
        let _ = collect_output(&mut r, &partial);

        r.reset();

        let silence = vec![0.0f32; 960];
        let out = collect_output(&mut r, &silence);

        if !out.is_empty() {
            let max_abs = out.iter().take(480).map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                max_abs < 0.001,
                "Passthrough mode: pending buffer should be cleared after reset, got max_abs={}",
                max_abs
            );
        }
    }

    #[test]
    fn finish_does_not_leak_tail_into_next_session() {
        let mut rs = FrameResampler::new(48000, 16000, Duration::from_millis(30));

        rs.push(&[0.5f32; 100], |_| {});
        rs.finish(|_| {});

        let mut emitted = 0usize;
        rs.push(&[0.25f32; RESAMPLER_CHUNK_SIZE], |frame| {
            emitted += frame.len()
        });
        assert_eq!(
            emitted, 0,
            "stale resampler tail from finish() leaked into the next session"
        );
    }
}
