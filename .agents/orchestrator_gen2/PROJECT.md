# Project: MASR Feature Improvements

## Architecture
- Handy is a Tauri v2 speech-to-text desktop application (Rust backend + React/TypeScript frontend).
- Core backend functions are in `src-tauri/src/actions.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/settings.rs`, and `src-tauri/src/shortcut/mod.rs`.
- Frontend UI components are under `src/components/settings/` and `src/components/footer/`.

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | Shortcut & Settings Layout | Place PostProcessingToggle at top, remove transcribe_with_post_process default hotkey, fix syntax error in actions.rs | None | DONE |
| 2 | Meeting Mode & UI | Emit meeting-summary event, MeetingsSettings UI, Footer indicator pill, navigate on event | None | DONE |
| 3 | Gemini Models Defaults & Transliteration | Pre-populate google provider models, default to gemma-4-26b-a4b-it, run_manglish_transliteration fallback | None | DONE |
| 4 | API Key Testing | Implement test_post_process_api_key in commands/ mod.rs and Test button in UI | None | IN_PROGRESS |
| 5 | Compilation & Build Verification | Build with target-dir C:\t, format and lint checks, run frontend build | M1, M2, M3, M4 | PENDING |

## Interface Contracts
### test_post_process_api_key Command
- Signature: `test_post_process_api_key(provider_id: String) -> Result<String, String>`
- Behaviour: Fetches the configured API key for the provider from settings, performs model fetch validation, and returns Ok(message) or Err(error_message).

### meeting-summary Event
- Event Name: `"meeting-summary"`
- Payload: `{ summary: String, transcript: String }`

### recording-state-changed Event
- Event Name: `"recording-state-changed"`
- Payload: `{ mode: String }` where mode is `"meeting"` or others.
