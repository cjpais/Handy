use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::whisperfile::{
    WhisperfileEngine, WhisperfileInferenceParams, WhisperfileLoadParams,
};

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let whisperfile_binary = PathBuf::from("models/whisperfile-0.9.3");
    let model_path = PathBuf::from("models/ggml-small.bin");
    let wav_path = PathBuf::from("samples/dots.wav");

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    println!("Using Whisperfile engine");
    println!("Loading model: {:?}", model_path);

    let load_start = Instant::now();
    let mut engine = WhisperfileEngine::load_with_params(
        &whisperfile_binary,
        &model_path,
        WhisperfileLoadParams {
            port: 8080,
            host: "127.0.0.1".to_string(),
            startup_timeout_secs: 60,
            ..Default::default()
        },
    )?;
    let load_duration = load_start.elapsed();
    println!("Whisperfile server started in {:.2?}", load_duration);

    println!("Transcribing file: {:?}", wav_path);
    let transcribe_start = Instant::now();

    let params = WhisperfileInferenceParams {
        language: Some("en".to_string()),
        translate: false,
        temperature: Some(0.0),
        response_format: Some("verbose_json".to_string()),
    };

    let samples = transcribe_rs::audio::read_wav_samples(&wav_path)?;
    let result = engine.transcribe_with(&samples, &params)?;
    let transcribe_duration = transcribe_start.elapsed();
    println!("Transcription completed in {:.2?}", transcribe_duration);

    let speedup_factor = audio_duration / transcribe_duration.as_secs_f64();
    println!(
        "Real-time speedup: {:.2}x faster than real-time",
        speedup_factor
    );

    println!("Transcription result:");
    println!("{}", result.text);

    if let Some(segments) = result.segments {
        println!("\nSegments:");
        for segment in segments {
            println!(
                "[{:.2}s - {:.2}s]: {}",
                segment.start, segment.end, segment.text
            );
        }
    }

    Ok(())
}
