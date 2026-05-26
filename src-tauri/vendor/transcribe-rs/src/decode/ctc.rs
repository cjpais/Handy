use ndarray::ArrayView3;

/// Result of CTC greedy decoding for a single utterance.
pub struct CtcDecoderResult {
    /// Decoded token IDs (excluding blanks and collapsed repeats).
    pub tokens: Vec<i64>,
    /// Frame indices corresponding to each decoded token.
    pub timestamps: Vec<i32>,
}

/// CTC greedy search decoder.
///
/// For each time step, selects the token with highest logit. Skips blank tokens
/// and consecutive repeated tokens.
pub fn ctc_greedy_decode(
    logits: &ArrayView3<f32>,
    logits_lengths: &[i64],
    blank_id: i64,
) -> Vec<CtcDecoderResult> {
    let batch_size = logits.shape()[0];
    let vocab_size = logits.shape()[2];
    let mut results = Vec::with_capacity(batch_size);

    for b in 0..batch_size {
        let num_frames = logits_lengths[b] as usize;
        let mut result = CtcDecoderResult {
            tokens: Vec::new(),
            timestamps: Vec::new(),
        };
        let mut prev_id: i64 = -1;

        for t in 0..num_frames {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_id: i64 = 0;
            for v in 0..vocab_size {
                let val = logits[[b, t, v]];
                if val > max_val {
                    max_val = val;
                    max_id = v as i64;
                }
            }

            if max_id != blank_id && max_id != prev_id {
                result.tokens.push(max_id);
                result.timestamps.push(t as i32);
            }
            prev_id = max_id;
        }

        results.push(result);
    }

    results
}
