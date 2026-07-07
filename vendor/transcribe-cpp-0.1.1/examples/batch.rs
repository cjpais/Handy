//! batch — transcribe several inputs in one call with per-result accessors.
//!
//!     cargo run --example batch -- [model.gguf] [audio.wav]
//!
//! `run_batch` takes a slice of PCM buffers and returns one `Result` per input,
//! so a single bad utterance does not sink the others. Here we submit the same
//! clip twice; a real caller would pass distinct utterances.

#[path = "common/mod.rs"]
mod common;

use transcribe_cpp::{Model, RunOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(model_path), Some(audio_path)) = (
        common::model_path(args.next().as_deref()),
        common::audio_path(args.next().as_deref()),
    ) else {
        eprintln!("skip batch: model/audio not found (set TRANSCRIBE_SMOKE_MODEL / _AUDIO)");
        return Ok(());
    };
    let pcm = common::load_wav(&audio_path);

    let model = Model::load(&model_path)?;
    let mut session = model.session()?;

    let inputs: Vec<&[f32]> = vec![&pcm, &pcm];
    let results = session.run_batch(&inputs, &RunOptions::default())?;

    println!("batch of {}:", results.len());
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(t) => println!("  [{i}] ok: {}", t.text.trim()),
            Err(e) => println!("  [{i}] error: {e}"),
        }
    }
    Ok(())
}
