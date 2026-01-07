use anyhow::{anyhow, Result};
use rodio::Source;
use rubato::{FftFixedIn, Resampler};
use std::time::Duration;

// Make this a constant you can tweak
const RESAMPLER_CHUNK_SIZE: usize = 1024;

pub struct FrameResampler {
    resampler: Option<FftFixedIn<f32>>,
    chunk_in: usize,
    in_buf: Vec<f32>,
    frame_samples: usize,
    pending: Vec<f32>,
}

impl FrameResampler {
    pub fn new(in_hz: usize, out_hz: usize, frame_dur: Duration) -> Self {
        let frame_samples = ((out_hz as f64 * frame_dur.as_secs_f64()).round()) as usize;
        assert!(frame_samples > 0, "frame duration too short");

        // Use fixed chunk size instead of GCD-based
        let chunk_in = RESAMPLER_CHUNK_SIZE;

        let resampler = (in_hz != out_hz).then(|| {
            FftFixedIn::<f32>::new(in_hz, out_hz, chunk_in, 1, 1)
                .expect("Failed to create resampler")
        });

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
                // let start = std::time::Instant::now();
                if let Ok(out) = self
                    .resampler
                    .as_mut()
                    .unwrap()
                    .process(&[&self.in_buf[..]], None)
                {
                    // let duration = start.elapsed();
                    // log::debug!("Resampler took: {:?}", duration);
                    self.emit_frames(&out[0], &mut emit);
                }
                self.in_buf.clear();
            }
        }
    }

    pub fn finish(&mut self, mut emit: impl FnMut(&[f32])) {
        // Process any remaining input samples
        if let Some(ref mut resampler) = self.resampler {
            if !self.in_buf.is_empty() {
                // Pad with zeros to reach chunk size
                self.in_buf.resize(self.chunk_in, 0.0);
                if let Ok(out) = resampler.process(&[&self.in_buf[..]], None) {
                    self.emit_frames(&out[0], &mut emit);
                }
            }
        }

        // Emit any remaining pending frame (padded with zeros)
        if !self.pending.is_empty() {
            self.pending.resize(self.frame_samples, 0.0);
            emit(&self.pending);
            self.pending.clear();
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

pub fn resample_audio<S>(source: S) -> Result<Vec<f32>>
where
    S: Source<Item = f32> + Send + 'static,
{
    let target_sample_rate = 16000;
    let source_rate = source.sample_rate();
    let channels = source.channels();

    if channels == 0 {
        return Err(anyhow!("Audio has no channels"));
    }

    // 1. Convert to mono and collect all samples
    let mut mono_samples = Vec::new();
    let mut channel_sum = 0.0;
    let mut channel_count = 0;

    for sample in source {
        channel_sum += sample;
        channel_count += 1;
        if channel_count == channels {
            mono_samples.push(channel_sum / channels as f32);
            channel_sum = 0.0;
            channel_count = 0;
        }
    }

    if source_rate == target_sample_rate {
        return Ok(mono_samples);
    }

    // 2. High-quality resampling using rubato
    // We use a fixed chunk size for the resampler
    let chunk_size = 1024;
    let mut resampler = FftFixedIn::<f32>::new(
        source_rate as usize,
        target_sample_rate as usize,
        chunk_size,
        1,
        1,
    )
    .map_err(|e| anyhow!("Failed to create resampler: {}", e))?;

    let mut output = Vec::new();
    let mut input_pos = 0;

    while input_pos + chunk_size <= mono_samples.len() {
        let chunk = &mono_samples[input_pos..input_pos + chunk_size];
        if let Ok(resampled_chunk) = resampler.process(&[chunk], None) {
            output.extend_from_slice(&resampled_chunk[0]);
        }
        input_pos += chunk_size;
    }

    // Handle remaining samples by padding with zeros
    if input_pos < mono_samples.len() {
        let mut last_chunk = vec![0.0; chunk_size];
        let remaining = mono_samples.len() - input_pos;
        last_chunk[..remaining].copy_from_slice(&mono_samples[input_pos..]);
        if let Ok(resampled_chunk) = resampler.process(&[last_chunk], None) {
            // Only take the relevant part of the output to avoid too much padding
            // (Though for transcription a bit of silence at the end is fine)
            let out_len =
                (remaining as f32 * (target_sample_rate as f32 / source_rate as f32)) as usize;
            output.extend_from_slice(&resampled_chunk[0][..out_len.min(resampled_chunk[0].len())]);
        }
    }

    Ok(output)
}
