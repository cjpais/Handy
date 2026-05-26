mod common;

use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Mutex;
use transcribe_rs::whisperfile::{WhisperfileEngine, WhisperfileLoadParams};
use transcribe_rs::SpeechModel;

fn binary_path() -> PathBuf {
    std::env::var("WHISPERFILE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("models/whisperfile-0.9.3"))
}

fn model_path() -> PathBuf {
    std::env::var("WHISPERFILE_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("models/ggml-small.bin"))
}

static ENGINE: Lazy<Mutex<Option<WhisperfileEngine>>> = Lazy::new(|| {
    let binary = binary_path();
    let model = model_path();

    if !common::require_paths(&[&binary, &model]) {
        return Mutex::new(None);
    }

    let params = WhisperfileLoadParams {
        port: 18080,
        startup_timeout_secs: 60,
        ..Default::default()
    };

    match WhisperfileEngine::load_with_params(&binary, &model, params) {
        Ok(engine) => Mutex::new(Some(engine)),
        Err(e) => {
            eprintln!("Failed to start whisperfile server: {}", e);
            Mutex::new(None)
        }
    }
});

fn get_engine() -> Option<std::sync::MutexGuard<'static, Option<WhisperfileEngine>>> {
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
            eprintln!("Skipping test: engine not available");
            return;
        }
    };
    let engine = match guard.as_mut() {
        Some(e) => e,
        None => {
            eprintln!("Skipping test: engine not initialized");
            return;
        }
    };

    let audio_path = PathBuf::from("samples/jfk.wav");

    let result = engine
        .transcribe_file(&audio_path, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe");

    let text_normalized: String = result
        .text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        text_normalized.contains("my fellow americans"),
        "Should contain 'my fellow Americans', got: {}",
        result.text
    );
    assert!(
        text_normalized.contains("ask not what your country can do for you"),
        "Should contain 'ask not what your country can do for you', got: {}",
        result.text
    );
    assert!(
        text_normalized.contains("ask what you can do for your country"),
        "Should contain 'ask what you can do for your country', got: {}",
        result.text
    );
}

#[test]
fn test_timestamps() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: engine not available");
            return;
        }
    };
    let engine = match guard.as_mut() {
        Some(e) => e,
        None => {
            eprintln!("Skipping test: engine not initialized");
            return;
        }
    };

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
            segment.end >= segment.start,
            "Segment {} end time ({}) should be >= start time ({})",
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

#[test]
fn test_transcribe_samples() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: engine not available");
            return;
        }
    };
    let engine = match guard.as_mut() {
        Some(e) => e,
        None => {
            eprintln!("Skipping test: engine not initialized");
            return;
        }
    };

    let audio_path = PathBuf::from("samples/jfk.wav");
    let samples =
        transcribe_rs::audio::read_wav_samples(&audio_path).expect("Failed to read audio samples");

    let result = engine
        .transcribe(&samples, &transcribe_rs::TranscribeOptions::default())
        .expect("Failed to transcribe samples");

    assert!(!result.text.is_empty(), "Transcription should not be empty");

    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("americans") || text_lower.contains("country"),
        "Should contain expected words from JFK speech, got: {}",
        result.text
    );
}

#[test]
fn test_language_parameter() {
    let mut guard = match get_engine() {
        Some(g) => g,
        None => {
            eprintln!("Skipping test: engine not available");
            return;
        }
    };
    let engine = match guard.as_mut() {
        Some(e) => e,
        None => {
            eprintln!("Skipping test: engine not initialized");
            return;
        }
    };

    let audio_path = PathBuf::from("samples/jfk.wav");

    let result = engine
        .transcribe_file(
            &audio_path,
            &transcribe_rs::TranscribeOptions {
                language: Some("en".to_string()),
                ..Default::default()
            },
        )
        .expect("Failed to transcribe with language parameter");

    assert!(!result.text.is_empty(), "Transcription should not be empty");

    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("country"),
        "Should transcribe English correctly, got: {}",
        result.text
    );
}
