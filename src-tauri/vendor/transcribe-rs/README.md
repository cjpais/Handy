# transcribe-rs

Multi-engine speech-to-text library for Rust. Supports Parakeet, Canary, Moonshine, SenseVoice, GigaAM, Whisper, Whisperfile, and OpenAI.

## Breaking Changes in 0.3.0

Version 0.3.0 changes the `SpeechModel` trait. If you need the old API, pin to `version = "=0.2.9"`.

- `transcribe()` and `transcribe_file()` now take `&TranscribeOptions` instead of `Option<&str>` for language
- `SpeechModel` requires `Send`, enabling `Box<dyn SpeechModel + Send>` across threads
- `TranscribeOptions` includes a `translate` field for Whisper/Whisperfile translation support
- `WhisperEngine::capabilities()` now returns actual model language support (English-only vs multilingual) instead of always reporting 99 languages

**Note:** 0.3.0 is a large migration. We believe correctness is preserved for all engines, but expect potential issues as this stabilizes. Please report any problems on [GitHub](https://github.com/cjpais/transcribe-rs/issues).

## Installation

```toml
[dependencies]
transcribe-rs = { version = "0.3", features = ["onnx"] }
```

No features are enabled by default. Pick the engines you need:

| Feature | Engines |
|---------|---------|
| `onnx` | Parakeet, Canary, Moonshine, SenseVoice, GigaAM (via ONNX Runtime) |
| `whisper-cpp` | Whisper (local, GGML via whisper.cpp with Metal/Vulkan) |
| `whisperfile` | Whisperfile (local server wrapper) |
| `openai` | OpenAI API (remote, async) |
| `all` | Everything above |

GPU accelerator features for ORT engines:

| Feature | Backend |
|---------|---------|
| `ort-cuda` | NVIDIA CUDA |
| `ort-rocm` | AMD ROCm |
| `ort-directml` | Microsoft DirectML (Windows) |

## Quick Start

```rust
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;
use std::path::PathBuf;

let mut model = ParakeetModel::load(
    &PathBuf::from("models/parakeet-tdt-0.6b-v3-int8"),
    &Quantization::Int8,
)?;

let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from("audio.wav"))?;
let result = model.transcribe_with(
    &samples,
    &ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    },
)?;
println!("{}", result.text);
```

All local engines implement the `SpeechModel` trait. Remote engines (OpenAI) implement `RemoteTranscriptionEngine` separately because they are async and file-based.

## Hardware Acceleration

By default, engines use CPU. To enable GPU acceleration, enable the appropriate feature and set the accelerator preference before loading any models:

```rust
use transcribe_rs::{set_ort_accelerator, OrtAccelerator};

// Use CUDA for all ORT engines (SenseVoice, GigaAM, Parakeet, Moonshine)
set_ort_accelerator(OrtAccelerator::Cuda);

// Or auto-detect the best available GPU
set_ort_accelerator(OrtAccelerator::Auto);
```

For whisper.cpp, GPU backend (Metal, Vulkan) is selected at compile time. You can control whether GPU is used at runtime:

```rust
use transcribe_rs::{set_whisper_accelerator, WhisperAccelerator};

set_whisper_accelerator(WhisperAccelerator::CpuOnly); // force CPU
```

**DirectML note:** DirectML requires special ORT session settings (`parallel_execution(false)`, `memory_pattern(false)`) that would hurt performance on other backends. Because of this, `Auto` mode does not include DirectML — you must explicitly select it with `OrtAccelerator::DirectMl`.

Query which ORT accelerators are compiled in with `OrtAccelerator::available()`.

## Usage by Engine

### Canary

```rust
use transcribe_rs::onnx::canary::{CanaryModel, CanaryParams};
use transcribe_rs::onnx::Quantization;
use std::path::PathBuf;

let mut model = CanaryModel::load(
    &PathBuf::from("models/canary-1b-v2"),
    &Quantization::Int8,
)?;

let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from("audio.wav"))?;
let result = model.transcribe_with(
    &samples,
    &CanaryParams {
        language: Some("en".to_string()),
        ..Default::default()
    },
)?;
```

Canary supports translation via `target_language`:

```rust
let result = model.transcribe_with(
    &samples,
    &CanaryParams {
        language: Some("de".to_string()),
        target_language: Some("en".to_string()),
        ..Default::default()
    },
)?;
```

Model variant (Flash vs V2) is auto-detected from vocabulary size. Flash models support en/de/es/fr; V2 supports 25 languages.

**Features:**
- **PnC** (punctuation and capitalization) — enabled by default. When on, the model adds proper punctuation and capitalization. Set `use_pnc: false` for raw output.
- **ITN** (inverse text normalization) — enabled by default. Converts spoken numbers to written form (e.g. "one hundred twenty three" becomes "123"). Set `use_itn: false` to disable. Only supported on V2 models; silently ignored on Flash.
- **Translation** — set `target_language` to translate between supported languages.

### SenseVoice

```rust
use transcribe_rs::onnx::sense_voice::{SenseVoiceModel, SenseVoiceParams};
use transcribe_rs::onnx::Quantization;
use std::path::PathBuf;

let mut model = SenseVoiceModel::load(
    &PathBuf::from("models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"),
    &Quantization::Int8,
)?;

let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from("audio.wav"))?;
let result = model.transcribe_with(
    &samples,
    &SenseVoiceParams {
        language: Some("en".to_string()),
        ..Default::default()
    },
)?;
```

### Moonshine

```rust
use transcribe_rs::onnx::moonshine::{MoonshineModel, MoonshineVariant};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;
use std::path::PathBuf;

let mut model = MoonshineModel::load(
    &PathBuf::from("models/moonshine-base"),
    MoonshineVariant::Base,
    &Quantization::default(),
)?;
let result = model.transcribe_file(&PathBuf::from("audio.wav"), &transcribe_rs::TranscribeOptions::default())?;
```

Streaming variant:

```rust
use transcribe_rs::onnx::moonshine::StreamingModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;
use std::path::PathBuf;

let mut model = StreamingModel::load(
    &PathBuf::from("models/moonshine-streaming/moonshine-tiny-streaming-en"),
    4,  // threads
    &Quantization::default(),
)?;
let result = model.transcribe_file(&PathBuf::from("audio.wav"), &transcribe_rs::TranscribeOptions::default())?;
```

### GigaAM

```rust
use transcribe_rs::onnx::gigaam::GigaAMModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;
use std::path::PathBuf;

let mut model = GigaAMModel::load(
    &PathBuf::from("models/giga-am-v3"),
    &Quantization::default(),
)?;
let result = model.transcribe_file(&PathBuf::from("audio.wav"), &transcribe_rs::TranscribeOptions::default())?;
```

### Whisper (whisper.cpp)

```rust
use transcribe_rs::whisper_cpp::{WhisperEngine, WhisperInferenceParams};
use std::path::PathBuf;

let mut engine = WhisperEngine::load(&PathBuf::from("models/whisper-medium-q4_1.bin"))?;

let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from("audio.wav"))?;
let result = engine.transcribe_with(
    &samples,
    &WhisperInferenceParams {
        initial_prompt: Some("Context prompt here.".to_string()),
        ..Default::default()
    },
)?;
```

### Whisperfile

```rust
use transcribe_rs::whisperfile::{
    WhisperfileEngine, WhisperfileInferenceParams, WhisperfileLoadParams,
};
use std::path::PathBuf;

let mut engine = WhisperfileEngine::load_with_params(
    &PathBuf::from("models/whisperfile-0.9.3"),
    &PathBuf::from("models/ggml-small.bin"),
    WhisperfileLoadParams {
        port: 8080,
        startup_timeout_secs: 60,
        ..Default::default()
    },
)?;

let samples = transcribe_rs::audio::read_wav_samples(&PathBuf::from("audio.wav"))?;
let result = engine.transcribe_with(
    &samples,
    &WhisperfileInferenceParams {
        language: Some("en".to_string()),
        ..Default::default()
    },
)?;
// Server shuts down automatically when engine is dropped.
```

### OpenAI (Remote)

```rust
use transcribe_rs::remote::openai::{self, OpenAIModel, OpenAIRequestParams};
use transcribe_rs::{remote, RemoteTranscriptionEngine};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = openai::default_engine();
    let result = engine
        .transcribe_file(
            &PathBuf::from("audio.wav"),
            OpenAIRequestParams::builder()
                .model(OpenAIModel::Gpt4oMiniTranscribe)
                .timestamp_granularity(remote::openai::OpenAITimestampGranularity::Segment)
                .build()?,
        )
        .await?;
    println!("{}", result.text);
    Ok(())
}
```

## Models

All audio input must be **16 kHz, mono, 16-bit PCM WAV**.

### Model Downloads

| Engine | Download |
|--------|----------|
| Parakeet (int8) | [blob.handy.computer](https://blob.handy.computer/parakeet-v3-int8.tar.gz) / [HuggingFace](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/tree/main) |
| Canary 180M Flash | [HuggingFace](https://huggingface.co/istupakov/canary-180m-flash-onnx) |
| Canary 1B Flash | [HuggingFace](https://huggingface.co/istupakov/canary-1b-flash-onnx) |
| Canary 1B v2 | [HuggingFace](https://huggingface.co/istupakov/canary-1b-v2-onnx) |
| SenseVoice (int8) | [blob.handy.computer](https://blob.handy.computer/sense-voice-int8.tar.gz) / [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models) |
| Moonshine | [HuggingFace](https://huggingface.co/UsefulSensors/moonshine/tree/main/onnx/merged) |
| GigaAM | [HuggingFace](https://huggingface.co/istupakov/gigaam-v3-onnx/tree/main) |
| Whisper (GGML) | [HuggingFace](https://huggingface.co/ggerganov/whisper.cpp/tree/main) |
| Whisperfile binary | [GitHub](https://github.com/mozilla-ai/llamafile/releases/download/0.9.3/whisperfile-0.9.3) |

### Directory Layouts

**Parakeet** (directory):
```
models/parakeet-tdt-0.6b-v3-int8/
├── encoder-model.int8.onnx
├── decoder_joint-model.int8.onnx
├── nemo128.onnx
└── vocab.txt
```

**Canary** (directory):
```
models/canary-1b-v2/
├── encoder-model.int8.onnx
├── decoder-model.int8.onnx
├── nemo128.onnx
└── vocab.txt
```

**SenseVoice** (directory):
```
models/sense-voice/
├── model.int8.onnx
└── tokens.txt
```

**Moonshine** (directory):
```
models/moonshine-base/
├── encoder_model.onnx
├── decoder_model_merged.onnx
└── tokenizer.json
```

**Moonshine Streaming** (directory):
```
models/moonshine-streaming/moonshine-tiny-streaming-en/
├── encoder.onnx
├── decoder.onnx
├── streaming_config.json
└── tokenizer.json
```

**GigaAM** (directory):
```
models/giga-am-v3/
├── model.onnx          (or model.int8.onnx)
└── vocab.txt
```

**Whisper**: single file (e.g. `whisper-medium-q4_1.bin`).

### Moonshine Variants

| Variant | Language |
|---------|----------|
| Tiny | English |
| TinyAr | Arabic |
| TinyZh | Chinese |
| TinyJa | Japanese |
| TinyKo | Korean |
| TinyUk | Ukrainian |
| TinyVi | Vietnamese |
| Base | English |
| BaseEs | Spanish |

## Examples and Tests

Each engine has an example in `examples/`. Run with the appropriate feature flag:

```bash
cargo run --example parakeet --features onnx
cargo run --example canary --features onnx
cargo run --example sense_voice --features onnx
cargo run --example moonshine --features onnx
cargo run --example moonshine_streaming --features onnx
cargo run --example gigaam --features onnx
cargo run --example whisper --features whisper-cpp
cargo run --example whisperfile --features whisperfile
cargo run --example openai --features openai
```

Tests are also feature-gated. Models must be present locally; tests skip gracefully if not found.

```bash
cargo test --features onnx
cargo test --features whisper-cpp
cargo test --features whisperfile
cargo test --all-features
```

Whisperfile tests look for the binary at `models/whisperfile-0.9.3` (override with `WHISPERFILE_BIN`) and model at `models/ggml-small.bin` (override with `WHISPERFILE_MODEL`). GigaAM tests require `samples/russian.wav`.

Development aliases from `.cargo/config.toml`:

```bash
cargo check-all    # cargo check --all-features
cargo build-all    # cargo build --all-features
cargo test-all     # cargo test --all-features
```

## Performance

Parakeet int8 benchmarks:

| Platform | Speed |
|----------|-------|
| MBP M4 Max | ~30x real-time |
| Zen 3 (5700X) | ~20x real-time |
| Skylake (i5-6500) | ~5x real-time |
| Jetson Nano CPU | ~5x real-time |

## Acknowledgments

- [istupakov](https://github.com/istupakov/onnx-asr) for the ONNX Parakeet, Canary, and GigaAM exports
- [NVIDIA](https://github.com/NVIDIA/NeMo) for Parakeet and Canary
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp)
- [jart](http://github.com/jart) / [Mozilla AI](https://github.com/mozilla-ai) for [llamafile](https://github.com/mozilla-ai/llamafile) and Whisperfile
- [UsefulSensors](https://github.com/usefulsensors) for Moonshine
- [FunASR](https://github.com/modelscope/FunASR) / [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) for SenseVoice
- [SberDevices](https://github.com/salute-developers/GigaAM) for GigaAM
