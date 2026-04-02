# Codebase Structure

**Analysis Date:** 2026-03-28

## Directory Layout

```
Handy/
├── src/                          # Frontend (React/TypeScript)
│   ├── App.tsx                   # Root component, onboarding gate
│   ├── main.tsx                  # Main window entry point
│   ├── bindings.ts               # Auto-generated Tauri type bindings (do not edit)
│   ├── components/
│   │   ├── settings/             # 35+ settings UI components, one per setting
│   │   │   ├── general/          # GeneralSettings.tsx, ModelSettingsCard.tsx
│   │   │   ├── advanced/         # AdvancedSettings.tsx
│   │   │   ├── debug/            # DebugSettings.tsx and sub-components
│   │   │   ├── history/          # HistorySettings.tsx
│   │   │   ├── models/           # ModelsSettings.tsx
│   │   │   ├── post-processing/  # PostProcessingSettings.tsx
│   │   │   └── PostProcessingSettingsApi/  # Provider/key/model/URL fields
│   │   ├── model-selector/       # ModelSelector, ModelDropdown, DownloadProgressDisplay
│   │   ├── onboarding/           # Onboarding.tsx, AccessibilityOnboarding.tsx, ModelCard.tsx
│   │   ├── update-checker/       # UpdateChecker.tsx
│   │   ├── ui/                   # Primitive UI components (Button, Input, Select, etc.)
│   │   ├── icons/                # SVG icon components
│   │   ├── shared/               # ProgressBar
│   │   ├── footer/               # Footer.tsx
│   │   ├── Sidebar.tsx           # Navigation sidebar + SECTIONS_CONFIG
│   │   └── AccessibilityPermissions.tsx
│   ├── overlay/
│   │   ├── main.tsx              # Overlay window entry point (separate webview)
│   │   └── RecordingOverlay.tsx  # Recording/transcribing/processing status HUD
│   ├── stores/
│   │   ├── settingsStore.ts      # Zustand store — settings, audio devices, post-process
│   │   └── modelStore.ts         # Zustand store — model list and download state
│   ├── hooks/
│   │   ├── useSettings.ts        # Convenience wrapper over settingsStore
│   │   └── useOsType.ts          # OS detection hook
│   ├── lib/
│   │   ├── types/
│   │   │   └── events.ts         # TypeScript event payload interfaces
│   │   ├── utils/
│   │   │   ├── format.ts         # Text formatting helpers
│   │   │   ├── keyboard.ts       # Keyboard binding utilities
│   │   │   ├── modelTranslation.ts  # Model ID → display name
│   │   │   └── rtl.ts            # RTL language direction helpers
│   │   └── constants/
│   │       └── languages.ts      # Language list constants
│   ├── i18n/
│   │   ├── index.ts              # i18next setup
│   │   ├── languages.ts          # Language metadata (name, code, direction)
│   │   └── locales/
│   │       ├── en/translation.json   # English — source of truth
│   │       ├── es/translation.json
│   │       ├── fr/translation.json
│   │       └── vi/translation.json
│   └── utils/
│       └── dateFormat.ts         # Date formatting helpers
│
├── src-tauri/                    # Backend (Rust / Tauri)
│   ├── src/
│   │   ├── main.rs               # Binary entry: parse CLI args, call lib::run()
│   │   ├── lib.rs                # App bootstrap: plugins, setup closure, initialize_core_logic()
│   │   ├── managers/
│   │   │   ├── mod.rs
│   │   │   ├── audio.rs          # AudioRecordingManager: record, device, mute, VAD, visualizer
│   │   │   ├── model.rs          # ModelManager: download, delete, load/unload, accelerators
│   │   │   ├── transcription.rs  # TranscriptionManager: Whisper/Parakeet inference
│   │   │   ├── transcription_mock.rs  # Mock transcription for dev/testing
│   │   │   └── history.rs        # HistoryManager: entries, WAV files, retention
│   │   ├── commands/
│   │   │   ├── mod.rs            # Shared commands + initialize_enigo/shortcuts
│   │   │   ├── audio.rs          # Audio device and permission commands
│   │   │   ├── history.rs        # History CRUD commands
│   │   │   ├── models.rs         # Model management commands
│   │   │   └── transcription.rs  # Transcription config commands
│   │   ├── audio_toolkit/
│   │   │   ├── mod.rs
│   │   │   ├── constants.rs
│   │   │   ├── text.rs           # Text post-processing utilities
│   │   │   ├── utils.rs          # WAV save/verify helpers
│   │   │   ├── audio/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── device.rs     # Device enumeration (cpal)
│   │   │   │   ├── recorder.rs   # Audio stream capture
│   │   │   │   ├── resampler.rs  # Sample rate conversion
│   │   │   │   ├── utils.rs
│   │   │   │   └── visualizer.rs # Mic level computation for overlay bars
│   │   │   ├── vad/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── silero.rs     # Silero VAD ONNX inference
│   │   │   │   └── smoothed.rs   # Smoothed VAD state
│   │   │   └── bin/cli.rs        # Standalone audio toolkit CLI binary
│   │   ├── shortcut/
│   │   │   ├── mod.rs            # init_shortcuts, register/unregister cancel shortcut
│   │   │   ├── handler.rs        # Shortcut event handler → TranscriptionCoordinator
│   │   │   ├── handy_keys.rs     # HandyKeys alternative shortcut recording
│   │   │   └── tauri_impl.rs     # tauri-plugin-global-shortcut integration
│   │   ├── helpers/
│   │   │   ├── mod.rs
│   │   │   └── clamshell.rs      # Laptop lid detection (is_laptop command)
│   │   ├── actions.rs            # ShortcutAction trait + ACTION_MAP + TranscribeAction pipeline
│   │   ├── transcription_coordinator.rs  # Single-thread pipeline state machine
│   │   ├── settings.rs           # AppSettings struct, get/write_settings, defaults
│   │   ├── cli.rs                # CliArgs (clap derive)
│   │   ├── portable.rs           # Portable mode: path redirection, marker detection
│   │   ├── overlay.rs            # Overlay window creation helpers
│   │   ├── tray.rs               # Tray icon, menu, icon theme logic
│   │   ├── tray_i18n.rs          # Tray menu localization
│   │   ├── input.rs              # EnigoState (keyboard simulation)
│   │   ├── clipboard.rs          # Clipboard write helpers
│   │   ├── audio_feedback.rs     # Sound playback (start/stop/cancel sounds)
│   │   ├── llm_client.rs         # HTTP client for LLM post-processing APIs
│   │   ├── apple_intelligence.rs # macOS ARM Apple Intelligence integration
│   │   ├── signal_handle.rs      # UNIX signal handlers (SIGUSR1/2) + send_transcription_input()
│   │   ├── utils.rs              # Shared utilities: paste, overlay show/hide, tray state
│   │   └── main.rs               # (see top)
│   ├── capabilities/             # Tauri permission capability files
│   ├── resources/
│   │   └── models/               # Downloaded ONNX/GGUF model files (gitignored)
│   ├── icons/                    # App icons for all platforms
│   ├── gen/
│   │   └── schemas/              # Auto-generated Tauri capability JSON schemas
│   └── Cargo.toml
│
├── tests/                        # Integration/E2E test stubs
├── scripts/                      # Build/release helper scripts
├── .planning/
│   └── codebase/                 # GSD codebase map documents (this file)
└── package.json / bun.lockb      # Frontend dependencies
```

## Directory Purposes

**`src/components/settings/`:**
- One file per user-facing setting (e.g. `MicrophoneSelector.tsx`, `GlobalShortcutInput.tsx`)
- Grouped into sub-directories by settings page: `general/`, `advanced/`, `debug/`, `history/`, `models/`, `post-processing/`
- Each component reads from `useSettings()` and calls `updateSetting(key, value)`

**`src/components/ui/`:**
- Reusable primitives: `Button`, `Input`, `Select`, `Slider`, `ToggleSwitch`, `Tooltip`, `SettingContainer`, `SettingsGroup`
- No business logic; purely presentational

**`src/stores/`:**
- `settingsStore.ts` — primary store; all settings, audio device lists, post-process state
- `modelStore.ts` — model list, download progress, current model status

**`src/overlay/`:**
- Separate Tauri webview window (not the main window)
- Bootstraps its own React root in `overlay/main.tsx`
- Communicates exclusively via Tauri events (`show-overlay`, `hide-overlay`, `mic-level`)

**`src-tauri/src/managers/`:**
- All four managers are initialized once in `initialize_core_logic()` and stored as `Arc<T>` in Tauri state
- Accessed anywhere in Rust via `app.state::<Arc<ManagerType>>()`

**`src-tauri/src/commands/`:**
- Thin Tauri command handlers; delegate to managers or settings
- All functions annotated `#[tauri::command] #[specta::specta]`
- Registered in `lib.rs` `collect_commands![]` macro

**`src-tauri/src/audio_toolkit/`:**
- Self-contained audio processing library; also has a standalone CLI binary (`bin/cli.rs`)
- Used by `AudioRecordingManager` and `TranscriptionManager`

## Key File Locations

**Entry Points:**
- `src/main.tsx` — frontend main window bootstrap
- `src/overlay/main.tsx` — frontend overlay window bootstrap
- `src-tauri/src/main.rs` — backend binary entry, CLI parsing
- `src-tauri/src/lib.rs` — backend app setup and initialization

**IPC Contract:**
- `src/bindings.ts` — auto-generated; defines all `commands.*` and event types; never edit manually
- `src/lib/types/events.ts` — manually maintained TypeScript event payload types

**State Management:**
- `src/stores/settingsStore.ts` — all frontend state for settings and devices
- `src/stores/modelStore.ts` — model-related frontend state
- `src-tauri/src/settings.rs` — backend settings struct and persistence

**Pipeline Core:**
- `src-tauri/src/actions.rs` — `TranscribeAction` implements full recording→transcription→paste pipeline
- `src-tauri/src/transcription_coordinator.rs` — pipeline serialization and state machine

**Configuration:**
- `src-tauri/Cargo.toml` — Rust dependencies
- `package.json` — frontend dependencies (managed with Bun)
- `src-tauri/src/settings.rs` — `AppSettings` struct is the canonical list of all user preferences

## Naming Conventions

**Frontend files:**
- React components: `PascalCase.tsx` (e.g. `MicrophoneSelector.tsx`)
- Stores: `camelCaseStore.ts` (e.g. `settingsStore.ts`)
- Hooks: `useCamelCase.ts` (e.g. `useSettings.ts`)
- Utilities: `camelCase.ts` (e.g. `dateFormat.ts`)
- Each directory has an `index.ts` barrel re-exporting public surface

**Backend files:**
- Rust modules: `snake_case.rs` following standard Rust conventions
- Managers: `{domain}.rs` in `managers/` directory
- Commands: `{domain}.rs` in `commands/` directory, one file per domain

## Where to Add New Code

**New setting (frontend + backend):**
1. Add field to `AppSettings` struct in `src-tauri/src/settings.rs`
2. Add default value in `get_default_settings()` in same file
3. Add Rust command handler in `src-tauri/src/commands/mod.rs` or relevant domain file
4. Register command in `collect_commands![]` in `src-tauri/src/lib.rs`
5. Add to `settingUpdaters` map in `src/stores/settingsStore.ts`
6. Create setting component in `src/components/settings/` following existing pattern
7. Add i18n key in `src/i18n/locales/en/translation.json` (and other locales)

**New settings page section:**
- Add component in `src/components/settings/{section}/`
- Register in `SECTIONS_CONFIG` in `src/components/Sidebar.tsx`

**New Tauri command:**
- Add `#[tauri::command] #[specta::specta]` function in `src-tauri/src/commands/{domain}.rs`
- Register in `collect_commands![]` in `src-tauri/src/lib.rs`
- Bindings auto-regenerate on next debug build

**New backend event (Rust → frontend):**
- Emit with `app.emit("event-name", payload)` in Rust
- Add payload type in `src/lib/types/events.ts`
- Listen with `listen("event-name", handler)` in frontend

**New manager:**
- Create `src-tauri/src/managers/{name}.rs`
- Export from `src-tauri/src/managers/mod.rs`
- Initialize in `initialize_core_logic()` in `src-tauri/src/lib.rs`
- Register with `app_handle.manage(arc_manager)`

**New UI primitive:**
- Add to `src/components/ui/` and export from `src/components/ui/index.ts`

## Special Directories

**`src/bindings.ts`:**
- Generated: Yes (by tauri-specta on debug builds via `cargo build` or `bun run tauri dev`)
- Committed: Yes
- Do not edit manually

**`src-tauri/resources/models/`:**
- Generated: Yes (downloaded at runtime or via setup script)
- Committed: No (gitignored, large binary files)
- Required: `silero_vad_v4.onnx` must be present for development

**`src-tauri/gen/`:**
- Generated: Yes (Tauri tooling)
- Committed: Yes (for Apple platform build artifacts)

**`.planning/codebase/`:**
- Generated: Yes (GSD map-codebase command)
- Committed: No (local planning only)
- Contains: ARCHITECTURE.md, STRUCTURE.md, STACK.md, etc.

---

*Structure analysis: 2026-03-28*
