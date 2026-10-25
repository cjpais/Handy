//! Decodes a file's audio (or a video's sound track) to 16 kHz mono, the
//! format every recording is transcribed and kept in.

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;

/// Frames the resampler takes per step.
const RESAMPLER_CHUNK_FRAMES: usize = 1024;

/// The whole of `path`'s first audio track, downmixed to mono and resampled
/// to 16 kHz.
pub fn decode_audio_file(path: &Path) -> Result<Vec<f32>> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Not a supported audio or video file: {}", e))?
        .format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("The file has no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("The file's audio format is not supported: {}", e))?;

    // Built on the first packet, which gives the sample rate.
    let mut resampled: Option<Resampled> = None;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // The end of the stream.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(anyhow!("Failed to read the file: {}", e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet loses only its own audio.
            Err(SymphoniaError::DecodeError(e)) => {
                warn!("Skipping an undecodable packet: {}", e);
                continue;
            }
            Err(e) => return Err(anyhow!("Failed to decode the file: {}", e)),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let resampled = match &mut resampled {
            None => resampled.insert(Resampled::new(spec.rate)?),
            Some(r) if r.rate != spec.rate => {
                return Err(anyhow!("The file's sample rate changes partway through"));
            }
            Some(r) => r,
        };
        let packet_frames = decoded.capacity() as u64;
        if buffer
            .as_ref()
            .is_none_or(|b| b.capacity() < decoded.capacity() * channels)
        {
            buffer = Some(SampleBuffer::new(packet_frames, spec));
        }
        let buffer = buffer.as_mut().expect("buffer was just sized");
        buffer.copy_interleaved_ref(decoded);
        resampled.push(
            buffer
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        )?;
    }

    let resampled = resampled.ok_or_else(|| anyhow!("The file has no audio"))?;
    debug!(
        "Decoded {:.1}s of audio at {} Hz from {}",
        resampled.input_len as f64 / resampled.rate as f64,
        resampled.rate,
        path.display()
    );
    resampled.finish()
}

/// Mono audio at the file's rate in, 16 kHz out, one resampler step at a
/// time, so the file's audio is never held whole at its own rate.
struct Resampled {
    rate: u32,
    /// Input frames taken in so far.
    input_len: usize,
    /// `None` when the file is already at 16 kHz.
    stage: Option<Stage>,
    output: Vec<f32>,
}

struct Stage {
    resampler: Fft<f32>,
    /// Input not yet resampled: less than one step, plus the latest packet.
    pending: Vec<f32>,
    /// One step's output.
    scratch: Vec<f32>,
    /// Output frames of the resampler's delay still to drop.
    delay_left: usize,
}

impl Resampled {
    fn new(rate: u32) -> Result<Self> {
        let target = WHISPER_SAMPLE_RATE as usize;
        let stage = if rate as usize == target {
            None
        } else {
            let resampler = Fft::<f32>::new(
                rate as usize,
                target,
                RESAMPLER_CHUNK_FRAMES,
                1,
                FixedSync::Input,
            )
            .map_err(|e| anyhow!("Failed to set up resampling from {} Hz: {}", rate, e))?;
            Some(Stage {
                pending: Vec::with_capacity(2 * resampler.input_frames_max()),
                scratch: vec![0.0; resampler.output_frames_max()],
                delay_left: resampler.output_delay(),
                resampler,
            })
        };
        Ok(Self {
            rate,
            input_len: 0,
            stage,
            output: Vec::new(),
        })
    }

    /// The 16 kHz length of `input_len` frames at the file's rate.
    fn expected_len(&self, input_len: usize) -> usize {
        match &self.stage {
            Some(stage) => (stage.resampler.resample_ratio() * input_len as f64).ceil() as usize,
            None => input_len,
        }
    }

    fn push(&mut self, samples: impl Iterator<Item = f32>) -> Result<()> {
        let Some(stage) = self.stage.as_mut() else {
            let before = self.output.len();
            self.output.extend(samples);
            self.input_len += self.output.len() - before;
            return Ok(());
        };
        let before = stage.pending.len();
        stage.pending.extend(samples);
        self.input_len += stage.pending.len() - before;
        let mut taken = 0;
        while stage.pending.len() - taken >= stage.resampler.input_frames_next() {
            taken += stage.step(taken, None, &mut self.output)?;
        }
        stage.pending.drain(..taken);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<f32>> {
        let expected = self.expected_len(self.input_len);
        let Some(mut stage) = self.stage.take() else {
            return Ok(self.output);
        };
        let left = stage.pending.len();
        if left > 0 {
            stage.step(0, Some(left), &mut self.output)?;
        }
        // The resampler holds back its delay's worth; silence pushes it out.
        while self.output.len() < expected {
            stage.step(0, Some(0), &mut self.output)?;
        }
        self.output.truncate(expected);
        Ok(self.output)
    }
}

impl Stage {
    /// Resamples one step of `pending` from `offset` (or only `partial` of
    /// it, padded with silence) into `output`. Returns the frames it took.
    fn step(
        &mut self,
        offset: usize,
        partial: Option<usize>,
        output: &mut Vec<f32>,
    ) -> Result<usize> {
        let input = InterleavedSlice::new(&self.pending[..], 1, self.pending.len())
            .map_err(|e| anyhow!("Failed to resample: {}", e))?;
        let scratch_len = self.scratch.len();
        let mut out = InterleavedSlice::new_mut(&mut self.scratch[..], 1, scratch_len)
            .map_err(|e| anyhow!("Failed to resample: {}", e))?;
        let mut indexing = Indexing::new().input_offset(offset);
        indexing.partial_len = partial;
        let (taken, produced) = self
            .resampler
            .process_into_buffer(&input, &mut out, Some(&indexing))
            .map_err(|e| anyhow!("Failed to resample: {}", e))?;
        let dropped = self.delay_left.min(produced);
        self.delay_left -= dropped;
        output.extend_from_slice(&self.scratch[dropped..produced]);
        Ok(taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};

    #[test]
    fn stereo_44k_file_becomes_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for sample in tone(44_100, 88_200) {
            let value = (sample * i16::MAX as f32) as i16;
            writer.write_sample(value).unwrap();
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();

        let samples = decode_audio_file(&path).unwrap();

        assert_eq!(samples.len(), 32_000);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((0.4..0.6).contains(&peak), "peak {}", peak);
    }

    fn resample_in_packets(mono: &[f32], rate: u32, packet: usize) -> Vec<f32> {
        let mut resampled = Resampled::new(rate).unwrap();
        for chunk in mono.chunks(packet) {
            resampled.push(chunk.iter().copied()).unwrap();
        }
        resampled.finish().unwrap()
    }

    fn tone(rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    #[test]
    fn resampling_keeps_length_and_timing_whatever_the_packet_size() {
        for rate in [16_000, 44_100, 48_000] {
            for frames in [500, rate as usize + 337] {
                let mono = tone(rate, frames);
                let whole = resample_in_packets(&mono, rate, frames);
                let expected_len = (frames as f64 * 16_000.0 / rate as f64).ceil() as usize;
                assert_eq!(whole.len(), expected_len, "{} Hz, {} frames", rate, frames);
                for packet in [333, 4096] {
                    assert_eq!(resample_in_packets(&mono, rate, packet), whole);
                }
                // Delay trimmed: within half a sample of the 16 kHz tone.
                let reference = tone(16_000, whole.len());
                let interior = 100..whole.len().saturating_sub(100);
                let max_error = interior
                    .map(|i| (whole[i] - reference[i]).abs())
                    .fold(0.0f32, f32::max);
                assert!(max_error < 0.04, "{} Hz: error {}", rate, max_error);
            }
        }
    }

    #[test]
    fn a_file_that_is_not_audio_fails_to_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "not audio at all").unwrap();
        assert!(decode_audio_file(&path).is_err());
    }
}
