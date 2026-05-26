use ndarray::Array4;
use ort::session::Session;
use ort::value::ValueType;
use ort::value::{DynValue, Tensor};

use super::vocab::Vocab;
use crate::TranscribeError;

pub fn decode_autoregressive(
    decoder: &mut Session,
    encoder_embeddings: &DynValue,
    encoder_mask: &DynValue,
    prompt_tokens: Vec<i64>,
    vocab: &Vocab,
    max_sequence_length: usize,
) -> Result<String, TranscribeError> {
    let (num_layers, hidden_dim) = extract_decoder_mems_shape(decoder)?;

    log::debug!(
        "Decoder cache dimensions: num_layers={}, hidden_dim={}",
        num_layers,
        hidden_dim
    );

    let empty_cache = Array4::<f32>::zeros((num_layers, 1, 0, hidden_dim));
    let mut decoder_mems: DynValue = Tensor::from_array(empty_cache)?.into_dyn();

    let eos_id = vocab.eos_token_id();
    let mut all_tokens = prompt_tokens;

    // Limit decode steps so total tokens (prompt + generated) stays within
    // the model's position embedding table (typically 1024).
    let max_steps = max_sequence_length.saturating_sub(all_tokens.len());

    log::debug!(
        "Starting autoregressive decode with {} prompt tokens, max_steps={}",
        all_tokens.len(),
        max_steps
    );

    for step in 0..max_steps {
        let input_ids_tensor = if step == 0 {
            let len = all_tokens.len();
            let shape = vec![1i64, len as i64];
            Tensor::from_array((shape, all_tokens.clone().into_boxed_slice()))?
        } else {
            let last = *all_tokens
                .last()
                .ok_or_else(|| TranscribeError::Inference("Token list is empty".to_string()))?;
            Tensor::from_array((vec![1i64, 1i64], vec![last].into_boxed_slice()))?
        };

        let mut outputs = decoder.run(ort::inputs![
            "input_ids" => input_ids_tensor,
            "encoder_embeddings" => encoder_embeddings,
            "encoder_mask" => encoder_mask,
            "decoder_mems" => decoder_mems
        ])?;

        // Extract logits in a scoped borrow, then release before remove()
        let next_token = {
            let (logits_shape, logits_data) =
                outputs["logits"].try_extract_tensor::<f32>().map_err(|e| {
                    TranscribeError::Inference(format!("Failed to extract logits: {e}"))
                })?;

            let seq_len = logits_shape[1] as usize;
            let vocab_size = logits_shape[2] as usize;

            let last_step_offset = (seq_len - 1) * vocab_size;
            let last_logits = &logits_data[last_step_offset..last_step_offset + vocab_size];

            argmax(last_logits) as i64
        };

        log::debug!("Step {}: predicted token ID {}", step, next_token);

        if next_token == eos_id {
            log::debug!("EOS token reached at step {}", step);
            break;
        }

        all_tokens.push(next_token);

        // Take the KV cache directly from outputs (Arc clone, no data copy)
        decoder_mems = outputs.remove("decoder_hidden_states").ok_or_else(|| {
            TranscribeError::Inference("Missing decoder_hidden_states output".to_string())
        })?;
    }

    let text = vocab.decode_tokens(&all_tokens);
    Ok(text)
}

fn extract_decoder_mems_shape(decoder: &Session) -> Result<(usize, usize), TranscribeError> {
    let mems_input = decoder
        .inputs
        .iter()
        .find(|outlet: &&ort::session::Input| outlet.name == "decoder_mems")
        .ok_or_else(|| {
            TranscribeError::Inference("Decoder model missing 'decoder_mems' input".to_string())
        })?;

    match &mems_input.input_type {
        ValueType::Tensor { shape, .. } => {
            let dims: &[i64] = shape;
            if dims.len() != 4 {
                return Err(TranscribeError::Inference(format!(
                    "Expected 4D decoder_mems, got {}D",
                    dims.len()
                )));
            }

            let num_layers = dims[0];
            let hidden_dim = dims[3];

            if num_layers <= 0 || hidden_dim <= 0 {
                return Err(TranscribeError::Inference(format!(
                    "decoder_mems has dynamic num_layers ({}) or hidden_dim ({}); expected fixed",
                    num_layers, hidden_dim
                )));
            }

            Ok((num_layers as usize, hidden_dim as usize))
        }
        other => Err(TranscribeError::Inference(format!(
            "decoder_mems input is not a tensor: {:?}",
            other
        ))),
    }
}

fn argmax(slice: &[f32]) -> usize {
    let mut max_idx = 0;
    let mut max_val = f32::NEG_INFINITY;
    for (i, &v) in slice.iter().enumerate() {
        if v > max_val {
            max_val = v;
            max_idx = i;
        }
    }
    max_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argmax() {
        assert_eq!(argmax(&[1.0, 3.0, 2.0]), 1);
        assert_eq!(argmax(&[-1.0, -3.0, -0.5]), 2);
        assert_eq!(argmax(&[5.0]), 0);
    }
}
