mod common;

use std::path::PathBuf;

use transcribe_rs::onnx::moonshine::{MoonshineModel, MoonshineVariant};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

#[test]
fn test_moonshine_base_jfk() {
    let model_path = PathBuf::from("models/moonshine-base");
    let audio_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_path, &audio_path]) {
        return;
    }

    let mut model = MoonshineModel::load(
        &model_path,
        MoonshineVariant::Base,
        &Quantization::default(),
    )
    .expect("Failed to load model");

    let result = model
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    println!("Transcription: {}", result.text);

    assert!(!result.text.is_empty(), "Transcription should not be empty");

    let expected = "And so my fellow Americans ask not what your country can do for you ask what you can do for your country";
    assert_eq!(
        result.text.trim(),
        expected,
        "\nExpected: '{}'\nActual: '{}'",
        expected,
        result.text.trim()
    );
}
