const LIVESTT_SAMPLE_RATE: usize = 16_000;
const LIVESTT_CHUNK_MS: usize = 250;
pub const LIVESTT_CHUNK_SAMPLES: usize = LIVESTT_SAMPLE_RATE * LIVESTT_CHUNK_MS / 1_000;
#[cfg(test)]
const LIVESTT_CHUNK_BYTES: usize = LIVESTT_CHUNK_SAMPLES * 2;

pub fn f32_samples_to_pcm_i16_le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm = if clamped == -1.0 {
            i16::MIN
        } else {
            (clamped * i16::MAX as f32).round() as i16
        };
        bytes.extend_from_slice(&pcm.to_le_bytes());
    }

    bytes
}

#[derive(Debug, Default)]
pub struct PcmChunkAccumulator {
    pending: Vec<f32>,
}

impl PcmChunkAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_samples(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);

        let chunk_count = self.pending.len() / LIVESTT_CHUNK_SAMPLES;
        if chunk_count == 0 {
            return Vec::new();
        }

        self.pending
            .drain(..chunk_count * LIVESTT_CHUNK_SAMPLES)
            .collect::<Vec<_>>()
            .chunks_exact(LIVESTT_CHUNK_SAMPLES)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.pending.is_empty() {
            return None;
        }

        Some(std::mem::take(&mut self.pending))
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn livestt_pcm_empty_input_returns_empty_bytes() {
        assert_eq!(f32_samples_to_pcm_i16_le(&[]), Vec::<u8>::new());
    }

    #[test]
    fn livestt_pcm_zero_encodes_to_zero() {
        assert_eq!(f32_samples_to_pcm_i16_le(&[0.0]), vec![0, 0]);
    }

    #[test]
    fn livestt_pcm_output_length_is_twice_sample_count() {
        let bytes = f32_samples_to_pcm_i16_le(&[0.0, 0.5, -0.5, 1.0]);

        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn livestt_pcm_positive_samples_are_little_endian() {
        let bytes = f32_samples_to_pcm_i16_le(&[1.0]);

        assert_eq!(bytes, i16::MAX.to_le_bytes().to_vec());
    }

    #[test]
    fn livestt_pcm_negative_samples_are_little_endian() {
        let bytes = f32_samples_to_pcm_i16_le(&[-1.0]);

        assert_eq!(bytes, i16::MIN.to_le_bytes().to_vec());
    }

    #[test]
    fn livestt_pcm_values_above_one_are_clamped() {
        let bytes = f32_samples_to_pcm_i16_le(&[2.0]);

        assert_eq!(bytes, i16::MAX.to_le_bytes().to_vec());
    }

    #[test]
    fn livestt_pcm_values_below_negative_one_are_clamped() {
        let bytes = f32_samples_to_pcm_i16_le(&[-2.0]);

        assert_eq!(bytes, i16::MIN.to_le_bytes().to_vec());
    }

    #[test]
    fn livestt_accumulator_3999_samples_emits_no_full_chunk() {
        let mut accumulator = PcmChunkAccumulator::new();
        let chunks = accumulator.push_samples(&vec![0.0; 3_999]);

        assert!(chunks.is_empty());
        assert_eq!(accumulator.pending_len(), 3_999);
    }

    #[test]
    fn livestt_accumulator_4000_samples_emits_one_chunk() {
        let mut accumulator = PcmChunkAccumulator::new();
        let chunks = accumulator.push_samples(&vec![0.0; LIVESTT_CHUNK_SAMPLES]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), LIVESTT_CHUNK_SAMPLES);
        assert_eq!(accumulator.pending_len(), 0);
    }

    #[test]
    fn livestt_accumulator_9000_samples_emits_two_chunks_and_buffers_remainder() {
        let mut accumulator = PcmChunkAccumulator::new();
        let chunks = accumulator.push_samples(&vec![0.0; 9_000]);

        assert_eq!(chunks.len(), 2);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.len() == LIVESTT_CHUNK_SAMPLES));
        assert_eq!(accumulator.pending_len(), 1_000);
    }

    #[test]
    fn livestt_accumulator_flush_emits_remainder() {
        let mut accumulator = PcmChunkAccumulator::new();
        accumulator.push_samples(&[0.1, 0.2, 0.3]);

        assert_eq!(accumulator.flush(), Some(vec![0.1, 0.2, 0.3]));
        assert_eq!(accumulator.pending_len(), 0);
    }

    #[test]
    fn livestt_accumulator_reset_clears_pending_samples() {
        let mut accumulator = PcmChunkAccumulator::new();
        accumulator.push_samples(&[0.1, 0.2, 0.3]);

        accumulator.reset();

        assert_eq!(accumulator.pending_len(), 0);
        assert_eq!(accumulator.flush(), None);
    }

    #[test]
    fn livestt_accumulator_reset_starts_next_recording_cleanly() {
        let mut accumulator = PcmChunkAccumulator::new();
        accumulator.push_samples(&vec![0.0; 3_999]);

        accumulator.reset();
        let chunks = accumulator.push_samples(&[1.0]);

        assert!(chunks.is_empty());
        assert_eq!(accumulator.pending_len(), 1);
    }

    #[test]
    fn livestt_chunk_constants_match_250ms_pcm_target() {
        assert_eq!(
            LIVESTT_CHUNK_SAMPLES,
            LIVESTT_SAMPLE_RATE * LIVESTT_CHUNK_MS / 1_000
        );
        assert_eq!(LIVESTT_CHUNK_BYTES, LIVESTT_CHUNK_SAMPLES * 2);
    }
}
