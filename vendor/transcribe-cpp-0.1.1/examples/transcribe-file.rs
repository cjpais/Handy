//! transcribe-file — load a model, transcribe a WAV, print text + segments.
//!
//!     cargo run --example transcribe-file -- [model.gguf] [audio.wav]
//!
//! With no args it falls back to `$TRANSCRIBE_SMOKE_MODEL` / `_AUDIO` (CI's
//! fetch-canary exports these) or the in-repo canary, and skips cleanly when
//! neither resolves. This is the offline-run Rosetta stone — the same program
//! exists under the same name in every first-class binding.

#[path = "common/mod.rs"]
mod common;

use transcribe_cpp::{Model, RunOptions, TimestampKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model_arg = args.next();
    let audio_arg = args.next();

    println!(
        "transcribe-cpp {} ({})",
        transcribe_cpp::version(),
        transcribe_cpp::version_commit()
    );

    let (Some(model_path), Some(audio_path)) = (
        common::model_path(model_arg.as_deref()),
        common::audio_path(audio_arg.as_deref()),
    ) else {
        eprintln!(
            "skip transcribe-file: model/audio not found \
             (pass paths, or set TRANSCRIBE_SMOKE_MODEL / _AUDIO)"
        );
        return Ok(());
    };
    let pcm = common::load_wav(&audio_path);

    let model = Model::load(&model_path)?;
    let caps = model.capabilities();
    println!(
        "model: {} on {} | max_ts={:?}",
        model.arch(),
        model.backend(),
        caps.max_timestamp_kind
    );

    let mut session = model.session()?;
    // Ask for the finest timestamps the model actually supports (segment for
    // whisper); a request finer than the model's max is a clean Unsupported.
    let result = session.run(
        &pcm,
        &RunOptions {
            timestamps: caps.max_timestamp_kind,
            ..Default::default()
        },
    )?;

    println!(
        "\nlanguage: {} | timestamps: {:?} | encode {:.0} ms",
        result.language.as_deref().unwrap_or("(n/a)"),
        result.timestamp_kind,
        result.timings.encode_ms
    );
    println!("\n{}\n", result.text.trim());

    if result.timestamp_kind != TimestampKind::None {
        for seg in &result.segments {
            println!(
                "  [{:6.2} -> {:6.2}]  {}",
                seg.t0_ms as f64 / 1000.0,
                seg.t1_ms as f64 / 1000.0,
                seg.text.trim()
            );
        }
    }
    Ok(())
}
