//! OpenAI speech to text API
//!
//! This module provides a wrapper of OpenAI speech to text API via
//! `async_openai` crate.
//!
//! Currently supported models are:
//!
//! - `whisper-1`
//! - `gpt-4o-mini-transcribe`
//! - `gpt-4o-transcribe`
//!
//! # Authentication
//!
//! `OpenAIConfig` is built on generics of `async_openai::config::Config`. For
//! most use cases, all you need to do is set `OPENAI_API_KEY` environment
//! variable and use `default_engine()`. For more fine-grained control over
//! the authenticatoin, see `OpenAIEngine<T>::with_config`.
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//! use transcribe_rs::remote::openai::{self, OpenAIModel, OpenAIRequestParams};
//! use transcribe_rs::{remote, RemoteTranscriptionEngine};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let engine = openai::default_engine();
//! let wav_path = PathBuf::from("audio.wav");
//!
//! let result = engine
//!     .transcribe_file(
//!         &wav_path,
//!         OpenAIRequestParams::builder()
//!             .model(OpenAIModel::Whisper1)
//!             .timestamp_granularity(remote::openai::OpenAITimestampGranularity::Segment)
//!             .build()?,
//!     )
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! Note that `timestamp_granularity` is only supported on `whisper-1` model.

use async_openai::{
    config::OpenAIConfig,
    types::{AudioInput, CreateTranscriptionRequestArgs, InputSource},
};
use async_trait::async_trait;
use derive_builder::Builder;

use crate::{
    RemoteTranscriptionEngine, TranscribeError, TranscriptionResult, TranscriptionSegment,
};

#[derive(Debug)]
pub struct OpenAIEngine<T>
where
    T: async_openai::config::Config,
{
    client: async_openai::Client<T>,
}

impl<T> OpenAIEngine<T>
where
    T: async_openai::config::Config,
{
    pub fn with_config(config: T) -> Self {
        Self {
            client: async_openai::Client::with_config(config),
        }
    }
}

pub fn default_engine() -> OpenAIEngine<OpenAIConfig> {
    OpenAIEngine {
        client: async_openai::Client::default(),
    }
}

pub use async_openai::types::TimestampGranularity as OpenAITimestampGranularity;

/// https://docs.rs/async-openai/latest/src/async_openai/types/audio.rs.html#72-99
#[derive(Builder, Debug)]
#[builder(setter(into), default)]
pub struct OpenAIRequestParams {
    model: OpenAIModel,
    /// Language code in ISO-639-1 format.
    language: Option<String>,
    /// A prompt to improve transcription quality with additional context.
    ///
    /// The prompt should match the audio language.
    ///
    /// Example:
    ///
    /// ```text
    /// The following conversation is a lecture about the recent developments
    /// around OpenAI, GPT-4.5 and the future of AI.
    /// ```
    prompt: Option<String>,
    /// The sampling temprature between 0 and 1.
    temperature: Option<f32>,
    /// The timestamp granularities to populate for this transcription.
    ///
    /// Only supported on Whisper model.
    timestamp_granularity: Option<OpenAITimestampGranularity>,
}

impl OpenAIRequestParams {
    pub fn builder() -> OpenAIRequestParamsBuilder {
        OpenAIRequestParamsBuilder::default()
    }
}

impl Default for OpenAIRequestParams {
    fn default() -> Self {
        Self {
            model: OpenAIModel::Gpt4oMiniTranscribe,
            language: None,
            prompt: None,
            temperature: None,
            timestamp_granularity: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum OpenAIModel {
    Whisper1,
    Gpt4oMiniTranscribe,
    Gpt4oTranscribe,
}

impl OpenAIModel {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper1 => "whisper-1",
            Self::Gpt4oMiniTranscribe => "gpt-4o-mini-transcribe",
            Self::Gpt4oTranscribe => "gpt-4o-transcribe",
        }
    }
}

#[async_trait]
impl<T> RemoteTranscriptionEngine for OpenAIEngine<T>
where
    T: async_openai::config::Config,
{
    type RequestParams = OpenAIRequestParams;

    async fn transcribe_file(
        &self,
        wav_path: &std::path::Path,
        params: Self::RequestParams,
    ) -> Result<crate::TranscriptionResult, TranscribeError> {
        let source = AudioInput {
            source: InputSource::Path {
                path: wav_path.to_path_buf(),
            },
        };

        let mut request = CreateTranscriptionRequestArgs::default();

        // mandatory fields
        request.file(source);
        request.model(params.model.as_str());

        if let Some(language) = params.language {
            request.language(language);
        }

        if let Some(prompt) = params.prompt {
            request.prompt(prompt);
        }

        if let Some(temperature) = params.temperature {
            request.temperature(temperature);
        }

        // To handle timestamp granularities, we need different response formats
        // for different models.
        match params.model {
            OpenAIModel::Gpt4oMiniTranscribe | OpenAIModel::Gpt4oTranscribe => {
                request.response_format(async_openai::types::AudioResponseFormat::Json);

                let request = request
                    .build()
                    .map_err(|e| TranscribeError::Inference(e.to_string()))?;

                let response = self
                    .client
                    .audio()
                    .transcribe(request)
                    .await
                    .map_err(|e| TranscribeError::Inference(e.to_string()))?;

                return Ok(TranscriptionResult {
                    text: response.text,
                    segments: None,
                });
            }
            OpenAIModel::Whisper1 => {
                request.response_format(async_openai::types::AudioResponseFormat::VerboseJson);

                if let Some(timestamp_granularity) = &params.timestamp_granularity {
                    // OpenAI APi allows multiple levels of granularities in the
                    // same request, but our trait only accept one.
                    request.timestamp_granularities(vec![timestamp_granularity.clone()]);
                }

                let request = request
                    .build()
                    .map_err(|e| TranscribeError::Inference(e.to_string()))?;

                let response = self
                    .client
                    .audio()
                    .transcribe_verbose_json(request)
                    .await
                    .map_err(|e| TranscribeError::Inference(e.to_string()))?;

                let segments = match params.timestamp_granularity {
                    Some(async_openai::types::TimestampGranularity::Word) => Some(
                        response
                            .words
                            .unwrap()
                            .into_iter()
                            .map(|word| TranscriptionSegment {
                                start: word.start,
                                end: word.end,
                                text: word.word,
                            })
                            .collect(),
                    ),
                    Some(async_openai::types::TimestampGranularity::Segment) => Some(
                        response
                            .segments
                            .unwrap()
                            .into_iter()
                            .map(|segment| TranscriptionSegment {
                                start: segment.start,
                                end: segment.end,
                                text: segment.text,
                            })
                            .collect(),
                    ),
                    None => None,
                };

                return Ok(TranscriptionResult {
                    text: response.text,
                    segments,
                });
            }
        }
    }
}
