# AGENTS.md

This file provides guidance to AI coding assistants working with code in this repository.

## Project Context

This repo is a ThegAi-branded (derived from Handy) Tauri desktop app.

- The product surface is now MASR-oriented and includes Malayalam-specific transcription flows.
- The app uses the `thegai` binary/app identity (`cli.rs`, window title, updater config, package metadata).
- Treat the current implementation as the source of truth over older Handy-era docs and assumptions.

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

# Build frontend only
bun run dev
bun run build
bun run preview

# Build the desktop app
bun run tauri build
```

**Verification and quality checks:**

```bash
# Frontend quality
bun run lint
bun run lint:fix
bun run check:translations

# Formatting
bun run format
bun run format:check
bun run format:frontend
bun run format:backend

# Browser E2E tests
bun run test:playwright
bun run test:playwright:ui

# Backend tests
cd src-tauri
cargo test
```

**Model setup (minimum local dev requirement):**

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

For platform-specific build setup, see [BUILD.md](BUILD.md).

## Architecture Overview

MASR is a cross-platform Tauri 2.x desktop app with a Rust backend and a React/TypeScript frontend. The architecture is still ThegAi/Handy-like, but several major features have been added on top:

- Malayalam ASR support on Windows via a native IndicConformer pipeline
- Multiple transcription engines beyond Whisper
- Meeting mode, meeting summaries, and meeting follow-up workflows
- Split Google integrations for Gmail/Tasks and Google Calendar
- A dedicated meeting prompt window in addition to the recording overlay
- Local audio-file transcription from the Meetings screen
- Post-processing via multiple LLM providers

## Backend Structure (`src-tauri/src/`)

- `lib.rs` - main Tauri entry point, command/event registration, manager wiring, overlay/prompt window creation
- `settings.rs` - persisted settings schema, defaults, migrations, provider defaults, meeting and Google flags
- `cli.rs` - runtime CLI flag definitions
- `signal_handle.rs` - reusable transcription trigger path shared by CLI and signal handling
- `overlay.rs` - recording overlay and meeting prompt window lifecycle/positioning
- `malayalam_asr.rs` - Windows-only Malayalam ASR runtime
- `transcription_coordinator.rs` - recording/transcription mode coordination
- `actions.rs`, `llm_client.rs`, `apple_intelligence.rs` - post-processing and local/remote LLM support
- `commands/` - Tauri command surface consumed by the frontend
- `managers/` - feature managers and core business logic:
  - `audio.rs` - recording, devices, state transitions, meeting shortcut behavior
  - `model.rs` - model catalog, downloads, discovery, migrations, engine metadata
  - `transcription.rs` - engine loading/inference, accelerator handling, post-processing hooks
  - `history.rs` - saved recordings/history, local file processing, meeting Q&A entry point
  - `meeting_assistant.rs` - local meeting detection, calendar polling, prompt emission
  - `google_oauth.rs` - desktop OAuth flow with feature-scoped Google scopes
  - `google_api.rs` - Gmail, Google Tasks, and Calendar API calls
- `audio_toolkit/` - low-level audio capture, resampling, VAD, visualization helpers

## Frontend Structure (`src/`)

- `App.tsx` - shell, onboarding flow, section switching, recording state listeners, meeting-summary navigation
- `bindings.ts` - auto-generated Tauri bindings via `tauri-specta`
- `stores/settingsStore.ts` - Zustand settings store and frontend update helpers
- `hooks/useSettings.ts` - settings fetch/update hook layer
- `components/settings/` - main settings UI
  - `meetings/MeetingsSettings.tsx` - meeting assistant toggles, Google integration cards, local-file upload, meeting history actions; also exports `MeetingEntryComponent` and `MeetingEntryProps` for reuse
  - `post-processing/` and `PostProcessingSettingsApi/` - provider/model/API-key/prompt configuration
  - `general/`, `advanced/`, `models/`, `history/`, `debug/`, `about/` - grouped settings sections
- `components/LocalFileTranscriber.tsx` - batch audio-file transcription modal for meeting or plain transcription actions
- `overlay/` - recording overlay frontend entry point
- `meeting_prompt/` - dedicated prompt window entry point for detected/upcoming meetings
- `primary/` - primary window frontend entry point (default app surface)
  - `main.tsx` - React root; imports `../App.css` to pull in Tailwind + theme variables + fonts
  - `PrimaryApp.tsx` - shell with Meetings / Transcription tab switcher and Settings button
  - `MeetingsView.tsx` - date-grouped meeting recordings list + upload audio button (no config UI)
- `i18n/` - translations and language metadata

## Current Feature Map

### Transcription engines

`model.rs` and `transcription.rs` currently support more than Whisper:

- Whisper
- Parakeet
- Moonshine
- Moonshine Streaming
- SenseVoice
- GigaAM
- Canary
- Cohere
- `MalayalamIndicConformerCTC` (Windows-only Malayalam ASR path)

When touching model behavior, inspect both `src-tauri/src/managers/model.rs` and `src-tauri/src/managers/transcription.rs`. Model catalog changes usually require corresponding translation, UI, and binding updates.

### Meeting workflow

Meeting functionality is now a first-class feature, not an add-on:

- There is a dedicated `meeting` shortcut binding in `settings.rs`.
- Meeting mode records continuously and stores history entries intended for summarization/follow-up flows.
- Meeting summaries route the frontend to the Meetings section.
- The recording overlay reflects meeting mode visually.
- The Meetings page supports summary viewing, transcript viewing, meeting Q&A, deletion, and Google follow-up actions.

Relevant files:

- `src-tauri/src/managers/meeting_assistant.rs`
- `src-tauri/src/commands/google.rs`
- `src/components/settings/meetings/MeetingsSettings.tsx`
- `tests/meeting_assistant.spec.ts`
- `tests/google_integration.spec.ts`

### Primary window

The app has **four** Vite/Tauri webview entry points:

| HTML entry                      | Window label        | Purpose                                                      |
| ------------------------------- | ------------------- | ------------------------------------------------------------ |
| `index.html`                    | `main`              | Settings shell (App.tsx) — onboarding, all settings sections |
| `src/primary/index.html`        | `primary`           | Primary window — default app surface on startup/tray/reopen  |
| `src/overlay/index.html`        | `recording_overlay` | Recording overlay                                            |
| `src/meeting_prompt/index.html` | `meeting_prompt`    | Meeting prompt panel                                         |

All four are declared in `vite.config.ts` under `build.rollupOptions.input`.

#### Window orchestration (`src-tauri/src/lib.rs`)

- **`primary`** is created on startup (1100×720, resizable+maximizable, hidden initially) and shown as the default window on:
  - App startup (unless `--start-hidden`)
  - Tray "Settings…" menu item
  - Single-instance activation (second launch without CLI flags)
  - macOS Dock reopen event
- **`main`** (settings) is created hidden (820×660) and shown only when:
  - The user clicks **Settings** in the primary window (`show_main_window_command`)
  - Onboarding or permission checks redirect from the primary window
  - The tray "Check for Updates" item triggers an update check
- Both `show_main_window` (internal) and `show_primary_window` (internal) set `ActivationPolicy::Regular` on macOS when called. The `CloseRequested` handler only switches to `ActivationPolicy::Accessory` when **both** `main` and `primary` are hidden.
- `show_primary_window_command` is a registered Tauri/specta command (available in `bindings.ts` as `commands.showPrimaryWindowCommand()`).
- `show_main_window_command` (existing) remains the correct command for the primary window to open the settings window.

#### CSS requirement for every entry point

Each entry's `main.tsx` must import `../App.css` (or a CSS file that does so) to get Tailwind utilities and the `@theme` custom tokens. Without this import the vite plugin generates no stylesheet for that window.

- `overlay/RecordingOverlay.css` does `@import "../App.css"`
- `primary/main.tsx` does `import "../App.css"` directly
- `meeting_prompt/MeetingPrompt.css` is standalone (uses its own custom properties)

#### Primary window content split

The **primary window** (`PrimaryApp.tsx`) intentionally shows only:

- **Meetings tab** → `MeetingsView` (date-grouped recording list + upload audio button)
- **Transcription tab** → `HistorySettings`
- **Settings button** → opens the `main` (settings) window

The **settings window** (`App.tsx` / `MeetingsSettings.tsx`) retains the full configuration surface:
Meeting Assistant toggles, Calendar Prompts, Google Services integration cards, and the full meeting history with all actions.

Do not merge these two surfaces. The split is intentional: the primary window is the everyday view; settings is the configuration panel.

#### Capabilities

Both `main` and `primary` windows must appear in `src-tauri/capabilities/default.json` and `src-tauri/capabilities/desktop.json`. If you add a new regular app window, add its label to both files.

### Meeting prompt window

Do not collapse prompt-window work back into the main app, overlay, or primary window unless the task explicitly requires that redesign. The multi-entry setup is intentional and configured in `vite.config.ts`.

### Google integrations

Google auth is split by feature:

- `gmail_tasks` for follow-up emails and task creation
- `calendar` for upcoming meeting reminders/prompts

Important constraints:

- Google Calendar is optional and should stay independent from Gmail/Tasks.
- Disconnecting Calendar should not implicitly disconnect Gmail/Tasks, and vice versa.
- Calendar prompts depend on a desktop OAuth client ID being configured for the build.
- Meeting prompt lead time is currently a fixed 5-minute UI option backed by settings.

Primary files:

- `src-tauri/src/managers/google_oauth.rs`
- `src-tauri/src/managers/google_api.rs`
- `src-tauri/src/commands/google.rs`
- `src/components/settings/meetings/MeetingsSettings.tsx`
- `src-tauri/src/settings.rs`

### Post-processing and output shaping

The app supports multiple post-processing providers and prompt presets. Current settings include:

- provider selection
- provider-specific base URLs and API keys
- model selection/fetching
- custom prompts
- meeting-summary defaults
- Malayalam output shaping via output language selection

Current provider/default logic lives in `src-tauri/src/settings.rs`. Frontend provider state lives under `src/components/settings/PostProcessingSettingsApi/`.

### Local file transcription

Users can upload audio files from the Meetings page and process them in the background as either:

- `meeting` - generate meeting-style output/history
- `transcribe` - plain transcription

Primary files:

- `src/components/LocalFileTranscriber.tsx`
- `src-tauri/src/commands/history.rs`
- `src-tauri/src/managers/history.rs`

## Key Architecture Patterns

**Manager pattern:** Core runtime capabilities are initialized in `lib.rs` and stored in Tauri managed state.

**Command-event architecture:** Frontend calls Tauri commands; backend emits events for history changes, recording state, prompt display, and navigation cues.

**Multi-window architecture:** Four separate webviews with separate frontend entry points: primary window (default app surface), settings/main window (floating settings panel), recording overlay, and meeting prompt. The primary window is the startup default; the settings window is shown on demand.

**Feature-scoped settings flow:** Zustand -> Tauri command -> Rust settings/store persistence.

**Mode-based transcription flow:** `transcribe`, `meeting`, and idle behavior are coordinated centrally; avoid introducing ad hoc recording-state flags.

## Settings System

Settings are stored via the Tauri store plugin and defined in `src-tauri/src/settings.rs`.

Recent settings surface includes:

- transcription shortcuts, including `meeting`
- microphone/output device selection
- model and accelerator preferences
- output language selection (`Malayalam`, `Manglish`, `English`)
- post-processing provider/model/prompt settings
- Google auth tokens and feature availability
- meeting detection, calendar prompts, and lead minutes
- history retention, paste behavior, debug/logging, autostart, tray visibility

If you add a setting, update all relevant layers:

1. `src-tauri/src/settings.rs`
2. Rust command/update path
3. `src/bindings.ts` via specta export
4. `src/stores/settingsStore.ts`
5. frontend UI
6. translations
7. tests when behavior changes

## Internationalization (i18n)

All user-facing strings must use i18next translations. ESLint enforces this in JSX.

When adding text:

1. Add the source key to `src/i18n/locales/en/translation.json`
2. Propagate translations to the other locale files
3. Run `bun run check:translations`

Meeting, Google, provider, and Malayalam-model UI changes frequently need translation updates across many locale files.

## Testing Guidance

Use the existing verification surface instead of inventing new one-off checks.

- `bun run lint`
- `bun run check:translations`
- `bun run build`
- `bun run test:playwright`
- `cd src-tauri && cargo test`

Read [TEST_INFRA.md](TEST_INFRA.md) before changing Playwright/Tauri mock behavior. The E2E suite already has mock plumbing for:

- Google integration state
- meeting detection flags
- calendar prompt state
- follow-up sending payloads
- output language settings

## CLI Parameters

The app supports runtime CLI flags for controlling a running instance and startup behavior.

| Flag                     | Description                                        |
| ------------------------ | -------------------------------------------------- |
| `--toggle-transcription` | Toggle recording on/off on a running instance      |
| `--toggle-post-process`  | Toggle recording with post-processing on/off       |
| `--cancel`               | Cancel the current operation on a running instance |
| `--start-hidden`         | Launch without showing the main window             |
| `--no-tray`              | Launch without system tray                         |
| `--debug`                | Enable debug mode with verbose logging             |

Implementation path:

- `src-tauri/src/cli.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/signal_handle.rs`

CLI flags are runtime-only overrides and should not mutate persisted settings.

## Platform Notes

- **Windows**
  - Malayalam ASR is currently Windows-only.
  - Local active-window meeting detection is currently implemented only on Windows.
  - Whisper GPU selection and ORT accelerator settings matter here.
- **macOS**
  - Apple Intelligence support exists behind availability/build checks on Apple Silicon.
  - Accessibility permissions still matter for keyboard simulation/shortcuts.
- **Linux**
  - Overlay behavior and typing-tool behavior remain platform-sensitive.
  - The overlay and prompt window implementation differs from macOS due to panel support.

## Implementation Tips

- Start by reading the real implementation files, not just README-era docs.
- When the task touches meetings or Google, inspect existing plumbing first instead of introducing parallel state or commands.
- Preserve the split Gmail/Tasks vs Calendar auth model.
- Preserve the multi-entry window architecture for primary window, overlay, and meeting prompts.
- Avoid accidental renames of branded runtime identifiers unless the task explicitly requests a product rename.
- Regenerate/update bindings only through the existing Rust specta export path.

## GitHub Workflow for AI Coding Assistants

**MANDATORY. Before opening any PR, issue, or discussion in this repo: you MUST read the relevant template file and follow it strictly.** That includes sections that look ceremonial.

- **Opening a PR:** Read [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md). If a section requires a human-written paragraph, leave a clear TODO placeholder rather than fabricating it.
- **Opening an issue:** Read [`.github/ISSUE_TEMPLATE/`](.github/ISSUE_TEMPLATE/). Feature requests belong in Discussions, not Issues.
- **Translations:** Follow [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).
- **Full contributor workflow:** [CONTRIBUTING.md](CONTRIBUTING.md).

**Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Focus the message on why, not what.
