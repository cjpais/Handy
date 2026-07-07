//! streaming — feed audio in chunks and watch committed vs tentative text.
//!
//!     cargo run --example streaming -- [streaming-model.gguf] [audio.wav]
//!
//! Committed text is the UI-stable prefix (it only grows); tentative text is
//! the still-revisable tail. The commit policy decides when tentative becomes
//! committed. Needs a streaming-capable model (the moonshine-streaming canary);
//! skips cleanly otherwise.

#[path = "common/mod.rs"]
mod common;

use transcribe_cpp::{CommitPolicy, Model, RunOptions, StreamOptions, StreamState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(model_path), Some(audio_path)) = (
        common::streaming_model_path(args.next().as_deref()),
        common::audio_path(args.next().as_deref()),
    ) else {
        eprintln!(
            "skip streaming: streaming model/audio not found \
             (set TRANSCRIBE_SMOKE_STREAMING_MODEL / _AUDIO)"
        );
        return Ok(());
    };
    let pcm = common::load_wav(&audio_path);

    let model = Model::load(&model_path)?;
    if !model.capabilities().supports_streaming {
        // Not an error — the wrong-model case, stated plainly (a whisper model
        // would land here). Exit 0 so CI's no-canary path stays green.
        eprintln!("{} does not support streaming", model.arch());
        return Ok(());
    }

    let mut session = model.session()?;
    let opts = StreamOptions {
        commit_policy: CommitPolicy::Auto,
        ..Default::default()
    };
    let mut stream = session.stream(&RunOptions::default(), &opts)?;
    println!(
        "commit policy: {:?} | initial state: {:?}",
        opts.commit_policy,
        stream.state()
    );

    // Feed 100 ms chunks; print only when the committed/tentative split moves.
    for (i, frame) in pcm.chunks(1600).enumerate() {
        let update = stream.feed(frame)?;
        if update.committed_changed || update.tentative_changed {
            let text = stream.text();
            println!(
                "chunk {i:>3} | committed: {:?} | tentative: {:?}",
                text.committed, text.tentative
            );
        }
    }

    let update = stream.finalize()?;
    assert!(update.is_final);
    assert_eq!(stream.state(), StreamState::Finished);

    let text = stream.text();
    println!(
        "\nfinal (revision {}): {}",
        stream.revision(),
        text.full.trim()
    );
    Ok(())
}
