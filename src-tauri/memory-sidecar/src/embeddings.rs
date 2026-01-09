//! Embedding generation using ONNX models from HuggingFace Hub
//!
//! Supports multiple embedding models with different size/quality tradeoffs.
//! Downloads and caches models from HuggingFace Hub.

use anyhow::{Context, Result};
use hf_hub::api::sync::Api;
use log::{debug, info};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

/// Available embedding models with their configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub repo: String,
    pub onnx_file: String,
    pub tokenizer_file: String,
    pub dimension: usize,
    pub size_mb: u32,
}

impl EmbeddingModelInfo {
    /// Get all available embedding models
    pub fn available_models() -> Vec<EmbeddingModelInfo> {
        vec![
            EmbeddingModelInfo {
                id: "minilm-l6".to_string(),
                name: "MiniLM-L6-v2".to_string(),
                description: "Fast, lightweight model (22M params). Good balance of speed and quality.".to_string(),
                repo: "sentence-transformers/all-MiniLM-L6-v2".to_string(),
                onnx_file: "onnx/model.onnx".to_string(),
                tokenizer_file: "tokenizer.json".to_string(),
                dimension: 384,
                size_mb: 90,
            },
            EmbeddingModelInfo {
                id: "minilm-l12".to_string(),
                name: "MiniLM-L12-v2".to_string(),
                description: "More accurate than L6 (33M params). Better quality, slightly slower.".to_string(),
                repo: "sentence-transformers/all-MiniLM-L12-v2".to_string(),
                onnx_file: "onnx/model.onnx".to_string(),
                tokenizer_file: "tokenizer.json".to_string(),
                dimension: 384,
                size_mb: 120,
            },
            EmbeddingModelInfo {
                id: "mpnet-base".to_string(),
                name: "MPNet Base v2".to_string(),
                description: "High quality embeddings (110M params). Best accuracy, slower.".to_string(),
                repo: "sentence-transformers/all-mpnet-base-v2".to_string(),
                onnx_file: "onnx/model.onnx".to_string(),
                tokenizer_file: "tokenizer.json".to_string(),
                dimension: 768,
                size_mb: 420,
            },
        ]
    }

    /// Get model info by ID
    pub fn get_by_id(id: &str) -> Option<EmbeddingModelInfo> {
        Self::available_models().into_iter().find(|m| m.id == id)
    }

    /// Get the default model
    pub fn default_model() -> EmbeddingModelInfo {
        Self::get_by_id("minilm-l6").unwrap()
    }
}

/// Embedding model wrapper
pub struct EmbeddingModel {
    session: Session,
    tokenizer: Tokenizer,
    info: EmbeddingModelInfo,
}

impl EmbeddingModel {
    /// Load an embedding model by ID, downloading if necessary
    pub fn load(model_id: &str) -> Result<Self> {
        let info = EmbeddingModelInfo::get_by_id(model_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown model ID: {}", model_id))?;

        Self::load_model(&info)
    }

    /// Load the default embedding model
    pub fn load_default() -> Result<Self> {
        let info = EmbeddingModelInfo::default_model();
        Self::load_model(&info)
    }

    /// Load a specific embedding model
    fn load_model(info: &EmbeddingModelInfo) -> Result<Self> {
        info!("Loading embedding model: {} ({})", info.name, info.repo);

        let api = Api::new().context("Failed to create HuggingFace API")?;
        let repo = api.model(info.repo.clone());

        // Download model and tokenizer
        let model_path = repo
            .get(&info.onnx_file)
            .context("Failed to download ONNX model")?;
        let tokenizer_path = repo
            .get(&info.tokenizer_file)
            .context("Failed to download tokenizer")?;

        info!("Model downloaded to: {:?}", model_path);

        // Load ONNX model
        let session = Session::builder()
            .context("Failed to create ONNX session builder")?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .context("Failed to set optimization level")?
            .commit_from_file(&model_path)
            .context("Failed to load ONNX model")?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        info!("Embedding model '{}' loaded successfully", info.name);

        Ok(Self {
            session,
            tokenizer,
            info: info.clone(),
        })
    }

    /// Check if a model is downloaded (files exist in HuggingFace cache)
    pub fn is_downloaded(model_id: &str) -> bool {
        let info = match EmbeddingModelInfo::get_by_id(model_id) {
            Some(i) => i,
            None => return false,
        };

        // Try to get model path from cache without downloading
        let api = match Api::new() {
            Ok(a) => a,
            Err(_) => return false,
        };

        let repo = api.model(info.repo);

        // Check if both files exist in cache
        // The hf_hub crate doesn't have a direct "check if cached" method,
        // so we try to get the files - they'll return quickly if cached
        repo.get(&info.onnx_file).is_ok() && repo.get(&info.tokenizer_file).is_ok()
    }

    /// Get the model info
    pub fn info(&self) -> &EmbeddingModelInfo {
        &self.info
    }

    /// Generate embedding for a single text
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text])?;
        Ok(embeddings.into_iter().next().unwrap())
    }

    /// Generate embeddings for a batch of texts
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Tokenize
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let batch_size = encodings.len();
        let seq_len = encodings.iter().map(|e| e.len()).max().unwrap_or(0);

        debug!(
            "Tokenized {} texts, max seq_len: {}",
            batch_size, seq_len
        );

        // Prepare input tensors
        let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * seq_len);
        let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * seq_len);
        let mut token_type_ids: Vec<i64> = Vec::with_capacity(batch_size * seq_len);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Pad to max length
            for i in 0..seq_len {
                if i < ids.len() {
                    input_ids.push(ids[i] as i64);
                    attention_mask.push(mask[i] as i64);
                    token_type_ids.push(0); // Single segment
                } else {
                    input_ids.push(0); // PAD token
                    attention_mask.push(0);
                    token_type_ids.push(0);
                }
            }
        }

        // Create tensors using (shape, data) tuple format
        let shape = [batch_size, seq_len];
        let input_ids_tensor = Tensor::from_array((shape, input_ids))
            .context("Failed to create input_ids tensor")?;
        let attention_mask_tensor = Tensor::from_array((shape, attention_mask))
            .context("Failed to create attention_mask tensor")?;
        let token_type_ids_tensor = Tensor::from_array((shape, token_type_ids))
            .context("Failed to create token_type_ids tensor")?;

        // Run inference
        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .context("ONNX inference failed")?;

        // Extract embeddings - output shape is (batch_size, seq_len, hidden_size)
        // We need to do mean pooling over the sequence dimension
        let output = outputs
            .get("last_hidden_state")
            .or_else(|| outputs.get("token_embeddings"))
            .context("No embedding output found")?;

        // Extract as (shape, data) tuple
        let (shape, data) = output
            .try_extract_tensor::<f32>()
            .context("Failed to extract embeddings tensor")?;

        debug!("Output shape: {:?}", shape);

        // Mean pooling with attention mask
        // shape is [batch_size, seq_len, hidden_size]
        let hidden_size = shape[2] as usize;
        let output_seq_len = shape[1] as usize;
        let mut result = Vec::with_capacity(batch_size);

        for batch_idx in 0..batch_size {
            let mut pooled = vec![0.0f32; hidden_size];
            let mut count = 0.0f32;

            for seq_idx in 0..output_seq_len {
                let mask_val = if seq_idx < encodings[batch_idx].get_attention_mask().len() {
                    encodings[batch_idx].get_attention_mask()[seq_idx] as f32
                } else {
                    0.0
                };

                if mask_val > 0.0 {
                    let offset = batch_idx * output_seq_len * hidden_size + seq_idx * hidden_size;
                    for (h, pooled_val) in pooled.iter_mut().enumerate().take(hidden_size) {
                        *pooled_val += data[offset + h];
                    }
                    count += 1.0;
                }
            }

            // Normalize by count
            if count > 0.0 {
                for val in &mut pooled {
                    *val /= count;
                }
            }

            // L2 normalize
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for val in &mut pooled {
                    *val /= norm;
                }
            }

            result.push(pooled);
        }

        Ok(result)
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        self.info.dimension
    }
}
