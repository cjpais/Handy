mod common;

use std::path::PathBuf;

use transcribe_rs::onnx::canary::{CanaryModel, CanaryParams};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

// ---------------------------------------------------------------------------
// V2 model tests
// ---------------------------------------------------------------------------

#[test]
fn test_canary_v2_transcribe() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    assert!(
        result
            .text
            .to_lowercase()
            .contains("ask not what your country can do for you"),
        "Expected JFK quote, got: '{}'",
        result.text
    );
}

#[test]
fn test_canary_v2_variant_detection() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    if !common::require_paths(&[&model_dir]) {
        return;
    }

    let model = CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");

    let caps = model.capabilities();
    assert_eq!(caps.name, "Canary 1B v2");
    assert!(caps.languages.contains(&"en"));
    assert!(caps.languages.contains(&"de"));
    assert!(caps.languages.contains(&"ru"));
    assert!(caps.supports_translation);
}

#[test]
fn test_canary_v2_itn() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/itn.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // With ITN: spoken numbers should be converted to digits
    let with_itn = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                use_itn: true,
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        with_itn.text.contains("123"),
        "ITN should convert spoken numbers to digits, got: '{}'",
        with_itn.text
    );

    // Without ITN: numbers should remain as words
    let without_itn = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                use_itn: false,
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        without_itn.text.to_lowercase().contains("one hundred"),
        "Without ITN, numbers should stay as words, got: '{}'",
        without_itn.text
    );
}

#[test]
fn test_canary_v2_pnc() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // With PnC: should have commas or periods
    let with_pnc = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                use_pnc: true,
                use_itn: false,
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        with_pnc.text.contains(',') || with_pnc.text.contains('.'),
        "PnC ON should produce punctuation, got: '{}'",
        with_pnc.text
    );

    // Without PnC: should still produce text
    let without_pnc = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                use_pnc: false,
                use_itn: false,
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        !without_pnc.text.is_empty(),
        "PnC OFF should still produce text"
    );
}

#[test]
fn test_canary_v2_translation() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/german.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // Translate German to English
    let result = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("de".to_string()),
                target_language: Some("en".to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(!result.text.is_empty(), "Translation should produce output");

    // The output should be English, not German
    // (basic heuristic: English text shouldn't contain common German-only characters/words)
    let text_lower = result.text.to_lowercase();
    assert!(
        !text_lower.contains("ich") && !text_lower.contains("und") && !text_lower.contains("ist"),
        "Translation output should be English, got: '{}'",
        result.text
    );
}

#[test]
fn test_canary_v2_german_transcription() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/german.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // Transcribe German as German (no translation)
    let result = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("de".to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    // Output should contain German words
    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("strand") || text_lower.contains("die") || text_lower.contains("der"),
        "German transcription should contain German words, got: '{}'",
        result.text
    );
}

#[test]
fn test_canary_v2_translate_en_to_de() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // Translate English to German
    let result = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                target_language: Some("de".to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    // Output should be German, not English
    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("land") || text_lower.contains("fragen") || text_lower.contains("ihr"),
        "EN->DE translation should contain German words, got: '{}'",
        result.text
    );
}

#[test]
fn test_canary_v2_translate_en_to_es() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-1b-v2");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    let result = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                target_language: Some("es".to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        !result.text.is_empty(),
        "EN->ES translation should produce output"
    );
}

// ---------------------------------------------------------------------------
// Flash model tests
// ---------------------------------------------------------------------------

#[test]
fn test_canary_flash_transcribe() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-180m-flash");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");

    let result = model
        .transcribe_file(&wav_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    assert!(
        result
            .text
            .to_lowercase()
            .contains("ask not what your country can do for you"),
        "Expected JFK quote, got: '{}'",
        result.text
    );
}

#[test]
fn test_canary_flash_variant_detection() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-180m-flash");
    if !common::require_paths(&[&model_dir]) {
        return;
    }

    let model = CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");

    let caps = model.capabilities();
    assert_eq!(caps.name, "Canary Flash");
    assert_eq!(caps.languages, &["en", "de", "es", "fr"]);
    assert!(caps.supports_translation);
}

#[test]
fn test_canary_flash_itn_ignored() {
    let _ = env_logger::try_init();

    let model_dir = PathBuf::from("models/canary-180m-flash");
    let wav_path = PathBuf::from("samples/jfk.wav");

    if !common::require_paths(&[&model_dir, &wav_path]) {
        return;
    }

    let mut model =
        CanaryModel::load(&model_dir, &Quantization::Int8).expect("Failed to load model");
    let samples = transcribe_rs::audio::read_wav_samples(&wav_path).expect("Failed to read WAV");

    // ITN enabled on Flash should be silently ignored and still produce output
    let result = model
        .transcribe_with(
            &samples,
            &CanaryParams {
                language: Some("en".to_string()),
                use_itn: true,
                ..Default::default()
            },
        )
        .expect("Failed to transcribe");

    assert!(
        !result.text.is_empty(),
        "Flash with use_itn=true should still produce output (ITN silently ignored)"
    );
}
