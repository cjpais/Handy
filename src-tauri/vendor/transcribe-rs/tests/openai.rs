use std::path::PathBuf;

use transcribe_rs::{
    remote::openai::{self, OpenAIRequestParams},
    RemoteTranscriptionEngine,
};

#[tokio::test]
async fn test_dots_transcription() {
    let engine = openai::default_engine();

    // Load the JFK audio file
    let audio_path = PathBuf::from("samples/dots.wav");

    // Transcribe with temperature 0
    let result = engine
        .transcribe_file(
            &audio_path,
            OpenAIRequestParams::builder()
                .temperature(0.0)
                .build()
                .expect("Default parameters shoul be valid"),
        )
        .await
        .expect("Failed to transcribe");

    let text = result.text.trim();
    assert!(!text.is_empty(), "Transcription should not be empty");
    // Check key phrases rather than exact match — remote API punctuation varies between calls
    for phrase in [
        "connect the dots",
        "looking forward",
        "looking backwards",
        "trust in something",
        "follow your heart",
        "make all the difference",
    ] {
        assert!(
            text.contains(phrase),
            "Transcription missing expected phrase '{}'\nActual: '{}'",
            phrase,
            text
        );
    }
}
