# External Integrations

**Analysis Date:** 2026-03-28

## APIs & External Services

**Auto-Update:**
- GitHub Releases — app update distribution
  - Endpoint: `https://github.com/cjpais/Handy/releases/latest/download/latest.json`
  - SDK: `tauri-plugin-updater` 2.10.0 (Rust) + `@tauri-apps/plugin-updater` 2.10.0 (JS)
  - Auth: minisign public key in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`)

**Model Hosting:**
- Custom blob server — binary model file delivery
  - URL: `https://blob.handy.computer/silero_vad_v4.onnx`
  - Client: `reqwest` 0.12 with streaming support (`src-tauri/src/managers/model.rs`)
  - Whisper/Parakeet models are downloaded on demand at runtime

**Code Signing:**
- Azure Trusted Signing (Windows) — binary signing via `trusted-signing-cli`
  - Endpoint: `https://eus.codesigning.azure.net/`
  - Account: `CJ-Signing`, certificate: `cjpais-dev`
  - Configured in `src-tauri/tauri.conf.json` (`bundle.windows.signCommand`)

## Data Storage

**Databases:**
- SQLite (bundled via `rusqlite` 0.37) — transcription history
  - No external server; file stored in Tauri app data directory
  - Client: `rusqlite` + `rusqlite_migration` 2.3 for schema migrations
  - History manager: `src-tauri/src/managers/history.rs`

**Key-Value Store:**
- `tauri-plugin-store` 2.4.1 — persisted app settings (JSON file in app data dir)
  - Frontend: `@tauri-apps/plugin-store` 2.4.1
  - Settings manager: `src-tauri/src/settings.rs`

**File Storage:**
- Local filesystem only (no cloud storage)
  - Model files: `src-tauri/resources/models/`
  - Accessed via `tauri-plugin-fs` 2.4.4 + `@tauri-apps/plugin-fs`

## Authentication & Identity

**Auth Provider:** None — local app, no user accounts

## ML / AI Inference

**Speech-to-Text:**
- `transcribe-rs` 0.3.2 — local inference engine wrapping:
  - whisper.cpp (CPU/Metal/Vulkan backends depending on platform)
  - ONNX Runtime (cross-platform; DirectML on Windows)
- Models run entirely on-device; no cloud inference calls

**Voice Activity Detection:**
- Silero VAD v4 via `vad-rs` (ONNX model: `silero_vad_v4.onnx`)
  - Runs locally via ONNX Runtime embedded in `vad-rs`
  - Model file bundled at `src-tauri/resources/models/silero_vad_v4.onnx`

## System Integrations

**Audio:**
- `cpal` 0.16.0 — cross-platform audio device enumeration and recording
- `rodio` (forked) — audio playback
- `rubato` 0.16.2 — audio resampling pipeline
- Windows: `windows` 0.61.3 Win32 audio endpoint APIs (`Win32_Media_Audio_Endpoints`)

**Input Simulation:**
- `enigo` 0.6.1 — keyboard/mouse automation (auto-paste transcription output)
- `rdev` (rustdesk-org fork) — global keyboard/mouse event listening

**Global Shortcuts:**
- `tauri-plugin-global-shortcut` 2.3.1 — OS-level hotkey registration
  - Frontend: `@tauri-apps/plugin-global-shortcut` 2.3.1

**Clipboard:**
- `tauri-plugin-clipboard-manager` 2.3.2 — read/write system clipboard
  - Frontend: `@tauri-apps/plugin-clipboard-manager` 2.3.2

**OS / Platform:**
- `tauri-plugin-os` 2.3.2 — OS detection (platform-specific UI/features)
- `tauri-plugin-autostart` 2.5.1 — launch at login
- `tauri-plugin-process` 2.3.1 — app restart/exit
- `tauri-plugin-opener` 2.5.2 — open URLs/files in default OS handler
- `tauri-plugin-dialog` 2.6 — native file/folder picker dialogs
- `tauri-plugin-single-instance` 2.3.2 — IPC between app instances (CLI remote control)

**macOS-specific:**
- `tauri-nspanel` (ahkohd fork) — NSPanel overlay window (recording HUD)
- `tauri-plugin-macos-permissions` 2.3.0 — microphone/accessibility permission requests
  - Frontend: `tauri-plugin-macos-permissions-api` 2.3.0

**Linux-specific:**
- `gtk-layer-shell` 0.8 — Wayland layer shell (overlay window)
- `gtk` 0.18 — GTK bindings for layer shell integration
- System dep: `libgtk-layer-shell0` (required at runtime)

**Windows-specific:**
- `winreg` 0.55 — Windows registry access
- Win32 APIs: `Win32_UI_WindowsAndMessaging`, `Win32_Foundation`, `Win32_System_Com_StructuredStorage`

**Unix signals:**
- `signal-hook` 0.3 — handle SIGTERM/SIGINT for graceful shutdown (`src-tauri/src/signal_handle.rs`)

## Monitoring & Observability

**Logging:**
- `tauri-plugin-log` 2.7.1 — structured logging to file + console
- `log` 0.4.25 + `env_filter` 0.1.0 — Rust log facade with env-based filtering
- Debug mode enables Trace-level logging (via `--debug` CLI flag)

**Error Tracking:** None (no external service)

## CI/CD & Deployment

**Hosting:** GitHub Releases (binary distribution)
**Update check:** GitHub Releases JSON endpoint (see Auto-Update above)
**CI Pipeline:** Not detected in codebase (no `.github/workflows/` examined)
**Windows signing:** Azure Trusted Signing via CLI tool

## Type Binding Code Generation

**tauri-specta** 2.0.0-rc.21 — generates `src/bindings.ts` at build time from Rust command/event types. This file is the contract between frontend and backend. Do not edit manually.

## Environment Configuration

**Required for development:**
- No external service credentials needed (all inference is local)
- Model file must be manually downloaded: `src-tauri/resources/models/silero_vad_v4.onnx`
  - Source: `https://blob.handy.computer/silero_vad_v4.onnx`

**Production secrets (not in repo):**
- Azure Trusted Signing credentials (Windows build pipeline only)
- minisign private key (for signing update manifests)

## Webhooks & Callbacks

**Incoming:** None
**Outgoing:** None (no server-side component; fully local app)

---

*Integration audit: 2026-03-28*
