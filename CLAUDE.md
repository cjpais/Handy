# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Fork Context

This is a fork of [cjpais/Handy](https://github.com/cjpais/Handy) (v0.7.9), customized for a **French-speaking developer** who uses speech-to-text primarily for software development (TypeScript, Python, Angular, React, Docker, K8s).

**Notion project page**: https://www.notion.so/31d2933a841f81ad971ece67d3212c2b

## Development Commands

**Prerequisites:** [Rust](https://rustup.rs/) (latest stable), [Bun](https://bun.sh/)

```bash
# Install dependencies
bun install

# Run in development mode
bun run tauri dev
# If cmake error on macOS:
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev

# Build for production
bun run tauri build

# Linting and formatting (run before committing)
bun run lint              # ESLint for frontend
bun run lint:fix          # ESLint with auto-fix
bun run format            # Prettier + cargo fmt
bun run format:check      # Check formatting without changes

# Run Rust tests
cd src-tauri && cargo test

# Check translations completeness
bun run check:translations
```

**Model Setup (Required for Development):**

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

## Architecture Overview

Handy is a cross-platform desktop speech-to-text app built with Tauri 2.x (Rust backend + React/TypeScript frontend).

**Codebase size:** ~16K lines Rust (49 files) + ~11K lines TypeScript (116 files).

### Transcription Pipeline (6 stages)

```
Microphone (cpal) → Resampling 16kHz (rubato) → VAD Silero (30ms frames)
→ Transcription Engine (Whisper/Parakeet/Moonshine/SenseVoice/GigaAM)
→ Text Processing (custom words + filler word removal)
→ [Optional] LLM Post-Processing → Clipboard/Paste
```

### Backend Structure (src-tauri/src/)

**4 Managers** (initialized at startup, managed via `Arc<T>` in Tauri state):

| Manager | File | Responsibility |
|---------|------|----------------|
| `AudioRecordingManager` | `managers/audio.rs` | Audio capture, device selection, VAD, resampling to 16kHz |
| `ModelManager` | `managers/model.rs` | Model registry, download, extraction, auto-discovery of custom models |
| `TranscriptionManager` | `managers/transcription.rs` | Engine loading/unloading, inference, idle timeout watcher |
| `HistoryManager` | `managers/history.rs` | SQLite persistence, WAV file storage, retention cleanup |

**Other key modules:**

| File | Role | Customization priority |
|------|------|----------------------|
| `settings.rs` | 40+ configurable options (`AppSettings` struct), persisted via tauri-plugin-store | **High** |
| `actions.rs` | Complete pipeline orchestration (record → transcribe → post-process → paste) | **High** |
| `audio_toolkit/text.rs` | Custom word correction (Levenshtein+Soundex+N-gram) + filler word removal | **High** |
| `llm_client.rs` | HTTP client for LLM post-processing (OpenAI-compatible API) | Medium |
| `transcription_coordinator.rs` | Thread-safe serialization of all events (debounce 30ms, 3 stages: Idle→Recording→Processing) | Low |
| `clipboard.rs` | 5 paste methods: Ctrl+V, Direct, Shift+Insert, Ctrl+Shift+V, External Script | Low |
| `shortcut/` | Global shortcuts (2 implementations: Tauri and HandyKeys) | Low |
| `cli.rs` | CLI flags (clap derive): --toggle-transcription, --toggle-post-process, --cancel, etc. | Low |
| `signal_handle.rs` | Unix signal handlers (SIGUSR1/SIGUSR2) + shared `send_transcription_input()` | Low |

### Frontend Structure (src/)

| Area | Files | Role |
|------|-------|------|
| `App.tsx` | Entry point | Onboarding flow + main layout |
| `stores/settingsStore.ts` | Zustand + Immer | Settings state with optimistic updates |
| `stores/modelStore.ts` | Zustand + Immer | Model download/status with Tauri event listeners |
| `hooks/useSettings.ts` | Hook wrapper | Clean interface over settingsStore |
| `bindings.ts` | Auto-generated | Tauri command types (via tauri-specta) - DO NOT EDIT |
| `components/settings/` | 35+ files | Individual setting components |
| `components/ui/` | 17 files | Reusable UI: SettingContainer, ToggleSwitch, Select, Slider, etc. |
| `overlay/` | Separate entry point | Recording overlay window (RecordingOverlay.tsx) |
| `i18n/` | 18 languages | i18next with auto-discovery via Vite glob |
| `lib/utils/` | Utilities | RTL support, keyboard formatting, model translations |

### Key Patterns

**Manager Pattern:** Core functionality in managers with `Arc<Mutex<T>>` for thread-safe shared state.

**Command-Event Architecture:** Frontend → Backend via Tauri commands; Backend → Frontend via events.

**TranscriptionCoordinator:** Single-thread serialization of all pipeline events with `FinishGuard` (Drop trait) for panic safety.

**Settings Pattern:** Each setting has a dedicated updater in `settingUpdaters` map that calls the corresponding Tauri command. Optimistic UI updates with rollback on error.

**State Flow:** Zustand → Tauri Command → Rust State → Persistence (tauri-plugin-store in `settings_store.json`)

## Transcription Engines

| Engine | Size | French support | Speed | Accuracy | Translation |
|--------|------|---------------|-------|----------|-------------|
| **Parakeet V3** (recommended) | 478 MB | Yes (25 EU langs) | 0.85 | 0.90 | No |
| Whisper Turbo | 1.6 GB | Yes (100+ langs) | 0.70 | 0.85 | Yes |
| Whisper Large V3 | 3.1 GB | Yes (100+ langs) | 0.20 | 0.95 | Yes |
| Whisper Small | 487 MB | Yes (100+ langs) | 0.60 | 0.70 | Yes |
| Moonshine (3 variants) | 31-192 MB | No (EN only) | 0.80-0.95 | 0.55-0.75 | No |
| SenseVoice | 160 MB | No (ZH/EN/JA/KO) | 0.95 | 0.65 | No |
| GigaAM v3 | 225 MB | No (RU only) | 0.75 | 0.85 | No |

**For French:** Use Parakeet V3 (best speed/quality ratio, CPU-optimized) or Whisper Turbo/Large (GPU, more languages).

Engine types defined in `managers/model.rs` enum `EngineType`: Whisper, Parakeet, Moonshine, MoonshineStreaming, SenseVoice, GigaAM.

## Post-Processing (LLM)

Post-processing sends transcription output to an LLM API for cleanup/formatting.

**Supported providers** (defined in `settings.rs` `default_post_process_providers()`):
- OpenAI, Anthropic, Groq, Cerebras, OpenRouter, Z.AI
- Apple Intelligence (macOS ARM64 only, via Swift bridge in `apple_intelligence.rs`)
- Custom (defaults to `localhost:11434` = Ollama)

**Implementation:** `llm_client.rs` handles OpenAI-compatible API calls. `actions.rs:post_process_transcription()` orchestrates: prompt building → API call → structured output extraction → fallback to legacy mode.

**Prompts:** Stored in `settings.rs` field `post_process_prompts: Vec<LLMPrompt>`. Each prompt has id, name, and template with `${output}` placeholder.

## Text Processing

### Custom Word Correction (`audio_toolkit/text.rs`)

- **Fuzzy matching** combining Levenshtein distance + Soundex phonetics
- **N-gram matching** (1-3 words) for compound terms: "Charge B" → "ChargeBee", "Chat G P T" → "ChatGPT"
- **Configurable threshold** via `word_correction_threshold` setting (default 0.18)
- Case and punctuation preservation

### Filler Word Removal (`audio_toolkit/text.rs`)

Language-aware filler word lists in `get_filler_words_for_language()`:
- **French:** "euh", "hmm", "hm", "mmm"
- **English:** "uh", "um", "uhm", "umm", "ah", "hmm", etc.
- Stutter collapse: 3+ repetitions of 1-2 letter words collapsed ("wh wh wh" → "wh")
- Custom filler word lists override defaults when set

## Internationalization (i18n)

All user-facing strings must use i18next translations. ESLint enforces this (`eslint-plugin-i18next` with `no-literal-string` rule).

**Adding new text:**

1. Add key to `src/i18n/locales/en/translation.json` (source of truth)
2. Add translations to other locale files
3. Use in component: `const { t } = useTranslation(); t('key.path')`

**18 UI languages** supported including French (`src/i18n/locales/fr/translation.json`). RTL support via `lib/utils/rtl.ts`.

## Code Style

**Rust:**
- Run `cargo fmt` and `cargo clippy` before committing
- Handle errors explicitly with `Result<T, anyhow::Error>` - avoid `unwrap()` in production
- Use `Arc<Mutex<T>>` for shared state, `catch_unwind` around engine calls
- Platform-specific code via `#[cfg(target_os = "...")]`

**TypeScript/React:**
- Strict TypeScript, avoid `any` types
- Functional components with hooks
- Tailwind CSS v4 for styling (CSS variables for theming in `App.css`)
- Path aliases: `@/` → `./src/`
- Zustand stores with Immer for complex state updates

## Commit Guidelines

Use conventional commits:
- `feat:` new features
- `fix:` bug fixes
- `docs:` documentation
- `refactor:` code refactoring
- `chore:` maintenance

## Custom Forks & Dependencies

**4 forked dependencies** — be cautious when updating:

| Dependency | Fork | Purpose |
|-----------|------|---------|
| `tauri` (runtime, wry, utils) | `cjpais/tauri` branch `handy-2.10.2` | Custom runtime modifications |
| `vad-rs` | Git fork | Voice Activity Detection (Silero ONNX) |
| `rodio` | Git fork | Audio playback modifications |
| `rdev` | Git fork (via handy-keys) | Global keyboard hooks |

## CLI Parameters

| Flag | Description |
|------|-------------|
| `--toggle-transcription` | Toggle recording on/off on a running instance |
| `--toggle-post-process` | Toggle recording with post-processing on/off |
| `--cancel` | Cancel the current operation |
| `--start-hidden` | Launch without showing the main window |
| `--no-tray` | Launch without the system tray icon |
| `--debug` | Enable debug mode with verbose (Trace) logging |

CLI flags are runtime-only overrides (do NOT modify persisted settings). Remote control flags work via `tauri_plugin_single_instance`.

## Debug Mode

Access debug features: `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux)

## Platform Notes

- **macOS**: Metal acceleration, accessibility permissions required, HandyKeys for shortcuts, NSPanel for overlay
- **Windows**: Vulkan acceleration, Azure Trusted Signing, NSIS installer with portable mode
- **Linux**: OpenBLAS + Vulkan, gtk-layer-shell for overlay, multiple typing tools (wtype/xdotool/dotool)

## Settings Reference

`AppSettings` struct in `settings.rs` — key fields for customization:

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `selected_model` | String | "" | Active transcription model ID |
| `selected_language` | String | "auto" | Transcription language (ISO 639-1) |
| `custom_words` | Vec<String> | [] | Custom word correction list |
| `custom_filler_words` | Option<Vec<String>> | None | Override default filler words |
| `word_correction_threshold` | f64 | 0.18 | Fuzzy matching sensitivity |
| `post_process_enabled` | bool | false | Enable LLM post-processing |
| `post_process_provider_id` | String | "openai" | Active LLM provider |
| `post_process_prompts` | Vec<LLMPrompt> | [default] | Custom prompt templates |
| `post_process_selected_prompt_id` | Option<String> | None | Active prompt |
| `paste_method` | PasteMethod | CtrlV (macOS/Win) | How text is output |
| `push_to_talk` | bool | true | Hold-to-record vs toggle |
| `app_language` | String | OS locale | UI language |
