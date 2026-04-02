# Technology Stack

**Analysis Date:** 2026-03-28

## Languages

**Primary:**
- TypeScript 5.6.3 - Frontend UI (`src/`)
- Rust 1.x (edition 2021) - Backend/core (`src-tauri/src/`)

**Secondary:**
- CSS (Tailwind) - Styling
- HTML - Tauri webview shell

## Runtime

**Environment:**
- Tauri 2 webview (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux)
- Rust stdlib + Tokio 1.43.0 async runtime (backend)

**Package Manager:**
- Bun (frontend) - replaces npm/yarn
- Cargo (Rust) - `src-tauri/Cargo.toml`
- Lockfile: `bun.lockb` (binary lockfile), `Cargo.lock`

## Frameworks

**Core:**
- Tauri 2.10.2 - Desktop app shell (`src-tauri/Cargo.toml`)
- React 18.3.1 - UI framework (`package.json`)

**Build/Dev:**
- Vite 6.4.1 - Frontend bundler and dev server (`package.json`)
- `@tauri-apps/cli` 2.10.0 - Tauri CLI (`package.json`)
- `tauri-build` 2.x - Rust build script (`src-tauri/Cargo.toml` build-dependencies)

**Testing:**
- Playwright 1.58.0 - E2E testing (`package.json` devDependencies)

## Key Dependencies

**Frontend:**
- Zustand 5.0.8 - State management (`package.json`)
- Tailwind CSS 4.1.16 - Utility-first CSS (`package.json`)
- i18next 25.7.2 + react-i18next 16.4.1 - Internationalization
- Zod 3.25.76 - Runtime schema validation
- Lucide React 0.542.0 - Icon library
- react-select 5.8.0 - Select/dropdown UI component
- sonner 2.0.7 - Toast notifications
- immer 11.1.3 - Immutable state helpers

**Backend (Rust) - Core:**
- `transcribe-rs` 0.3.2 - Speech-to-text (Whisper/Parakeet); features vary per platform:
  - macOS: `whisper-metal`
  - Windows: `whisper-vulkan` + `ort-directml`
  - Linux: `whisper-vulkan`
  - Cross-platform: `whisper-cpp` + `onnx`
- `cpal` 0.16.0 - Cross-platform audio I/O
- `rubato` 0.16.2 - Audio resampling
- `vad-rs` (git: `cjpais/vad-rs`) - Voice Activity Detection (Silero VAD, ONNX)
- `tokio` 1.43.0 - Async runtime
- `reqwest` 0.12 - HTTP client (model downloads, streaming)
- `enigo` 0.6.1 - Keyboard/mouse simulation (auto-paste)
- `rdev` (git: `rustdesk-org/rdev`) - Global input events
- `rodio` (git: `cjpais/rodio`) - Audio playback
- `rusqlite` 0.37 (bundled) - SQLite database
- `rusqlite_migration` 2.3 - DB migrations
- `clap` 4.x - CLI argument parsing
- `tauri-specta` 2.0.0-rc.21 + `specta` 2.0.0-rc.22 - Type binding codegen (`src/bindings.ts`)

**Backend (Rust) - Platform-specific:**
- `windows` 0.61.3 - Win32 audio/UI APIs (Windows only)
- `winreg` 0.55 - Windows registry (Windows only)
- `tauri-nspanel` (git: `ahkohd/tauri-nspanel`) - macOS NSPanel overlay (macOS only)
- `gtk-layer-shell` 0.8 + `gtk` 0.18 - Wayland layer shell (Linux only)
- `signal-hook` 0.3 - Unix signal handling (Unix only)

**Backend (Rust) - Utilities:**
- `serde` 1.x + `serde_json` 1.x - Serialization
- `anyhow` 1.0.95 - Error handling
- `chrono` 0.4 - Date/time
- `regex` 1.x - Text processing
- `sha2` 0.10 - Hashing (model integrity)
- `tar` 0.4.44 + `flate2` 1.0 - Archive extraction (model bundles)
- `strsim` 0.11.0 + `natural` 0.5.0 - String similarity / NLP
- `rustfft` 6.4.0 - FFT (audio processing)
- `ferrous-opencc` 0.2.3 - Chinese text conversion
- `handy-keys` 0.2.4 - Custom keybinding utility
- `once_cell` 1.x - Lazy statics

## Configuration

**TypeScript:**
- `tsconfig.json` - ES2020 target, strict mode, bundler resolution
- Path alias: `@/` → `./src/`

**Frontend Build:**
- `vite.config.*` - Vite config
- `eslint.config.*` - ESLint 9.x
- `.prettierrc` - Prettier 3.6.2

**Tauri:**
- `src-tauri/tauri.conf.json` - App identity (`com.pais.handy`), bundle config, plugin config
- `src-tauri/Entitlements.plist` - macOS entitlements

**Rust Patches:**
- `tauri-runtime`, `tauri-runtime-wry`, `tauri-utils` patched from `cjpais/tauri` fork (`handy-2.10.2` branch) — `src-tauri/Cargo.toml` `[patch.crates-io]`

## Platform Requirements

**Development:**
- Rust (latest stable) + Cargo
- Bun (latest)
- ONNX model file: `src-tauri/resources/models/silero_vad_v4.onnx`
- Linux: `libgtk-layer-shell-dev` + GTK dev libs

**Production Targets:**
- macOS 10.15+ (Catalina minimum), code-signed, hardened runtime
- Windows (NSIS installer, Azure Trusted Signing)
- Linux (deb/rpm/AppImage; requires `libgtk-layer-shell0`)

---

*Stack analysis: 2026-03-28*
