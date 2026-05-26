//! # transcribe-rs
//!
//! A Rust library providing unified transcription capabilities using multiple speech recognition engines.
//!
//! ## Features
//!
//! - **ONNX Models**: SenseVoice, GigaAM, Parakeet, Moonshine (requires `onnx` feature)
//! - **Whisper**: OpenAI Whisper via GGML (requires `whisper-cpp` feature)
//! - **Whisperfile**: Mozilla Whisperfile server (requires `whisperfile` feature)
//! - **Remote**: OpenAI API (requires `openai` feature)
//! - **Timestamped Results**: Detailed timing information for transcribed segments
//! - **Unified API**: `SpeechModel` trait for all local engines
//! - **Hardware Acceleration**: GPU support for ORT engines (`ort-cuda`, `ort-rocm`,
//!   `ort-directml`) and whisper.cpp (Metal/Vulkan) via the [`accel`] module
//!
//! ## Backend Categories
//!
//! This crate provides two categories of transcription backend:
//!
//! - **Local models** implement [`SpeechModel`] and run inference in-process or via
//!   a local binary. This includes all ONNX models, Whisper (via whisper.cpp), and
//!   Whisperfile.
//! - **Remote services** implement [`RemoteTranscriptionEngine`] (requires `openai`
//!   feature) and make async network calls to external APIs. This includes OpenAI.
//!
//! These traits are intentionally separate because the execution model differs:
//! local models are synchronous and take audio samples directly, while remote
//! services are async and may only accept file uploads.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! transcribe-rs = { version = "0.3", features = ["onnx"] }
//! ```
//!
//! ```ignore
//! use std::path::PathBuf;
//! use transcribe_rs::onnx::sense_voice::{SenseVoiceModel, SenseVoiceParams};
//! use transcribe_rs::onnx::Quantization;
//! use transcribe_rs::SpeechModel;
//!
//! let mut model = SenseVoiceModel::load(
//!     &PathBuf::from("models/sense-voice"),
//!     &Quantization::Int8,
//! )?;
//!
//! let result = model.transcribe(&samples, &transcribe_rs::TranscribeOptions::default())?;
//! println!("Transcription: {}", result.text);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Audio Requirements
//!
//! Input audio files must be:
//! - WAV format
//! - 16 kHz sample rate
//! - 16-bit samples
//! - Mono (single channel)
//!
//! ## Migrating from 0.2.x to 0.3.0
//!
//! Version 0.3.0 is a breaking release. If you need the old API, pin to `version = "=0.2.9"`.
//!
//! **`SpeechModel::transcribe` signature changed:**
//!
//! ```rust,ignore
//! // Before (0.2.x):
//! model.transcribe(&samples, Some("en"))?;
//! model.transcribe_file(&path, None)?;
//!
//! // After (0.3.0):
//! use transcribe_rs::TranscribeOptions;
//! model.transcribe(&samples, &TranscribeOptions { language: Some("en".into()), ..Default::default() })?;
//! model.transcribe_file(&path, &TranscribeOptions::default())?;
//! ```
//!
//! **`SpeechModel` now requires `Send`**, enabling `Box<dyn SpeechModel + Send>` for
//! use across threads.
//!
//! **`TranscribeOptions` includes a `translate` field** (default `false`). Engines that
//! support translation (Whisper, Whisperfile) will translate to English when set to `true`.
//!
//! **Whisper capabilities are now dynamic.** `WhisperEngine::capabilities()` returns the
//! actual language support of the loaded model (English-only vs multilingual) rather than
//! always reporting all 99 languages.

pub mod accel;
pub mod audio;
pub mod error;
pub use accel::{
    get_ort_accelerator, get_whisper_accelerator, set_ort_accelerator, set_whisper_accelerator,
    OrtAccelerator, WhisperAccelerator,
};
pub use error::TranscribeError;

#[cfg(feature = "audio-features")]
pub mod decode;
#[cfg(feature = "audio-features")]
pub mod features;
#[cfg(feature = "onnx")]
pub mod onnx;

#[cfg(feature = "whisper-cpp")]
pub mod whisper_cpp;
#[cfg(feature = "whisperfile")]
pub mod whisperfile;

#[cfg(feature = "openai")]
pub mod remote;
#[cfg(feature = "openai")]
pub use remote::RemoteTranscriptionEngine;

use std::path::Path;

/// Describes the capabilities of a speech model.
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    /// Human-readable model name.
    pub name: &'static str,
    /// Machine-friendly engine identifier (e.g. "sense_voice", "whisper_cpp").
    pub engine_id: &'static str,
    /// Expected input sample rate in Hz (e.g. 16000).
    pub sample_rate: u32,
    /// Languages supported (BCP-47 codes, e.g. "en", "zh"). Empty = any/unknown.
    pub languages: &'static [&'static str],
    /// Whether the model can produce word/segment timestamps.
    pub supports_timestamps: bool,
    /// Whether the model can translate to English.
    pub supports_translation: bool,
    /// Whether the model supports streaming inference.
    pub supports_streaming: bool,
}

/// Options for transcription.
#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// Language hint (BCP-47 code, e.g. "en", "zh").
    /// Multilingual models use this as a hint; single-language models ignore it.
    pub language: Option<String>,
    /// Whether to translate the output to English (only supported by some engines).
    pub translate: bool,
}

/// Unified interface for speech-to-text models.
///
/// Each model implements this trait to provide a common transcription API.
/// Model-specific parameters are exposed via a separate `transcribe_with()` method
/// on the concrete type.
pub trait SpeechModel: Send {
    /// Report this model's capabilities.
    fn capabilities(&self) -> ModelCapabilities;

    /// Transcribe audio samples (16 kHz, mono, f32 in [-1, 1]).
    fn transcribe(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError>;

    /// Transcribe a WAV file (16 kHz, 16-bit, mono).
    fn transcribe_file(
        &mut self,
        wav_path: &Path,
        options: &TranscribeOptions,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let samples = audio::read_wav_samples(wav_path)?;
        self.transcribe(&samples, options)
    }
}

/// The result of a transcription operation.
///
/// Contains both the full transcribed text and detailed timing information
/// for individual segments within the audio.
#[derive(Debug)]
pub struct TranscriptionResult {
    /// The complete transcribed text from the audio
    pub text: String,
    /// Individual segments with timing information
    pub segments: Option<Vec<TranscriptionSegment>>,
}

/// A single transcribed segment with timing information.
///
/// Represents a portion of the transcribed audio with start and end timestamps
/// and the corresponding text content.
#[derive(Debug)]
pub struct TranscriptionSegment {
    /// Start time of the segment in seconds
    pub start: f32,
    /// End time of the segment in seconds
    pub end: f32,
    /// The transcribed text for this segment
    pub text: String,
}
