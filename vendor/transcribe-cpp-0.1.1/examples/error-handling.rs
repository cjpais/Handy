//! error-handling — idiomatic error mapping, cancellation, and cleanup.
//!
//!     cargo run --example error-handling -- [model.gguf] [audio.wav]
//!
//! Every `transcribe_status` maps to a distinct `Error` variant, so callers
//! `match` instead of parsing strings. The error-mapping half needs no model
//! and always runs; cancellation needs a model and skips cleanly otherwise.
//! Cleanup is automatic — `Model` and `Session` free on `Drop`, in any order.

#[path = "common/mod.rs"]
mod common;

use std::thread;
use std::time::Duration;

use transcribe_cpp::{CancelToken, Error, Model, RunOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Error mapping (no model needed): a missing file is a typed variant,
    //    distinct from a corrupt-file load error.
    match Model::load("/no/such/model.gguf") {
        Err(Error::ModelFileNotFound(msg)) => {
            println!("missing file -> ModelFileNotFound: {msg}")
        }
        other => println!("unexpected for missing file: {other:?}"),
    }

    let mut args = std::env::args().skip(1);
    let (Some(model_path), Some(audio_path)) = (
        common::model_path(args.next().as_deref()),
        common::audio_path(args.next().as_deref()),
    ) else {
        eprintln!("skip the model half of error-handling: no model/audio found");
        return Ok(());
    };
    let pcm = common::load_wav(&audio_path);
    let model = Model::load(&model_path)?;

    // 2. Empty PCM is an InvalidArgument — a clean error, not a panic.
    {
        let mut session = model.session()?;
        match session.run(&[], &RunOptions::default()) {
            Err(Error::InvalidArgument(msg)) => {
                println!("empty PCM -> InvalidArgument: {msg}")
            }
            other => println!("unexpected for empty PCM: {other:?}"),
        }
    }

    // 3. Cancellation: cancel an in-flight run from another thread. On a long
    //    enough clip the run aborts with a partial transcript; a fast machine
    //    may finish the run before the timer — both outcomes are handled.
    {
        let mut session = model.session()?;
        let token = CancelToken::new();
        session.set_cancel_token(&token);

        let long: Vec<f32> = std::iter::repeat(pcm.iter().copied())
            .take(12)
            .flatten()
            .collect();

        let canceller = {
            let token = token.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(40));
                token.cancel();
            })
        };

        match session.run(&long, &RunOptions::default()) {
            Err(Error::Aborted { partial, .. }) => println!(
                "run aborted by request; partial transcript preserved: {}",
                partial.is_some()
            ),
            Ok(_) => println!("run completed before the cancel fired (fast machine)"),
            Err(other) => println!("unexpected run error: {other:?}"),
        }
        canceller.join().unwrap();
    }

    // 4. Cleanup is automatic: dropping the model and its sessions frees the
    //    native handles, safe in any order under error paths.
    println!("done; native resources released on Drop");
    Ok(())
}
