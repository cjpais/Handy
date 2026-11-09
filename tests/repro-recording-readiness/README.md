# Recording readiness cue regression

This standalone fixture checks ownership of the recording-ready overlay event
and start chime. It compiles source extracted from this checkout without building
Tauri, opening audio devices, playing sounds, or changing system mute.

Run from the repository root:

```sh
env -u HANDY_DEBUG_MIC_READY_DELAY_MS cargo test --locked \
  --manifest-path tests/repro-recording-readiness/Cargo.toml \
  -- --test-threads=1 --nocapture
```

The repaired implementation passes 15 tests. The earlier reproduction at commit
`242a2849` fails the UI and chime assertions against production base
`73cc8579cff74e97e08de9d86035a92e4335ff7f`, with four passing controls. That earlier
fixture records chime-helper entry, whereas this fixture reaches the actual
player-append call in extracted production code.

## Source boundary

`build.rs` uses `syn` to extract these production bodies without rewriting their
synchronization, comparisons, or dispatch calls:

- The complete readiness worker closure in `src-tauri/src/actions.rs`.
- `RecordingReadiness`, `RecordingReadinessOwnership`, their implementations,
  and the manager's ownership allocation, invalidation, and query methods.
- The actual startup statements through token allocation, cancel statements
  through state acquisition and its second invalidation, and stop statements
  through the matching-binding check and its second invalidation. The recording
  state enum is extracted too.
- `emit_recording_ready` from `src-tauri/src/overlay.rs`, including its queued
  closure and actual `emit_to` call.
- `play_start_feedback_if_current` and the full `play_audio_file` implementation
  from `src-tauri/src/audio_feedback.rs`, including device preparation, decoder
  creation, fresh player creation, guarded append, and playback wait.

Extraction requires exactly one of each body and verifies the production
`capture_generation` field is `Arc<Mutex<u64>>`. The generated source is in
Cargo's `target/debug/build/.../out/production.rs`.

Loom replaces the ownership and recording-state `Arc` and `Mutex`, plus the
cancellation-generation atomic. Readiness uses the extracted real
receiver wait with an already-delivered sample notification. The fixture supplies
settings, path resolution, device, file, decoder, and player adapters. Device
opening and decoding provide scheduling points. The fake player's `append`
records the chime; `sleep_until_end` records completion. The actual guarded append
is production code, not a guarded substitute implemented by the adapter.

The fake main thread retains production closures, drains them in FIFO order, and
records their actual `emit_to` calls. It also supports inline dispatch. Queue
locks are released before executing jobs. The effects ledger marks invalidation
only after the production invalidation method returns. Cue commitments that
precede invalidation are allowed; dispatches that follow it are rejected.

Mute is explicitly outside this fixture: `apply_mute` is a no-op adapter for the
unchanged call at the end of the extracted worker. No mute state, helpers, or
restore behavior are extracted or asserted.

## Checks and limits

The two race tests run readiness concurrently with invalidation and a simulated
newer overlay show. Loom explores these two-thread schedules without permutation,
duration, or preemption limits, with a 1,000-branch bound per execution. There are
also deterministic controls for:

- Current readiness dispatching both cues; invalid readiness dispatching neither.
- Readiness queued before invalidation and drained afterward.
- A newer arming show preceding an old ready job.
- Inline main-thread execution without a deadlock.
- Invalidation during sound preparation preventing append.
- Invalidation completing before playback completion.
- New ownership superseding old ownership, and later invalidation remaining final.
- Disabled feedback preserving the UI readiness event.

Two additional races execute the extracted admission statements. Start and
cancel, or start and matching stop, run concurrently. A test return after token
allocation simulates successful capture by setting Recording while the actual
startup state guard remains held. The cancel return observes that state under
its actual guard; stop retains its production match and binding predicate. If
ending observes the admitted recording, the returned token must be invalid.
The tests require Loom to reach a schedule where early invalidation precedes
token allocation but start owns state before ending acquires it. With both
state-protected invalidations removed, both tests fail because token 2 remains
current at generation 2.

Controls allow new ownership when cancel finishes before start admission, and
check that a wrong-binding stop does not add another invalidation after acquiring
state. The injected returns and simulated capture completion are test adapters.
All locks, token allocation, invalidation, Idle checks, and the stop binding check
in these admission prefixes are unchanged production statements.

Build-time source checks require the start action's invalidation before either
recording-overlay show and before capture startup; allocation after Idle admission
and before microphone opening; no later generation access in startup; and leading
invalidation in action stop, manager stop, and manager cancel. These are source
ordering checks. The admission races execute the extracted startup, stop, and
cancel prefixes, with synthetic capture completion. They do not execute native
microphone opening, capture draining, state transitions after admission, or the
whole start/stop/cancel methods.

For sensitivity checks, removing only the extracted UI guard makes the delayed
queue control fail. Removing only the extracted append guard makes the sound
preparation control fail. The production sources retain both guards.

This verifies the extracted ownership protocol with the stated adapters. It does
not verify Tauri transport, React rendering, native capture, real output devices,
audible timing, mute, or the whole application. A chime committed before
invalidation may finish afterward; invalidation does not cancel committed audio.
