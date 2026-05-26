use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::onnx::sense_voice::{SenseVoiceModel, SenseVoiceParams};
use transcribe_rs::onnx::Quantization;

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    let int8 = args.iter().any(|a| a == "--int8");
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let model_path = PathBuf::from(
        positional
            .first()
            .map(|s| s.as_str())
            .unwrap_or("models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"),
    );
    let wav_path = PathBuf::from(
        positional
            .get(1)
            .map(|s| s.as_str())
            .unwrap_or("samples/dots.wav"),
    );

    let quantization = if int8 {
        Quantization::Int8
    } else {
        Quantization::FP32
    };

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    println!("Using SenseVoice engine");
    println!(
        "Loading model: {:?} (quantization: {})",
        model_path,
        if int8 { "int8" } else { "fp32" }
    );

    let load_start = Instant::now();
    let mut model = SenseVoiceModel::load(&model_path, &quantization)?;
    let load_duration = load_start.elapsed();
    println!("Model loaded in {:.2?}", load_duration);

    println!("Transcribing file: {:?}", wav_path);
    let transcribe_start = Instant::now();

    let samples = transcribe_rs::audio::read_wav_samples(&wav_path)?;
    let result = model.transcribe_with(
        &samples,
        &SenseVoiceParams {
            language: Some("en".to_string()),
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
