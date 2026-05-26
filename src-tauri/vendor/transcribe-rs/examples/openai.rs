use std::path::PathBuf;
use std::time::Instant;
use transcribe_rs::remote::openai::{self, OpenAIModel, OpenAIRequestParams};
use transcribe_rs::{remote, RemoteTranscriptionEngine};

fn get_audio_duration(path: &PathBuf) -> Result<f64, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration() as f64 / spec.sample_rate as f64;
    Ok(duration)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let engine = openai::default_engine();

    // Change model here.
    let model = OpenAIModel::Gpt4oMiniTranscribe;

    let wav_path = PathBuf::from("samples/dots.wav");

    let audio_duration = get_audio_duration(&wav_path)?;
    println!("Audio duration: {:.2}s", audio_duration);

    println!("Transcribing file: {:?}", wav_path);

    let transcribe_start = Instant::now();

    let result = engine
        .transcribe_file(
            &wav_path,
            OpenAIRequestParams::builder()
                .model(model)
                // Will be ignored on unsupported models.
                .timestamp_granularity(remote::openai::OpenAITimestampGranularity::Segment)
                .build()?,
        )
        .await?;

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
