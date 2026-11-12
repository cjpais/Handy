# Model-check model publication against unload

This standalone crate runs the transcription manager's load, unload and
lost-worker methods, and the engine supervisor's residency rules, against fake
boundaries. It checks that the model id and the `model-state-changed` events the
manager publishes always end on what the engine holds.

Run from the repository root:

```sh
cargo test --release --locked --manifest-path tests/repro-model-publication/Cargo.toml -- --test-threads=1 --nocapture
```

All nine tests pass on the repaired source in about 30 seconds. No model file is
required, and the crate's own workspace and lockfile keep the full Tauri build
out of this run.

## The defect

The engine supervisor decides what is loaded, and it orders loads and unloads in
one queue. The manager repeats that state in two places: the model id that
`get_current_model` returns, and the event stream the frontend replays. It wrote
both from whichever thread called it, in an order unrelated to the engine's.

Three races share that cause.

1. A load that finishes behind an unload restores its model id and emits
   `loading_completed`. The manager and the model selector read "ready" while
   the engine holds nothing. This is the reported defect. On upstream commit
   `73cc8579cff74e97e08de9d86035a92e4335ff7f` the first version of this fixture
   read `Some("model-a")` after the unload, with events
   `["loading_started", "unloaded", "loading_completed"]`.
2. An unload asks the engine first and clears the id afterwards. A load queued
   behind that unload can publish its model in between. The engine then holds
   the model and the manager says none is loaded.
3. A lost-worker report checks the engine, then clears the id. A reload that
   finishes in between loses its id, and the report's `unloaded` can arrive
   after the reload's `loading_completed`.

Loom found the second and third. The deterministic schedules cover the first.

## Repair under test

The model id and an outbox of events now sit in one `ModelState` behind one
lock. `publish` applies a change to the id and queues the event for it in the
same step, so the events leave in the order the id changed.

- A finished load asks the engine inside that step. It publishes its id and
  `loading_completed` only while the engine holds a model. Otherwise it emits
  `loading_cancelled` and publishes nothing.
- An unload sends its request to the engine inside the same step that clears
  the id.
- A lost-worker report reads the engine inside the step.

`publish` emits after it releases the lock, and only one thread emits at a time.
The lock is never held across an emit. Listeners run inside `emit`, and the tray
listener waits for the main thread to answer a theme query. The main thread can
be inside `request_unload` at that moment, waiting for the same lock.

The recording overlay ends its loading state on `loading_cancelled`, and the
model selector reads it as unloaded.

## Properties

`Settled::check` runs once every load and unload has returned and the engine has
emptied its queue. It asserts all of these.

1. `get_current_model` names the model the engine holds, or neither names one.
2. The model selector ends on what the engine holds. `Settled::frontend` replays
   the event stream through the handlers of `ModelSelector.tsx` and
   `RecordingOverlay.tsx`.
3. The overlay no longer shows a load.
4. Every `loading_completed` belongs to a load the engine reported loaded.
5. Each unload produced one `unloaded`, and no load produced any.

Loom also fails a schedule in which no thread can run. The fake `emit` waits for
the main thread unless it is called on it, which makes a lock held across an
emit show up as a deadlock.

## Scenarios

Loom runs each scenario under every interleaving of its threads. The engine's
owner thread is one of them.

| Scenario                          | Threads | Interleavings | Bound             |
| --------------------------------- | ------- | ------------- | ----------------- |
| `unload_races_one_load`           | 3       | 3,600         | none              |
| `unload_races_two_loads_in_a_row` | 3       | 115,370       | none              |
| `lost_worker_report_races_reload` | 4       | 2,669         | none              |
| `two_unloads_race_one_load`       | 4       | 971,544       | 4 forced switches |

The last scenario is too large to exhaust. With `LOOM_MAX_PREEMPTIONS=5` it
runs 6,604,592 interleavings in about three minutes and passes.

The deterministic tests pause the load inside the engine, or after `engine.load`
has returned, and request the unload there. Both end with
`["loading_started", "unloaded", "loading_cancelled"]`. Two controls cover a
serial load then unload, and a new load after an unload.

## Code under test

`build.rs` parses the production sources with `syn` and writes these items to
`OUT_DIR`, unchanged:

- From `managers/transcription.rs`, the `ModelState` struct and the methods
  `load_model_with_device`, `begin_unload`, `publish`, `request_unload`,
  `forget_model_if_engine_dropped`, `get_current_model`, `is_model_loaded`,
  `now_ms` and `touch_activity`.
- From `engine_supervisor/supervisor.rs`, the `EngineSupervisor` methods `ask`,
  `load`, `unload` and `loaded`, and `Owner::publish_loaded`.

Extraction requires exactly one method with the expected visibility and
signature, so the build fails when the source shape changes. `src/world.rs`
holds the fakes. `src/lib.rs` compiles it twice, once over `std` and once over
Loom's `Mutex`, atomics, channels and threads.

The owner thread's loop is written by hand. It takes a load and an unload in the
order `Owner::run`, `Owner::load` and `Owner::start` take them, without the
worker process.

## Mutations

Each part of the repair was removed in a scratch copy. Every mutant fails.

| Removed                                            | First failure                                |
| -------------------------------------------------- | -------------------------------------------- |
| The load asks the engine before it publishes       | The id names a model the engine doesn't hold |
| The unload reaches the engine inside the step      | The id is empty while the engine holds one   |
| The lost-worker report reads the engine inside it  | The id is empty while the engine holds one   |
| The lock is released before each emit              | Loom reports a deadlock                      |
| One thread emits the outbox in order               | The selector ends on the wrong state         |
| A superseded load emits `loading_cancelled`        | The selector stays on "loading"              |
| A superseded load doesn't emit `loading_completed` | The selector ends on "ready"                 |

## Limits

Two loads at once are outside the repair. The engine's snapshot doesn't say
which load produced it, so the older load can publish its id over the newer
one. `overlapping_loads_are_outside_the_repair` records that as an expected
failure. Production doesn't run two loads at once: `switch_active_model` and
`initiate_model_load` both claim `is_loading` first, and the command-line path
loads on one thread. The fixture doesn't model that claim.

The four-thread scenario is bounded, as the table says.

The fake engine has no worker process or GPU. Under Loom the owner doesn't
report a worker's exit, because Loom counts a reply nobody receives as a leak.
The blocking `unload_model` differs from `request_unload` only by waiting for
that reply.

The main-thread wait in `emit` follows `tray.rs`, where the tray snapshot reads
the window theme on the calling thread. It was not observed on macOS or Windows.
The frontend handlers were transcribed by hand and not run in a browser.
