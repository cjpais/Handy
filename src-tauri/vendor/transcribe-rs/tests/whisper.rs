mod common;

use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Mutex;
use transcribe_rs::whisper_cpp::{WhisperEngine, WhisperInferenceParams, WhisperLoadParams};
use transcribe_rs::SpeechModel;

fn model_path() -> PathBuf {
    PathBuf::from("models/whisper-medium-q4_1.bin")
}

static ENGINE: Lazy<Mutex<Option<WhisperEngine>>> = Lazy::new(|| {
    let model = model_path();

    if !common::require_paths(&[&model]) {
        return Mutex::new(None);
    }

    let params = WhisperLoadParams { use_gpu: false };
    match WhisperEngine::load_with_params(&model, params) {
        Ok(engine) => Mutex::new(Some(engine)),
        Err(e) => {
            eprintln!("Failed to load whisper model: {}", e);
            Mutex::new(None)
        }
    }
});

fn get_engine() -> Option<std::sync::MutexGuard<'static, Option<WhisperEngine>>> {
    let guard = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        return None;
    }
    Some(guard)
}

#[test]
fn test_jfk_transcription() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: whisper engine not available");
            return;
        }
    };
    let engine = guard.as_mut().unwrap();

    let audio_path = PathBuf::from("samples/jfk.wav");

    let result = engine
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    let expected = "And so my fellow Americans, ask not what your country can do for you, ask what you can do for your country.";
    assert_eq!(
        result.text.trim(),
        expected,
        "\nExpected: '{}'\nActual: '{}'",
        expected,
        result.text.trim()
    );
}

#[test]
fn test_prompt_product_names() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: whisper engine not available");
            return;
        }
    };
    let engine = guard.as_mut().unwrap();

    let audio_path = PathBuf::from("samples/product_names.wav");
    if !audio_path.exists() {
        eprintln!("Skipping test: {:?} not found", audio_path);
        return;
    }

    let baseline_result = engine
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe without prompt");

    println!("\n=== Baseline Transcription (no prompt) ===");
    println!("{}", baseline_result.text);

    let glossary_prompt = "QuirkQuid Quill Inc, P3-Quattro, O3-Omni, B3-BondX, E3-Equity, W3-WrapZ, O2-Outlier, U3-UniFund, M3-Mover";
    let samples =
        transcribe_rs::audio::read_wav_samples(&audio_path).expect("Failed to read audio samples");
    let prompted_result = engine
        .transcribe_with(
            &samples,
            &WhisperInferenceParams {
                initial_prompt: Some(glossary_prompt.to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe with prompt");

    println!("\n=== Transcription with Glossary Prompt ===");
    println!("{}", prompted_result.text);

    assert_ne!(
        baseline_result.text, prompted_result.text,
        "Prompt should influence transcription output"
    );

    assert!(
        prompted_result.text.contains("P3-Quattro") || prompted_result.text.contains("O3-Omni"),
        "Prompted output should contain hyphenated product names from glossary"
    );

    assert!(
        !baseline_result.text.contains("P3-Quattro"),
        "Baseline should not contain prompted spelling"
    );
}

#[test]
fn test_timestamps() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: whisper engine not available");
            return;
        }
    };
    let engine = guard.as_mut().unwrap();

    let audio_path = PathBuf::from("samples/jfk.wav");

    let result = engine
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    assert!(
        result.segments.is_some(),
        "Transcription should return segments"
    );

    let segments = result.segments.unwrap();
    assert!(!segments.is_empty(), "Segments should not be empty");

    for (i, segment) in segments.iter().enumerate() {
        assert!(
            segment.start >= 0.0,
            "Segment {} start time should be non-negative, got {}",
            i,
            segment.start
        );

        assert!(
            segment.end > segment.start,
            "Segment {} end time ({}) should be greater than start time ({})",
            i,
            segment.end,
            segment.start
        );

        assert!(
            !segment.text.trim().is_empty(),
            "Segment {} should have non-empty text",
            i
        );
    }

    for i in 1..segments.len() {
        assert!(
            segments[i].start >= segments[i - 1].start,
            "Segments should be in chronological order"
        );
    }

    let last_segment = segments.last().unwrap();
    assert!(
        last_segment.end > 10.0 && last_segment.end < 15.0,
        "Last segment end time should be around 11 seconds for JFK clip, got {}",
        last_segment.end
    );
}
