use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::onnx::canary::{CanaryModel, CanaryParams};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let int8 = args.iter().any(|a| a == "--int8");
    let translate = args.iter().any(|a| a == "--translate");
    let model_path = PathBuf::from(
        positional
            .first()
            .map(|s| s.as_str())
            .unwrap_or("models/canary-180m-flash"),
    );
    let wav_path = PathBuf::from(
        positional
            .get(1)
            .map(|s| s.as_str())
            .unwrap_or("samples/jfk.wav"),
    );

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    let quantization = if int8 {
        Quantization::Int8
    } else {
        Quantization::FP32
    };

    println!("Using Canary engine");
    println!(
        "Loading model: {:?} (quantization: {})",
        model_path,
        if int8 { "int8" } else { "fp32" }
    );

    let load_start = Instant::now();
    let mut model = CanaryModel::load(&model_path, &quantization)?;
    let load_duration = load_start.elapsed();
    println!("Model loaded in {:.2?}", load_duration);

    println!("Transcribing file: {:?}", wav_path);
    let transcribe_start = Instant::now();

    if translate {
        let options = transcribe_rs::TranscribeOptions {
            translate: true,
            ..Default::default()
        };
        let result = model.transcribe_file(&wav_path, &options)?;
        let transcribe_duration = transcribe_start.elapsed();
        println!(
            "Transcription (translated) completed in {:.2?}",
            transcribe_duration
        );
        println!("{}", result.text);
    } else {
        let params = CanaryParams::default();
        let samples = transcribe_rs::audio::read_wav_samples(&wav_path)?;
        let result = model.transcribe_with(&samples, &params)?;
        let transcribe_duration = transcribe_start.elapsed();
        println!("Transcription completed in {:.2?}", transcribe_duration);

        let speedup_factor = audio_duration / transcribe_duration.as_secs_f64();
        println!(
            "Real-time speedup: {:.2}x faster than real-time",
            speedup_factor
        );

        println!("Transcription result:");
        println!("{}", result.text);
    }

    println!("\nCapabilities: {:?}", model.capabilities());

    Ok(())
}
