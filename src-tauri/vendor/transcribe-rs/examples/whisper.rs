use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::whisper_cpp::{WhisperEngine, WhisperInferenceParams};

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let model_path = PathBuf::from("models/whisper-medium-q4_1.bin");
    let wav_path = PathBuf::from("samples/dots.wav");

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    println!("Using Whisper engine");
    println!("Loading model: {:?}", model_path);

    let load_start = Instant::now();
    let mut engine = WhisperEngine::load(&model_path)?;
    let load_duration = load_start.elapsed();
    println!("Model loaded in {:.2?}", load_duration);

    println!("Transcribing file: {:?}", wav_path);
    let transcribe_start = Instant::now();

    let samples = transcribe_rs::audio::read_wav_samples(&wav_path)?;
    let result = engine.transcribe_with(
        &samples,
        &WhisperInferenceParams {
            initial_prompt: Some("This is a conversation about technology and AI.".to_string()),
            ..Default::default()
        },
    )?;
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
