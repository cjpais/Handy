# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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

# Code quality
bun run lint       # Lint frontend code
bun run lint:fix   # Auto-fix linting issues
bun run format     # Format all code (frontend + backend)
bun run format:check  # Check formatting without modifying
```

**Model Setup (Required for Development):**

```bash
# Create models directory
mkdir -p src-tauri/resources/models

# Download required VAD model
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

**macOS Only - Qwen3 ASR Setup:**

```bash
# Install MLX framework for Qwen3 ASR support
pip install mlx mlx-audio

# Or use HuggingFace mirror for faster download in China
HF_ENDPOINT=https://hf-mirror.com pip install mlx mlx-audio
```

## Architecture Overview

Handy is a cross-platform desktop speech-to-text application built with Tauri (Rust backend + React/TypeScript frontend).

### Core Components

**Backend (Rust - src-tauri/src/):**

- `lib.rs` - Main application entry point with Tauri setup, tray menu, and managers
- `managers/` - Core business logic managers:
  - `audio.rs` - Audio recording and device management
  - `model.rs` - Whisper model downloading and management
  - `transcription.rs` - Speech-to-text processing pipeline
  - `qwen3_engine.rs` - Qwen3 ASR engine for macOS (MLX-based)
  - `history.rs` - Transcription history with SQLite database
- `audio_toolkit/` - Low-level audio processing:
  - `audio/` - Device enumeration, recording, resampling
  - `vad/` - Voice Activity Detection using Silero VAD
- `commands/` - Tauri command handlers for frontend communication
- `shortcut.rs` - Global keyboard shortcut handling
- `settings.rs` - Application settings management
- `llm_client.rs` - LLM client for post-processing transcription
- `clipboard.rs` - Clipboard management and text pasting
- `audio_feedback.rs` - Audio feedback system for recording states
- `overlay.rs` - Recording overlay window management

**Frontend (React/TypeScript - src/):**

- `App.tsx` - Main application component with onboarding flow
- `components/settings/` - Settings UI components
- `components/model-selector/` - Model management interface
- `components/onboarding/` - User onboarding and setup flow
- `components/AccessibilityPermissions.tsx` - macOS accessibility permissions UI
- `components/Sidebar.tsx` - Main navigation sidebar
- `stores/` - Zustand state management stores
- `hooks/` - React hooks for settings and model management
- `lib/types.ts` - Shared TypeScript type definitions
- `i18n/` - Internationalization support (i18next)

### Key Architecture Patterns

**Manager Pattern:** Core functionality is organized into managers (Audio, Model, Transcription, History, Qwen3Engine) that are initialized at startup and managed by Tauri's state system.

**Command-Event Architecture:** Frontend communicates with backend via Tauri commands, backend sends updates via events.

**Pipeline Processing:** Audio → VAD → Transcription Engine (Whisper/Parakeet/Qwen3) → Text output with configurable components at each stage.

**Multi-Engine Support:** The transcription system supports multiple ASR engines:
- **Whisper:** GPU-accelerated models (Small/Medium/Turbo/Large)
- **Parakeet V3:** CPU-optimized model with automatic language detection
- **Qwen3 ASR:** MLX-accelerated model for Apple Silicon (macOS only)

### Technology Stack

**Core Libraries:**

- `transcribe-rs` - Multi-engine speech recognition (Whisper, Parakeet, Moonshine)
- `whisper-rs` - Local Whisper inference with GPU acceleration
- `cpal` - Cross-platform audio I/O
- `vad-rs` - Voice Activity Detection
- `rdev` - Global keyboard shortcuts
- `rubato` - Audio resampling
- `rodio` - Audio playback for feedback sounds
- `rusqlite` - SQLite database for transcription history
- `tokio` - Async runtime
- `enigo` - Keyboard/mouse simulation for text input
- `handy-keys` - Advanced keyboard input handling

**Frontend Libraries:**

- React 18 with TypeScript
- Tailwind CSS 4.1
- Zustand for state management
- Tauri 2.9 for desktop integration
- i18next for internationalization
- Sonner for toast notifications
- Lucide React for icons

**Platform-Specific Features:**

- macOS: Metal acceleration for Whisper, MLX for Qwen3 ASR, accessibility permissions, Apple Intelligence integration
- Windows: Vulkan acceleration, code signing
- Linux: OpenBLAS + Vulkan acceleration, Wayland support via wtype/dotool

### Application Flow

1. **Initialization:** App starts minimized to tray, loads settings, initializes managers
2. **Model Setup:** First-run downloads preferred ASR model (Whisper/Parakeet/Qwen3)
3. **Recording:** Global shortcut triggers audio recording with VAD filtering
4. **Processing:** Audio sent to selected ASR model for transcription
5. **Post-Processing (Optional):** Text can be processed by LLM for corrections
6. **Output:** Text pasted to active application via system clipboard
7. **History:** Transcription saved to SQLite database with audio file

### Settings System

Settings are stored using Tauri's store plugin with reactive updates:

- Keyboard shortcuts (configurable, supports push-to-talk)
- Audio devices (microphone/output selection)
- Model preferences (Whisper/Parakeet/Qwen3 variants)
- Audio feedback and translation options
- Post-processing settings (LLM provider, API key, prompts)
- History management (retention period, save preferences)
- Overlay positioning and debug mode

### Post-Processing System

The application supports optional LLM-based post-processing for transcription refinement:

- **Multiple Providers:** OpenAI, Anthropic, Custom endpoints
- **Custom Prompts:** User-defined prompts for different use cases
- **Prompt Management:** Create, update, and delete custom prompts
- **Model Selection:** Choose appropriate LLM model per provider

### History System

Transcription history is managed through SQLite database:

- **Database:** `history.db` in app data directory
- **Audio Files:** Stored in `recordings/` directory as WAV files
- **Retention Policies:** Time-based (3 days, 2 weeks, 3 months) or count-based
- **Saved Entries:** Mark entries as saved to prevent automatic deletion
- **Migration Support:** Handles migration from tauri-plugin-sql to rusqlite_migration

### Single Instance Architecture

The app enforces single instance behavior - launching when already running brings the settings window to front rather than creating a new process.

### Debug Features

**Debug Mode:** Access via `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux)
- View application logs
- Access app data directory
- Open recordings folder
- Check model status

**Signal Handling (Unix/Linux):**
- `SIGUSR2` signal toggles recording on/off
- Allows Wayland window managers to control Handy via external tools

## Model Types

### Whisper Models
- **Small:** 487 MB - Good balance of speed and accuracy
- **Medium:** 492 MB - Higher accuracy than Small
- **Turbo:** 1600 MB - Fast transcription with large model
- **Large:** 1100 MB - Highest accuracy for complex speech

### Parakeet Models
- **V2:** 473 MB - CPU-optimized with good performance
- **V3:** 478 MB - Latest version with automatic language detection

### Qwen3 ASR (macOS Only)
- **Qwen3-ASR-0.6B-8bit:** MLX-accelerated for Apple Silicon
- Requires `mlx` and `mlx-audio` Python packages
- Automatic language detection
- Fast inference on Apple Silicon
