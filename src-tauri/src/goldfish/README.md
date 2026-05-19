# Goldfish Rust modules

Goldfish-only Rust code lives here. Engine code (audio, VAD, transcription, history, models) stays in upstream-owned paths:

- `src-tauri/src/managers/`
- `src-tauri/src/audio_toolkit/`
- `src-tauri/src/transcription_coordinator.rs`

Anything in this directory is non-upstream and should not be PR'd back to `cjpais/Handy`.

See [`docs/fork-strategy.md`](../../../docs/fork-strategy.md) and [`docs/scaffold.md`](../../../docs/scaffold.md) for the rules around extending Handy without merge pain.
