mod common;

use std::path::PathBuf;

use transcribe_rs::onnx::parakeet::ParakeetModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

#[test]
fn test_jfk_transcription() {
    let model_path = PathBuf::from("models/parakeet-tdt-0.6b-v3-int8");
    let audio_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_path, &audio_path]) {
        return;
    }

    let mut model =
        ParakeetModel::load(&model_path, &Quantization::Int8).expect("Failed to load model");

    let result = model
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    let expected = "And so, my fellow Americans, ask not what your country can do for you. Ask what you can do for your country.";
    assert_eq!(
        result.text.trim(),
        expected,
        "\nExpected: '{}'\nActual: '{}'",
        expected,
        result.text.trim()
    );
}

#[test]
fn test_timestamps() {
    let model_path = PathBuf::from("models/parakeet-tdt-0.6b-v3-int8");
    let audio_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_path, &audio_path]) {
        return;
    }

    let mut model =
        ParakeetModel::load(&model_path, &Quantization::Int8).expect("Failed to load model");

    let result = model
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    assert!(
        result.segments.is_some(),
        "Transcription should return segments"
    );

    let segments = result.segments.unwrap();
    assert!(!segments.is_empty(), "Segments should not be empty");

    assert!(
        segments.len() > 10,
        "Parakeet should return multiple token-level segments, got {}",
        segments.len()
    );

    for (i, segment) in segments.iter().enumerate() {
        assert!(
            segment.start >= 0.0,
            "Segment {} start time should be non-negative, got {}",
            i,
            segment.start
        );

        assert!(
            segment.end >= segment.start,
            "Segment {} end time ({}) should be >= start time ({})",
            i,
            segment.end,
            segment.start
        );

        assert!(
            !segment.text.is_empty(),
            "Segment {} should have non-empty text",
            i
        );
    }

    for i in 1..segments.len() {
        assert!(
            segments[i].start >= segments[i - 1].start,
            "Segments should be in chronological order: segment {} starts at {} but segment {} starts at {}",
            i,
            segments[i].start,
            i - 1,
            segments[i - 1].start
        );
    }

    let last_segment = segments.last().unwrap();
    assert!(
        last_segment.end > 10.0 && last_segment.end < 15.0,
        "Last segment end time should be around 11 seconds for JFK clip, got {}",
        last_segment.end
    );

    let first_segment = segments.first().unwrap();
    assert!(
        first_segment.start < 1.0,
        "First segment should start near the beginning, got {}",
        first_segment.start
    );
}
