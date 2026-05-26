use ndarray::{ArrayD, ArrayViewD, IxDyn};
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::onnx::session;
use crate::onnx::Quantization;
use crate::{
    ModelCapabilities, SpeechModel, TranscribeError, TranscribeOptions, TranscriptionResult,
};

use super::SAMPLE_RATE;

const CHUNK_SIZE: usize = 1280;

const STREAMING_CAPABILITIES: ModelCapabilities = ModelCapabilities {
    name: "Moonshine Streaming",
    engine_id: "moonshine_streaming",
    sample_rate: 16000,
    languages: &["en"],
    supports_timestamps: false,
    supports_translation: false,
    supports_streaming: true,
};

/// Per-model inference parameters for Moonshine Streaming.
#[derive(Debug, Clone, Default)]
pub struct MoonshineStreamingParams {
    /// Language hint (currently unused).
    pub language: Option<String>,
    /// Maximum number of tokens to generate.
    pub max_length: Option<usize>,
}

/// Streaming model configuration parsed from `streaming_config.json`.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    pub encoder_dim: usize,
    pub decoder_dim: usize,
    pub depth: usize,
    pub nheads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub bos_id: i64,
    pub eos_id: i64,
    pub frame_len: usize,
    pub total_lookahead: usize,
    pub d_model_frontend: usize,
    pub c1: usize,
    pub c2: usize,
    pub max_seq_len: usize,
}

impl StreamingConfig {
    fn load(model_dir: &Path) -> Result<Self, TranscribeError> {
        let config_path = model_dir.join("streaming_config.json");
        if !config_path.exists() {
            return Err(TranscribeError::ModelNotFound(config_path));
        }

        let contents = fs::read_to_string(&config_path)?;
        let json: serde_json::Value = serde_json::from_str(&contents)?;

        let get_usize =
            |key: &str| -> usize { json.get(key).and_then(|v| v.as_i64()).unwrap_or(0) as usize };

        let get_i64 = |key: &str| -> i64 { json.get(key).and_then(|v| v.as_i64()).unwrap_or(0) };

        let max_seq_len = {
            let v = get_usize("max_seq_len");
            if v > 0 {
                v
            } else {
                448
            }
        };

        let config = StreamingConfig {
            encoder_dim: get_usize("encoder_dim"),
            decoder_dim: get_usize("decoder_dim"),
            depth: get_usize("depth"),
            nheads: get_usize("nheads"),
            head_dim: get_usize("head_dim"),
            vocab_size: get_usize("vocab_size"),
            bos_id: get_i64("bos_id"),
            eos_id: get_i64("eos_id"),
            frame_len: get_usize("frame_len"),
            total_lookahead: get_usize("total_lookahead"),
            d_model_frontend: get_usize("d_model_frontend"),
            c1: get_usize("c1"),
            c2: get_usize("c2"),
            max_seq_len,
        };

        if config.depth == 0 || config.decoder_dim == 0 || config.vocab_size == 0 {
            return Err(TranscribeError::Config(
                "Invalid streaming config: depth, decoder_dim, and vocab_size must be > 0"
                    .to_string(),
            ));
        }

        Ok(config)
    }
}

/// Internal state for streaming inference.
pub struct StreamingState {
    // Frontend state
    pub sample_buffer: Vec<f32>,
    pub sample_len: i64,
    pub conv1_buffer: Vec<f32>,
    pub conv2_buffer: Vec<f32>,
    pub frame_count: i64,
    // Feature accumulator
    pub accumulated_features: Vec<f32>,
    pub accumulated_feature_count: i32,
    // Encoder tracking
    pub encoder_frames_emitted: i32,
    // Adapter position tracking
    pub adapter_pos_offset: i64,
    // Memory accumulator
    pub memory: Vec<f32>,
    pub memory_len: i32,
    // Decoder self-attention KV cache
    pub k_self: Vec<f32>,
    pub v_self: Vec<f32>,
    pub cache_seq_len: i32,
    // Cross-attention KV cache
    pub k_cross: Vec<f32>,
    pub v_cross: Vec<f32>,
    pub cross_len: i32,
    pub cross_kv_valid: bool,
}

impl StreamingState {
    fn new(config: &StreamingConfig) -> Self {
        let mut state = StreamingState {
            sample_buffer: Vec::new(),
            sample_len: 0,
            conv1_buffer: Vec::new(),
            conv2_buffer: Vec::new(),
            frame_count: 0,
            accumulated_features: Vec::new(),
            accumulated_feature_count: 0,
            encoder_frames_emitted: 0,
            adapter_pos_offset: 0,
            memory: Vec::new(),
            memory_len: 0,
            k_self: Vec::new(),
            v_self: Vec::new(),
            cache_seq_len: 0,
            k_cross: Vec::new(),
            v_cross: Vec::new(),
            cross_len: 0,
            cross_kv_valid: false,
        };
        state.reset(config);
        state
    }

    fn reset(&mut self, config: &StreamingConfig) {
        self.sample_buffer = vec![0.0f32; 79];
        self.sample_len = 0;
        self.conv1_buffer = vec![0.0f32; config.d_model_frontend * 4];
        self.conv2_buffer = vec![0.0f32; config.c1 * 4];
        self.frame_count = 0;
        self.accumulated_features.clear();
        self.accumulated_feature_count = 0;
        self.encoder_frames_emitted = 0;
        self.adapter_pos_offset = 0;
        self.memory.clear();
        self.memory_len = 0;
        self.k_self.clear();
        self.v_self.clear();
        self.cache_seq_len = 0;
        self.k_cross.clear();
        self.v_cross.clear();
        self.cross_len = 0;
        self.cross_kv_valid = false;
    }
}

/// Binary tokenizer for streaming models.
struct BinTokenizer {
    tokens_to_bytes: Vec<Vec<u8>>,
}

impl BinTokenizer {
    fn new(path: &Path) -> Result<Self, TranscribeError> {
        let tokenizer_path = path.join("tokenizer.bin");

        if !tokenizer_path.exists() {
            return Err(TranscribeError::ModelNotFound(tokenizer_path));
        }

        let mut file = File::open(&tokenizer_path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        let mut tokens_to_bytes = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let first_byte = data[offset];
            offset += 1;

            if first_byte == 0 {
                tokens_to_bytes.push(Vec::new());
                continue;
            }

            let byte_count = if first_byte < 128 {
                first_byte as usize
            } else {
                if offset >= data.len() {
                    break;
                }
                let second_byte = data[offset];
                offset += 1;
                (second_byte as usize * 128) + first_byte as usize - 128
            };

            if offset + byte_count > data.len() {
                break;
            }

            let bytes = data[offset..offset + byte_count].to_vec();
            offset += byte_count;
            tokens_to_bytes.push(bytes);
        }

        if tokens_to_bytes.is_empty() {
            return Err(TranscribeError::Config(
                "No tokens found in tokenizer.bin".to_string(),
            ));
        }

        Ok(Self { tokens_to_bytes })
    }

    fn decode(&self, tokens: &[i64]) -> Result<String, TranscribeError> {
        let mut result_bytes: Vec<u8> = Vec::new();

        for &token in tokens {
            let idx = token as usize;
            if idx >= self.tokens_to_bytes.len() {
                continue;
            }
            let bytes = &self.tokens_to_bytes[idx];
            if bytes.is_empty() {
                continue;
            }
            if bytes.len() > 2 && bytes[0] == b'<' && bytes[bytes.len() - 1] == b'>' {
                continue;
            }
            result_bytes.extend_from_slice(bytes);
        }

        let text = String::from_utf8_lossy(&result_bytes);
        let text = text.replace('\u{2581}', " ");
        let text = text.trim().to_string();

        Ok(text)
    }
}

/// Streaming Moonshine model with 5 ONNX sessions.
pub struct StreamingModel {
    frontend: Session,
    encoder: Session,
    adapter: Session,
    cross_kv: Session,
    decoder_kv: Session,
    tokenizer: BinTokenizer,
    config: StreamingConfig,
}

impl StreamingModel {
    pub fn load(
        model_dir: &Path,
        num_threads: usize,
        quantization: &Quantization,
    ) -> Result<Self, TranscribeError> {
        let config = StreamingConfig::load(model_dir)?;

        let load = |name: &str| -> Result<Session, TranscribeError> {
            // Try quantized variants first if requested, preferring .ort format
            let suffix = match quantization {
                Quantization::FP32 => None,
                Quantization::FP16 => Some("fp16"),
                Quantization::Int8 => Some("int8"),
            };

            let candidates: Vec<std::path::PathBuf> = if let Some(suffix) = suffix {
                vec![
                    model_dir.join(format!("{}.{}.ort", name, suffix)),
                    model_dir.join(format!("{}.ort", name)),
                    model_dir.join(format!("{}.{}.onnx", name, suffix)),
                    model_dir.join(format!("{}.onnx", name)),
                ]
            } else {
                vec![
                    model_dir.join(format!("{}.ort", name)),
                    model_dir.join(format!("{}.onnx", name)),
                ]
            };

            for path in &candidates {
                if path.exists() {
                    log::info!("Loading streaming model component: {}", path.display());
                    return Ok(session::create_session_with_threads(path, num_threads)?);
                }
            }

            Err(TranscribeError::ModelNotFound(
                candidates.into_iter().next().unwrap(),
            ))
        };

        let frontend = load("frontend")?;
        let encoder = load("encoder")?;
        let adapter = load("adapter")?;
        let cross_kv = load("cross_kv")?;
        let decoder_kv = load("decoder_kv")?;

        let tokenizer = BinTokenizer::new(model_dir)?;

        log::info!("Loaded streaming model from {:?}", model_dir);

        Ok(Self {
            frontend,
            encoder,
            adapter,
            cross_kv,
            decoder_kv,
            tokenizer,
            config,
        })
    }

    /// Transcribe with model-specific parameters.
    pub fn transcribe_with(
        &mut self,
        samples: &[f32],
        params: &MoonshineStreamingParams,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let tokens = self.generate(samples, 6.5, params.max_length)?;
        let text = self.tokenizer.decode(&tokens)?;

        Ok(TranscriptionResult {
            text,
            segments: None,
        })
    }

    fn create_state(&self) -> StreamingState {
        StreamingState::new(&self.config)
    }

    fn process_audio_chunk(
        &mut self,
        state: &mut StreamingState,
        audio_chunk: &[f32],
    ) -> Result<i32, TranscribeError> {
        if audio_chunk.is_empty() {
            return Ok(0);
        }

        let chunk_len = audio_chunk.len();

        let audio_dyn = ArrayD::from_shape_vec(IxDyn(&[1, chunk_len]), audio_chunk.to_vec())?;

        let sample_buffer_dyn =
            ArrayD::from_shape_vec(IxDyn(&[1, 79]), state.sample_buffer.clone())?;

        let sample_len_dyn = ArrayD::from_shape_vec(IxDyn(&[1]), vec![state.sample_len])?;

        let conv1_dyn = ArrayD::from_shape_vec(
            IxDyn(&[1, self.config.d_model_frontend, 4]),
            state.conv1_buffer.clone(),
        )?;

        let conv2_dyn =
            ArrayD::from_shape_vec(IxDyn(&[1, self.config.c1, 4]), state.conv2_buffer.clone())?;

        let frame_count_dyn = ArrayD::from_shape_vec(IxDyn(&[1]), vec![state.frame_count])?;

        let t_audio_chunk = TensorRef::from_array_view(audio_dyn.view())?;
        let t_sample_buffer = TensorRef::from_array_view(sample_buffer_dyn.view())?;
        let t_sample_len = TensorRef::from_array_view(sample_len_dyn.view())?;
        let t_conv1_buffer = TensorRef::from_array_view(conv1_dyn.view())?;
        let t_conv2_buffer = TensorRef::from_array_view(conv2_dyn.view())?;
        let t_frame_count = TensorRef::from_array_view(frame_count_dyn.view())?;
        let run_inputs = inputs![
            "audio_chunk" => t_audio_chunk,
            "sample_buffer" => t_sample_buffer,
            "sample_len" => t_sample_len,
            "conv1_buffer" => t_conv1_buffer,
            "conv2_buffer" => t_conv2_buffer,
            "frame_count" => t_frame_count,
        ];

        let outputs = self.frontend.run(run_inputs)?;

        let features = outputs
            .get("features")
            .ok_or_else(|| TranscribeError::Inference("Missing output: features".to_string()))?
            .try_extract_array::<f32>()?;

        let feat_shape = features.shape();
        let num_features = feat_shape[1] as i32;

        if num_features > 0 {
            let feat_data = features
                .as_slice()
                .ok_or_else(|| TranscribeError::Inference("features not contiguous".to_string()))?;
            let feat_size = feat_shape[1] * feat_shape[2];
            state
                .accumulated_features
                .extend_from_slice(&feat_data[..feat_size]);
            state.accumulated_feature_count += num_features;
        }

        // Update frontend state from outputs
        let sample_buffer_out = outputs
            .get("sample_buffer_out")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: sample_buffer_out".to_string())
            })?
            .try_extract_array::<f32>()?;
        state.sample_buffer = sample_buffer_out.as_slice().unwrap()[..79].to_vec();

        let sample_len_out = outputs
            .get("sample_len_out")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: sample_len_out".to_string())
            })?
            .try_extract_array::<i64>()?;
        state.sample_len = sample_len_out.as_slice().unwrap()[0];

        let conv1_out = outputs
            .get("conv1_buffer_out")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: conv1_buffer_out".to_string())
            })?
            .try_extract_array::<f32>()?;
        let conv1_data = conv1_out.as_slice().unwrap();
        let conv1_expected = self.config.d_model_frontend * 4;
        if conv1_data.len() >= conv1_expected {
            state.conv1_buffer = conv1_data[..conv1_expected].to_vec();
        } else {
            state.conv1_buffer = vec![0.0; conv1_expected];
            state.conv1_buffer[..conv1_data.len()].copy_from_slice(conv1_data);
        }

        let conv2_out = outputs
            .get("conv2_buffer_out")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: conv2_buffer_out".to_string())
            })?
            .try_extract_array::<f32>()?;
        let conv2_data = conv2_out.as_slice().unwrap();
        let conv2_expected = self.config.c1 * 4;
        if conv2_data.len() >= conv2_expected {
            state.conv2_buffer = conv2_data[..conv2_expected].to_vec();
        } else {
            state.conv2_buffer = vec![0.0; conv2_expected];
            state.conv2_buffer[..conv2_data.len()].copy_from_slice(conv2_data);
        }

        let frame_count_out = outputs
            .get("frame_count_out")
            .ok_or_else(|| {
                TranscribeError::Inference("Missing output: frame_count_out".to_string())
            })?
            .try_extract_array::<i64>()?;
        state.frame_count = frame_count_out.as_slice().unwrap()[0];

        Ok(num_features)
    }

    fn encode_streaming(
        &mut self,
        state: &mut StreamingState,
        is_final: bool,
    ) -> Result<i32, TranscribeError> {
        let total_features = state.accumulated_feature_count;
        if total_features == 0 {
            return Ok(0);
        }

        let stable_count = if is_final {
            total_features
        } else {
            (total_features - self.config.total_lookahead as i32).max(0)
        };

        let new_frames = stable_count - state.encoder_frames_emitted;
        if new_frames <= 0 {
            return Ok(0);
        }

        let left_context_frames = (16 * self.config.depth) as i32;
        let window_start = (state.encoder_frames_emitted - left_context_frames).max(0);
        let window_size = total_features - window_start;

        let start_idx = (window_start as usize) * self.config.encoder_dim;
        let end_idx = start_idx + (window_size as usize) * self.config.encoder_dim;
        let window_features = &state.accumulated_features[start_idx..end_idx];

        let features_view = ArrayViewD::from_shape(
            IxDyn(&[1, window_size as usize, self.config.encoder_dim]),
            window_features,
        )?;

        let t_features = TensorRef::from_array_view(features_view)?;
        let enc_inputs = inputs![
            "features" => t_features,
        ];

        let enc_outputs = self.encoder.run(enc_inputs)?;

        let encoded = enc_outputs
            .get("encoded")
            .ok_or_else(|| TranscribeError::Inference("Missing output: encoded".to_string()))?
            .try_extract_array::<f32>()?;

        let enc_shape = encoded.shape();
        let total_encoded = enc_shape[1] as i32;
        let encoded_data = encoded
            .as_slice()
            .ok_or_else(|| TranscribeError::Inference("encoded not contiguous".to_string()))?;

        let slice_start = (state.encoder_frames_emitted - window_start) as usize;
        if slice_start + new_frames as usize > total_encoded as usize {
            return Err(TranscribeError::Inference(format!(
                "Encoder window misaligned: start={}, new_frames={}, total={}",
                slice_start, new_frames, total_encoded
            )));
        }

        let new_encoded: Vec<f32> = (0..new_frames as usize)
            .flat_map(|i| {
                let base = (slice_start + i) * self.config.encoder_dim;
                encoded_data[base..base + self.config.encoder_dim]
                    .iter()
                    .copied()
            })
            .collect();

        // Run adapter
        let enc_slice_view = ArrayViewD::from_shape(
            IxDyn(&[1, new_frames as usize, self.config.encoder_dim]),
            &new_encoded,
        )?;

        let pos_offset_val = [state.adapter_pos_offset];
        let pos_offset_view = ArrayViewD::from_shape(IxDyn(&[1]), &pos_offset_val)?;

        let t_encoded = TensorRef::from_array_view(enc_slice_view)?;
        let t_pos_offset = TensorRef::from_array_view(pos_offset_view)?;
        let adapter_inputs = inputs![
            "encoded" => t_encoded,
            "pos_offset" => t_pos_offset,
        ];

        let adapter_outputs = self.adapter.run(adapter_inputs)?;

        let memory_out = adapter_outputs
            .get("memory")
            .ok_or_else(|| TranscribeError::Inference("Missing output: memory".to_string()))?
            .try_extract_array::<f32>()?;

        let mem_data = memory_out
            .as_slice()
            .ok_or_else(|| TranscribeError::Inference("memory not contiguous".to_string()))?;
        let mem_size = new_frames as usize * self.config.decoder_dim;
        state.memory.extend_from_slice(&mem_data[..mem_size]);
        state.memory_len += new_frames;

        state.cross_kv_valid = false;
        state.encoder_frames_emitted = stable_count;
        state.adapter_pos_offset += new_frames as i64;

        Ok(new_frames)
    }

    fn compute_cross_kv(&mut self, state: &mut StreamingState) -> Result<(), TranscribeError> {
        if state.memory_len == 0 {
            return Err(TranscribeError::Inference(
                "Memory is empty, cannot compute cross K/V".to_string(),
            ));
        }

        let memory_view = ArrayViewD::from_shape(
            IxDyn(&[1, state.memory_len as usize, self.config.decoder_dim]),
            &state.memory,
        )?;

        let t_memory = TensorRef::from_array_view(memory_view)?;
        let run_inputs = inputs![
            "memory" => t_memory,
        ];

        let outputs = self.cross_kv.run(run_inputs)?;

        let k_cross = outputs
            .get("k_cross")
            .ok_or_else(|| TranscribeError::Inference("Missing output: k_cross".to_string()))?
            .try_extract_array::<f32>()?;

        let v_cross = outputs
            .get("v_cross")
            .ok_or_else(|| TranscribeError::Inference("Missing output: v_cross".to_string()))?
            .try_extract_array::<f32>()?;

        let k_shape = k_cross.shape();
        let cross_len = k_shape[3] as i32;
        let kv_size =
            self.config.depth * self.config.nheads * cross_len as usize * self.config.head_dim;

        state.k_cross = k_cross.as_slice().unwrap()[..kv_size].to_vec();
        state.v_cross = v_cross.as_slice().unwrap()[..kv_size].to_vec();
        state.cross_len = cross_len;
        state.cross_kv_valid = true;

        Ok(())
    }

    fn run_decoder(
        &mut self,
        state: &mut StreamingState,
        token: i64,
    ) -> Result<ort::session::SessionOutputs<'_>, TranscribeError> {
        if !state.cross_kv_valid {
            self.compute_cross_kv(state)?;
        }

        let cache_len = state.cache_seq_len as usize;
        let kv_self_size =
            self.config.depth * self.config.nheads * cache_len * self.config.head_dim;

        if state.k_self.len() != kv_self_size {
            state.k_self.resize(kv_self_size, 0.0f32);
            state.v_self.resize(kv_self_size, 0.0f32);
        }

        let token_val = [token];
        let token_view = ArrayViewD::from_shape(IxDyn(&[1, 1]), &token_val)?;

        let kv_shape = &[
            self.config.depth,
            1,
            self.config.nheads,
            cache_len,
            self.config.head_dim,
        ];
        let k_self_view = ArrayViewD::from_shape(IxDyn(kv_shape), &state.k_self)?;
        let v_self_view = ArrayViewD::from_shape(IxDyn(kv_shape), &state.v_self)?;

        let cross_len = state.cross_len as usize;
        let cross_shape = &[
            self.config.depth,
            1,
            self.config.nheads,
            cross_len,
            self.config.head_dim,
        ];
        let k_cross_view = ArrayViewD::from_shape(IxDyn(cross_shape), &state.k_cross)?;
        let v_cross_view = ArrayViewD::from_shape(IxDyn(cross_shape), &state.v_cross)?;

        let t_token = TensorRef::from_array_view(token_view)?;
        let t_k_self = TensorRef::from_array_view(k_self_view)?;
        let t_v_self = TensorRef::from_array_view(v_self_view)?;
        let t_k_cross = TensorRef::from_array_view(k_cross_view)?;
        let t_v_cross = TensorRef::from_array_view(v_cross_view)?;
        let run_inputs = inputs![
            "token" => t_token,
            "k_self" => t_k_self,
            "v_self" => t_v_self,
            "out_k_cross" => t_k_cross,
            "out_v_cross" => t_v_cross,
        ];

        let outputs = self.decoder_kv.run(run_inputs)?;

        let k_self_out = outputs
            .get("out_k_self")
            .ok_or_else(|| TranscribeError::Inference("Missing output: out_k_self".to_string()))?
            .try_extract_array::<f32>()?;

        let v_self_out = outputs
            .get("out_v_self")
            .ok_or_else(|| TranscribeError::Inference("Missing output: out_v_self".to_string()))?
            .try_extract_array::<f32>()?;

        let new_cache_len = k_self_out.shape()[3] as i32;
        let new_cache_size =
            self.config.depth * self.config.nheads * new_cache_len as usize * self.config.head_dim;

        let k_src = &k_self_out.as_slice().unwrap()[..new_cache_size];
        let v_src = &v_self_out.as_slice().unwrap()[..new_cache_size];

        state.k_self.resize(new_cache_size, 0.0);
        state.k_self.copy_from_slice(k_src);
        state.v_self.resize(new_cache_size, 0.0);
        state.v_self.copy_from_slice(v_src);
        state.cache_seq_len = new_cache_len;

        Ok(outputs)
    }

    fn decode_step_greedy(
        &mut self,
        state: &mut StreamingState,
        token: i64,
    ) -> Result<i64, TranscribeError> {
        let vocab_size = self.config.vocab_size;
        let outputs = self.run_decoder(state, token)?;

        let logits = outputs
            .get("logits")
            .ok_or_else(|| TranscribeError::Inference("Missing output: logits".to_string()))?
            .try_extract_array::<f32>()?;

        let logits_data = logits.as_slice().unwrap();
        let vocab = &logits_data[..vocab_size];

        let mut best_idx = 0u32;
        let mut best_val = vocab[0];
        for (i, &v) in vocab.iter().enumerate().skip(1) {
            if v > best_val {
                best_val = v;
                best_idx = i as u32;
            }
        }

        Ok(best_idx as i64)
    }

    fn generate(
        &mut self,
        samples: &[f32],
        max_tokens_per_second: f32,
        max_tokens_override: Option<usize>,
    ) -> Result<Vec<i64>, TranscribeError> {
        let mut state = self.create_state();

        for chunk in samples.chunks(CHUNK_SIZE) {
            self.process_audio_chunk(&mut state, chunk)?;
        }

        self.encode_streaming(&mut state, true)?;

        if state.memory_len == 0 {
            return Ok(Vec::new());
        }

        self.compute_cross_kv(&mut state)?;

        let max_tokens = match max_tokens_override {
            Some(m) => m.min(self.config.max_seq_len),
            None => {
                let duration_sec = samples.len() as f32 / SAMPLE_RATE as f32;
                ((duration_sec * max_tokens_per_second).ceil() as usize)
                    .min(self.config.max_seq_len)
            }
        };

        let mut tokens: Vec<i64> = Vec::new();
        let mut current_token = self.config.bos_id;

        for _step in 0..max_tokens {
            let next_token = self.decode_step_greedy(&mut state, current_token)?;

            if next_token == self.config.eos_id {
                break;
            }

            tokens.push(next_token);
            current_token = next_token;
        }

        Ok(tokens)
    }
}

impl SpeechModel for StreamingModel {
    fn capabilities(&self) -> ModelCapabilities {
        STREAMING_CAPABILITIES
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        _options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let tokens = self.generate(samples, 6.5, None)?;
        let text = self.tokenizer.decode(&tokens)?;

        Ok(TranscriptionResult {
            text,
            segments: None,
        })
    }
}
