//! Model-gated transcription tests (port of the Python test_transcribe /
//! test_pcm / test_lifetime case list). Skip cleanly when the canary GGUF and
//! jfk.wav are not on disk.

mod common;

use std::sync::Arc;
use std::thread;

use transcribe_cpp::{Error, Model, RunOptions, TimestampKind};

#[test]
fn transcribes_jfk_with_segments() {
    let Some((model_path, pcm)) = common::smoke_fixtures("transcribes_jfk_with_segments") else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    assert!(!model.arch().is_empty(), "empty arch string");

    let mut session = model.session().unwrap();
    let result = session.run(&pcm, &RunOptions::default()).unwrap();

    let text = result.text.trim().to_lowercase();
    assert!(!text.is_empty(), "empty transcription");
    assert!(
        text.contains("country"),
        "unexpected transcription: {:?}",
        result.text
    );
    assert!(!result.segments.is_empty(), "no segments materialized");
}

#[test]
fn one_model_many_sessions() {
    let Some((model_path, pcm)) = common::smoke_fixtures("one_model_many_sessions") else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    // Two sessions from one model, used serially.
    for _ in 0..2 {
        let mut s = model.session().unwrap();
        let r = s.run(&pcm, &RunOptions::default()).unwrap();
        assert!(r.text.to_lowercase().contains("country"));
    }
}

#[test]
fn batch_run_two_utterances() {
    let Some((model_path, pcm)) = common::smoke_fixtures("batch_run_two_utterances") else {
        return;
    };
    let mut session = Model::load(&model_path).unwrap().session().unwrap();
    let inputs: Vec<&[f32]> = vec![&pcm, &pcm];
    let results = session.run_batch(&inputs, &RunOptions::default()).unwrap();
    assert_eq!(results.len(), 2);
    for r in &results {
        let t = r.as_ref().expect("utterance ok");
        assert!(t.text.to_lowercase().contains("country"), "{:?}", t.text);
        assert!(!t.segments.is_empty());
    }
}

#[test]
fn capabilities_and_identity() {
    let Some((model_path, _)) = common::smoke_fixtures("capabilities_and_identity") else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    assert_eq!(model.arch(), "whisper", "arch: {}", model.arch());
    assert!(model.backend().to_lowercase().contains("cpu") || !model.backend().is_empty());
    let caps = model.capabilities();
    assert!(caps.native_sample_rate > 0, "{caps:?}");
}

#[test]
fn session_limits_are_sane() {
    let Some((model_path, _)) = common::smoke_fixtures("session_limits_are_sane") else {
        return;
    };
    let session = Model::load(&model_path).unwrap().session().unwrap();
    let limits = session.limits().unwrap();
    assert!(limits.effective_n_ctx >= 0);
    assert!(limits.effective_max_audio_ms >= 0);
    assert!(limits.max_kv_bytes >= 0);
}

#[test]
fn spec_decode_default_equals_disabled() {
    // -1 (family default) and 0 (disabled) must agree for a non-spec family.
    let Some((model_path, pcm)) = common::smoke_fixtures("spec_decode_default_equals_disabled")
    else {
        return;
    };
    let mut session = Model::load(&model_path).unwrap().session().unwrap();
    let base = session
        .run(
            &pcm,
            &RunOptions {
                spec_k_drafts: -1,
                ..Default::default()
            },
        )
        .unwrap()
        .text;
    let nospec = session
        .run(
            &pcm,
            &RunOptions {
                spec_k_drafts: 0,
                ..Default::default()
            },
        )
        .unwrap()
        .text;
    assert_eq!(base, nospec);
}

#[test]
fn empty_pcm_is_invalid_argument() {
    let Some((model_path, _)) = common::smoke_fixtures("empty_pcm_is_invalid_argument") else {
        return;
    };
    let mut session = Model::load(&model_path).unwrap().session().unwrap();
    let err = session.run(&[], &RunOptions::default()).unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)), "got {err:?}");
}

#[test]
fn requested_timestamps_populate_rows() {
    let Some((model_path, pcm)) = common::smoke_fixtures("requested_timestamps_populate_rows")
    else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    let caps = model.capabilities();
    // Request the finest granularity the model actually supports (a request
    // finer than max_timestamp_kind correctly returns Unsupported, status 12).
    if caps.max_timestamp_kind == TimestampKind::None {
        return;
    }
    let mut session = model.session().unwrap();
    let result = session
        .run(
            &pcm,
            &RunOptions {
                timestamps: caps.max_timestamp_kind,
                ..Default::default()
            },
        )
        .unwrap();
    assert_ne!(result.timestamp_kind, TimestampKind::None);
    assert!(!result.segments.is_empty());
    // The aligned segments carry non-degenerate time spans.
    assert!(result.segments.iter().any(|s| s.t1_ms >= s.t0_ms));
}

#[test]
fn timestamps_finer_than_supported_is_unsupported() {
    // Validates the Unsupported (status 12) mapping: requesting Token on a
    // model whose max is coarser must error, not silently downgrade.
    let Some((model_path, pcm)) =
        common::smoke_fixtures("timestamps_finer_than_supported_is_unsupported")
    else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    let caps = model.capabilities();
    if caps.max_timestamp_kind == TimestampKind::Token {
        return; // already at the finest; nothing finer to ask for
    }
    let mut session = model.session().unwrap();
    let err = session
        .run(
            &pcm,
            &RunOptions {
                timestamps: TimestampKind::Token,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "got {err:?}");
}

#[test]
fn close_ordering_drop_model_before_session() {
    // Rust ownership makes the C "model must outlive its sessions" contract
    // automatic: the session holds an Arc to the model, so dropping the Model
    // handle first is safe and the session keeps working.
    let Some((model_path, pcm)) =
        common::smoke_fixtures("close_ordering_drop_model_before_session")
    else {
        return;
    };
    let model = Model::load(&model_path).unwrap();
    let mut session = model.session().unwrap();
    drop(model); // model handle gone; session's Arc keeps the native model alive
    let r = session.run(&pcm, &RunOptions::default()).unwrap();
    assert!(r.text.to_lowercase().contains("country"));
    drop(session); // native model freed here
}

#[test]
fn shared_model_across_threads_serializes() {
    // Model is Send+Sync; the per-model mutex serializes the compute path. Two
    // threads each run on their own session of one shared model — they queue
    // rather than race, and both succeed.
    let Some((model_path, pcm)) = common::smoke_fixtures("shared_model_across_threads_serializes")
    else {
        return;
    };
    let model = Arc::new(Model::load(&model_path).unwrap());
    let pcm = Arc::new(pcm);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let model = Arc::clone(&model);
            let pcm = Arc::clone(&pcm);
            thread::spawn(move || {
                let mut s = model.session().unwrap();
                s.run(&pcm, &RunOptions::default()).unwrap().text
            })
        })
        .collect();
    for h in handles {
        let text = h.join().unwrap();
        assert!(text.to_lowercase().contains("country"), "{text}");
    }
}
