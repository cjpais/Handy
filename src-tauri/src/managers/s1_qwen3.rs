//! Memory-bounded, CPU-only quantized Qwen3 inference for S1-mini.
//!
//! Derived from Hugging Face Candle 0.11's `quantized_qwen3.rs`, copyright
//! the Candle contributors, licensed under MIT OR Apache-2.0:
//! <https://github.com/huggingface/candle/blob/0.11.0/candle-transformers/src/models/quantized_qwen3.rs>.
//! This focused adaptation keeps the tied token embedding quantized and makes
//! both rotary tables and the raw KV cache match the request's context bound.

use candle_core::quantized::{gguf_file, QMatMul as QuantizedMatMul, QTensor};
use candle_core::{DType, Device, Module, Result, Storage, Tensor};
use candle_nn::attention::cpu_flash::causal::causal_decode_f32_interleaved;
use candle_nn::attention::{flash_attn, AttnMask};
use candle_nn::kv_cache::{InterleavedKvCache, RawInterleavedKvCache};
use candle_nn::Activation;
use candle_transformers::models::with_tracing::QMatMul;
use candle_transformers::quantized_nn::RmsNorm;
use std::io::{Read, Seek};
use std::sync::Arc;

struct Gguf<R: Read + Seek> {
    content: gguf_file::Content,
    reader: R,
    device: Device,
}

impl<R: Read + Seek> Gguf<R> {
    fn new(content: gguf_file::Content, reader: R, device: Device) -> Self {
        Self {
            content,
            reader,
            device,
        }
    }

    fn qmatmul(&mut self, name: &str) -> Result<QMatMul> {
        let weights = self.content.tensor(&mut self.reader, name, &self.device)?;
        QMatMul::from_weights(weights.into())
    }

    fn rms_norm(&mut self, name: &str, epsilon: f64) -> Result<RmsNorm> {
        let weights = self.content.tensor(&mut self.reader, name, &self.device)?;
        RmsNorm::from_qtensor(weights, epsilon)
    }

    fn tensor(&mut self, name: &str) -> Result<QTensor> {
        self.content.tensor(&mut self.reader, name, &self.device)
    }

    fn metadata(&self, name: &str) -> Result<&gguf_file::Value> {
        self.content
            .metadata
            .get(name)
            .ok_or_else(|| candle_core::Error::Msg(format!("cannot find {name} in metadata")))
    }
}

#[derive(Debug, Clone)]
struct MlpWeights {
    gate_proj: QMatMul,
    up_proj: QMatMul,
    down_proj: QMatMul,
    activation: Activation,
}

impl MlpWeights {
    fn new<R: Read + Seek>(gguf: &mut Gguf<R>, prefix: &str) -> Result<Self> {
        Ok(Self {
            gate_proj: gguf.qmatmul(&format!("{prefix}.ffn_gate.weight"))?,
            up_proj: gguf.qmatmul(&format!("{prefix}.ffn_up.weight"))?,
            down_proj: gguf.qmatmul(&format!("{prefix}.ffn_down.weight"))?,
            activation: Activation::Silu,
        })
    }
}

impl Module for MlpWeights {
    fn forward(&self, input: &Tensor) -> Result<Tensor> {
        let gate = self.gate_proj.forward(input)?.apply(&self.activation)?;
        let up = self.up_proj.forward(input)?;
        self.down_proj.forward(&(gate * up)?)
    }
}

#[derive(Debug, Clone)]
struct RotaryEmbedding {
    sin: Tensor,
    cos: Tensor,
}

impl RotaryEmbedding {
    fn new(
        dtype: DType,
        head_dim: usize,
        context_capacity: usize,
        rope_theta: f64,
        device: &Device,
    ) -> Result<Self> {
        let inverse_frequencies = (0..head_dim)
            .step_by(2)
            .map(|index| 1f32 / rope_theta.powf(index as f64 / head_dim as f64) as f32)
            .collect::<Vec<_>>();
        let inverse_frequencies =
            Tensor::from_vec(inverse_frequencies, (1, head_dim / 2), device)?.to_dtype(dtype)?;
        let positions = Tensor::arange(0u32, context_capacity as u32, device)?
            .to_dtype(dtype)?
            .reshape((context_capacity, 1))?;
        let frequencies = positions.matmul(&inverse_frequencies)?;
        Ok(Self {
            sin: frequencies.sin()?,
            cos: frequencies.cos()?,
        })
    }

    fn apply(&self, query: &Tensor, key: &Tensor, offset: usize) -> Result<(Tensor, Tensor)> {
        let (_, _, sequence_length, _) = query.dims4()?;
        let cos = self
            .cos
            .narrow(0, offset, sequence_length)?
            .to_dtype(query.dtype())?;
        let sin = self
            .sin
            .narrow(0, offset, sequence_length)?
            .to_dtype(query.dtype())?;
        Ok((
            candle_nn::rotary_emb::rope(&query.contiguous()?, &cos, &sin)?,
            candle_nn::rotary_emb::rope(&key.contiguous()?, &cos, &sin)?,
        ))
    }
}

#[derive(Debug, Clone)]
struct AttentionWeights {
    q_proj: QMatMul,
    k_proj: QMatMul,
    v_proj: QMatMul,
    o_proj: QMatMul,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    head_dim: usize,
    hidden_size: usize,
    rotary: Arc<RotaryEmbedding>,
    interleaved_cache: InterleavedKvCache,
    raw_cache: RawInterleavedKvCache,
}

impl AttentionWeights {
    #[allow(clippy::too_many_arguments)]
    fn new<R: Read + Seek>(
        gguf: &mut Gguf<R>,
        num_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rms_norm_epsilon: f64,
        rotary: Arc<RotaryEmbedding>,
        context_capacity: usize,
        prefix: &str,
    ) -> Result<Self> {
        Ok(Self {
            q_proj: gguf.qmatmul(&format!("{prefix}.attn_q.weight"))?,
            k_proj: gguf.qmatmul(&format!("{prefix}.attn_k.weight"))?,
            v_proj: gguf.qmatmul(&format!("{prefix}.attn_v.weight"))?,
            o_proj: gguf.qmatmul(&format!("{prefix}.attn_output.weight"))?,
            q_norm: gguf.rms_norm(&format!("{prefix}.attn_q_norm.weight"), rms_norm_epsilon)?,
            k_norm: gguf.rms_norm(&format!("{prefix}.attn_k_norm.weight"), rms_norm_epsilon)?,
            num_heads,
            num_kv_heads,
            head_dim,
            hidden_size: num_heads * head_dim,
            rotary,
            interleaved_cache: InterleavedKvCache::new(head_dim),
            raw_cache: RawInterleavedKvCache::new(num_kv_heads, head_dim, context_capacity),
        })
    }

    fn forward(&mut self, input: &Tensor, offset: usize) -> Result<Tensor> {
        let (batch_size, sequence_length, _) = input.dims3()?;
        if batch_size != 1 {
            candle_core::bail!("S1-mini Qwen3 requires a batch size of one")
        }
        if self.raw_cache.len() != offset {
            candle_core::bail!(
                "S1-mini Qwen3 cache has {} positions, expected {offset}",
                self.raw_cache.len()
            )
        }

        let query = self
            .q_proj
            .forward(input)?
            .reshape((batch_size, sequence_length, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let key = self
            .k_proj
            .forward(input)?
            .reshape((
                batch_size,
                sequence_length,
                self.num_kv_heads,
                self.head_dim,
            ))?
            .transpose(1, 2)?;
        let value = self
            .v_proj
            .forward(input)?
            .reshape((
                batch_size,
                sequence_length,
                self.num_kv_heads,
                self.head_dim,
            ))?
            .transpose(1, 2)?;

        let query = self.q_norm.forward(&query.flatten(0, 2)?)?.reshape((
            batch_size,
            self.num_heads,
            sequence_length,
            self.head_dim,
        ))?;
        let key = self.k_norm.forward(&key.flatten(0, 2)?)?.reshape((
            batch_size,
            self.num_kv_heads,
            sequence_length,
            self.head_dim,
        ))?;
        let (query, key) = self.rotary.apply(&query, &key, offset)?;
        let scale = 1.0 / (self.head_dim as f32).sqrt();

        if sequence_length == 1 && query.dtype() == DType::F32 {
            self.decode(query, key, value, scale)
        } else {
            self.prefill(query, key, value, scale, offset)
        }
    }

    fn decode(&mut self, query: Tensor, key: Tensor, value: Tensor, scale: f32) -> Result<Tensor> {
        let query = query.squeeze(0)?.squeeze(1)?.contiguous()?;
        let key = key.squeeze(0)?.squeeze(1)?.contiguous()?;
        let value = value.squeeze(0)?.squeeze(1)?.contiguous()?;
        let (query_guard, query_layout) = query.storage_and_layout();
        let (key_guard, key_layout) = key.storage_and_layout();
        let (value_guard, value_layout) = value.storage_and_layout();
        let query_data = match &*query_guard {
            Storage::Cpu(storage) => &storage.as_slice::<f32>()?[query_layout.start_offset()..],
            _ => candle_core::bail!("expected CPU query storage"),
        };
        let key_data = match &*key_guard {
            Storage::Cpu(storage) => &storage.as_slice::<f32>()?[key_layout.start_offset()..],
            _ => candle_core::bail!("expected CPU key storage"),
        };
        let value_data = match &*value_guard {
            Storage::Cpu(storage) => &storage.as_slice::<f32>()?[value_layout.start_offset()..],
            _ => candle_core::bail!("expected CPU value storage"),
        };

        let kv_length = self.num_kv_heads * self.head_dim;
        self.raw_cache
            .write_kv(&key_data[..kv_length], &value_data[..kv_length]);
        let context = causal_decode_f32_interleaved(
            &query_data[..self.num_heads * self.head_dim],
            self.raw_cache.data(),
            self.num_heads,
            self.num_kv_heads,
            self.head_dim,
            self.raw_cache.len(),
            scale,
        )?;
        context
            .reshape((1, 1, self.hidden_size))?
            .apply(&self.o_proj)
    }

    fn prefill(
        &mut self,
        query: Tensor,
        key: Tensor,
        value: Tensor,
        scale: f32,
        offset: usize,
    ) -> Result<Tensor> {
        let sequence_length = query.dim(2)?;
        let key_values = self.interleaved_cache.append(&key, &value)?;

        let key_contiguous = key.squeeze(0)?.transpose(0, 1)?.contiguous()?;
        let value_contiguous = value.squeeze(0)?.transpose(0, 1)?.contiguous()?;
        let (key_guard, key_layout) = key_contiguous.storage_and_layout();
        let (value_guard, value_layout) = value_contiguous.storage_and_layout();
        let key_data = match &*key_guard {
            Storage::Cpu(storage) => &storage.as_slice::<f32>()?[key_layout.start_offset()..],
            _ => candle_core::bail!("expected CPU key storage"),
        };
        let value_data = match &*value_guard {
            Storage::Cpu(storage) => &storage.as_slice::<f32>()?[value_layout.start_offset()..],
            _ => candle_core::bail!("expected CPU value storage"),
        };
        self.raw_cache
            .write_kv_batch(key_data, value_data, sequence_length);

        let key = key_values
            .narrow(2, 0, self.head_dim)?
            .unsqueeze(0)?
            .contiguous()?;
        let value = key_values
            .narrow(2, self.head_dim, self.head_dim)?
            .unsqueeze(0)?
            .contiguous()?;
        let query = query.transpose(1, 2)?.contiguous()?;
        let context = flash_attn::<f32>(
            &query,
            &key,
            &value,
            scale,
            AttnMask::causal_with_offset(offset),
            None,
            None,
        )?;
        context
            .transpose(1, 2)?
            .reshape((1, sequence_length, self.hidden_size))?
            .apply(&self.o_proj)
    }

    fn clear_kv_cache(&mut self) {
        self.interleaved_cache.reset();
        self.raw_cache.reset();
    }
}

#[derive(Debug, Clone)]
struct LayerWeights {
    attention: AttentionWeights,
    mlp: MlpWeights,
    attention_norm: RmsNorm,
    feed_forward_norm: RmsNorm,
}

impl LayerWeights {
    #[allow(clippy::too_many_arguments)]
    fn new<R: Read + Seek>(
        gguf: &mut Gguf<R>,
        num_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rms_norm_epsilon: f64,
        rotary: Arc<RotaryEmbedding>,
        context_capacity: usize,
        layer_index: usize,
    ) -> Result<Self> {
        let prefix = format!("blk.{layer_index}");
        Ok(Self {
            attention_norm: gguf
                .rms_norm(&format!("{prefix}.attn_norm.weight"), rms_norm_epsilon)?,
            feed_forward_norm: gguf
                .rms_norm(&format!("{prefix}.ffn_norm.weight"), rms_norm_epsilon)?,
            attention: AttentionWeights::new(
                gguf,
                num_heads,
                num_kv_heads,
                head_dim,
                rms_norm_epsilon,
                rotary,
                context_capacity,
                &prefix,
            )?,
            mlp: MlpWeights::new(gguf, &prefix)?,
        })
    }

    fn forward(&mut self, input: &Tensor, offset: usize) -> Result<Tensor> {
        let attention = self.attention_norm.forward(input)?;
        let attention = self.attention.forward(&attention, offset)?;
        let residual = (input + attention)?;
        let feed_forward = self.feed_forward_norm.forward(&residual)?;
        residual + self.mlp.forward(&feed_forward)?
    }

    fn clear_kv_cache(&mut self) {
        self.attention.clear_kv_cache();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ModelWeights {
    embed_tokens: QuantizedMatMul,
    layers: Vec<LayerWeights>,
    output_norm: RmsNorm,
    lm_head: QuantizedMatMul,
    context_capacity: usize,
}

impl ModelWeights {
    pub(crate) fn from_gguf<R: Read + Seek>(
        content: gguf_file::Content,
        reader: &mut R,
        device: &Device,
        context_capacity: usize,
    ) -> Result<Self> {
        if !device.is_cpu() {
            candle_core::bail!("S1-mini Qwen3 currently supports CPU inference only")
        }
        if context_capacity == 0 {
            candle_core::bail!("S1-mini Qwen3 context capacity must be positive")
        }

        let mut gguf = Gguf::new(content, reader, device.clone());
        let num_heads = gguf.metadata("qwen3.attention.head_count")?.to_u32()? as usize;
        let num_kv_heads = gguf.metadata("qwen3.attention.head_count_kv")?.to_u32()? as usize;
        let head_dim = gguf.metadata("qwen3.attention.key_length")?.to_u32()? as usize;
        let num_layers = gguf.metadata("qwen3.block_count")?.to_u32()? as usize;
        let model_context = gguf.metadata("qwen3.context_length")?.to_u32()? as usize;
        let rms_norm_epsilon = gguf
            .metadata("qwen3.attention.layer_norm_rms_epsilon")?
            .to_f32()? as f64;
        let rope_theta = gguf.metadata("qwen3.rope.freq_base")?.to_f32()? as f64;
        if context_capacity > model_context {
            candle_core::bail!(
                "S1-mini Qwen3 context capacity {context_capacity} exceeds model limit {model_context}"
            )
        }

        let rotary_dtype = match gguf.content.metadata.get("general.dtype") {
            Some(value) => match value.to_u32() {
                Ok(0) => DType::F32,
                Ok(1) => DType::F16,
                _ => DType::F16,
            },
            None => DType::F16,
        };
        let embedding_tensor = Arc::new(gguf.tensor("token_embd.weight")?);
        let embed_tokens = QuantizedMatMul::from_arc(embedding_tensor.clone())?;
        let rotary = Arc::new(RotaryEmbedding::new(
            rotary_dtype,
            head_dim,
            context_capacity,
            rope_theta,
            device,
        )?);
        let mut layers = Vec::with_capacity(num_layers);
        for layer_index in 0..num_layers {
            layers.push(LayerWeights::new(
                &mut gguf,
                num_heads,
                num_kv_heads,
                head_dim,
                rms_norm_epsilon,
                rotary.clone(),
                context_capacity,
                layer_index,
            )?);
        }
        let output_norm = gguf.rms_norm("output_norm.weight", rms_norm_epsilon)?;
        let lm_head = match gguf.tensor("output.weight") {
            Ok(tensor) => QuantizedMatMul::from_qtensor(tensor)?,
            Err(_) => QuantizedMatMul::from_arc(embedding_tensor)?,
        };
        Ok(Self {
            embed_tokens,
            layers,
            output_norm,
            lm_head,
            context_capacity,
        })
    }

    pub(crate) fn forward(&mut self, input: &Tensor, offset: usize) -> Result<Tensor> {
        let (batch_size, sequence_length) = input.dims2()?;
        if batch_size != 1 || sequence_length == 0 {
            candle_core::bail!("S1-mini Qwen3 expects one non-empty token sequence")
        }
        let end = offset
            .checked_add(sequence_length)
            .ok_or_else(|| candle_core::Error::Msg("S1-mini Qwen3 position overflow".into()))?;
        if end > self.context_capacity {
            candle_core::bail!(
                "S1-mini Qwen3 needs {end} positions, capacity is {}",
                self.context_capacity
            )
        }

        let mut hidden = self.embed_tokens.embedding(input)?;
        for layer in &mut self.layers {
            hidden = layer.forward(&hidden, offset)?;
        }
        let hidden = self.output_norm.forward(&hidden)?;
        self.lm_head
            .forward(&hidden.narrow(1, sequence_length - 1, 1)?)?
            .squeeze(1)
    }

    pub(crate) fn clear_kv_cache(&mut self) {
        for layer in &mut self.layers {
            layer.clear_kv_cache();
        }
    }
}
