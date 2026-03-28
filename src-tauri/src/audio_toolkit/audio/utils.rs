use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use log::debug;
use rubato::{FftFixedIn, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Read a WAV file and return normalised f32 samples.
pub fn read_wav_samples<P: AsRef<Path>>(file_path: P) -> Result<Vec<f32>> {
    let reader = WavReader::open(file_path.as_ref())?;
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<f32>, _>>()?;
    Ok(samples)
}

/// Verify a WAV file by reading it back and checking the sample count.
pub fn verify_wav_file<P: AsRef<Path>>(file_path: P, expected_samples: usize) -> Result<()> {
    let reader = WavReader::open(file_path.as_ref())?;
    let actual_samples = reader.len() as usize;
    if actual_samples != expected_samples {
        anyhow::bail!(
            "WAV sample count mismatch: expected {}, got {}",
            expected_samples,
            actual_samples
        );
    }
    Ok(())
}

/// Save audio samples as a WAV file
pub fn save_wav_file<P: AsRef<Path>>(file_path: P, samples: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = WavWriter::create(file_path.as_ref(), spec)?;

    // Convert f32 samples to i16 for WAV
    for sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
    }

    writer.finalize()?;
    debug!("Saved WAV file: {:?}", file_path.as_ref());
    Ok(())
}

/// Decode any supported audio file (MP3, M4A, WAV, OGG, FLAC, etc.) and
/// resample to 16 kHz mono f32 samples suitable for Whisper inference.
pub fn decode_and_resample(path: std::path::PathBuf) -> Result<Vec<f32>, String> {
    // Open the media source.
    let src = File::open(&path).map_err(|e| format!("failed to open file: {}", e))?;

    // Create the media source stream.
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    // Create a hint to help the format registry guess the appropriate reader.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    // Probe the media source.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("failed to probe: {}", e))?;

    let mut format = probed.format;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no supported audio tracks")?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| format!("failed to create decoder: {}", e))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("missing sample rate")?;

    let mut samples: Vec<f32> = Vec::new();

    // The decode loop.
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::IoError(e)) => return Err(format!("io error: {}", e)),
            Err(e) => return Err(format!("failed to read packet: {}", e)),
        };

        // Consume any new metadata that has been read since the last packet.
        while !format.metadata().is_latest() {
            format.metadata().pop();
        }

        // Skip packets that don't belong to the selected track.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples, mixing down to mono.
        match decoder.decode(&packet) {
            Ok(decoded) => match decoded {
                AudioBufferRef::F32(buf) => {
                    let ch = buf.spec().channels.count();
                    for i in 0..buf.frames() {
                        let mut sum: f32 = 0.0;
                        for c in 0..ch {
                            sum += buf.chan(c)[i];
                        }
                        samples.push(sum / ch as f32);
                    }
                }
                AudioBufferRef::U8(buf) => {
                    let ch = buf.spec().channels.count();
                    for i in 0..buf.frames() {
                        let mut sum: f32 = 0.0;
                        for c in 0..ch {
                            sum += (buf.chan(c)[i] as f32 - 128.0) / 128.0;
                        }
                        samples.push(sum / ch as f32);
                    }
                }
                AudioBufferRef::S16(buf) => {
                    let ch = buf.spec().channels.count();
                    for i in 0..buf.frames() {
                        let mut sum: f32 = 0.0;
                        for c in 0..ch {
                            sum += buf.chan(c)[i] as f32 / 32768.0;
                        }
                        samples.push(sum / ch as f32);
                    }
                }
                AudioBufferRef::F64(buf) => {
                    let ch = buf.spec().channels.count();
                    for i in 0..buf.frames() {
                        let mut sum: f32 = 0.0;
                        for c in 0..ch {
                            sum += buf.chan(c)[i] as f32;
                        }
                        samples.push(sum / ch as f32);
                    }
                }
                _ => {
                    log::warn!("Unsupported sample format in audio buffer");
                }
            },
            Err(SymphoniaError::DecodeError(e)) => {
                // Recoverable decode error — log and continue.
                log::warn!("decode error: {}", e);
            }
            Err(e) => return Err(format!("failed to decode: {}", e)),
        }
    }

    // If already at 16 kHz, no resampling needed.
    if sample_rate == 16000 {
        return Ok(samples);
    }

    // Resample to 16 kHz using rubato.
    let chunk_size = 1024;
    let mut resampler = FftFixedIn::<f32>::new(sample_rate as usize, 16000, chunk_size, 1, 1)
        .map_err(|e| format!("failed to create resampler: {}", e))?;

    let mut resampled_samples = Vec::with_capacity(samples.len());
    let mut input_buf = vec![0.0f32; chunk_size];

    for chunk in samples.chunks(chunk_size) {
        let current_chunk_len = chunk.len();
        input_buf[..current_chunk_len].copy_from_slice(chunk);

        // Pad with zeros if the last chunk is shorter.
        if current_chunk_len < chunk_size {
            for sample in input_buf[current_chunk_len..].iter_mut() {
                *sample = 0.0;
            }
        }

        let waves_in = vec![&input_buf[..]];
        let waves_out = resampler
            .process(&waves_in, None)
            .map_err(|e| format!("resampling error: {}", e))?;

        resampled_samples.extend_from_slice(&waves_out[0]);
    }

    Ok(resampled_samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_decode_and_resample() {
        // Use an existing resource file if available
        let path = PathBuf::from("resources/pop_start.wav");
        if !path.exists() {
            println!("Skipping test: {:?} not found", path);
            return;
        }

        println!("Decoding {:?}", path);
        match decode_and_resample(path) {
            Ok(samples) => {
                println!("Success: {} samples", samples.len());
                assert!(!samples.is_empty());
            }
            Err(e) => {
                panic!("Failed to decode: {}", e);
            }
        }
    }
}
