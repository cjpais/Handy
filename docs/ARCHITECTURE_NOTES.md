# Architecture notes (Phase 0)

Recon of the Handy fork (v0.9.8) before any Voxa change. Voxa-specific code goes
into separate modules (`src-tauri/src/voxa/`, `src/voxa/`) and touches upstream
files only at the points listed in "Touch points".

## Stack

- Tauri 2.11 shell, Rust backend in `src-tauri/`, React 18 + TypeScript + Tailwind 4
  frontend in `src/`, bun as package manager.
- Speech recognition: `transcribe-cpp` 0.4 (GGUF/ggml). On macOS it is linked
  statically with the `metal` feature. The model runs in a **separate worker
  process** (`engine_supervisor/`), the main process only talks to it over a pipe.
- Recording: `handy-recorder`; VAD: Silero v6.2 GGUF (`src-tauri/resources/models/`,
  committed in the repo) plus `earshot`.
- Persistence: settings in `tauri-plugin-store` (`settings_store.json`), history in
  SQLite (`history.db`, `rusqlite` + `rusqlite_migration`), WAVs in `recordings/`.
- Frontend IPC: `tauri-specta`; `src/bindings.ts` is regenerated on every debug
  build of the app (`lib.rs`, `specta_builder.export`).

## Dictation pipeline

```
shortcut (shortcut/, transcription_coordinator.rs)
  -> actions.rs  TranscribeAction::start   recording + overlay
  -> actions.rs  TranscribeAction::stop    (async task)
       rm.stop_recording()                 samples (16 kHz mono f32)
       tm.transcribe_recording(samples)    managers/transcription.rs
            finalize_live / decode_batch -> transcribe_cpp() -> engine worker
            finish_text -> post_process_transcription_text
                 chinese script, apply_custom_words (fuzzy, ASCII only),
                 filler removal, normalize
       process_transcription_output()      optional LLM post-processing
       hm.save_entry()                     history row (+ WAV already saved)
       utils::paste()                      clipboard.rs / input.rs, restores clipboard
       hide_recording_overlay()
```

Findings that matter for Voxa:

- **Custom words** reach the model as `RunOptions.vocabulary`
  (`vocabulary_for_model`). Whisper supports it (`Glossary: a, b, c` in the
  `<|startofprev|>` slot). When the vocabulary was sent, fuzzy correction
  (`apply_custom_words`) is skipped (`custom_words_already_prompted`). The fuzzy
  matcher only handles ASCII keys, so it can never turn Cyrillic "питон" into
  "Python".
- **`initial_prompt`**: Whisper advertises `Feature::InitialPrompt` and
  `Feature::ContextPrompt`. `RunOptions.prompt` (context prompt) is rendered after
  the glossary in the same slot and may be combined with `vocabulary`. The Whisper
  extension `initial_prompt` can NOT be combined with them. Handy does not set
  any prompt today. Voxa passes the editable phrase through `RunOptions.prompt`.
- `transcribe_recording()` returns only the final string. Raw text (before custom
  words), the model id, language and timings are not exposed.
- Cleanup of history (`HistoryManager::cleanup_old_entries`) deletes rows **and**
  WAVs by count (`history_limit`, default 5) or age.

## Overlay

- `src-tauri/src/overlay.rs`: on macOS an `NSPanel` (`tauri-nspanel`, non-activating,
  transparent, level Status) labelled `recording_overlay`, URL `src/overlay/index.html`;
  on Windows/Linux a regular transparent always-on-top window (GTK layer shell on
  Wayland). Window sizes `OVERLAY_WIDTH/HEIGHT` (256x50) and stream size (400x120).
- Events to the overlay window: `show-overlay` (payload `recording | streaming |
transcribing | processing`), `hide-overlay`, `recording-ready`, `mic-level`
  (16 FFT buckets, throttled to ~30 Hz by `emit_levels`), `model-state-changed`,
  stream text/phase events.
- UI: `src/overlay/RecordingOverlay.tsx` + `RecordingOverlay.css` (flat pill, "Live"
  panel with streaming text).
- Hide is delayed by 300 ms (`hide_recording_overlay`) and guarded by a generation
  counter.
- Esc is registered as a global cancel shortcut while recording
  (`shortcut::register_cancel_shortcut`).

## History DB

`managers/history.rs`, table `transcription_history` (id, file_name, timestamp,
saved, title, transcription_text, post_processed_text, post_process_prompt,
post_process_requested), 4 migrations tracked through `PRAGMA user_version`.
Upstream may append migrations at any time, so Voxa does **not** append to this
list (see DECISIONS.md, D-05).

## Settings

`settings.rs` `AppSettings` (serde, `#[serde(default)]` per field, salvage on parse
errors, one-time migrations in `apply_settings_migrations`). Relevant fields:
`selected_model`, `selected_language`, `custom_words`, `model_unload_timeout`
(default Min5), `clipboard_handling`, `overlay_style/position`,
`update_checks_enabled`, `history_limit`, `recording_retention_period`.
Fresh install: `selected_model = ""`, `onboarding_completed = false`.

## Models

`catalog/catalog.json` (generated, compiled in) lists 69 GGUF models of the
`handy-computer` HF org; downloads go through hf-hub with a `blob.handy.computer`
mirror fallback and are started by an explicit click only. Whisper large-v3-turbo is
already in the catalog with quants Q4_K_M / Q5_K_M / Q6_K / **Q8_0 (886 MB, default)** /
F16 (1.6 GB). GigaAM v3 (ru) variants exist as well. The `.bin` format from
whisper.cpp named in the plan is not loadable by transcribe-cpp.

## Updater and network

- `tauri-plugin-updater` + `UpdateChecker.tsx`; endpoint in `tauri.conf.json`
  points at `github.com/cjpais/Handy/releases/latest/download/latest.json`;
  `createUpdaterArtifacts: true` makes `tauri build` demand a signing key.
  `settings::update_checks_forced_disabled()` already exists (env
  `HANDY_DISABLE_UPDATER`).
- No telemetry / analytics SDKs found (grep for sentry, posthog, mixpanel,
  analytics, telemetry). Network use: model downloads (user action), optional
  post-processing LLM calls (off by default), updater, "About" links opened in the
  browser on click.

## CI

Upstream workflows: `build.yml` (reusable, tauri-action, needs Apple signing
secrets), `test.yml` (Rust tests on ubuntu-24.04), `code-quality.yml` (bun lint,
translations, prettier), release/nix/playwright. None builds an ad-hoc signed
macOS arm64 zip, so Voxa adds `voxa-macos.yml`.

GitHub API access from the session works through the GitHub MCP server
(`actions_list`, `actions_run_trigger`, `get_job_logs`,
`actions_get download_workflow_run_artifact`) and `gh`. The repo is public, so
Actions minutes are free.

## Local verification possible in the cloud sandbox (Linux)

- Rust: `apt` dev libs installed, `cargo test` runs the whole crate on Linux
  (Vulkan backend). macOS-only code (`cfg(target_os = "macos")`) is only compiled
  by CI.
- Frontend: `bun run lint`, `tsc`, `vite`, Playwright + preinstalled Chromium.
- HuggingFace is blocked by the sandbox proxy, so no model downloads / real
  transcription locally.

## Touch points (upstream files Voxa edits)

| File                                                | Change                                                        |
| --------------------------------------------------- | ------------------------------------------------------------- |
| `src-tauri/tauri.conf.json`                         | productName, identifier, window title, updater off, vibrancy  |
| `src-tauri/Cargo.toml`                              | package name/description/authors only                         |
| `src-tauri/src/lib.rs`                              | `mod voxa`, command registration, window title, setup hook    |
| `src-tauri/src/settings.rs`                         | `voxa` settings sub-struct, Voxa defaults, updater forced off |
| `src-tauri/src/actions.rs`                          | collect timings, record dictation stats, done/error overlay   |
| `src-tauri/src/managers/transcription.rs`           | detailed result (raw text, replacements), prompt              |
| `src-tauri/src/audio_toolkit/text.rs`               | report variant of `apply_custom_words`                        |
| `src-tauri/src/overlay.rs`                          | done/error states, vibrancy, capsule geometry                 |
| `src/overlay/RecordingOverlay.tsx/.css`             | glass pill + new states                                       |
| `src/components/Sidebar.tsx`, `App.tsx`, `main.tsx` | Statistics section, preview route                             |
| `src/styles/theme.css`, `src/App.css`               | glass tokens                                                  |
| `src-tauri/src/catalog/catalog.json`                | turbo Q8_0 flagged recommended                                |

## Iteration 2 additions

| Area                       | Where                                                                                                   |
| -------------------------- | ------------------------------------------------------------------------------------------------------- |
| In-app updates (backend)   | `src-tauri/src/voxa/updates.rs` (state, background check, install), commands in `voxa/commands.rs`      |
| In-app updates (frontend)  | `src/voxa/updates/` (store, About card, footer link), sidebar badge, tray item in `tray.rs`             |
| Release pipeline           | `.github/workflows/voxa-release.yml`, `scripts/voxa/release_manifest.py`, `make-signing-cert.sh`        |
| Word statistics (backend)  | `src-tauri/src/voxa/words/` (`analyze`, `stopwords`, `index`, `query`), migration 2 in `voxa/store.rs`  |
| Word statistics (frontend) | `src/voxa/words/` (tab, blocks, trend line, word panel), tabs in `src/voxa/stats/StatsScreen.tsx`       |
| Upstream touch points      | `settings.rs` (updater flag, one migration), `tray.rs` (menu item), `managers/history.rs` (delete hook) |
