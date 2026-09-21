# Handy Hands-Free Always-On Capture + "dude" Command Gate — Build Report

**Date:** 2026-06-02
**Branch:** `handsfree-capture`
**Spec:** `~/Projects/jarvis-stack/docs/superpowers/specs/2026-06-02-handy-handsfree-capture.md`
**Local-only. No network code touched.**

## TL;DR

- **`cargo build`: PASS** (backend, Linux target). `Finished dev profile ... in 45.18s`. The `handy` crate compiles clean with the new feature.
- **`cargo test --lib`: PASS** — 73 passed, 0 failed, including 8 new wake-word-gate tests and 3 new VAD-segmenter tests.
- Compile + test validation ran **on the N5 Linux box inside a `rust:trixie` Docker container** (the M4 Air is fanless; the first Tauri+ONNX+whisper.cpp compile is heavy). Source-of-truth + commits stay on the Mac branch. The final macOS `.app` bundle build is a separate step on the Air.

## What was implemented (file-by-file)

### `src-tauri/src/settings.rs`
Four new serde-defaulted settings next to `always_on_microphone`, plus `default_*` fns and entries in the default-settings builder (~line 777):
- `hands_free_capture: bool` (default `false`)
- `wake_word: String` (default `"dude"`)
- `wake_word_required_for_paste: bool` (default `true`)
- `capture_all_to_history: bool` (default `true`)

### `src-tauri/src/audio_toolkit/audio/recorder.rs`
- New `AudioRecorder::with_speech_frame_callback(cb)` where `cb: Fn(Option<&[f32]>)`. `Some(frame)` = a VAD-classified 30ms speech frame; `None` = a silence/noise frame. (Adapted from PR #618's `with_speech_frame_callback`, signature widened to also signal silence so the loop can detect utterance end.)
- Threaded `speech_cb` through `open` → worker → `run_consumer` → `handle_frame`.
- `handle_frame` now runs VAD and forwards frames to the hands-free tap **even when the manual recording gate is off** (so a continuous capture loop works with no shortcut press), while preserving the original `out_buf` behavior for manual recording. The stop-drain/finish flush paths pass `&None` so the manual recording's trailing audio is never double-fed to the segmenter.

### `src-tauri/src/managers/hands_free.rs` (NEW)
- `HandsFreeSegmenter`: accumulates speech frames into an utterance buffer; finalizes after ~0.6s of trailing silence (`SILENCE_FRAMES_TO_FINALIZE = 20`), drops utterances shorter than ~0.3s, caps a single utterance at ~30s. Cheap enough to drive from the audio callback.
- `HandsFreeManager`: owns a worker channel + `paused`/`running` atomics. `on_speech_frame` buffers and ships finalized utterances over an mpsc channel to a single worker thread (serialized, one utterance at a time, never blocks the audio callback — mirrors #618's "serialize through one thread" rule). `start`/`stop`/`toggle_pause`/`is_running`/`is_paused`.
- `process_utterance`: persists a WAV (`handy-handsfree-<ms>.wav`), transcribes via `TranscriptionManager`, then routes via `actions::route_hands_free_utterance` on the async runtime.
- 3 unit tests for the segmenter.

### `src-tauri/src/actions.rs`
- `strip_wake_word(text, wake_word) -> Option<String>`: whole-word, case-insensitive prefix match; strips the wake word + trailing punctuation/separators; returns the remainder with original casing preserved. Returns `None` when the wake word is not a leading whole word (e.g. "dudette" does NOT match "dude").
- `route_hands_free_utterance(app, transcription, file_name, wav_saved)`:
  1. Capture-all sink: when `capture_all_to_history`, saves EVERY utterance to history via `HistoryManager::save_entry` (reuses the existing `process_transcription_output` for Chinese-variant handling).
  2. Paste gate: when `wake_word_required_for_paste`, pastes **only** the stripped remainder when the wake word matched; otherwise pastes nothing. When the flag is `false`, pastes every utterance verbatim (pure always-on dictation). Paste reuses the exact `utils::paste(...) on main thread` flow from `TranscribeAction::stop`.
- 8 unit tests for `strip_wake_word` (the no-paste-spray safety guarantee).

### `src-tauri/src/managers/audio.rs`
- `create_audio_recorder` now takes the `HandsFreeManager` handle and attaches the speech-frame callback (no-op unless the loop is running).
- `AudioRecordingManager` gained a `hands_free` field; constructor wires it and auto-starts the loop when `settings.hands_free_capture` is true.
- New methods: `start_hands_free` (ensures the mic stream stays open + cancels lazy-close so VAD frames flow with no shortcut press), `stop_hands_free` (releases the mic in on-demand mode when idle), `toggle_hands_free_pause`, `is_hands_free_running`, `is_hands_free_paused`.

### `src-tauri/src/managers/mod.rs`
- Added `pub mod hands_free;`.

### `src-tauri/src/commands/audio.rs`
Five Tauri commands (mirror #618's wake-word command wiring):
- `start_hands_free` / `stop_hands_free` (persist the setting + start/stop the loop)
- `toggle_hands_free_pause` → returns new paused state
- `is_hands_free_running` / `is_hands_free_paused`

### `src-tauri/src/lib.rs`
- Registered the five commands in the `collect_commands![...]` (specta) list.

## Frontend
Not implemented (optional per spec). The Tauri commands are exposed and the feature is fully drivable from the settings JSON. A React/TS toggle can be added later against the existing commands; `specta` will generate the TS bindings for the new commands on the next codegen.

## Design notes / deviations from PR #618

- **No acoustic openWakeWord model.** Per the spec, V0 gates on the **transcript prefix**, not an ONNX acoustic model. #618's `managers/wakeword.rs` (melspec/embedding/wake ONNX sessions, `ort` dep, `resources/models/*.onnx`, the `hey_mycroft` test) is intentionally NOT pulled in. We reuse only #618's **non-acoustic scaffolding idea** (the speech-frame callback) and replace the fixed 5-second post-detection record window with continuous Silero-VAD segmentation — which is more robust and exactly what the spec's "Design (V0)" describes.
- The recorder's Silero VAD (already in-tree, `SmoothedVad`) does the segmentation; we never load a second model.

## How to enable + test

### Enable (settings JSON, no UI needed)
Edit the Handy settings store (the `store` plugin file in the app data dir; key `settings`) or call the command at runtime. Set:
```json
"hands_free_capture": true,
"wake_word": "dude",
"wake_word_required_for_paste": true,
"capture_all_to_history": true
```
Or invoke the Tauri command `start_hands_free` (persists `hands_free_capture = true` and starts the loop). A microphone must be selected/available.

### Acceptance steps (from the spec)
1. Enable `hands_free_capture`. Say **"dude hello world"** → `hello world` is pasted at the cursor, and a history entry is saved (source = command).
2. Say **"just testing"** → NOT pasted, but it appears in history (source = ambient).
3. Invoke `toggle_hands_free_pause` → capture stops (in-flight utterance discarded). Invoke again → capture resumes.
4. Set `wake_word_required_for_paste: false` → every utterance pastes verbatim (pure dictation mode).

> Note: "source = command/ambient" is currently a debug-log distinction (no schema change to the history table). Every captured utterance lands in the existing `transcription_history` SQLite table referencing its WAV. If a persisted source column is wanted later, add a migration in `managers/history.rs` (see TODOs).

### Reproduce the compile/test on N5 (Linux, Docker — keeps the Air cool)
```bash
# Mac is source-of-truth; sync to N5 (exclude target, node_modules, root-owned gen/schemas)
rsync -a --delete --exclude target --exclude node_modules \
  --exclude 'src-tauri/gen/schemas' ~/Projects/Handy/ jared@10.10.0.114:~/handy-build/

# Build + test inside rust:trixie (newer Vulkan headers than bookworm; see blockers)
ssh jared@10.10.0.114 'docker run --rm \
  -v ~/handy-build:/work \
  -v handy-cargo-registry:/usr/local/cargo/registry \
  -v handy-cargo-target-trixie:/work/src-tauri/target \
  -w /work/src-tauri rust:trixie bash -c "
    git config --global --add safe.directory /work
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
      cmake pkg-config libssl-dev libasound2-dev \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
      libjavascriptcoregtk-4.1-dev librsvg2-dev clang libgtk-layer-shell-dev \
      glslc glslang-tools libvulkan-dev libclang-dev >/dev/null
    export LIBCLANG_PATH=/usr/lib/llvm-19/lib
    cargo build && cargo test --lib
  "'
```

## Blockers encountered (and how they were resolved)

All blockers were **build-environment** issues in vendored C/C++ deps, none in the Rust feature code:

1. **macOS local build:** `cmake` not installed → `whisper-rs-sys` (whisper.cpp) can't configure. Redirected all compilation to N5 per instruction (the Air is fanless).
2. **N5 sudo wall:** N5 `sudo` needs a password (non-interactive apt blocked). Resolved by building inside a Docker container (`docker` works without sudo on N5), installing all system deps as root inside the container.
3. **Debian bookworm Vulkan headers too old:** the vendored whisper.cpp Vulkan backend (`transcribe-rs` enables `whisper-vulkan` on Linux, Cargo.toml line ~108) failed to compile against bookworm's Vulkan-Headers (`vk::LayerSettingEXT` missing). Resolved by switching the build image to `rust:trixie` (Debian 13, newer Vulkan-Headers). This is a **Linux-only GPU-backend toolchain detail**, unrelated to the feature.
4. **System libs:** needed `libgtk-layer-shell-dev` (Linux Wayland overlay glue, Cargo.toml line ~106) and, for `cargo test`'s bindgen path, `libclang-dev` with `LIBCLANG_PATH=/usr/lib/llvm-19/lib`. Both are Linux-build-env only.

None of these are blockers for the **macOS** build (where whisper uses Metal, not Vulkan, and gtk-layer-shell/alsa don't apply). They were purely artifacts of validating on Linux.

## TODOs / follow-ups (not blocking)

- **Frontend toggle** in `src/` (React/TS) wired to `start_hands_free`/`stop_hands_free`/`toggle_hands_free_pause` + the new settings. (Optional per spec.)
- **Tray menu pause item** — the spec mentioned a tray item; the command exists (`toggle_hands_free_pause`) but a tray menu entry was not added. Wire it in `tray.rs` if desired.
- **Persisted "source" column** (ambient vs command) on `transcription_history` if you want to filter the history UI by source — currently it's a debug-log-only distinction to avoid a schema migration in V0.
- **macOS `.app` build** on the Air: `bun install` then the Tauri build script (separate from this backend-compile validation).
- **VAD thresholds** (`SILENCE_FRAMES_TO_FINALIZE`, `MIN_UTTERANCE_SAMPLES`) are sensible defaults; tune against real speech if over/under-segmentation shows up.
