# Adding a New ONNX Model

This guide covers adding a new speech recognition model to the `onnx` engine family.

## Directory Structure

Every model gets its own directory under `src/onnx/`, even if it starts as a single file:

```
src/onnx/
  mod.rs            # Engine-level module, registers all models
  session.rs        # Shared ONNX session utilities
  your_model/
    mod.rs          # Model implementation (start here)
```

If the model grows, split into sibling files:

```
src/onnx/your_model/
  mod.rs            # Re-exports, variant enum, constants
  model.rs          # Core model struct + load + infer
  streaming.rs      # Streaming variant (if applicable)
```

## Step-by-Step

### 1. Create the model directory and mod.rs

```
mkdir src/onnx/your_model
touch src/onnx/your_model/mod.rs
```

### 2. Register the module

Add to `src/onnx/mod.rs`:

```rust
pub mod your_model;
```

### 3. Implement the model

Use this skeleton in `src/onnx/your_model/mod.rs`. GigaAM (`src/onnx/gigaam/mod.rs`) is the simplest reference — start there.

```rust
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;

use crate::decode::{ctc_greedy_decode, sentencepiece_to_text};
use crate::decode::tokens::load_vocab;
use crate::features::{compute_mel, MelConfig, WindowType};
use crate::TranscribeError;
use super::session;
use super::Quantization;
use crate::{ModelCapabilities, SpeechModel, TranscriptionResult};

const CAPABILITIES: ModelCapabilities = ModelCapabilities {
    name: "Your Model",           // Human-readable name
    engine_id: "your_model",      // Machine identifier, matches directory name
    sample_rate: 16000,           // Expected input sample rate
    languages: &["en"],           // Supported language codes
    supports_timestamps: false,
    supports_translation: false,
    supports_streaming: false,
};

/// Per-model inference parameters.
#[derive(Debug, Clone, Default)]
pub struct YourModelParams {
    pub language: Option<String>,
    // Add model-specific params here
}

pub struct YourModel {
    session: Session,
    // Add model state: vocab, mel config, metadata, etc.
}

impl YourModel {
    /// Load the model from a directory.
    ///
    /// The directory must contain:
    /// - `model.onnx` (or `model.int8.onnx`, `model.fp16.onnx`)
    /// - Any required vocab/token files
    pub fn load(model_dir: &Path, quantization: &Quantization) -> Result<Self, TranscribeError> {
        let model_path = session::resolve_model_path(model_dir, "model", quantization);

        if !model_path.exists() {
            return Err(TranscribeError::ModelNotFound(model_path));
        }

        let session = session::create_session(&model_path)?;

        Ok(Self { session })
    }

    /// Transcribe with model-specific parameters.
    pub fn transcribe_with(
        &mut self,
        samples: &[f32],
        params: &YourModelParams,
    ) -> Result<TranscriptionResult, TranscribeError> {
        self.infer(samples)
    }

    fn infer(&mut self, samples: &[f32]) -> Result<TranscriptionResult, TranscribeError> {
        // 1. Feature extraction (mel spectrogram, FBANK, etc.)
        // 2. Prepare input tensors
        // 3. Run ONNX forward pass: self.session.run(inputs)?
        // 4. Decode output (CTC greedy, beam search, etc.)
        // 5. Return TranscriptionResult { text, segments }
        todo!()
    }
}

impl SpeechModel for YourModel {
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
```

### 4. Add a test

Create `tests/your_model.rs`:

```rust
mod common;

use std::path::PathBuf;
use transcribe_rs::onnx::your_model::YourModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

#[test]
fn test_your_model_transcribe() {
    env_logger::init();

    let model_dir = PathBuf::from("models/your-model");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        YourModel::load(&model_dir, &Quantization::default()).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, None)
        .expect("Failed to transcribe");

    assert!(!result.text.is_empty(), "Transcription should not be empty");
    println!("Transcription: {}", result.text);
}
```

Register the test in `Cargo.toml`:

```toml
[[test]]
name = "your_model"
required-features = ["onnx"]
```

### 5. Add an example

Create `examples/your_model.rs` (see `examples/gigaam.rs` for the pattern).

Register in `Cargo.toml`:

```toml
[[example]]
name = "your_model"
required-features = ["onnx"]
```

### 6. Prepare model files

Place model files in `models/your-model/`:

```
models/your-model/
  model.onnx              # FP32 model (or model.int8.onnx, model.fp16.onnx)
  vocab.txt               # Vocabulary file (format: "token id" per line)
  tokens.txt              # Alternative token file (if model uses SymbolTable format)
```

The naming convention for quantized variants is `model.{quantization}.onnx`:
- `model.onnx` — FP32 (default)
- `model.fp16.onnx` — FP16
- `model.int8.onnx` — INT8

`session::resolve_model_path()` handles fallback automatically.

## Shared Utilities

### Feature Extraction (`crate::features`)

- `compute_mel(samples, &MelConfig)` — Mel spectrogram, returns `[frames, mels]`
- `apply_lfr(features, window_size, shift)` — Low frame rate downsampling
- `apply_cmvn(features, neg_mean, inv_stddev)` — Cepstral mean-variance normalization
- `MelConfig` — Configure sample rate, num_mels, n_fft, hop_length, window type, etc.
- `WindowType::Hamming` / `WindowType::Hann` — Window functions

### Decoding (`crate::decode`)

- `ctc_greedy_decode(logits, lengths, blank_id)` — CTC greedy search, returns token IDs + timestamps
- `sentencepiece_to_text(tokens)` — Join SentencePiece tokens into text (handles `▁` markers)
- `load_vocab(path)` — Load `token id` format vocab file, returns `(Vec<String>, Option<blank_idx>)`
- `SymbolTable::load(path)` — Load symbol table (for models like SenseVoice with metadata-rich tokens)

### Session Utilities (`super::session`)

- `create_session(path)` — Create ONNX session with standard settings
- `create_session_with_threads(path, n)` — Create session with explicit thread count
- `resolve_model_path(dir, name, &Quantization)` — Resolve quantized model file path
- `read_metadata_str/i32/float_vec(session, key)` — Read ONNX model metadata

## Key Patterns

- **Constructor**: Always `Model::load(model_dir: &Path, quantization: &Quantization) -> Result<Self, TranscribeError>`
- **Params struct**: Always `{Model}Params` with `#[derive(Debug, Clone, Default)]`
- **Two transcribe methods**: `transcribe_with(&mut self, samples, &Params)` for model-specific params, plus `SpeechModel::transcribe()` for the generic trait interface
- **Error handling**: Use `TranscribeError` variants — `ModelNotFound`, `Inference`, `Audio`, `Config`
- **Session mutability**: `session.run(inputs)` requires `&mut Session`
- **Input construction**: Use `inputs!["name" => TensorRef::from_array_view(arr.view())]`
- **Output extraction**: `outputs[0].try_extract_array::<f32>()?`

## Existing Models as Reference

| Model | Complexity | Good reference for |
|---|---|---|
| `gigaam` | Simple | Single-session CTC model, minimal features |
| `sense_voice` | Medium | ONNX metadata parsing, FBANK+LFR+CMVN pipeline, SymbolTable |
| `parakeet` | Complex | Multi-session (encoder/decoder/preprocessor), RNN-T decode, timestamps |
| `moonshine` | Complex | Multi-file split, autoregressive decoding, streaming variant |
