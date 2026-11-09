# Reproduce a stale callback feeding a replacement stream

This fixture covers a conditional frame ownership failure on upstream
`main` at `73cc8579cff74e97e08de9d86035a92e4335ff7f`. An old recording's
callback resumes after its route is cancelled and a replacement route opens.
The replacement receives both the old frame `[1.0]` and its own frame `[2.0]`.
The regression assertion requires only `[[2.0]]`. Before the fix, the original
fixture failed that assertion with `[[1.0], [2.0]]`.

Run from the repository root with stable Rust:

```sh
cargo test --manifest-path tests/repro-stale-stream-callback/Cargo.toml --locked -- --nocapture
```

Expected result is five passing tests and exit status `0`. The tests require
fresh delivery, permanent inactivity of a callback captured without a route,
isolation from replacement routes, and no feeds after finalize or clear.
The cancellation regression also checks that cancellation marks the old route
and disconnects its channel before replacement. It uses channels to suspend and
resume the old callback, without sleeps or microphone access.

## Production source and fixture boundaries

`build.rs` parses repository Rust with `syn`. It emits the actual declarations of
`StreamRouter`, `Route`, and `StreamCmd`, the actual router methods `new`, `open`,
`take`, `clear`, `audio_callback`, and `feed`, `CaptureSink::emit`, and
`TranscriptionManager::cancel_stream`. Production method bodies are not rewritten.

The fixture provides a minimal `CaptureSink` type and the two manager fields used
by `cancel_stream`. The unused `StreamFinalizeReply` payload is replaced with
`Result<Option<()>, ()>`. It invokes `emit` directly and does not execute
`CaptureSink::new`, VAD, the recorder, model workers, or the Tauri application.
Each callback comes from the extracted factory, as it does for a new production
sink. Cancellation and replacement use the extracted production methods.

Source correspondence gates require the router to live on the audio manager,
each recording attempt to capture a fresh callback inside sink construction
before recorder start, and live stream opening to precede recorder start.
They reject a manager-global audio callback. They also check the
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

## Recording ownership

Each route has an immutable `Arc<()>` identity. `audio_callback` captures that
identity when the sink is created. `feed` compares the captured identity with the
active route and sends under the same route lock. An old callback retains its
old identity when it resumes. Capturing no route produces an inactive callback.

The original failing output and exit status are saved locally under
`.audit/stale-stream-callback/`. Fixed verification output and a scratch mutation
that removes the identity check are saved under `.audit/publish-formal/`.
