use std::path::PathBuf;
use std::time::Instant;

use transcribe_rs::onnx::moonshine::StreamingModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::SpeechModel;

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

fn print_usage() {
    eprintln!("Usage: moonshine_streaming [tiny|small|medium|<path>] [wav_file]");
    eprintln!();
    eprintln!("  Model (default: tiny):");
    eprintln!("    tiny     models/moonshine-streaming/tiny-streaming-en");
    eprintln!("    small    models/moonshine-streaming/small-streaming-en");
    eprintln!("    medium   models/moonshine-streaming/medium-streaming-en");
    eprintln!("    <path>   custom path to a streaming model directory");
    eprintln!();
    eprintln!("  wav_file (default: samples/dots.wav):");
    eprintln!("    Path to a 16kHz mono 16-bit WAV file");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return Ok(());
    }

    let model_path = match args.get(1).map(|s| s.as_str()) {
        None | Some("tiny") => {
            PathBuf::from("models/moonshine-streaming/moonshine-tiny-streaming-en")
        }
        Some("small") => PathBuf::from("models/moonshine-streaming/moonshine-small-streaming-en"),
        Some("medium") => PathBuf::from("models/moonshine-streaming/moonshine-medium-streaming-en"),
        Some(path) => PathBuf::from(path),
    };

    let wav_path = match args.get(2) {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("samples/dots.wav"),
    };

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    println!("Using Moonshine streaming engine");
    println!("Loading model: {:?}", model_path);

    let load_start = Instant::now();
    let mut model = StreamingModel::load(&model_path, 4, &Quantization::default())?;
    let load_duration = load_start.elapsed();
    println!("Model loaded in {:.2?}", load_duration);

    println!("Transcribing file: {:?}", wav_path);
    let transcribe_start = Instant::now();
    let result = model.transcribe_file(&wav_path, &transcribe_rs::TranscribeOptions::default())?;
    let transcribe_duration = transcribe_start.elapsed();

    println!("Transcription completed in {:.2?}", transcribe_duration);

    let speedup_factor = audio_duration / transcribe_duration.as_secs_f64();
    println!(
        "Real-time speedup: {:.2}x faster than real-time",
        speedup_factor
    );

    println!("Transcription result:");
    println!("{}", result.text);

    Ok(())
}
