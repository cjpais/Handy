mod common;

use std::path::PathBuf;

use transcribe_rs::onnx::gigaam::GigaAMModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

#[test]
fn test_gigaam_transcribe() {
    env_logger::init();

    let model_dir = PathBuf::from("models/gigaam-v3");
    let wav_path = PathBuf::from("samples/russian.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        GigaAMModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    let expected = "Проверка связи.";
    assert_eq!(
        result.text, expected,
        "\nExpected: '{}'\nActual: '{}'",
        expected, result.text
    );
}
