# Adding Engines and Models to transcribe-rs

This guide covers the full process of adding new models and new engine families to the library, including tests and examples.

## Architecture Overview

```
src/
  lib.rs              # SpeechModel trait, TranscriptionResult, ModelCapabilities
  error.rs            # TranscribeError enum
  audio.rs            # WAV file reading (read_wav_samples)
  features/           # Shared audio feature extraction (mel, LFR, CMVN)
  decode/             # Shared decoding (CTC, SentencePiece, vocab loading)
  onnx/               # ONNX engine family (feature: "onnx")
    PORTING.md        # Detailed guide for adding ONNX models
    mod.rs            # Quantization enum, registers model modules
    session.rs        # Shared ONNX session utilities
    gigaam/mod.rs     # Model implementation
    sense_voice/mod.rs
    parakeet/mod.rs
    moonshine/        # Multi-file model (mod.rs, model.rs, streaming.rs)
  whisper_cpp/        # whisper.cpp engine (feature: "whisper-cpp")
    mod.rs
  whisperfile.rs      # Whisperfile engine (feature: "whisperfile")
  remote/             # Remote engines (feature: "openai")
    mod.rs            # RemoteTranscriptionEngine trait
    openai.rs
tests/
  common/mod.rs       # Shared test utilities (require_paths)
  gigaam.rs           # One test file per model
  ...
examples/
  gigaam.rs           # One example per model
  ...
```

## Adding a New Model to an Existing Engine

If your model uses an existing inference runtime (e.g. ONNX), see the engine-specific porting guide:

- **ONNX models**: See `src/onnx/PORTING.md`

The short version:

1. Create `src/onnx/your_model/mod.rs`
2. Register in `src/onnx/mod.rs`
3. Implement `SpeechModel` trait
4. Add test, example, and Cargo.toml entries

## Adding a New Engine Family

A new engine family is needed when you're integrating a new inference runtime (e.g. Candle, Burn, MLX, TensorRT). Each engine family gets its own feature flag and source directory.

### 1. Create the source directory

```
src/your_engine/
  mod.rs              # Engine-level types, re-exports model modules
  your_model/
    mod.rs            # First model implementation
```

For single-model engines, a flat file is also acceptable:

```
src/your_engine.rs    # Everything in one file (like whisperfile.rs)
```

### 2. Add the feature flag to Cargo.toml

```toml
[features]
your-engine = ["dep:your-runtime-crate"]

# Update the "all" feature
all = ["onnx", "whisper-cpp", "whisperfile", "openai", "your-engine"]

[dependencies]
your-runtime-crate = { version = "...", optional = true }
```

If your engine needs shared audio feature extraction (mel spectrograms, CTC decoding), depend on the `audio-features` feature:

```toml
your-engine = ["audio-features", "dep:your-runtime-crate"]
```

### 3. Register the module in lib.rs

```rust
#[cfg(feature = "your-engine")]
pub mod your_engine;
```

### 4. Implement the model

Every local model must implement the `SpeechModel` trait:

```rust
pub trait SpeechModel {
    fn capabilities(&self) -> ModelCapabilities;
    fn transcribe(
        &mut self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<TranscriptionResult, TranscribeError>;
    // transcribe_file has a default impl that reads the WAV then calls transcribe()
}
```

Required conventions:

- **CAPABILITIES constant**: Define a `const CAPABILITIES: ModelCapabilities` with all fields populated
- **Constructor**: `Model::load(...)` — single step, returns a ready-to-use model
- **Params struct**: `{Model}Params` with `#[derive(Debug, Clone, Default)]`
- **Two transcribe methods**: `transcribe_with(&mut self, samples, &Params)` for engine-specific params, plus the trait `transcribe()` for the generic interface
- **Errors**: Use `TranscribeError` variants (`ModelNotFound`, `Inference`, `Audio`, `Config`)

### 5. Add error conversions (if needed)

If your runtime crate has its own error type, add a `From` impl in `src/error.rs`:

```rust
#[cfg(feature = "your-engine")]
impl From<your_runtime::Error> for TranscribeError {
    fn from(e: your_runtime::Error) -> Self {
        TranscribeError::Inference(e.to_string())
    }
}
```

### 6. Add a PORTING.md

Create `src/your_engine/PORTING.md` documenting how to add models within this engine family. See `src/onnx/PORTING.md` as a reference.

## Adding Tests

There are two patterns depending on how expensive model loading is.

### Pattern A: Lightweight models (reload per test)

Use this for models that load quickly (most ONNX models). Each test loads its own model instance.

```rust
// tests/your_model.rs
mod common;

use std::path::PathBuf;
use transcribe_rs::your_engine::your_model::YourModel;
use transcribe_rs::SpeechModel;

#[test]
fn test_your_model_transcribe() {
    env_logger::init();

    let model_dir = PathBuf::from("models/your-model");
    let wav_path = PathBuf::from("samples/jfk.wav");

    // Skip gracefully if model files aren't present
    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model = YourModel::load(&model_dir).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, None)
        .expect("Failed to transcribe");

    assert!(!result.text.is_empty(), "Transcription should not be empty");
    println!("Transcription: {}", result.text);
}
```

### Pattern B: Expensive models (shared instance)

Use this for models that are slow to load (whisper.cpp, whisperfile server startup). A `Lazy<Mutex<Option<Engine>>>` shares one instance across all tests in the file.

```rust
// tests/your_model.rs
mod common;

use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Mutex;
use transcribe_rs::your_engine::YourEngine;
use transcribe_rs::SpeechModel;

fn model_path() -> PathBuf {
    PathBuf::from("models/your-model.bin")
}

static ENGINE: Lazy<Mutex<Option<YourEngine>>> = Lazy::new(|| {
    let model = model_path();

    if !common::require_paths(&[&model]) {
        return Mutex::new(None);
    }

    match YourEngine::load(&model) {
        Ok(engine) => Mutex::new(Some(engine)),
        Err(e) => {
            eprintln!("Failed to load model: {}", e);
            Mutex::new(None)
        }
    }
});

fn get_engine() -> Option<std::sync::MutexGuard<'static, Option<YourEngine>>> {
    let guard = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        return None;
    }
    Some(guard)
}

#[test]
fn test_transcription() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: engine not available");
            return;
        }
    };
    let engine = guard.as_mut().unwrap();

    let audio_path = PathBuf::from("samples/jfk.wav");

    let result = engine
        .transcribe_file(&audio_path, None)
        .expect("Failed to transcribe");

    assert!(!result.text.is_empty());
}
```

### Test registration in Cargo.toml

Every test file must be registered with its required feature:

```toml
[[test]]
name = "your_model"
required-features = ["your-engine"]
```

### Test conventions

- Always use `common::require_paths()` to skip gracefully when model files are absent
- Never panic on missing models — CI environments won't have them
- Test at minimum: non-empty transcription output
- If the model is deterministic, assert exact text output
- If the model supports timestamps, add a timestamp test asserting chronological order and reasonable ranges
- Use `env_logger::init()` at the start of tests (only call once per process — fine for single-test files, the Lazy pattern handles multi-test files)

## Adding Examples

Every model should have an example demonstrating load + transcribe.

```rust
// examples/your_model.rs
use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::your_engine::your_model::{YourModel, YourModelParams};
use transcribe_rs::SpeechModel;

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let model_path = PathBuf::from("models/your-model");
    let wav_path = PathBuf::from("samples/jfk.wav");

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    // Load
    let load_start = Instant::now();
    let mut model = YourModel::load(&model_path)?;
    println!("Model loaded in {:.2?}", load_start.elapsed());

    // Transcribe
    let transcribe_start = Instant::now();
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path)?;
    let result = model.transcribe_with(
        &samples,
        &YourModelParams {
            language: Some("en".to_string()),
            ..Default::default()
        },
    )?;
    let transcribe_duration = transcribe_start.elapsed();

    // Results
    println!("Transcription completed in {:.2?}", transcribe_duration);
    println!(
        "Real-time speedup: {:.2}x faster than real-time",
        audio_duration / transcribe_duration.as_secs_f64()
    );
    println!("Transcription result:");
    println!("{}", result.text);

    if let Some(segments) = result.segments {
        println!("\nSegments:");
        for segment in segments {
            println!(
                "[{:.2}s - {:.2}s]: {}",
                segment.start, segment.end, segment.text
            );
        }
    }

    Ok(())
}
```

### Example registration in Cargo.toml

```toml
[[example]]
name = "your_model"
required-features = ["your-engine"]
```

### Example conventions

- Show load timing and transcription timing
- Calculate real-time speedup factor
- Print segments if the model supports timestamps
- Use `transcribe_with()` to demonstrate model-specific params
- Accept model/audio paths as CLI args with sensible defaults

## Checklist

When adding a new model, make sure all of these are done:

- [ ] Model source file: `src/{engine}/{model}/mod.rs`
- [ ] Module registered in engine's `mod.rs`
- [ ] `const CAPABILITIES` with all fields filled in
- [ ] `{Model}Params` struct with `#[derive(Debug, Clone, Default)]`
- [ ] `load()` constructor
- [ ] `transcribe_with()` method
- [ ] `impl SpeechModel` with `capabilities()` and `transcribe()`
- [ ] Test file: `tests/{model}.rs` using `common::require_paths`
- [ ] Test registered in `Cargo.toml` with `required-features`
- [ ] Example file: `examples/{model}.rs`
- [ ] Example registered in `Cargo.toml` with `required-features`
- [ ] Model files placed in `models/{model-name}/` with correct naming

When adding a new engine family, also:

- [ ] Feature flag in `Cargo.toml` `[features]`
- [ ] Feature added to `all` convenience feature
- [ ] Optional dependency in `[dependencies]`
- [ ] `#[cfg(feature = "...")]` guard in `lib.rs`
- [ ] `From` impl in `error.rs` for runtime error type (if applicable)
- [ ] `PORTING.md` in the engine directory
- [ ] If the engine supports GPU, integrate with the accelerator system in `src/accel.rs` (add an enum, global preference, and wire it into session/model creation)
