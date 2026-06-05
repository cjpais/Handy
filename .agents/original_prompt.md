## 2026-06-05T10:56:52Z

Add Gemini post-processing provider, Manglish transliteration, and Meeting mode.

Working directory: d:/Downloads/Projects/MASR
Integrity mode: development

## Requirements

### R1. Google Gemini Post-Processing Provider

- Add the `google` (Gemini) provider using OpenAI compatibility endpoint `https://generativelanguage.googleapis.com/v1beta/openai`.
- Automatically load `GoogleAPI` from `.env` using `dotenvy` at startup, and default to it if the settings key is empty.
- Set Google Gemini as the active post-processing provider if `GoogleAPI` is found in `.env`.

### R2. Manglish Transliteration Toggle

- Add `manglish_output` to settings and expose a toggle in the UI.
- Add default prompt "Transliterate the following Malayalam text into Manglish:\n\n${output}" with ID `default_manglish_transliteration`.
- If `manglish_output` is enabled, run the Manglish transliteration prompt after transcription/post-processing before pasting.

### R3. Meeting Mode

- Implement a continuous recording/summarization mode triggered by the keyboard shortcut `ctrl+shift+m`.
- Meeting Mode should bypass standard timeouts, record continuously using VAD noise filtering, and summarize the final Malayalam transcript to English using the Gemini API.

## Acceptance Criteria

### Settings & Environment

- [ ] Google (Gemini) provider is added to `default_post_process_providers()` using endpoint `https://generativelanguage.googleapis.com/v1beta/openai`.
- [ ] `manglish_output` and `meeting` shortcut binding are present in AppSettings.
- [ ] Dotenvy is integrated into the build/run cycle to load `.env` variables on startup.
- [ ] If `GoogleAPI` is found in `.env`, the Google provider API key is initialized and selected, with post-processing enabled.

### Transcription & Post-processing

- [ ] Transcribing in standard mode with `manglish_output` enabled transliterates output to Manglish before pasting.
- [ ] Pressing `Ctrl+Shift+M` starts continuous meeting recording, which runs until `Ctrl+Shift+M` is pressed again.
- [ ] Stopping a meeting recording sends the full transcript to Gemini with the English summary prompt, then pastes the summary.

## 2026-06-05T10:58:05Z

The user has requested to run the implementation much faster by maximizing parallelism. Please configure the orchestrator or direct the system to spin up at least 5 parallel agents to divide and conquer the requirements (e.g., separating Gemini Provider, Manglish Transliteration, Meeting Mode, UI updates, and the E2E Test Suite). Proceed with maximum parallelism immediately.

## 2026-06-05T12:21:15Z

Implement MASR Feature Improvements including shortcut cleanup, meeting mode fix, meetings sidebar section, meeting recording indicator pill, Gemini model defaults, Manglish transliteration fix, and API key testing button.

Working directory: d:/Downloads/Projects/MASR
Integrity mode: development

## Requirements

### R1. Shortcut Cleanup (Backend + Frontend)

- Remove `transcribe_with_post_process` from the default settings bindings so it has no default shortcut slot.
- Remove references to `transcribe_with_post_process` in `shortcut/mod.rs`, `shortcut/tauri_impl.rs`, `shortcut/handy_keys.rs`, and UI components (`GeneralSettings.tsx`, `PostProcessingSettings.tsx`).
- Keep `"transcribe_with_post_process"` in `ACTION_MAP` (mapping to `TranscribeAction`) and in the coordinator's `is_transcribe_binding` list to ensure backward compatibility for `--toggle-post-process` CLI flag and `SIGUSR1` signal (forces post-processing on for that run).
- Fix the existing syntax error in `src-tauri/src/actions.rs` around line 731 by restoring the missing `tauri::async_runtime::spawn(async move {` line.
- Place `PostProcessingToggle` as a visible on/off toggle at the top of the Post-Processing settings page.

### R2. Meeting Mode Fix (Toggle + No Paste)

- Modify `MeetingAction` in `actions.rs` so that upon completion, instead of pasting the summary into the active window, it emits a `"meeting-summary"` event carrying a payload of `{ summary: String, transcript: String }`.

### R3. Meetings Sidebar Section (Frontend)

- Add a new "Meetings" section in the settings sidebar (using icon `Users2` or `ClipboardList`).
- Implement the `MeetingsSettings.tsx` component that listens to `"meeting-summary"` events, stores summaries reactively in `localStorage`, displays them in a scrollable list with timestamp, expandable summary, collapsible raw transcript, and a copy button.
- Configure `App.tsx` to automatically navigate to the "Meetings" section when a new `"meeting-summary"` event is received.

### R4. Meeting Recording Indicator Pill (Frontend)

- Emit `"recording-state-changed"` event carrying `{ mode: "meeting" | "transcribe" | "idle" }` from transcription actions.
- Render a pulsing indicator pill in the main UI (App footer) when `mode === "meeting"`.

### R5. Gemini Model Defaults (Frontend)

- Pre-populate the `google` provider's model list with a curated list of Gemini models, defaulting to `gemma-4-26b-a4b-it`.

### R6. Manglish Transliteration Fix (Backend)

- Implement `run_manglish_transliteration` in `actions.rs` that calls Gemini with model `gemma-4-26b-a4b-it` if the Google API key is set, falling back to the active provider if the key is empty.

### R7. API Key Test Button (Frontend + Backend)

- Implement `test_post_process_api_key(provider_id: String) -> Result<String, String>` command in `commands/` that runs a validation request for the selected provider.
- Add a "Test" button next to the API key field in `PostProcessingSettings.tsx` to call this command and show inline validation status (✅ or ❌ with error message).

## Acceptance Criteria

### Compilation & Build

- [ ] The Rust backend compiles successfully under target `C:\t` to avoid path length issues.
- [ ] The frontend compiles and builds successfully via `bun run build`.

### Shortcut Cleanup

- [ ] No `transcribe_with_post_process` shortcut input is shown in the General Settings or Post-Processing Settings UI.
- [ ] Changing the `post_process_enabled` toggle at the top of Post-Processing settings page successfully persists the setting.

### Meeting Mode & Sidebar

- [ ] Pressing `Ctrl+Shift+M` toggles meeting recording.
- [ ] Stopping meeting recording emits `"meeting-summary"` and navigates the app to the Meetings sidebar section without pasting text.
- [ ] The Meetings page lists the new summary, timestamp, copy button, and collapsible transcript.

### Recording Pill

- [ ] The pulsing red indicator pill is displayed at the bottom of the main window during meeting recording, and disappears when recording stops.

### Gemini & Key Test

- [ ] Selecting the Google provider shows the curated Gemini models.
- [ ] Clicking the "Test" button with an invalid key shows a red error/cross, and with a valid key shows a green checkmark.
