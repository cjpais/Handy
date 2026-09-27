# `src-tauri/src/` — backend source

All Rust source for the backend. If you read only one thing, read the "dictation
spine" below — it is the app's load-bearing control flow.

Line-level detail, including the full state machine and every timing constant,
lives in [`context/references/codebase-v1.md`](../../context/references/codebase-v1.md).
This file is the map; that one is the reference.

---

## The dictation spine

Every input source converges on one channel consumed by one thread. This is the
central design decision in the codebase and the reason the lifecycle has no races.

```
keyboard / signal / CLI flag
  → shortcut/handler.rs        routing only; transcribe bindings → coordinator
  → transcription_coordinator.rs   pure state machine → Effect::Start | Stop
  → actions.rs                 ACTION_MAP[binding].start() / .stop()
  → managers/audio.rs          try_start_recording / stop_recording
  → managers/transcription.rs  finalize_stream()  OR  transcribe(samples)
  → actions.rs                 optional post-processing
  → utils::paste → clipboard.rs    paste into the focused app
  → FinishGuard drop           model unload, coordinator notify, memory trim
```

`TranscriptionCoordinator` runs a dedicated thread; `CoordinatorState` holds all
decisions and is **pure** (no `AppHandle`), which is why the tests can drive exact
production transitions. All three activation modes — push-to-talk, toggle,
hold-or-toggle — run through one machine; only how a recording *ends* differs.

**Do not add a second path into recording.** Route new triggers through the
coordinator so debounce, release-grace, busy-pipeline parity and cancellation stay
in one place.

---

## Files

| File | Responsibility |
| --- | --- |
| `main.rs` | 29 lines. Windows Vulkan-layer guard, Linux DMABUF guard, then `run(cli_args)` |
| `lib.rs` | Module list, specta command/event registration, plugin setup, `initialize_core_logic()`, tray, window creation, headless mode |
| `transcription_coordinator.rs` | Lifecycle state machine + serialising executor thread |
| `actions.rs` | `ShortcutAction` trait, `ACTION_MAP`, `TranscribeAction` pipeline |
| `settings.rs` | `AppSettings` (~55 fields), all setting enums, store persistence |
| `cli.rs` | clap `CliArgs`, including headless flags |
| `signal_handle.rs` | Unix signal handlers; `send_transcription_input()` shared with CLI |
| `portable.rs` | Portable-data detection via a `portable` marker file |
| `utils.rs` | Cancellation, overlay show/hide helpers, `redact_text`, platform probes |
| `memory.rs` | glibc allocator tuning; `trim_freed_memory()` after a pipeline run |
| `llm_client.rs` | Post-processing HTTP client — **non-streaming** |
| `clipboard.rs` | `paste()` dispatcher and clipboard handling |
| `input.rs` | Enigo keyboard/mouse simulation |
| `audio_feedback.rs` | rodio chimes for start/stop/test |
| `overlay.rs` | Recording overlay window (Windows/Linux) and NSPanel (macOS) |
| `tray.rs`, `tray_i18n.rs` | Tray icon/menu with desired-state diffing; menu localisation |
| `autostart.rs` | Autostart registration |
| `secure_input.rs` | macOS Secure Input monitoring and Carbon shadow hotkeys |
| `apple_intelligence.rs` | macOS aarch64 only |

### Subdirectories

| Directory | Contents |
| --- | --- |
| `managers/` | `audio.rs` (mic lifecycle), `transcription.rs` (STT engines + streaming), `model.rs` (catalog/downloads), `history.rs` (SQLite), `gguf_meta.rs`, `model_capabilities.rs` |
| `audio_toolkit/` | `audio/` (recorder, resampler, device, visualizer), `vad/` (Silero, Earshot, smoothing), `text.rs`, `lang_id.rs`, `constants.rs` |
| `commands/` | Tauri command handlers: `audio`, `history`, `models`, `transcription`, `mod` |
| `catalog/` | `catalog.json` (69 models) embedded via `include_str!`, plus mirror/rank helpers |
| `shortcut/` | `mod.rs` dispatcher, `handler.rs` shared event logic, `tauri_impl.rs`, `handy_keys.rs` |
| `paste_tx/` | Receipt-sequenced "reliable paste": `mod.rs`, `windows.rs`, `macos.rs` |
| `helpers/` | `clamshell.rs` — laptop lid detection |

---

## Conventions

- **Managers are registered as `Arc<T>` in Tauri state** by
  `initialize_core_logic()` in `lib.rs`. Commands take
  `State<'_, Arc<Manager>>`. (One command, `get_model_load_status`, takes
  `State<TranscriptionManager>` — a harmless inconsistency, not a pattern to copy.)
- **Settings commands live in `shortcut/mod.rs`.** This is historical: that module
  owns the `change_*_setting` family, including settings unrelated to shortcuts.
  Add new setting commands there and register them in `lib.rs`.
- **Errors:** return `Result<T, String>` from commands. Handle errors explicitly;
  avoid `unwrap()` in production paths. `FinishGuard` (in `actions.rs`) is the RAII
  pattern for guaranteed cleanup on every exit path.
- **Cancellation** uses monotonic generation counters
  (`was_cancelled_since(generation)`), checked at every stage boundary. Add a check
  at any new await point that can outlive a user cancel.
- **Redact transcripts in logs.** `utils::redact_text()` exists for this; webview
  log streaming is gated behind debug mode precisely because logs can carry file
  paths and transcribed text.
- **Frame contracts are not interchangeable.** Silero requires exactly 480 samples
  (30 ms at 16 kHz); Earshot uses 256. Read the selected backend's
  `frame_samples()` rather than assuming a size.
- **Streaming is opt-in per model.** Capability comes from the catalog and a GGUF
  header probe. Treat unknown support as unsupported.

---

## Traps

- **`llm_client.rs` cannot stream.** `stream: false` is hardcoded, and a unit test
  asserts it. Conversation needs a separate streaming adapter — do not try to
  parameterise this one without reading why it is fixed.
- **Capability is reconciled at runtime**, not just read from the catalog:
  `set_runtime_capabilities()` can overwrite catalog values after a model loads.
- `signal_handle.rs` deliberately does **not** handle `SIGUSR1` on Linux — it
  belongs to WebKitGTK's garbage collector.
- The `cancel` shortcut is registered **only while recording**; a stray cancel
  keypress when idle does nothing by design.
- macOS Secure Input can degrade hotkeys (modifier groups widen) or kill them
  while a password manager holds secure input. `secure_input.rs` shadow-registers
  Carbon equivalents and reports impact — global shortcut reliability is
  **conditional**, not guaranteed.
- Settings reads can **write**: `get_settings()` performs salvage-on-parse-failure,
  migrations and binding back-fill. Adding a field is backward-compatible by
  construction; don't "optimise" it into a pure read.
- `AGENTS.md`'s description of this directory is stale — it omits
  `transcription_coordinator.rs`, `actions.rs`, `catalog/`, `paste_tx/`,
  `secure_input.rs`, `input.rs`, `memory.rs` and `portable.rs`.
- `../transcribe-libs/` is gitignored and generated by the build, not committed.
  An empty `transcribe-libs/` in a fresh clone is expected.
