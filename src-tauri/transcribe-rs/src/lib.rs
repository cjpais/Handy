pub mod audio;
pub mod engines;

use std::path::PathBuf;

#[derive(Debug)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
}

#[derive(Debug)]
pub struct TranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

pub trait TranscriptionEngine {
    type InferenceParams;
    type ModelParams: Default;

    fn load_model(&mut self, model_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        self.load_model_with_params(model_path, Self::ModelParams::default())
    }

    fn load_model_with_params(
        &mut self,
        model_path: &PathBuf,
        params: Self::ModelParams,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn unload_model(&mut self);
    fn transcribe_samples(
        &mut self,
        samples: Vec<f32>,
        params: Option<Self::InferenceParams>,
    ) -> Result<TranscriptionResult, Box<dyn std::error::Error>>;

    fn transcribe_file(
        &mut self,
        wav_path: &PathBuf,
        params: Option<Self::InferenceParams>,
    ) -> Result<TranscriptionResult, Box<dyn std::error::Error>> {
        let samples = audio::read_wav_samples(wav_path)?;
        self.transcribe_samples(samples, params)
    }
}
