mod common;

use std::path::PathBuf;

use transcribe_rs::onnx::sense_voice::SenseVoiceModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

#[test]
fn test_sense_voice_transcribe() {
    env_logger::init();

    let model_path = PathBuf::from("models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17");
    let wav_path = PathBuf::from("samples/dots.wav");

    if !common::require_paths(&[&model_path, &wav_path]) {
        return;
    }

    let mut model =
        SenseVoiceModel::load(&model_path, &Quantization::FP32).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    assert!(!result.text.is_empty(), "Transcription should not be empty");
    println!("Transcription: {}", result.text);
}
