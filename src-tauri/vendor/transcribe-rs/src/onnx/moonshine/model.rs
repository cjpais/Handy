use ndarray::{Array2, ArrayD, IxDyn};
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::onnx::session;
use crate::onnx::Quantization;
use crate::{
    ModelCapabilities, SpeechModel, TranscribeError, TranscribeOptions, TranscriptionResult,
};

use super::{MoonshineVariant, SAMPLE_RATE};

const DECODER_START_TOKEN_ID: i64 = 1;
const EOS_TOKEN_ID: i64 = 2;

const CAPABILITIES: ModelCapabilities = ModelCapabilities {
    name: "Moonshine",
    engine_id: "moonshine",
    sample_rate: 16000,
    languages: &["en"],
    supports_timestamps: false,
    supports_translation: false,
    supports_streaming: false,
};

/// Per-model inference parameters for Moonshine.
#[derive(Debug, Clone, Default)]
pub struct MoonshineParams {
    /// Language hint (currently unused).
    pub language: Option<String>,
    /// Maximum number of tokens to generate.
    pub max_length: Option<usize>,
}

pub struct MoonshineModel {
    encoder: Session,
    decoder: Session,
    tokenizer: MoonshineTokenizer,
    variant: MoonshineVariant,
    encoder_input_names: Vec<String>,
    decoder_input_names: Vec<String>,
}

impl MoonshineModel {
    pub fn load(
        model_dir: &Path,
        variant: MoonshineVariant,
        quantization: &Quantization,
    ) -> Result<Self, TranscribeError> {
        let encoder_path = session::resolve_model_path(model_dir, "encoder_model", quantization);
        let decoder_path =
            session::resolve_model_path(model_dir, "decoder_model_merged", quantization);

        if !encoder_path.exists() {
            return Err(TranscribeError::ModelNotFound(encoder_path));
        }
        if !decoder_path.exists() {
            return Err(TranscribeError::ModelNotFound(decoder_path));
        }

        log::info!("Loading Moonshine encoder from {:?}...", encoder_path);
        let encoder = session::create_session(&encoder_path)?;

        log::info!("Loading Moonshine decoder from {:?}...", decoder_path);
        let decoder = session::create_session(&decoder_path)?;

        let encoder_input_names: Vec<String> =
            encoder.inputs.iter().map(|i| i.name.clone()).collect();
        let decoder_input_names: Vec<String> =
            decoder.inputs.iter().map(|i| i.name.clone()).collect();

        let tokenizer = MoonshineTokenizer::new(model_dir)?;

        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            variant,
            encoder_input_names,
            decoder_input_names,
        })
    }

    /// Transcribe with model-specific parameters.
    pub fn transcribe_with(
        &mut self,
        samples: &[f32],
        params: &MoonshineParams,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let max_length = params.max_length.unwrap_or_else(|| {
            let audio_duration_sec = samples.len() as f32 / SAMPLE_RATE as f32;
            (audio_duration_sec * self.variant.token_rate() as f32).ceil() as usize
        });

        self.infer(samples, max_length)
    }

    fn infer(
        &mut self,
        samples: &[f32],
        max_length: usize,
    ) -> Result<TranscriptionResult, TranscribeError> {
        log::debug!(
            "Transcribing {} samples ({:.2}s), max_length={}",
            samples.len(),
            samples.len() as f32 / SAMPLE_RATE as f32,
            max_length
        );

        let tokens = self.generate(samples, max_length)?;
        let text = self.decode_tokens(&tokens)?;

        Ok(TranscriptionResult {
            text,
            segments: None,
        })
    }

    fn encode(&mut self, audio: &Array2<f32>) -> Result<ArrayD<f32>, TranscribeError> {
        let audio_dyn = audio.clone().into_dyn();

        let outputs = if self
            .encoder_input_names
            .contains(&"attention_mask".to_string())
        {
            let attention_mask =
                Array2::<i64>::ones((audio.shape()[0], audio.shape()[1])).into_dyn();
            let t_input_values = TensorRef::from_array_view(audio_dyn.view())?;
            let t_attention_mask = TensorRef::from_array_view(attention_mask.view())?;
            let inputs = inputs![
                "input_values" => t_input_values,
                "attention_mask" => t_attention_mask,
            ];
            self.encoder.run(inputs)?
        } else {
            let t_input_values = TensorRef::from_array_view(audio_dyn.view())?;
            let inputs = inputs![
                "input_values" => t_input_values,
            ];
            self.encoder.run(inputs)?
        };

        let hidden_state = outputs
            .get("last_hidden_state")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: last_hidden_state".to_string())
            })?
            .try_extract_array::<f32>()?;

        Ok(hidden_state.to_owned())
    }

    fn generate(
        &mut self,
        samples: &[f32],
        max_length: usize,
    ) -> Result<Vec<i64>, TranscribeError> {
        let audio_duration = samples.len() as f32 / SAMPLE_RATE as f32;
        if audio_duration < 0.1 || audio_duration > 64.0 {
            return Err(TranscribeError::Inference(format!(
                "Audio duration must be between 0.1s and 64s, got {:.2}s",
                audio_duration
            )));
        }

        let audio = Array2::from_shape_vec((1, samples.len()), samples.to_vec())?;
        let audio_attention_mask = Array2::<i64>::ones((1, samples.len()));

        let encoder_hidden_states = self.encode(&audio)?;

        let mut cache = KVCache::new(&self.variant);
        let mut tokens: Vec<i64> = vec![DECODER_START_TOKEN_ID];
        let mut input_ids = Array2::from_shape_vec((1, 1), vec![DECODER_START_TOKEN_ID])?;

        for i in 0..max_length {
            let use_cache_branch = i > 0;

            let input_ids_dyn = input_ids.clone().into_dyn();
            let use_cache_branch_arr = ndarray::arr1(&[use_cache_branch]).into_dyn();

            let cache_inputs = cache.get_inputs();

            let mut ort_inputs: Vec<(std::borrow::Cow<'_, str>, ort::value::DynValue)> = vec![
                (
                    "input_ids".into(),
                    ort::value::Value::from_array(input_ids_dyn)?.into_dyn(),
                ),
                (
                    "encoder_hidden_states".into(),
                    ort::value::Value::from_array(encoder_hidden_states.clone())?.into_dyn(),
                ),
                (
                    "use_cache_branch".into(),
                    ort::value::Value::from_array(use_cache_branch_arr)?.into_dyn(),
                ),
            ];

            if self
                .decoder_input_names
                .contains(&"encoder_attention_mask".to_string())
            {
                let mask_dyn = audio_attention_mask.clone().into_dyn();
                ort_inputs.push((
                    "encoder_attention_mask".into(),
                    ort::value::Value::from_array(mask_dyn)?.into_dyn(),
                ));
            }

            for (name, arr) in cache_inputs {
                ort_inputs.push((name.into(), ort::value::Value::from_array(arr)?.into_dyn()));
            }

            let outputs = self.decoder.run(ort_inputs)?;

            let logits = outputs
                .get("logits")
                .ok_or_else(|| TranscribeError::Inference("Missing output: logits".to_string()))?
                .try_extract_array::<f32>()?;

            let logits_shape = logits.shape();
            let last_pos = logits_shape[1] - 1;

            let last_logits = logits.slice(ndarray::s![0, last_pos, ..]);
            let next_token = last_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx as i64)
                .unwrap_or(EOS_TOKEN_ID);

            tokens.push(next_token);

            if next_token == EOS_TOKEN_ID {
                break;
            }

            input_ids = Array2::from_shape_vec((1, 1), vec![next_token])?;
            cache.update_from_outputs(&outputs, use_cache_branch)?;
        }

        Ok(tokens)
    }

    fn decode_tokens(&self, tokens: &[i64]) -> Result<String, TranscribeError> {
        self.tokenizer.decode(tokens)
    }
}

impl SpeechModel for MoonshineModel {
    fn capabilities(&self) -> ModelCapabilities {
        CAPABILITIES
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        _options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let max_length = {
            let audio_duration_sec = samples.len() as f32 / SAMPLE_RATE as f32;
            (audio_duration_sec * self.variant.token_rate() as f32).ceil() as usize
        };
        self.infer(samples, max_length)
    }
}

// ---- KV Cache ----

struct KVCache {
    cache: HashMap<String, ArrayD<f32>>,
    num_layers: usize,
}

impl KVCache {
    fn new(variant: &MoonshineVariant) -> Self {
        let num_layers = variant.num_layers();
        let num_heads = variant.num_key_value_heads();
        let head_dim = variant.head_dim();

        let mut cache = HashMap::new();

        for i in 0..num_layers {
            for attention_type in &["decoder", "encoder"] {
                for kv_type in &["key", "value"] {
                    let key = format!("past_key_values.{}.{}.{}", i, attention_type, kv_type);
                    let empty_tensor = ArrayD::<f32>::zeros(IxDyn(&[0, num_heads, 1, head_dim]));
                    cache.insert(key, empty_tensor);
                }
            }
        }

        Self { cache, num_layers }
    }

    fn get_inputs(&self) -> Vec<(String, ArrayD<f32>)> {
        let mut inputs = Vec::new();

        for i in 0..self.num_layers {
            for attention_type in &["decoder", "encoder"] {
                for kv_type in &["key", "value"] {
                    let key = format!("past_key_values.{}.{}.{}", i, attention_type, kv_type);
                    if let Some(tensor) = self.cache.get(&key) {
                        inputs.push((key, tensor.clone()));
                    }
                }
            }
        }

        inputs
    }

    fn update_from_outputs(
        &mut self,
        outputs: &ort::session::SessionOutputs,
        use_cache_branch: bool,
    ) -> Result<(), TranscribeError> {
        for i in 0..self.num_layers {
            for attention_type in &["decoder", "encoder"] {
                if use_cache_branch && *attention_type == "encoder" {
                    continue;
                }

                for kv_type in &["key", "value"] {
                    let output_key = format!("present.{}.{}.{}", i, attention_type, kv_type);
                    let cache_key = format!("past_key_values.{}.{}.{}", i, attention_type, kv_type);

                    if let Some(output) = outputs.get(&output_key) {
                        let tensor = output.try_extract_array::<f32>()?;
                        self.cache.insert(cache_key, tensor.to_owned());
                    }
                }
            }
        }

        Ok(())
    }
}

// ---- Tokenizer ----

struct MoonshineTokenizer {
    vocab: HashMap<u32, String>,
    special_token_ids: Vec<u32>,
}

impl MoonshineTokenizer {
    fn new(model_dir: &Path) -> Result<Self, TranscribeError> {
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !tokenizer_path.exists() {
            return Err(TranscribeError::ModelNotFound(tokenizer_path));
        }

        let file = File::open(&tokenizer_path)?;
        let reader = BufReader::new(file);
        let json: serde_json::Value = serde_json::from_reader(reader)?;

        let mut vocab = HashMap::new();
        if let Some(model) = json.get("model") {
            if let Some(v) = model.get("vocab").and_then(|v| v.as_object()) {
                for (token, id) in v {
                    if let Some(id) = id.as_u64() {
                        vocab.insert(id as u32, token.clone());
                    }
                }
            }
        }

        if vocab.is_empty() {
            return Err(TranscribeError::Config(
                "No vocabulary found in tokenizer.json".to_string(),
            ));
        }

        let mut special_token_ids = Vec::new();
        if let Some(added_tokens) = json.get("added_tokens").and_then(|v| v.as_array()) {
            for token in added_tokens {
                let is_special = token
                    .get("special")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_special {
                    if let Some(id) = token.get("id").and_then(|v| v.as_u64()) {
                        special_token_ids.push(id as u32);
                    }
                }
            }
        }

        Ok(Self {
            vocab,
            special_token_ids,
        })
    }

    fn decode(&self, token_ids: &[i64]) -> Result<String, TranscribeError> {
        let mut tokens: Vec<String> = Vec::with_capacity(token_ids.len());

        for &id in token_ids {
            let id = id as u32;
            if self.special_token_ids.contains(&id) {
                continue;
            }
            if let Some(token) = self.vocab.get(&id) {
                tokens.push(token.clone());
            }
        }

        let mut bytes: Vec<u8> = Vec::new();

        for token in &tokens {
            if let Some(byte_val) = Self::parse_byte_token(token) {
                bytes.push(byte_val);
            } else {
                let decoded = token.replace('\u{2581}', " ");
                bytes.extend(decoded.as_bytes());
            }
        }

        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix(' ').unwrap_or(&text);

        Ok(text.to_string())
    }

    fn parse_byte_token(token: &str) -> Option<u8> {
        if token.starts_with("<0x") && token.ends_with('>') && token.len() == 6 {
            let hex = &token[3..5];
            u8::from_str_radix(hex, 16).ok()
        } else {
            None
        }
    }
}
