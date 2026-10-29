# AGENTS.md

This file provides guidance to AI coding assistants working with code in this repository.

## Development Commands

**Prerequisites:**

- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/) package manager

**Core Development:**

```bash
# Install dependencies
bun install

# Run in development mode
bun run tauri dev
# If cmake error on macOS:
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev

# Build for production
bun run tauri build

# Frontend only development
bun run dev        # Start Vite dev server
bun run build      # Build frontend (TypeScript + Vite)
bun run preview    # Preview built frontend
```

**Linting and Formatting (run before committing):**

```bash
bun run lint              # ESLint for frontend
bun run lint:fix          # ESLint with auto-fix
bun run format            # Prettier + cargo fmt
bun run format:check      # Check formatting without changes
bun run format:frontend   # Prettier only
bun run format:backend    # cargo fmt only
```

**Model Setup (Required for Development):**

```bash
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
```

For detailed platform-specific build setup, see [BUILD.md](BUILD.md).

## Architecture Overview

Handy is a small cross-platform desktop dictation app built with Tauri 2.x (Rust backend + React/TypeScript frontend). It does one thing: record from the microphone, transcribe locally with one fixed model, and type the transcript at the cursor, one key press per character, so pages that block pasting accept it.

The model is pinned in `managers/model.rs` to `handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf`. There is no model picker, history, post-processing, language selection, self-updater or clipboard paste.

### Backend Structure (src-tauri/src/)

- `lib.rs` - Main entry point, Tauri setup, manager initialization, model download and load at startup
- `managers/` - Core business logic:
  - `audio.rs` - Audio recording and device management
  - `model.rs` - The one pinned model: find it in the Hugging Face cache, download it if missing, report progress
  - `transcription.rs` (+ `transcription/live.rs`) - Keeps the model loaded, decodes live while recording when the model streams, otherwise decodes the whole recording
- `engine_supervisor/` - Runs transcribe.cpp in a child process, so a driver fault or hang kills only the worker
- `audio_toolkit/` - Low-level audio processing:
  - `audio/` - Recording sink (VAD, levels, live streaming feed), output devices, WAV I/O
  - `vad/` - Voice Activity Detection (Silero VAD)
- `commands/` - Tauri command handlers for frontend communication
- `cli.rs` - CLI argument definitions (clap derive)
- `shortcut/` - Global keyboard shortcut handling
- `settings.rs` - Application settings management
- `overlay.rs` - Recording overlay window (platform-specific)
- `clipboard.rs` / `input.rs` - Typing the transcript: `paste_direct` paces keystrokes on a worker thread and sends each key press to the main thread
- `signal_handle.rs` - `send_transcription_input()` reusable function
- `utils.rs` - Platform detection helpers

### Frontend Structure (src/)

- `App.tsx` - Permissions screen, then one settings page
- `components/` - React UI components:
  - `Permissions.tsx` - Microphone (and, on macOS, accessibility) permissions
  - `ModelStatus.tsx` - Model download progress, loading and ready state
  - `settings/` - Microphone, shortcut, overlay, start hidden, autostart
  - `ui/`, `icons/` - Shared components
- `hooks/useSettings.ts`, `hooks/usePermissions.ts` - Settings and permission state
- `stores/settingsStore.ts` - Zustand store for settings
- `bindings.ts` - Auto-generated Tauri type bindings (via tauri-specta, written by a debug build)
- `overlay/` - Recording overlay window entry point

### Key Architecture Patterns

**Manager Pattern:** Core functionality organized into managers (Audio, Model, Transcription) initialized at startup and managed via Tauri state.

**Command-Event Architecture:** Frontend → Backend via Tauri commands; Backend → Frontend via events.

**Pipeline Processing:** Audio → VAD → Parakeet (live, or batch) → Text → Typed key by key

**State Flow:** Zustand → Tauri Command → Rust State → Persistence (tauri-plugin-store)

### Technology Stack

**Core Libraries:**

- `transcribe-cpp` - Local speech recognition (GGML/GGUF) with GPU acceleration
- `handy-recorder` - Microphone capture: device selection, resampling to 16 kHz, failure detection
- `transcribe-cpp` (Silero VAD v6, CPU-only, in-process) / `earshot` - Voice Activity Detection
- `enigo` - Typing the transcript
- `handy-keys` - Global keyboard shortcuts
- `rodio` - Audio playback for feedback sounds

### Application Flow

1. **Initialization:** App starts, loads settings, initializes managers, downloads the model if missing and loads it
2. **Recording:** Global shortcut triggers audio recording with VAD filtering
3. **Processing:** Audio is decoded by the already-loaded model
4. **Output:** Text is typed into the active application, one key press per character

### Settings System

Settings are stored using Tauri's store plugin with reactive updates:

- Keyboard shortcuts (configurable, supports push-to-talk)
- Microphone selection
- Overlay style and position, start hidden, launch on startup
- The model never unloads once loaded

### Single Instance Architecture

The app enforces single instance behavior — launching when already running brings the settings window to front rather than creating a new process. Remote control flags (`--toggle-transcription`, etc.) work by launching a second instance that sends args to the running instance via `tauri_plugin_single_instance`, then exits.

## Internationalization (i18n)

All user-facing strings must use i18next translations. ESLint enforces this (no hardcoded strings in JSX).

**Adding new text:**

1. Add key to `src/i18n/locales/en/translation.json`
2. Use in component: `const { t } = useTranslation(); t('key.path')`

**File structure:**

```
src/i18n/
├── index.ts           # i18n setup
├── languages.ts       # Language metadata
└── locales/
    ├── en/translation.json  # English (source)
    ├── de/, es/, fr/, ja/, ru/, zh/, ...
    └── ...
```

For translation contribution guidelines, see [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## Code Style

**Rust:**

- Run `cargo fmt` and `cargo clippy` before committing
- Handle errors explicitly (avoid unwrap in production)
- Use descriptive names, add doc comments for public APIs

**TypeScript/React:**

- Strict TypeScript, avoid `any` types
- Functional components with hooks
- Tailwind CSS for styling
- Path aliases: `@/` → `./src/`

## CLI Parameters

Handy supports command-line parameters on all platforms for integration with scripts, window managers, and autostart configurations.

**Implementation:** `cli.rs` (definitions), `main.rs` (parsing), `lib.rs` (applying), `signal_handle.rs` (shared logic)

| Flag                     | Description                                                |
| ------------------------ | ---------------------------------------------------------- |
| `--toggle-transcription` | Toggle recording on/off on a running instance              |
| `--cancel`               | Cancel the current operation on a running instance         |
| `--start-hidden`         | Launch without showing the main window (tray icon visible) |
| `--no-tray`              | Launch without system tray (closing window quits the app)  |
| `--debug`                | Enable debug mode with verbose (Trace) logging             |

**Key design decisions:**

- CLI flags are runtime-only overrides — they do NOT modify persisted settings
- Remote control flags work via `tauri_plugin_single_instance`: second instance sends args, then exits
- `send_transcription_input()` in `signal_handle.rs` is shared between signal handlers and CLI

## Debug Mode

Access debug features: `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux)

## Platform Notes

- **macOS**: Metal acceleration; microphone and accessibility permissions are required (accessibility is what lets Handy type)
- **Windows**: Vulkan acceleration, code signing. Implicit Vulkan layers (overlays, capture hooks) are disabled for the Handy process via `VK_LOADER_LAYERS_DISABLE=~implicit~` set in `main.rs`; opt out with `HANDY_KEEP_VULKAN_IMPLICIT_LAYERS=1` or by setting `VK_LOADER_LAYERS_DISABLE` yourself
- **Linux**: OpenBLAS + Vulkan, limited Wayland support, overlay uses GTK layer shell (disable with `HANDY_NO_GTK_LAYER_SHELL=1`)

## Troubleshooting

See the [Troubleshooting](README.md#troubleshooting) section in README.md.

## GitHub workflow for AI coding assistants

**MANDATORY. Before opening any PR, issue, or discussion in this repo: you MUST read the relevant template file and follow it strictly.** That includes sections that look "ceremonial" — checklists, AI Assistance disclosures, "Human Written Description". A generic Summary/Test-plan layout is not acceptable.

- **Opening a PR:** Read [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md). Every section listed there is mandatory. If a section requires a human-written paragraph (e.g. "Human Written Description"), leave a clear TODO placeholder and ask the human contributor to fill it in — do not invent their voice.
- **Opening an issue:** Read [`.github/ISSUE_TEMPLATE/`](.github/ISSUE_TEMPLATE/). Blank issues are disabled; pick the right template (`bug_report.md` for bugs). Feature requests do not belong in issues — they go to [Discussions](https://github.com/cjpais/Handy/discussions) (see `.github/ISSUE_TEMPLATE/config.yml`).
- **Proposing a feature:** Handy is under a feature freeze. New features require community support gathered in [Discussions](https://github.com/cjpais/Handy/discussions) before any PR is opened — see the PR template's "Community Feedback" section.
- **Translations:** Follow [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).
- **Full contributor workflow:** [CONTRIBUTING.md](CONTRIBUTING.md).

**Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Focus the message on _why_, not _what_.
