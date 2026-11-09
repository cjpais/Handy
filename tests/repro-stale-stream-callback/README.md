# Reproduce a stale callback feeding a replacement stream

This fixture demonstrates a conditional frame ownership failure on upstream
`main` at `73cc8579cff74e97e08de9d86035a92e4335ff7f`. An old recording's
callback resumes after its route is cancelled and a replacement route opens.
The replacement receives both the old frame `[1.0]` and its own frame `[2.0]`.
The regression assertion requires only `[[2.0]]`, so the test fails on main.

Run from the repository root with stable Rust:

```sh
cargo test --manifest-path tests/repro-stale-stream-callback/Cargo.toml --locked -- --nocapture
```

Expected result is two passing controls, one failed regression, and exit status
`101`. The controls require fresh delivery and no delivery while no route exists.
The regression also checks that cancellation marks the old route and disconnects
its channel before replacement. It uses channels to suspend and resume the old
callback, without sleeps or microphone access.

## Production source and fixture boundaries

`build.rs` parses repository Rust with `syn`. It emits the actual declarations of
`StreamRouter`, `Route`, and `StreamCmd`, the actual router methods `new`, `open`,
`take`, and `feed`, `CaptureSink::emit`, and
`TranscriptionManager::cancel_stream`. It also emits the actual `on_audio`
initializer from `AudioRecordingManager::new` inside a small constructor.
Production method bodies and the callback initializer are not rewritten.

The fixture provides a minimal `CaptureSink` type and the two manager fields used
by `cancel_stream`. The unused `StreamFinalizeReply` payload is replaced with
`Result<Option<()>, ()>`. It invokes `emit` directly and does not execute
`CaptureSink::new`, VAD, the recorder, model workers, or the Tauri application.
An old and a fresh callback clone correspond to the clones passed to each new
sink. Cancellation and replacement use the extracted production methods.

Source correspondence gates require the callback to live on the audio manager,
each recording start to build a new sink with that callback before recorder
start, and live stream opening to precede recorder start. They also check the
manager's stop failure, idle transition, close, reopen, and empty-recording
cancellation paths. The product lockfile must select `handy-recorder 0.1.0` with
its current registry checksum. A gate failure means the reproduction needs
review; it is not evidence of this bug.

## Optional recorder source gate

The ordinary command requires only this checkout and the fixture's four locked
Rust build dependencies. It does not require a recorder source cache.

To check the recorder timeout boundary too, set `HANDY_RECORDER_SOURCE` to an
unmodified `handy-recorder-0.1.0` package source directory:

```sh
HANDY_RECORDER_SOURCE=/path/to/handy-recorder-0.1.0 \
  cargo test --manifest-path tests/repro-stale-stream-callback/Cargo.toml --locked -- --nocapture
```

Cargo normally stores that directory under
`~/.cargo/registry/src/<registry>/handy-recorder-0.1.0`. Its published source is
also available in the [versioned crate archive](https://crates.io/api/v1/crates/handy-recorder/0.1.0/download).
The gate checks `Engine::collect` returning `SinkStalled` on its stop timeout,
`Engine::stop` retiring its slot without joining delivery, and `Engine::shutdown`
allowing a delivery timeout to detach the thread. The recorded run enabled this
gate. It does not simulate those timeouts.

This is a source-supported callback overlap replay, not a native timeout
reproduction or exhaustive driver test. It assumes an old delivery thread remains
inside the sink until after stop and shutdown time out, then resumes before the
replacement ends. No microphone, Bluetooth failure, operating system scheduling,
or actual recorder timeout was exercised. The fixture proves routing behavior
under that ordering, not how often that ordering happens in practice.

## Proposed ownership rule

Give each route an immutable token. Capture the recording's route token when its
sink is created, and carry it with every callback. Compare that token with the
active route and send under the same route lock. An old callback must retain its
old token when it resumes. Looking up the current token at callback execution
would accept the stale frame again. This branch contains reproduction code only.

The full observed output and exit status are saved under
`.audit/stale-stream-callback/`. The expected stale-frame failure remains present.
