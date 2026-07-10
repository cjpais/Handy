# Wake-word activation ("Hey Jarvis") for Handy

## Context

The user wants hands-free transcription: saying a wake word (default **"Hey Jarvis"**) starts a recording; after they stop speaking it auto-transcribes and pastes — no keyboard. Confirmed decisions:

- **Detection:** openWakeWord-compatible ONNX models. Preset dropdown (Hey Jarvis default + Alexa, Hey Mycroft, Hey Rhasspy) plus "import custom model" (.onnx head trained via openWakeWord's free Colab).
- **Stop:** wake-word-initiated sessions auto-stop after ~2 s of silence (Silero VAD; timeout adjustable). Shortcut/PTT sessions keep today's manual behavior.
- **Opt-in:** default off. Enabling keeps the mic stream always on (~1–2 % CPU, disclosed in the UI).
- **License flag:** openWakeWord code is Apache-2.0, but pretrained heads are **CC BY-NC-SA 4.0** — fine for a personal fork, must be resolved before commercial distribution. Add attribution.

Verified foundation: `ort` v2.0.0-rc.12 is already in Cargo.lock (via cjpais/vad-rs + transcribe-rs); the recorder already resamples every chunk to 16 kHz/30 ms frames even when idle and merely drops them in one early-return (`recorder.rs` `handle_frame` ~line 573); all triggers funnel into `TranscriptionCoordinator`, and the CLI/signal path (`signal_handle.rs:16` `send_transcription_input`) is the exact template for a wake trigger; the coordinator already ignores triggers while busy; `TranscribeAction::start` receives the trigger source string, so wake-initiated sessions are detectable without new plumbing.

## Architecture

```
cpal stream ─► run_consumer (recorder worker thread, 480-sample/30ms 16kHz frames)
                 ├─ recording? ─► existing VAD/handle_frame path
                 │                  └─► AutoStopTracker (NEW): speech-seen / consecutive-noise
                 │                       counters → auto_stop_cb(AutoStopEvent), fires once
                 └─ idle?      ─► WakeWordRuntime.push_frame (NEW): buffer 480→1280,
                                   3-stage ort pipeline (melspec → embedding → head),
                                   threshold + debounce + 2s refractory → on_detect()
on_detect  ──► send_transcription_input(app, "transcribe", "wakeword")   [existing path]
auto_stop  ──► coordinator.notify_auto_stop(event)                        [NEW Command]
```

Inference runs inline on the recorder consumer thread (~1–3 ms per 80 ms chunk; the unbounded mpsc absorbs jitter). Runtime enable/disable/model-swap via `Arc<WakeWordRuntime>`: per-frame `AtomicBool` check, `Mutex<Option<WakeWordDetector>>` with `try_lock` so a swap never stalls capture.

## Steps

### 0. Persist plan copy

Copy this plan to `features/wake-word/plan.md` (per user's global planning convention).

### 1. Wake-word detector module (~1–1.5 d)

New `src-tauri/src/audio_toolkit/wakeword/{mod.rs,detector.rs}` (export from `audio_toolkit/mod.rs`). Cargo.toml: add `ort = { version = "=2.0.0-rc.12", default-features = false, features = ["ndarray"] }` + matching `ndarray` (pin exactly; ort RCs are API-incompatible — mimic session construction from the cjpais/vad-rs fork which compiles against it in-tree).

`WakeWordDetector` — `new(melspec_path, embedding_path, head_path, WakeWordConfig{threshold, trigger_chunks, refractory})`, `push_frame(&[f32;480]) -> Result<bool>` (returns true once per detection, then clears buffers + 2 s refractory), internal `process_chunk(&[f32;1280]) -> f32`. Pipeline (per openWakeWord reference + oww_rs): scale −1..1 floats ×32767; melspectrogram.onnx `[1,1280]` → `[1,1,5,32]`, transform `x/10 + 2.0`; rolling 76-frame mel window → embedding_model.onnx `[1,76,32,1]` → `[1,1,1,96]`; rolling 16-embedding window → head `[1,16,96]` → sigmoid score. **Verify tensor shapes from session metadata at load; assert, don't assume** (custom Colab heads keep head I/O shape).

`WakeWordRuntime` — `enabled: AtomicBool`, `detector: Mutex<Option<WakeWordDetector>>`, `on_detect: Mutex<Option<Arc<dyn Fn()+Send+Sync>>>`; `push_frame` no-ops unless enabled+present.

### 2. Recorder: idle-frame hook + auto-stop tracker (~1 d)

Edit `src-tauri/src/audio_toolkit/audio/recorder.rs`:

- `RecordSession { vad_policy: VadPolicy, auto_stop: Option<AutoStopConfig> }` replaces the bare policy in `Cmd::Start` (line 26) and `AudioRecorder::start` (line 319); `AutoStopConfig { silence_frames, no_speech_frames (~267 = 8 s), max_frames (2000 = 60 s) }`; `enum AutoStopEvent { SilenceStop, NoSpeechCancel, MaxDurationStop }`. Update callers (`managers/audio.rs::try_start_recording` line 481; check `audio_toolkit/bin`).
- Builder: `.with_wake_word(Arc<WakeWordRuntime>)`, `.with_auto_stop_callback(Arc<dyn Fn(AutoStopEvent)+Send+Sync>)` — cloned into the worker in `open()` (survives device switches, verified).
- `handle_frame` (~line 565): in the `!recording` early-return, call `wake.push_frame(samples)`; make it return a `FrameVerdict { Inactive, Speech, Noise }` derived from the existing VAD match.
- `run_consumer`: per-session `AutoStopTracker { frames, speech_seen, consecutive_noise, fired }` (frame-count timing, 30 ms/frame); on first trip invoke callback once; recording continues until the coordinator issues the real stop. Note: SmoothedVad hangover (15 frames) adds ~0.5 s to effective stop latency — acceptable, document.

### 3. WakeWordManager + mic lifecycle (~0.5–1 d)

New `src-tauri/src/managers/wakeword.rs`: holds `Arc<WakeWordRuntime>`; `apply_settings()` (re)loads detector on a spawned thread from settings; `resolve_model_paths` resolves bundled files via `app.path().resolve("resources/models/wakeword/…", BaseDirectory::Resource)` (same pattern as `preload_vad`, `managers/audio.rs:349`), custom head from the settings path. `on_detect` → `send_transcription_input(&app, "transcribe", WAKE_SOURCE)` (define `pub const WAKE_SOURCE: &str = "wakeword";` here).

Edit `managers/audio.rs`: `create_audio_recorder` (line 131) gains wake runtime + auto-stop callback (callback calls `coordinator.notify_auto_stop(event)`); `AudioRecordingManager::new` takes the runtime; new `effective_microphone_mode(settings)` = AlwaysOn if `always_on_microphone || wake_word_enabled` — use in `new()` and in `commands/audio.rs::update_microphone_mode` (line 154). Edit `lib.rs` setup (~line 160): create WakeWordManager before AudioRecordingManager, manage both, `apply_settings()` after the coordinator is managed (~line 837).

### 4. Coordinator + action wiring (~0.5 d)

Edit `transcription_coordinator.rs`: `Stage::Recording { binding_id, wake_initiated }` (set from `hotkey_string == WAKE_SOURCE` in `start()`, line 161; update matches at lines 76, 85). New `Command::AutoStop { event }` + `notify_auto_stop()`: acts **only** when `Recording { wake_initiated: true }` (dedicated command, NOT toggle `send_input` — a toggle racing a manual stop would start a phantom recording); SilenceStop/MaxDurationStop → existing `stop()`; NoSpeechCancel → `utils::cancel_current_operation(&app)` + `stage = Idle`.

Edit `actions.rs` `TranscribeAction::start` (line 464): if source is WAKE_SOURCE, force `VadPolicy::Offline` when settings produced `Disabled` (silence tracking needs VAD) and build `AutoStopConfig` from `wake_word_silence_timeout_ms`; pass `RecordSession` through (lines 549, 558). **No changes to `stop`** — auto-stop reuses it (sound, overlay, paste). No new "listening" tray/overlay state in v1.

### 5. Model files (~0.5 d)

Bundle six files in `src-tauri/resources/models/wakeword/` (covered by `resources/**/*` glob — verify): `melspectrogram.onnx` (~1.1 MB), `embedding_model.onnx` (~1.4 MB), `hey_jarvis_v0.1.onnx`, `alexa_v0.1.onnx`, `hey_mycroft_v0.1.onnx`, `hey_rhasspy_v0.1.onnx` (~0.3 MB each; ~5 MB total). Source: openWakeWord GitHub release v0.5.1 assets (verify filenames at download time). Bundling beats download-on-select (offline, no ModelManager plumbing). Add CC BY-NC-SA attribution note to README/credits.

### 6. Settings + commands (~0.5 d)

`settings.rs`: `WakeWordModel { HeyJarvis(default), Alexa, HeyMycroft, HeyRhasspy, Custom }` (specta `Type`, snake*case) + fields `wake_word_enabled: bool(false)`, `wake_word_model`, `wake_word_custom_model_path: Option<String>`, `wake_word_threshold: f32(0.5)`, `wake_word_silence_timeout_ms: u64(2000)` — all `#[serde(default…)]`; mirror in `get_default_settings()`; check frozen-store test (settings.rs:1131).
Five `change_wake_word*\*`commands in`commands/audio.rs`(patterns:`change_audio_feedback_setting`, `change_transcribe_accelerator_setting`): enabled (also `apply_settings()`+`update_mode(effective_microphone_mode)`), model, custom path (validate by attempting detector load, Err on bogus file), threshold (`WakeWordRuntime::set_threshold`, no session reload), silence timeout. Register in `collect_commands!`(lib.rs ~539); debug build regenerates`src/bindings.ts`.

### 7. Frontend + i18n (~0.5–1 d)

`settingsStore.ts`: five `settingUpdaters` entries (pattern `vad_enabled`). New `src/components/settings/WakeWordSettings.tsx` (SettingsGroup): ToggleSwitch (pattern `AudioFeedback.tsx`, description discloses always-on mic + CPU); model Dropdown (pattern `AccelerationSelector.tsx`); Custom → file-pick row via `open()` from `@tauri-apps/plugin-dialog` with `.onnx` filter (pattern: `PasteMethod.tsx` external-script branch), errors via toast; threshold slider 0.1–0.9/0.05 (pattern `VolumeSlider.tsx`); timeout slider 500–5000 ms/250. Render in `general/GeneralSettings.tsx` after the Sound group. i18n: `settings.wakeWord.*` keys in `en/translation.json` **and all other locales** (English placeholders OK; `bun run check:translations` enforces key sync).

### 8. Verification (~0.5 d)

- **Unit tests** (`cd src-tauri && cargo test`): detector rechunk test (silence frames → one process_chunk per 1280 samples, score < threshold) + a checked-in "hey jarvis" wav fixture asserting exactly one trigger + refractory suppression (`#[ignore]`/env-gated if resources unresolvable in tests); `AutoStopTracker` counter tests (silence-after-speech → SilenceStop at right frame; never-speech → NoSpeechCancel; max → MaxDurationStop; fires once); coordinator `AutoStop` no-op while Idle/Processing/non-wake Recording.
- **Manual** (`bun run tauri dev`): enable → stream stays open; "hey jarvis" → start sound + overlay; speak, ~2.5 s silence → transcribe + paste; wake + stay silent 8 s → cancel, no paste; wake during Processing → ignored; shortcut PTT/toggle unaffected, never auto-stop; disable → OnDemand restored; device switch keeps detection alive; bogus custom .onnx rejected with error.
- Regression: `cargo test`, `bun run check:translations`, `bun run lint`, confirm regenerated `bindings.ts`.

## Risks

1. **ort rc.12 API quirks** — pin exact version, `default-features = false` (avoid a second execution-provider set colliding with transcribe-rs features), copy session-construction idioms from vad-rs.
2. **Tensor-shape assumptions** (5 mel frames/chunk, `x/10+2`, 76/16 windows) — validate against openWakeWord reference + oww_rs in Step 1 before building the rolling buffers; assert shapes from session metadata.
3. **False positives** — threshold slider, `trigger_chunks` constant (raise to 2 if noisy), 2 s refractory, coordinator busy-guard.
4. **Coordinator races** — solved by dedicated `Command::AutoStop` (stop-only-if-wake-recording).
5. **Model licensing** (CC BY-NC-SA heads) — attribution now; resolve before any commercial distribution.
6. **CPU/battery** — ~1–2 % when enabled; zero-cost when disabled (one atomic load per frame).

## Effort: ~5–6.5 days total

Sequencing: 1 → 2 → 3 → 4 (backend slice testable via logs with hardcoded enable) → 6 → 7 → 5 (models droppable any time after 1) → 8.
