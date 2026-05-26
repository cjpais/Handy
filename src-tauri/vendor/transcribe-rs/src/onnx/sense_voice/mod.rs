use ndarray::Array1;
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::collections::HashMap;
use std::path::Path;

use super::session;
use super::Quantization;
use crate::decode::{ctc_greedy_decode, CtcDecoderResult, SymbolTable};
use crate::features::{apply_cmvn, apply_lfr, compute_mel, MelConfig, WindowType};
use crate::TranscribeError;
use crate::{
    ModelCapabilities, SpeechModel, TranscribeOptions, TranscriptionResult, TranscriptionSegment,
};

const CAPABILITIES: ModelCapabilities = ModelCapabilities {
    name: "SenseVoice",
    engine_id: "sense_voice",
    sample_rate: 16000,
    languages: &["zh", "en", "ja", "ko", "yue"],
    supports_timestamps: true,
    supports_translation: false,
    supports_streaming: false,
};

/// Per-model inference parameters for SenseVoice.
#[derive(Debug, Clone, Default)]
pub struct SenseVoiceParams {
    /// Language for transcription (e.g. "en", "zh", "auto"). Defaults to "auto".
    pub language: Option<String>,
    /// Whether to apply inverse text normalization. Defaults to true.
    pub use_itn: Option<bool>,
}

// ---- Model ----

struct SenseVoiceMetadata {
    vocab_size: i32,
    blank_id: i32,
    lfr_window_size: usize,
    lfr_window_shift: usize,
    normalize_samples: bool,
    with_itn_id: i32,
    without_itn_id: i32,
    lang2id: HashMap<String, i32>,
    neg_mean: Array1<f32>,
    inv_stddev: Array1<f32>,
    is_funasr_nano: bool,
}

pub struct SenseVoiceModel {
    session: Session,
    metadata: SenseVoiceMetadata,
    symbol_table: SymbolTable,
    input_names: Vec<String>,
}

impl SenseVoiceModel {
    pub fn load(model_dir: &Path, quantization: &Quantization) -> Result<Self, TranscribeError> {
        let model_path = session::resolve_model_path(model_dir, "model", quantization);
        let tokens_path = model_dir.join("tokens.txt");

        if !model_path.exists() {
            return Err(TranscribeError::ModelNotFound(model_path));
        }
        if !tokens_path.exists() {
            return Err(TranscribeError::ModelNotFound(tokens_path));
        }

        log::info!("Loading SenseVoice model from {:?}...", model_path);
        let session = session::create_session(&model_path)?;

        let input_names: Vec<String> = session.inputs.iter().map(|i| i.name.clone()).collect();
        log::debug!("Model inputs: {:?}", input_names);

        let metadata = Self::parse_metadata(&session)?;
        log::info!(
            "Model metadata: vocab_size={}, lfr_window_size={}, lfr_window_shift={}, is_nano={}",
            metadata.vocab_size,
            metadata.lfr_window_size,
            metadata.lfr_window_shift,
            metadata.is_funasr_nano,
        );

        let mut symbol_table = SymbolTable::load(&tokens_path)?;
        if metadata.is_funasr_nano {
            log::info!("FunASR Nano model detected, applying base64 decode to tokens");
            symbol_table.apply_base64_decode();
        }

        Ok(Self {
            session,
            metadata,
            symbol_table,
            input_names,
        })
    }

    fn parse_metadata(session: &Session) -> Result<SenseVoiceMetadata, TranscribeError> {
        let comment = session::read_metadata_str(session, "comment")
            .map_err(|e| {
                TranscribeError::Config(format!("failed to read metadata 'comment': {}", e))
            })?
            .unwrap_or_default();
        let is_funasr_nano = comment.contains("Nano");

        let vocab_size =
            session::read_metadata_i32(session, "vocab_size", None)?.ok_or_else(|| {
                TranscribeError::Config("Missing required metadata key: vocab_size".into())
            })?;
        let blank_id = session::read_metadata_i32(session, "blank_id", Some(0))?.unwrap();
        let lfr_window_size =
            session::read_metadata_i32(session, "lfr_window_size", Some(7))?.unwrap() as usize;
        let lfr_window_shift =
            session::read_metadata_i32(session, "lfr_window_shift", Some(6))?.unwrap() as usize;
        let normalize_samples_int =
            session::read_metadata_i32(session, "normalize_samples", Some(0))?.unwrap();

        let (with_itn_id, without_itn_id, lang2id, neg_mean_vec, inv_stddev_vec) = if is_funasr_nano
        {
            (14, 15, HashMap::new(), Vec::new(), Vec::new())
        } else {
            let with_itn_id = session::read_metadata_i32(session, "with_itn", Some(14))?.unwrap();
            let without_itn_id =
                session::read_metadata_i32(session, "without_itn", Some(15))?.unwrap();

            let mut lang2id = HashMap::new();
            for (lang, key) in [
                ("auto", "lang_auto"),
                ("zh", "lang_zh"),
                ("en", "lang_en"),
                ("ja", "lang_ja"),
                ("ko", "lang_ko"),
                ("yue", "lang_yue"),
            ] {
                if let Some(id) = session::read_metadata_i32(session, key, None)? {
                    lang2id.insert(lang.to_string(), id);
                }
            }
            if lang2id.is_empty() {
                lang2id = HashMap::from([
                    ("auto".to_string(), 0),
                    ("zh".to_string(), 3),
                    ("en".to_string(), 4),
                    ("yue".to_string(), 7),
                    ("ja".to_string(), 11),
                    ("ko".to_string(), 12),
                ]);
            }

            let neg_mean_vec =
                session::read_metadata_float_vec(session, "neg_mean")?.unwrap_or_default();
            let inv_stddev_vec =
                session::read_metadata_float_vec(session, "inv_stddev")?.unwrap_or_default();

            (
                with_itn_id,
                without_itn_id,
                lang2id,
                neg_mean_vec,
                inv_stddev_vec,
            )
        };

        Ok(SenseVoiceMetadata {
            vocab_size,
            blank_id,
            lfr_window_size,
            lfr_window_shift,
            normalize_samples: normalize_samples_int != 0,
            with_itn_id,
            without_itn_id,
            lang2id,
            neg_mean: Array1::from_vec(neg_mean_vec),
            inv_stddev: Array1::from_vec(inv_stddev_vec),
            is_funasr_nano,
        })
    }

    /// Transcribe with model-specific parameters.
    pub fn transcribe_with(
        &mut self,
        samples: &[f32],
        params: &SenseVoiceParams,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let language = params.language.as_deref().unwrap_or("auto");
        let use_itn = params.use_itn.unwrap_or(true);
        self.infer(samples, language, use_itn)
    }

    fn infer(
        &mut self,
        samples: &[f32],
        language: &str,
        use_itn: bool,
    ) -> Result<TranscriptionResult, TranscribeError> {
        // Copy metadata values to avoid borrow conflicts with &mut self
        let normalize_samples = self.metadata.normalize_samples;
        let lfr_window_size = self.metadata.lfr_window_size;
        let lfr_window_shift = self.metadata.lfr_window_shift;
        let is_funasr_nano = self.metadata.is_funasr_nano;
        let blank_id = self.metadata.blank_id as i64;
        let has_cmvn = !is_funasr_nano && !self.metadata.neg_mean.is_empty();
        let neg_mean = self.metadata.neg_mean.clone();
        let inv_stddev = self.metadata.inv_stddev.clone();

        // 1. Compute FBANK features
        let mel_config = MelConfig {
            sample_rate: 16000,
            num_mels: 80,
            n_fft: 400,
            hop_length: 160,
            window: WindowType::Hamming,
            f_min: 20.0,
            f_max: None,
            pre_emphasis: Some(0.97),
            snip_edges: true,
            normalize_samples,
        };
        let features = compute_mel(samples, &mel_config);

        log::debug!(
            "FBANK features: [{}, {}]",
            features.nrows(),
            features.ncols()
        );

        // 2. Apply LFR
        let features = apply_lfr(&features, lfr_window_size, lfr_window_shift);
        log::debug!("After LFR: [{}, {}]", features.nrows(), features.ncols());

        if features.nrows() == 0 {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: None,
            });
        }

        // 3. Apply CMVN
        let mut features = features;
        if has_cmvn {
            apply_cmvn(&mut features, &neg_mean, &inv_stddev);
        }

        let num_feature_frames = features.nrows();

        // 4. Run ONNX forward pass
        let logits = if is_funasr_nano {
            self.forward_nano(&features.view())?
        } else {
            self.forward(&features.view(), language, use_itn)?
        };

        log::debug!("Logits shape: {:?}", logits.shape());

        // 5. CTC greedy decode
        let num_frames = if is_funasr_nano {
            logits.shape()[1] as i64
        } else {
            num_feature_frames as i64 + 4
        };
        let logits_lengths = vec![num_frames];
        let logits_view = logits.view();
        let decoder_results = ctc_greedy_decode(&logits_view, &logits_lengths, blank_id);

        // 6. Convert result
        let result = self.convert_result(&decoder_results[0]);
        Ok(result)
    }

    fn forward(
        &mut self,
        features: &ndarray::ArrayView2<f32>,
        language: &str,
        use_itn: bool,
    ) -> Result<ndarray::Array3<f32>, TranscribeError> {
        let meta = &self.metadata;
        let num_frames = features.nrows() as i32;

        let feat_3d =
            features
                .to_owned()
                .into_shape_with_order((1, features.nrows(), features.ncols()))?;

        let x_length = ndarray::arr1(&[num_frames]);

        let lang_id = if language.is_empty() {
            0i32
        } else {
            *meta
                .lang2id
                .get(language)
                .ok_or_else(|| TranscribeError::Config(format!("Unknown language: {}", language)))?
        };
        let language_arr = ndarray::arr1(&[lang_id]);

        let text_norm_id = if use_itn {
            meta.with_itn_id
        } else {
            meta.without_itn_id
        };
        let text_norm_arr = ndarray::arr1(&[text_norm_id]);

        let feat_dyn = feat_3d.into_dyn();
        let x_length_dyn = x_length.into_dyn();
        let language_dyn = language_arr.into_dyn();
        let text_norm_dyn = text_norm_arr.into_dyn();

        let t_feat = TensorRef::from_array_view(feat_dyn.view())?;
        let t_len = TensorRef::from_array_view(x_length_dyn.view())?;
        let t_lang = TensorRef::from_array_view(language_dyn.view())?;
        let t_norm = TensorRef::from_array_view(text_norm_dyn.view())?;

        let inputs = inputs![
            self.input_names[0].as_str() => t_feat,
            self.input_names[1].as_str() => t_len,
            self.input_names[2].as_str() => t_lang,
            self.input_names[3].as_str() => t_norm,
        ];

        let outputs = self.session.run(inputs)?;
        let logits = outputs[0].try_extract_array::<f32>()?;
        let logits_owned = logits.to_owned().into_dimensionality::<ndarray::Ix3>()?;

        Ok(logits_owned)
    }

    fn forward_nano(
        &mut self,
        features: &ndarray::ArrayView2<f32>,
    ) -> Result<ndarray::Array3<f32>, TranscribeError> {
        let feat_3d =
            features
                .to_owned()
                .into_shape_with_order((1, features.nrows(), features.ncols()))?;

        let feat_dyn = feat_3d.into_dyn();

        let t_feat = TensorRef::from_array_view(feat_dyn.view())?;

        let inputs = inputs![
            self.input_names[0].as_str() => t_feat,
        ];

        let outputs = self.session.run(inputs)?;
        let logits = outputs[0].try_extract_array::<f32>()?;
        let logits_owned = logits.to_owned().into_dimensionality::<ndarray::Ix3>()?;

        Ok(logits_owned)
    }

    fn convert_result(&self, decoder_result: &CtcDecoderResult) -> TranscriptionResult {
        let meta = &self.metadata;
        let tokens = &decoder_result.tokens;
        let timestamps = &decoder_result.timestamps;

        let (start, _language, _emotion, _event) = if meta.is_funasr_nano {
            (0, None, None, None)
        } else {
            let lang = tokens
                .first()
                .and_then(|&id| self.symbol_table.get(id))
                .map(|s| s.to_string());
            let emo = tokens
                .get(1)
                .and_then(|&id| self.symbol_table.get(id))
                .map(|s| s.to_string());
            let evt = tokens
                .get(2)
                .and_then(|&id| self.symbol_table.get(id))
                .map(|s| s.to_string());
            (4usize, lang, emo, evt)
        };

        // Build text from remaining tokens
        let mut text = String::new();
        let mut result_tokens = Vec::new();
        for &id in tokens.iter().skip(start) {
            let sym = self.symbol_table.get_or_empty(id);
            text.push_str(&sym.replace('\u{2581}', " "));
            result_tokens.push(sym.to_string());
        }
        let text = text.trim().to_string();
        let text = text.replace(" '", "'").replace(" \u{2581}'", "'");

        // Calculate timestamps in seconds
        let frame_shift_s = 0.01 * meta.lfr_window_shift as f32;
        let result_timestamps: Vec<f32> = timestamps
            .iter()
            .skip(start)
            .map(|&t| frame_shift_s * (t - start as i32) as f32)
            .collect();

        let segments = if !result_timestamps.is_empty() {
            let mut segs = Vec::new();
            for (i, token) in result_tokens.iter().enumerate() {
                let start_t = result_timestamps.get(i).copied().unwrap_or(0.0);
                let end_t = result_timestamps
                    .get(i + 1)
                    .copied()
                    .unwrap_or(start_t + 0.06);
                segs.push(TranscriptionSegment {
                    start: start_t,
                    end: end_t,
                    text: token.clone(),
                });
            }
            Some(segs)
        } else {
            None
        };

        TranscriptionResult { text, segments }
    }
}

impl SpeechModel for SenseVoiceModel {
    fn capabilities(&self) -> ModelCapabilities {
        CAPABILITIES
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        self.infer(samples, options.language.as_deref().unwrap_or("auto"), true)
    }
}
