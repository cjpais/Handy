# Reproduce stale recording readiness effects

This standalone Rust fixture reproduces two readiness cue races at upstream
`73cc8579cff74e97e08de9d86035a92e4335ff7f`. It changes no application code and
does not build Tauri, open an audio device, play sounds, or change system mute.

Run from the repository root:

```sh
env -u HANDY_DEBUG_MIC_READY_DELAY_MS cargo test --locked \
  --manifest-path tests/repro-recording-readiness/Cargo.toml \
  -- --test-threads=1 --nocapture
```

The reproduced result is exit `101`, with four passing controls and two failing
regression assertions. These are intentional failures, without `should_panic`:

```text
readiness_chime_never_follows_ended_session ... FAILED
readiness_ui_never_changes_newer_session ... FAILED
test result: FAILED. 4 passed; 2 failed
```

## Source boundary

`build.rs` parses the checkout with `syn` and compiles these production bodies:

- The complete readiness worker closure in `src-tauri/src/actions.rs`, starting
  with `readiness.wait()` and ending after UI, chime, and mute dispatch. The wait
  returns true in the fixture. The development delay hook remains in the source
  and its environment variable is unset by the command above.
- `invalidate_recording_readiness`, `is_recording_readiness_current`,
  `apply_mute`, `remove_mute`, and `MuteState` from
  `src-tauri/src/managers/audio.rs`.
- `emit_recording_ready` from `src-tauri/src/overlay.rs`, including the actual
  `run_on_main_thread` closure and its eventual `emit_to` call.

The production generation checks, atomic orderings, mutex boundaries, and mute
state assignments remain intact. Extraction checks that each body exists exactly
once and that the modeled audio fields retain their production types.
Generated code is in Cargo's `target/debug/build/.../out/production.rs`.

The fixture supplies a reduced manager with the covered fields. Loom replaces
`Arc`, `Mutex`, and the generation atomic. A `syn` visitor changes only free
`get_mute`, `set_mute`, and `restore_mute` calls to methods on the fake app.
Those adapters record mute requests and implement the platform helper's restore
policy, including prior-muted and unknown prior states. They do not prove the
operating system's mute implementation.

The chime adapter records entry into `play_feedback_sound_blocking`. A failure
proves stale dispatch to that helper. It does not prove audible playback, device
selection, sound availability, or when playback finishes.

The app adapter stores main-thread jobs and later drains them in FIFO order.
The UI assertion observes the actual extracted helper's `emit_to` boundary,
rather than only its acceptance of a job. Real Tauri scheduling, event transport,
and React rendering remain outside the test. After invalidation and restore, the
ending worker enqueues a simulated newer recording's show job on this same queue.
That job sets `newer_ui` when it runs. An old ready job queued before this show
does not count as a stale UI effect. An old ready job queued after this show does.
The `newer_session` marker marks backend ownership after invalidation and restore
for the chime and mute assertions. The fixture does not run a second recording's
startup or actual overlay-show code. It proves stale untagged backend ready
dispatch after a simulated newer show, not a displayed frontend transition.

## Schedules and controls

Two Loom workers run the real readiness closure and the real invalidation method.
The ending worker also calls the real `remove_mute` before setting a test-only
`newer_session` marker. The fixture leaves `is_open` true to model always-on
capture. It does not reimplement stop, cancellation, the coordinator, or paste.

The failing chime execution passes a production generation check, then
invalidates before the helper invocation. The UI witness passes its generation
check, then queues readiness after invalidation and the simulated newer show's
enqueue. The simulated newer show therefore runs first. This execution preserves
FIFO main-thread queue ordering.

The passing controls verify:

- A current session dispatches all effects and mutes.
- A worker that begins after invalidation dispatches nothing.
- Duplicate mute preserves the original snapshot. Duplicate restore does nothing.
  Prior-unmuted, prior-muted, and unknown prior system states follow restore policy.
- Disabled mute and a closed stream produce no mute request or restore.

Loom searches the modeled two-worker schedules until each regression finds a
counterexample. There is no claim of full application verification or a passing
exhaustive proof. The saved verification removes inherited `LOOM_*` variables.
The builder has no permutation, duration, or preemption limit, and permits 1,000
branches per execution. No timing sleeps or hardware operations supply the failure.

## Repair considerations

No repair is included. Generation checks and effect commitment need shared
synchronization with invalidation. Rechecking just before a call still leaves
a check-to-dispatch window.

For the UI, synchronizing only `run_on_main_thread` submission does not protect
the queued emission. Revalidate at actual main-thread emission, or tag both
show and ready events with session identity and reject stale ready events.
Avoid holding a readiness lock across `run_on_main_thread` or sound playback.
Sound work needs a defined dispatch commitment that invalidation synchronizes
with, without requiring a lock throughout playback.

[PR #2388](https://github.com/cjpais/Handy/pull/2388) concerns waiting for sound;
that ordering alone does not close these generation-check races.

Mute repair and a mute regression are excluded from this publication. The
existing mute adapters and controls support running the unchanged readiness
closure; they do not assert a mute-race repair.
