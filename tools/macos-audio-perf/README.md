# handy-audio-perf

Tiny native-Swift harness for measuring the absolute best-case keypress → tone
round-trip latency on macOS, as a baseline to compare Handy against.

**Not part of the Handy app.** Only measurement infrastructure.

## What it measures

On every `Cmd+Shift+H` press, the harness:

1. Captures `t0 = mach_absolute_time()`
2. Builds (or starts, in warm mode) an `AVAudioEngine`
3. Installs a tap on the input node, records to `/tmp/handy-audio-perf/press-*.wav`
4. On the first tap callback, schedules a preloaded sine-tone buffer onto an
   `AVAudioPlayerNode` attached to the same engine, then plays it
5. Logs one CSV row per completed press

Stop recording with another `Cmd+Shift+H`.

## Columns

CSV path: `/tmp/handy-audio-perf/perf.csv` (override with `--csv=PATH`)

| Column | Meaning |
|---|---|
| `iso` | ISO timestamp |
| `mode` | `cold` or `warm` |
| `press` | 1-based press index within this process |
| `cold_or_warm` | same as `mode` (redundant; kept for grouping) |
| `mic` | Current default input device name |
| `engine_start_ms` | Time spent in `engine.start()` alone |
| `first_sample_ms_since_t0` | Swift-thread wall time from keypress to the first tap callback |
| `tone_play_call_ms_since_t0` | Swift-thread wall time from keypress to `player.play()` returning |
| `tap_host_ms_since_t0` | Audio-clock time (`AVAudioTime.hostTime`) of the first sample, as ms since keypress. This is the authoritative "when did audio really start" number. |
| `sample_rate` / `channels` | Input format the tap used |

## Run

```bash
cd tools/macos-audio-perf
swift build -c release
# Cold mode (rebuild engine every press)
.build/release/handy-audio-perf
# Warm mode (reuse engine)
.build/release/handy-audio-perf --warm
```

## Auto mode

Run without a human — useful for CI-style verification and for collecting
repeatable numbers:

```bash
.build/release/handy-audio-perf --auto --iterations=5 --min-hold=1 --max-hold=3 --idle=1
.build/release/handy-audio-perf --warm --auto --iterations=5
```

Defaults: 5 iterations, hold 1.0–5.0s random, idle 1.0s between presses.
Prints a per-mode summary (min/median/mean/max for each timing column) and
a per-press table, then exits cleanly.

## Triggers

Any one of these toggles a press:

* `Cmd+Shift+H` (primary)
* `Ctrl+Shift+H`
* `Ctrl+Opt+Space`
* `F19`
* Pressing **Enter** in the terminal where the harness is running

The Carbon hotkeys are global (work while any app is focused); the Enter
fallback is handy for quick sanity checks when terminal-emulator keyboard
protocols swallow a combo.

The first microphone-using press will prompt for Microphone access. Grant it
— the harness needs the real hardware timing.

No Accessibility permission is required: Carbon `RegisterEventHotKey` is
narrowly scoped and doesn't need TCC.

## Scope discipline

Deliberately minimal: one hotkey, one mic, one tone, one wav. Resist feature
creep — every addition risks adding noise to the numbers. If you want to
test a different mic, change the system-default input device in
System Settings → Sound.
