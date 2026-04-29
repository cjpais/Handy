# Handy shortcut→tone latency investigation

*Working document, not for commit. Captures state-of-knowledge, where we got it wrong, and what to measure next.*

## What we actually know

### Measurements from the installed app (Rob's Mac, Studio Display USB mic)

Real numbers from `~/Library/Logs/com.pais.handy/handy.log`. Every line is a real press.

**Cold path (`lazy_stream_close = false`, every press cold-opens the mic stream):**

| Metric | Observed |
|---|---|
| `Microphone stream initialized` (`start_microphone_stream`) | **624–711ms**, steady ~650ms ±30ms |
| `Recording started in` (`try_start_recording` end-to-end) | +~0–2ms on top |
| `TranscribeAction::start completed` (synchronous Rust part) | +~10ms on top |

Seven presses spread across ~15 minutes all paid ~650ms. `lazy_stream_close=false` was closing the stream on every stop.

**Warm path (`lazy_stream_close = true`, back-to-back within 30s):**

| Press | `TranscribeAction::start completed` |
|---|---|
| #1 (cold, post-restart) | 663ms |
| #2 (warm, 17s later) | 7.0ms |
| #3 (warm) | 8.9ms |
| #4 (warm) | 6.6ms |

`try_start_recording` on warm path is microseconds — it's just an mpsc send to a running worker thread.

### Where I got it wrong

I read "7ms" and declared the warm path "essentially instant." **That number measures only the synchronous Rust work in `TranscribeAction::start` — it completes the moment the tone-playing thread is *spawned*, not when the tone plays.**

The tone path runs entirely *after* `start_time.elapsed()` is logged:

```rust
std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(100));     // ← 100ms hard sleep
    play_feedback_sound_blocking(&app_clone, Start);    // ← cold rodio output every press
    rm_clone.apply_mute();
});
```

So the real warm-path keypress→tone-audible budget is roughly:

| Stage | Estimate |
|---|---|
| Rust synchronous (the "7ms" I quoted) | ~8ms |
| Hard sleep before tone | 100ms |
| `host.output_devices()` + `OutputStreamBuilder::open_stream()` — **cold every press** | ~50–150ms |
| WAV decode (`symphonia` probe + `rodio::play`) | ~5–15ms |
| CoreAudio scheduling → first sample to speaker | ~10–20ms |
| **Total** | **~175–290ms** |

Those numbers are estimates, not measurements. **We have not measured the tone tail.** That's the gap in our data, and it's the thing Rob is actually hearing. The warm-path logs contain no timing between `TranscribeAction::start completed` and the moment audio hits the speaker.

### What does still hold

* `lazy_stream_close=true` eliminated the ~650ms cold input-stream open on back-to-back presses. That is a real, measured, ~100× win — just not the full story.
* The remaining perceived lag on the warm path is in the *output* audio path and the hardcoded sleep, not the input/recording path.
* First press of each session is still cold — another ~650ms that we have not yet attacked.

## What we don't know (and should)

1. **How much of the ~180–290ms warm-path tone tail is the 100ms sleep, vs rodio cold-open, vs scheduling?** Instrumentation could answer this inside Handy.
2. **How low can this go in principle on macOS?** That is, what's the platform floor vs Handy's choices? We have no baseline.
3. **Is the ~650ms cold input-stream open a cpal cost, a CoreAudio cost, or a USB-mic hardware cost?** Without a non-cpal reference, we can't tell.
4. **Does keeping an `AudioUnit` initialized but not started give us a faster warm re-start than lazy_stream_close's full-stream retention?** Open question.

Answering 2 and 3 before writing more Rust is the right move. Otherwise we'll keep optimizing the wrong layer.

## Proposal: a native-Swift perf harness

Rob's idea, written out:

### Goals

A standalone, minimal program — not part of Handy — that exercises the same pipeline (global hotkey → start recording → play tone → capture wav) on the same Mac with the same microphone, using native macOS APIs directly. No Rust, no cpal, no rodio. Numbers from this become the baseline we compare Handy against.

### What to measure

Every run, write a single line per event with monotonic timestamps (mach_absolute_time or ContinuousClock). At minimum:

* **t0** — keypress received from the hotkey handler
* **t1** — audio input session/engine fully started
* **t2** — first input sample callback fires
* **t3** — tone playback scheduled (sink.play / AVAudioPlayer.play)
* **t4** — first audible tone sample hits the output (can approximate via AVAudioPlayer.deviceCurrentTime or audio render callback timestamps)
* **t5** — tone playback finishes
* **t6** — key released (for push-to-talk equivalent) / second press (for toggle equivalent)
* **t7** — stop() returns, wav flushed to disk

Report: t1-t0, t2-t0, t4-t0, t7-t6, plus cold vs warm runs.

### Matrix to run

For each combination:

* **Mic**: built-in MacBook mic, Studio Display USB mic, Bluetooth headset (if handy).
* **Start strategy**: cold (create + start engine every press) vs warm (pre-created engine, start/stop only).
* **API**: AVAudioEngine, AudioToolbox/AudioUnit direct. Optionally AVAudioSession.
* **Buffer size**: default, 512, 256, 128 samples — see how low latency scales.

Key hypothesis to disconfirm: *the ~650ms cold-open is inherent to CoreAudio / USB-audio on this Mac.* If the Swift harness can cold-open AVAudioEngine in, say, 40ms on the Studio Display mic, cpal's overhead is the story and we know where to fix it. If Swift also pays ~500ms+, we stop trying to shave the cold-open and instead redesign around "don't cold-open."

### Approach outline

* SwiftPM executable target in `tools/macos-audio-perf/`. Runs as a background AppKit app (`NSApplication`, `.accessory` activation policy, no dock icon) so we get an event loop for the Carbon hotkey.
* Hotkey: Carbon `RegisterEventHotKey` + `InstallEventHandler` against the application event target. This is technically deprecated (since 10.8) but remains stable and is the *only* macOS global-hotkey path that does not require Accessibility permission — which matters because we want the harness to run without TCC prompts. Note: on macOS 15 Sequoia+, at least one modifier must be something other than Shift or Option; `Cmd+Shift+<key>` is fine. Handy itself goes through `rdev` / `handy-keys` / `tauri-plugin-global-shortcut`, which ultimately use `CGEventTap` (Accessibility-gated); the perf harness deliberately takes a simpler, lower-layer path.
* Input: AVAudioEngine (actively maintained on macOS, straightforward API). Install a tap on `engine.inputNode` with `installTap(onBus:0, bufferSize:1024, format: inputNode.outputFormat(forBus: 0)) { buffer, when in ... }`. The `when` is an `AVAudioTime` whose `hostTime` is mach ticks at the first sample in the buffer — authoritative timing for when audio actually started flowing. Record to disk with `AVAudioFile(forWriting:)` + `file.write(from: buffer)` inside the tap.
* Output: generate a short sine tone into an `AVAudioPCMBuffer` at startup (no file I/O on the hot path). Attach an `AVAudioPlayerNode` to the same engine, connect to `engine.mainMixerNode`, `scheduleBuffer(_:)` + `play()` to fire the tone. Using the same engine as input means no second stream to cold-open.
* Timestamps: `mach_absolute_time()` (or `DispatchTime.now().uptimeNanoseconds` which wraps it) for Swift-side points — ns precision, safe in audio callbacks. Convert tap's `AVAudioTime.hostTime` the same way. All timestamps as u64 ns; deltas computed at log-write time.
* `AVAudioSession` is iOS-only in practice — `sharedInstance` is unavailable on macOS. No session configuration needed; we just use AVAudioEngine directly against the current default input/output devices.
* Note on device routing: AVAudioEngine's `inputNode` wraps the *current system default input*. Since your Handy mic setting is "Default" and the system default is the Studio Display mic, the harness picks it up automatically — no device-id plumbing required for a first pass.
* Output: append one CSV row per press to `tools/macos-audio-perf/perf.csv` with cold/warm flag, mic name, each timestamp. Easy to grep/plot later.

### What we'd do with the results

* Compare Swift cold-open vs Handy/cpal cold-open on the same mic. Difference tells us how much of Handy's 650ms is cpal vs CoreAudio.
* Compare Swift warm-restart (engine pre-built, `.start()` on already-configured engine) vs Handy's `lazy_stream_close` path. Tells us whether keeping the engine initialized but inactive (as opposed to fully-running like Handy does today) is competitive.
* Establish the real tone tail: keypress → tone audible, for a pre-loaded tone through a warm output node. That's our "instantaneous feels" target.

### Scope discipline

The harness is for **measurement only**. It doesn't need VAD, transcription, overlay, settings, or multiple bindings. It's one hotkey, one mic, one tone, one wav. Resist feature creep — every addition risks adding noise to the numbers.

If we want to preserve this work long-term, it lives in its own directory (e.g. `tools/macos-audio-perf/`) with its own README. Not part of the Handy build.

## What to do with the Rust changes from this session

On this branch (`recorder-open-tests`):

* `test(audio): add AudioRecorder lifecycle coverage` — solid, stays. Safety net for later refactors regardless of which fix we land.
* `chore: pin bun 1.3.13 via .mise.toml` — also solid, stays. Independent of the latency work.

Nothing else has been committed. The "skip 100ms sleep on warm path" and "pre-warm rodio output" changes were not written — stopped before touching code, which is where this document picks up.

## Decisions (2026-04-24)

* Harness lives in-repo at `tools/macos-audio-perf/`, its own SwiftPM target.
* Mic: Studio Display USB mic (same as Handy's current default).
* API: AVAudioEngine first. AudioUnit/AudioToolbox is a follow-up if we need to explain a delta between the harness and Handy.
* Collette writes the first version of the harness.

## Results (2026-04-24)

Harness driven by `--auto` mode (autonomous press/stop cycles, random 1–3s hold,
1s idle between). Five iterations per mode. Same Mac and mic as Handy.

### Cold — `AVAudioEngine` rebuilt every press

| metric | min | median | mean | max |
|---|---|---|---|---|
| engine_start_ms | 468.06 | 492.06 | 485.90 | 495.90 |
| first_sample_ms (swift) | 644.20 | 670.32 | 534.21* | 679.15 |
| tone_play_call_ms | 654.70 | 680.66 | 542.55* | 689.61 |
| tap_host_ms | 527.94 | 553.82 | 441.13* | 562.85 |

\* One press out of five produced zero samples (CoreAudio release/reacquire
glitch between rapid cold cycles) which dragged the means. Medians are the
honest signal.

### Warm — engine started once at init, kept running

| metric | min | median | mean | max |
|---|---|---|---|---|
| engine_start_ms | 0 | 0 | 0 | 0 |
| first_sample_ms (swift) | 104.26 | 106.50 | 107.60 | 115.00 |
| tone_play_call_ms | 104.27 | 106.50 | 109.72 | 125.51 |
| tap_host_ms | **−12.01** | **−9.89** | **−8.70** | **−1.27** |

### Reading these numbers

1. **The cold-path 640ms we measured in Handy is the platform floor on this
   mic, not a cpal problem.** Native Swift + AVAudioEngine cold-opens pay
   ~490ms in `engine.start()` and ~180ms more before the first sample
   callback arrives. Total ~670ms, essentially identical to Handy's 640ms.
   No Rust-level change to the cold-open path can beat this.

2. **The warm path — engine kept running between presses — gives 107ms
   end-to-end on this mic with native APIs.** That's 6× better than the
   cold path and 2–3× better than what Handy achieves today with
   `lazy_stream_close = true`. Handy's warm path costs more because it:
   * still `.stop()`s and `.start()`s the cpal stream on press boundaries
     (not fully warm)
   * adds a 100ms hard sleep before tone playback
   * cold-opens the rodio output stream per press

3. **`tap_host_ms` = −10ms in warm mode is informative, not a bug.** The
   engine has been capturing audio continuously, so the first buffer
   delivered to a newly-installed tap contains samples that were captured
   ~10ms *before* our keypress. The hardware is producing samples the whole
   time; we just weren't looking until the press.

4. **Most of the remaining 105ms in warm mode is `installTap()` latency**,
   not hardware latency. Since the audio is already flowing, installing a
   tap is fundamentally a bookkeeping operation — but AVAudioEngine appears
   to take ~5 buffer periods (≈100ms at 1024 frames @ 48kHz) to actually
   route audio to the new tap. A cheaper design: install the tap once at
   engine start and gate file-writing with a `recording?` flag. The
   keypress becomes a single atomic boolean flip.

## Implications for Handy

Two concrete avenues emerge:

1. **Stay with cpal, but adopt the "always-running, tap-once" pattern.**
   Instead of Handy's current "open stream when recording starts, close it
   after" model (or even lazy_stream_close's "hold it open for 30s"), keep
   the stream open indefinitely at app launch and gate sample retention
   behind a state flag. This maps to always_on_microphone semantics but
   without the current implementation's cost structure. Target: match the
   Swift harness's ~100ms warm-path number, or beat it.

2. **Parallel the ~500ms cold open.** For the "first press of each session"
   case, there's no way to make cold-open fast, so move it out of the
   critical path: pre-warm the stream at app launch (or on main-window
   focus), so the first press the user makes is already on the warm path.
   Tradeoff: mic indicator lights up earlier.

The 100ms sleep in Handy is a red herring in light of these numbers — it's
100ms on top of a 500–640ms stream-open operation. Worth cleaning up but
not the hot thing.

### Flakes and unknowns

* One press in five in cold mode produced zero samples (`samples_written=0`,
  no tap callbacks fired) with no error from `engine.start()`. Likely a
  CoreAudio hiccup between rapid cold engine cycles. Not investigated.
* We haven't measured a "tap-always-installed" mode inside the harness.
  That's the next natural experiment — if it drops warm-path below 30ms,
  we've quantified the remaining headroom.
* Numbers are specific to Studio Display USB mic. Built-in MacBook mic
  would likely show a faster cold floor (50–100ms) and similar warm path.
  Worth re-running on a different mic to confirm the pattern.

## Upstream issue #1283: "v0.7.9 was faster" — and what we found when we looked

Issue: <https://github.com/cjpais/Handy/issues/1283>

domdomegg [bisected the `Microphone stream initialized in X.XXms` log across
Handy releases][bisection] with push-to-talk and `always_on_microphone=false`:

[bisection]: https://github.com/cjpais/Handy/issues/1283#issuecomment-4236423275

| Version | Built-in mic | Brio 500 (USB) |
|---|---|---|
| **v0.7.9** | **16–21ms** | **22–32ms** |
| v0.7.10 | 133–175ms | 332–362ms |
| v0.7.11 | 123–144ms | 334–371ms |
| v0.7.12 | 140–142ms | 380–416ms |
| v0.8.0 | 154–163ms | 394–399ms |
| v0.8.2 | 159–165ms | 394–425ms |

That looks like a 10–20× regression, dramatic especially at the v0.7.9 → v0.7.10
step. It's also flatly inconsistent with everything we know about macOS
audio initialization — Apple's own docs and the [m13v comment in the same
thread][m13v] both place the CoreAudio HAL I/O unit cold-spin-up at
hundreds of ms, and our native-Swift AVAudioEngine harness measured ~500ms
of pure `engine.start()` on the same machine. 16ms for a true cold-open on
a USB mic is not physically plausible.

[m13v]: https://github.com/cjpais/Handy/issues/1283#issuecomment-4233676810

### What actually changed between v0.7.9 and v0.7.10

Code analysis of the 11 commits between the two tags; `settings.rs`
defaults are identical (`always_on_microphone = false` in both), and
`managers/audio.rs::start_microphone_stream` is byte-for-byte unchanged —
including the `start_time = Instant::now() … info!("Microphone stream
initialized in …")` log. The only material change on the hot path is
PR [#945 "Handle microphone init failure without aborting"][pr945],
which restructured `AudioRecorder::open()` in
`src-tauri/src/audio_toolkit/audio/recorder.rs`:

[pr945]: https://github.com/cjpais/Handy/pull/945

**v0.7.9 `open()`** (async / fire-and-forget):

```rust
pub fn open(&mut self, device: Option<Device>) -> Result<(), _> {
    // ... channel setup ...
    let worker = std::thread::spawn(move || {
        let config = AudioRecorder::get_preferred_config(&thread_device)
            .expect("failed to fetch preferred config");
        let stream = match config.sample_format() { /* build_stream */ };
        stream.play().expect("failed to start stream");
        run_consumer(...);
    });

    self.device = Some(device);
    self.cmd_tx = Some(cmd_tx);
    self.worker_handle = Some(worker);
    Ok(())   // ← returns immediately; the CPAL work is still running
}
```

**v0.7.10 `open()`** (synchronous handshake):

```rust
pub fn open(&mut self, device: Option<Device>) -> Result<(), _> {
    // ... channel setup ...
    let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    let worker = std::thread::spawn(move || {
        let init_result = (|| {
            let config = AudioRecorder::get_preferred_config(&thread_device)?;
            let stream = build_stream_typed(...)?;
            stream.play()?;   // ← real CoreAudio / HAL I/O cold-spin happens here
            Ok((stream, sample_rate))
        })();
        match init_result {
            Ok((stream, _)) => {
                let _ = init_tx.send(Ok(()));
                run_consumer(...);
            }
            Err(msg) => { let _ = init_tx.send(Err(msg)); }
        }
    });

    match init_rx.recv() {
        Ok(Ok(())) => { /* assign fields, Ok */ }   // ← blocks until worker finishes stream.play()
        // ... error paths ...
    }
}
```

The goal of PR #945 was error propagation — in a release build with
`panic = "abort"`, the v0.7.9 `.expect("failed to start stream")` would
terminate the process if CPAL couldn't open the device (notably, when
Windows privacy blocked the mic). The fix converted every panic path in
the worker into a real `Result`, and added the init channel so `open()`
could return a real `Err` to the caller. Converting the worker to
synchronous init is a correct fix for the reliability problem it solved.

### So the "500ms regression" isn't a runtime regression

It's an **observability correction**. The work done between keypress and
first-sample-available is the same: CoreAudio allocates the HAL I/O unit,
negotiates formats with the device, and starts producing buffers. That
still takes ~150–500ms depending on the mic.

What changed is which of those milliseconds `Microphone stream initialized
in X.XXms` actually measures:

* **v0.7.9**: measured `mpsc::channel` creation + `std::thread::spawn` +
  three struct-field assignments. Real stream startup continued in the
  background after the log fired. The "16ms" was honest about what
  `open()` took to return, but misleading about when the mic was ready.
* **v0.7.10+**: measures `mpsc::channel` creation + thread spawn +
  blocking until the worker's `stream.play()` has returned. That is
  actual mic readiness. The "500ms" is the CoreAudio HAL cold-open cost,
  always present, finally reported.

This matches cjpais' reaction in the thread ("I don't recall much changing
in that path") — because the runtime behaviour didn't really change.

### Consequences for what the user perceives

In **v0.7.9**, because `open()` returned in ~16ms but the CPAL stream
wasn't actually producing samples for another ~480ms:

* Tone plays at ~116ms post-keypress (100ms hardcoded sleep + 16ms open).
* Samples start flowing at ~500ms post-keypress.
* The first ~400ms of any speech after the tone is silently lost.
* Short press-and-release cycles (<500ms) produced effectively empty
  recordings — the `Cmd::Start`/`Cmd::Stop` pair was queued against a
  worker that hadn't started producing samples yet.

In **v0.7.10+**, `open()` blocks honestly:

* Tone plays at ~600ms post-keypress (open ~500ms + 100ms sleep).
* Samples flow from before the tone plays.
* No lost first words.
* The press feels laggier because it *is* honest about when the mic is
  actually live — the user waits for feedback instead of losing audio.

The upstream cjpais correctly described v0.7.9's real model in the thread:
*"This was the default way in initial versions of Handy to eliminate
latency"* — though "eliminate" is partly true (the log reported it
eliminated) and partly a mirage (samples still took 500ms to appear).

### Implication for our spike

1. **Don't revert to the v0.7.9 approach.** The synchronous handshake is
   a correctness win and we shouldn't drop it to chase the old
   "16ms" number — that number wasn't measuring anything useful and the
   UX costs (lost first words, silent empty recordings on quick presses)
   were real.
2. **The 500ms cold-open problem that Handy's cold path still pays is
   genuine, not artificial.** Our Swift harness confirmed that cost
   independently via AVAudioEngine. Any fix has to move the cost (always-on
   or pre-warm or warm-on-focus) rather than eliminate it in-place.
3. **Consider renaming the log.** "Microphone stream initialized" sounds
   like a platform-level fact; it's actually measuring Handy's
   `start_microphone_stream()` wall-clock. Something like
   `start_microphone_stream() returned in` would at least not mislead the
   next bisector.

### Related thread context worth keeping

* m13v's [pattern recommendation][m13v] (always-on engine + small circular
  ring buffer for pre-roll + VAD-gated retention) matches the
  "tap-always-installed, flag-gated" design the Swift harness is pointing
  toward.
* cjpais' [response][response]: the pattern is already in Handy as the
  experimental "always on microphone" toggle (Cmd+Shift+D); they chose
  not to ship it by default because users get unnerved by a permanently
  hot mic indicator. UX decision, not a technical limitation.

[response]: https://github.com/cjpais/Handy/issues/1283#issuecomment-4234077982

## External prior art: macos-mic-keepwarm (2026-04-24)

Rob found [drewburchfield/macos-mic-keepwarm][keepwarm], a standalone
open-source utility that attacks the same hardware-sleep problem from the
opposite end: instead of measuring latency, it eliminates it by holding
the mic open permanently. Cloned to `~/src/oss/macos-mic-keepwarm/` for
analysis.

[keepwarm]: https://github.com/drewburchfield/macos-mic-keepwarm

### What it does

A single-file (~650 LOC) SwiftPM executable that runs as a LaunchAgent.
It opens an `AVCaptureSession` on the system default input mic, installs
an `AVCaptureAudioDataOutput` delegate, and *discards every sample*.
Nothing is recorded, stored, or transmitted. The sole effect: the mic
hardware stays powered and the Bluetooth SCO channel stays negotiated,
so the *next* app that opens the mic (SuperWhisper, Handy, macOS
Dictation, etc.) gets instant activation instead of a 2–5s cold-start.

### Architecture comparison: keep-warm vs our harness

| Dimension | macos-mic-keepwarm | Our handy-audio-perf harness |
|---|---|---|
| *Purpose* | Production keep-warm daemon | Measurement harness |
| *Framework* | AVCaptureSession + AVCaptureAudioDataOutput | AVAudioEngine + installTap |
| *Audio layer* | AVFoundation capture pipeline (CMIO) | AVAudioEngine (wraps AudioUnit) |
| *Lifecycle* | Runs continuously at login, never stops | Run-on-demand, press-by-press |
| *Samples* | Received and immediately discarded | Written to WAV files on disk |
| *Tone playback* | None | Pre-loaded 880 Hz sine via AVAudioPlayerNode |
| *Device tracking* | CoreAudio property listeners + debounced restart | None (uses default at launch) |
| *Bluetooth handling* | Extensive — debounced restart, background teardown, deadlock avoidance | None |
| *Recovery* | Heartbeat timer (5s), auto-restart on stall | None |
| *Timing* | Date() for logging only | mach_absolute_time() for sub-ms precision |
| *Hotkey* | None (daemon, not interactive) | Carbon RegisterEventHotKey |
| *Build* | SwiftPM, universal binary (arm64 + x86_64) | SwiftPM, debug-only local builds |
| *LOC* | ~650 | ~700 |

### The interesting API choice: AVCaptureSession vs AVAudioEngine

Our harness used `AVAudioEngine` because we wanted to measure the full
recording pipeline (tap → file write → tone playback) through a single
engine. keep-warm uses `AVCaptureSession` + `AVCaptureAudioDataOutput`
instead, which is the higher-level AVFoundation capture API (the same
one backing the Camera app, screen recording, etc.).

This is a meaningful difference:

* **AVCaptureSession** wraps CoreMedia I/O (CMIO), which manages the
  hardware lifecycle independently from the audio render graph.
  Starting/stopping capture is the session's job; there's no
  `AudioUnit` exposed to the caller. The delegate receives
  `CMSampleBuffer`s on a dispatch queue, not `AVAudioPCMBuffer`s on
  the audio render thread.
* **AVAudioEngine** wraps an `AUAudioUnit` graph. `installTap()` hooks
  into the render thread, and `engine.start()` spins up the underlying
  AudioUnit. It gives finer timing control (`AVAudioTime.hostTime`)
  but couples input and output into one graph.

For a keep-warm daemon that just needs to hold hardware open and discard
samples, AVCaptureSession is the right call — simpler lifecycle, no
render graph to manage, and the delegate callback on `.main` queue
makes heartbeat/recovery logic trivial.

For our *measurement* harness, AVAudioEngine was the right call — we
needed `AVAudioTime.hostTime` for authoritative sample timestamps, and
sharing one engine for input + output let us measure tone-playback
latency without a second stream cold-open.

### What keep-warm teaches us about Handy

**1. The "hardware sleep" problem is real, documented, and filed with Apple.**

keep-warm's author filed [FB21969131][feedback] with Apple (Feb 2026),
documenting the 2–5s hardware sleep on Apple Silicon, the Bluetooth SCO
negotiation delay, and proposing three API additions
(`prepareForCapture()`, `kAudioDevicePropertyStandbyMode`,
`maintainsStandbyState`). Apple hasn't responded yet.

[feedback]: APPLE_FEEDBACK.md

This confirms our harness finding: the ~500ms cold-open on Rob's Studio
Display mic is *not* even the worst case. On Apple Silicon with
Bluetooth, it can be 2–5s. The platform provides no API to pre-warm
without actually opening the stream.

**2. The "always-on, discard samples" pattern works in production.**

keep-warm is shipping exactly the pattern our harness results pointed
toward (and that m13v recommended in the #1283 thread): hold the mic
open, throw away audio you don't need, gate retention on a flag. The
difference is that keep-warm does it as an external daemon, not inside
the app itself.

For Handy, the equivalent would be:

* Keep the cpal input stream open at app launch (the existing
  `always_on_microphone` toggle, currently experimental).
* Don't write samples to the ring buffer until the user presses the
  hotkey — just let cpal's callback run and discard.
* On press, flip a flag; samples start accumulating immediately.
* On release, flip the flag back.

This is what cjpais already built behind `Cmd+Shift+D`. The blocker
isn't technical — it's the UX concern about the permanent mic indicator.

**3. Bluetooth teardown is a landmine we haven't stepped on yet.**

keep-warm's most battle-hardened code is its session teardown path:

* `AVCaptureSession.stopRunning()` can deadlock when a Bluetooth
  device disconnects during capture. CoreAudio's `HALB_Guard` waits
  on a condition variable for the dead device, blocking the calling
  thread indefinitely.
* keep-warm's fix: tear down the CMIO graph (remove inputs/outputs)
  on the main thread *first* to release the semaphore coreaudiod
  depends on, *then* dispatch `stopRunning()` to a background thread.
  If it hangs, it's a leaked thread, not a frozen process.
* A 10-second watchdog logs a warning if `stopRunning()` doesn't
  return, and the heartbeat timer will eventually restart the session
  on a new device anyway.

Handy's `lazy_stream_close` path (and the proposed always-on path)
will hit this same deadlock if users have AirPods. Handy's cpal-based
stream stop is synchronous today. Worth proactively adding a timeout
or background-thread teardown when we implement the always-on pattern.

**4. Device change debouncing is necessary, not optional.**

keep-warm debounces device changes by 3 seconds after discovering that
Bluetooth handoffs fire multiple rapid device-change events before the
new device is actually ready. Without the debounce, the session thrashes
(start → fail → retry → start → fail) during the SCO negotiation
window.

Handy doesn't currently listen for device changes at all — it picks the
device at recording start. If we move to an always-on model, we'll need
device-change handling, and keep-warm's 3-second debounce is a tested
starting point.

**5. Virtual audio plugins (Teams, Zoom) are a real-world complication.**

keep-warm documents that HAL plugins from Teams and Zoom
(`/Library/Audio/Plug-Ins/HAL/`) add startup latency and cause
`coreaudiod` to consume 36.8% CPU when idle. This is a real factor for
users who have conferencing apps installed — and it's outside Handy's
control. Worth noting in troubleshooting docs.

### What keep-warm *doesn't* tell us

* **Warm-start latency numbers.** keep-warm doesn't measure the latency
  from "another app opens the mic" to "first sample arrives" when the
  hardware is already warm. That's exactly what our harness measures.
  The two projects are complementary: keep-warm ensures the hardware
  is warm, our harness quantifies how much that's worth.

* **Whether AVCaptureSession-based warm-keeping benefits AVAudioEngine
  / cpal consumers.** The hardware warm state should be shared (it's a
  CoreAudio HAL property, not an API-specific one), but we haven't
  confirmed that keep-warm running alongside Handy actually eliminates
  Handy's cold-open cost. That's a simple experiment: run keep-warm,
  wait 60s, then do a cold press in Handy and check the
  `Microphone stream initialized in` log. If it drops from ~500ms to
  ~50ms, the answer is yes and we could recommend keep-warm as a
  stopgap while Handy moves to always-on.

### Possible next steps informed by keep-warm

* [ ] **Experiment**: run keep-warm alongside Handy, measure whether
  Handy's cold-open cost drops (tests whether hardware warm state is
  shared across API boundaries).
* [ ] **Experiment**: add a "tap-always-installed, flag-gated" mode to
  our harness. If warm-path drops below 30ms (vs current 107ms from
  `installTap()` latency), that's the design target for Handy.
* [ ] **Design note**: when Handy moves to always-on, add Bluetooth
  teardown resilience (background-thread stop, timeout watchdog) per
  keep-warm's battle-tested pattern.
* [ ] **Design note**: add device-change listeners with debouncing when
  implementing always-on mode.
* [ ] **Troubleshooting**: document the Teams/Zoom HAL plugin issue in
  Handy's troubleshooting section, since it affects cold-open latency
  regardless of what Handy does.
