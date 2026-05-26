use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;

use super::session;
use super::Quantization;
use crate::decode::tokens::load_vocab;
use crate::decode::{ctc_greedy_decode, sentencepiece_to_text};
use crate::features::{compute_mel, MelConfig, WindowType};
use crate::TranscribeError;
use crate::{ModelCapabilities, SpeechModel, TranscribeOptions, TranscriptionResult};

const CAPABILITIES: ModelCapabilities = ModelCapabilities {
    name: "GigaAM",
    engine_id: "gigaam",
    sample_rate: 16000,
    languages: &["ru"],
    supports_timestamps: false,
    supports_translation: false,
    supports_streaming: false,
};

/// Per-model inference parameters for GigaAM.
#[derive(Debug, Clone, Default)]
pub struct GigaAMParams {
    /// Language hint (currently unused, GigaAM is Russian-only).
    pub language: Option<String>,
}

pub struct GigaAMModel {
    session: Session,
    mel_config: MelConfig,
    vocab: Vec<String>,
    blank_idx: i64,
}

impl GigaAMModel {
    pub fn load(model_dir: &Path, quantization: &Quantization) -> Result<Self, TranscribeError> {
        let model_path = session::resolve_model_path(model_dir, "model", quantization);
        let vocab_path = model_dir.join("vocab.txt");

        if !model_path.exists() {
            return Err(TranscribeError::ModelNotFound(model_path));
        }
        if !vocab_path.exists() {
            return Err(TranscribeError::ModelNotFound(vocab_path));
        }

        log::info!("Loading GigaAM model from {:?}...", model_path);
        let session = session::create_session(&model_path)?;

        let (vocab, blank_idx) = load_vocab(&vocab_path)?;
        let blank_idx = blank_idx.unwrap_or(vocab.len() as i32) as i64;

        log::info!(
            "Loaded vocabulary with {} tokens, blank_idx={}",
            vocab.len(),
            blank_idx
        );

        let mel_config = MelConfig {
            sample_rate: 16000,
            num_mels: 64,
            n_fft: 320,
            hop_length: 160,
            window: WindowType::Hann,
            f_min: 0.0,
            f_max: Some(8000.0),
            pre_emphasis: None,
            snip_edges: false,
            normalize_samples: true,
        };

        Ok(Self {
            session,
            mel_config,
            vocab,
            blank_idx,
        })
    }

    /// Transcribe with model-specific parameters.
    pub fn transcribe_with(
        &mut self,
        samples: &[f32],
        _params: &GigaAMParams,
    ) -> Result<TranscriptionResult, TranscribeError> {
        self.infer(samples)
    }

    fn infer(&mut self, samples: &[f32]) -> Result<TranscriptionResult, TranscribeError> {
        if samples.len() < self.mel_config.n_fft {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: None,
            });
        }

        // 1. Compute mel spectrogram [frames, mels]
        let mel = compute_mel(samples, &self.mel_config);
        let time_steps = mel.shape()[0];

        log::debug!(
            "Mel spectrogram shape: [{}, {}]",
            mel.shape()[0],
            mel.shape()[1]
        );

        // 2. Prepare input tensors: features [1, n_mels, time], feature_lengths [1]
        // ONNX model expects [1, mels, time], so transpose then add batch dim
        let features = mel.t().to_owned().insert_axis(ndarray::Axis(0)); // [1, 64, T]
        let features_dyn = features.into_dyn();
        let feature_lengths = ndarray::arr1(&[time_steps as i64]).into_dyn();

        // 3. Run ONNX forward pass
        let t_features = TensorRef::from_array_view(features_dyn.view())?;
        let t_lengths = TensorRef::from_array_view(feature_lengths.view())?;
        let inputs = inputs! {
            "features" => t_features,
            "feature_lengths" => t_lengths,
        };
        let outputs = self.session.run(inputs)?;

        // 4. Extract log_probs [1, T', vocab_size]
        let log_probs = outputs[0].try_extract_array::<f32>()?;
        let log_probs = log_probs.to_owned().into_dimensionality::<ndarray::Ix3>()?;

        log::debug!("Log probs shape: {:?}", log_probs.shape());

        // 5. CTC greedy decode
        let num_frames = log_probs.shape()[1] as i64;
        let logits_lengths = vec![num_frames];
        let results = ctc_greedy_decode(&log_probs.view(), &logits_lengths, self.blank_idx);

        // 6. Convert token IDs to text
        let tokens: Vec<&str> = results[0]
            .tokens
            .iter()
            .filter_map(|&id| {
                let idx = id as usize;
                if idx < self.vocab.len() {
                    let token = self.vocab[idx].as_str();
                    if token == "<unk>" {
                        None
                    } else {
                        Some(token)
                    }
                } else {
                    None
                }
            })
            .collect();

        let text = sentencepiece_to_text(&tokens);

        Ok(TranscriptionResult {
            text,
            segments: None,
        })
    }
}

impl SpeechModel for GigaAMModel {
    fn capabilities(&self) -> ModelCapabilities {
        CAPABILITIES
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        _options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        self.infer(samples)
    }
}
