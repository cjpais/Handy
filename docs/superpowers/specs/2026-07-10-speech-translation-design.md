# Design: Dictate with Translation

**Date:** 2026-07-10
**Status:** Approved (pending spec review)

## Summary

Add a "Dictate with Translation" mode to Handy: the user presses a dedicated
shortcut, speaks, Handy transcribes the speech (existing Whisper/ASR pipeline),
then sends the transcript to a local LLM with an instruction to translate it
into a user-chosen target language, and pastes **only the translation** into the
active application.

This reuses the existing LLM post-processing pipeline (provider/model/API-key
settings, `llm_client`, history, overlays). The translation runs against the
**same LLM provider configured for post-processing** — intended to be a local
OpenAI-compatible endpoint (Ollama / LM Studio) via the "custom" provider, so
translation works fully offline.

## Motivation

Whisper's built-in translate task only ever targets English (`translate_to_english`
already exists for that). The user wants to translate their speech into *any*
language, offline. The app already has a complete LLM post-processing subsystem
that can send transcribed text to a local LLM with a prompt — translation is
exactly such a prompt, so we build a focused UX layer on top rather than a new
subsystem.

## Requirements

- Translate dictated speech into a user-selected target language.
- Any of the languages in `LANGUAGE_METADATA` (24 languages) may be the target.
- Offline: uses a local LLM through the existing custom provider (base URL).
- Triggered by a dedicated global shortcut, separate from plain dictation and
  from dictation-with-post-processing.
- Output = translation only (pasted into the focused app, saved to history).

## Non-Goals (YAGNI for v1)

- A separate LLM provider/model/API config just for translation (reuse
  post-processing's).
- Free-text custom translation prompt editing.
- On-the-fly / quick target-language switching (target is set in Settings).
- A bundled in-app translation model / runtime.
- Non-English UI localization of the new strings beyond the English source
  (other locales optional; ESLint forbids hardcoded strings so English keys are
  mandatory).

## Architecture

### Trigger → Action

`actions.rs` currently models the two dictation modes with
`TranscribeAction { post_process: bool }` and registers them in `ACTION_MAP`
(`"transcribe"`, `"transcribe_with_post_process"`).

Replace the boolean with an explicit mode:

```rust
enum TranscribeMode { Plain, PostProcess, Translate }
struct TranscribeAction { mode: TranscribeMode }
```

Add `ACTION_MAP` entry `"transcribe_with_translation" -> TranscribeAction { mode: Translate }`.

`TranscribeAction::stop` already threads a single value (`self.post_process`)
into the async task; it will thread `self.mode` instead. Overlay "polishing"
state shows for both `PostProcess` and `Translate` (any LLM step).

### Post-transcription pipeline

`process_transcription_output(app, transcription, mode)` (currently takes
`post_process: bool`):

- `Plain`: unchanged (Chinese-variant OpenCC conversion still applies).
- `PostProcess`: unchanged — runs the user's selected prompt.
- `Translate`: runs the LLM path with a **generated translation system prompt**.

`post_process_transcription(settings, text)` is refactored to
`run_llm(settings, text, system_prompt)` taking the system prompt explicitly:

- `PostProcess` passes the selected `post_process_prompts` entry (as today, via
  `build_system_prompt`).
- `Translate` passes a generated prompt (see below).

All other logic in that function is reused verbatim: provider resolution
(`active_post_process_provider`), model (`post_process_models`), API key
(`post_process_api_keys`), reasoning-effort tuning, structured-output schema
(the `transcription` string field), legacy fallback, `strip_invisible_chars`.

### Translation prompt

Built from the target language's **English name**. Settings store the language
**code** (e.g. `"de"`); the Rust side resolves it to an English name via a
small static `code -> English name` map (24 entries, kept in sync with the
frontend's `LANGUAGE_METADATA`). An unknown code falls back to using the code
string itself in the prompt.

Prompt template:

```
Translate the following text into {LANGUAGE}. Output only the translation,
preserving the original meaning and tone. Do not add explanations, notes, or
quotation marks.
```

`{LANGUAGE}` = English name of `translation_target_language` (e.g. `"de"` →
`"German"`). Uses structured output (reuse the existing `transcription` JSON
schema) so the model returns just the translated string.

### Settings (`settings.rs`)

- New field `translation_target_language: String` on `AppSettings`.
  - `#[serde(default = "default_translation_target_language")]` for
    backward-compatible loading of existing config files.
  - `default_translation_target_language() -> String` returns `"en"`.
  - Added to `get_default_settings()`.
- New default `ShortcutBinding` `"transcribe_with_translation"` in
  `get_default_settings()`:
  - name: "Transcribe with Translation"
  - description: "Converts your speech into text and translates it."
  - default binding: `ctrl+alt+space` (Windows/Linux),
    `option+ctrl+space` (macOS), `alt+ctrl+space` (other).
  - Verify the chosen defaults don't collide with existing defaults
    (`ctrl+space`, `ctrl+shift+space`, `escape`).

Because the target language always defaults to `"en"` and the dropdown always
holds a value, there is no "unset target language" state to handle.

### Frontend (`src/`)

- `settingsStore.ts`: add `translation_target_language` to the settings type/store.
- `bindings.ts`: regenerated automatically by tauri-specta at build time.
- New component `src/components/settings/TranslationTargetLanguage.tsx`: a
  language dropdown sourced from `LANGUAGE_METADATA` (label = native + English
  name), bound to `translation_target_language` via `useSettings`.
- New settings section `TranslationSettings` (near post-processing) containing:
  - the target-language dropdown,
  - a `ShortcutInput` for `transcribe_with_translation`,
  - an inline note that translation uses the same LLM provider as
    post-processing, linking to that section.
- i18n: add keys under `settings.translation.*` to
  `src/i18n/locales/en/translation.json`. No hardcoded JSX strings (ESLint).

## Error handling / edge cases

- **No provider/model configured:** the LLM step no-ops (same as post-processing
  today) and the original transcript is pasted. Additionally emit a UI toast
  hinting the user to configure an LLM provider. (Consistent with the existing
  `debug!` no-op behavior; toast is the only new surface.)
- **Blank/silent recording:** skipped via existing `is_blank_transcription`.
- **Cancellation (Escape):** works at every stage via the existing
  `complete_unless_cancelled` / `was_cancelled_since` machinery — no new paths.
- **LLM failure/timeout:** falls back to the original transcript (existing
  `run_llm` error handling), logged.
- **Target == source language:** acceptable; the model returns text unchanged or
  lightly rephrased. Not specially handled in v1.

## Testing

Rust unit tests (in `actions.rs` tests module or a focused module):

- Translation prompt generation from a language code yields the expected English
  language name and prompt text (e.g. `"de"` → contains `"German"`).
- `TranscribeMode` maps to the correct `ACTION_MAP` entries.
- `run_llm` with no active provider returns `None` (original transcript kept) —
  mirrors the post-processing no-op test.

Manual verification:

- Configure custom provider → local Ollama (`http://localhost:11434/v1`, a
  chat model). Set target language. Press the translation shortcut, speak,
  confirm the translated text is pasted and the history entry stores original +
  translation.

## Files touched (anticipated)

- `src-tauri/src/actions.rs` — mode enum, action map entry, prompt builder,
  `run_llm` refactor.
- `src-tauri/src/settings.rs` — new setting + default, new default binding.
- `src/stores/settingsStore.ts` — new setting.
- `src/components/settings/TranslationTargetLanguage.tsx` — new.
- `src/components/settings/` — new `TranslationSettings` section + wiring into
  the settings UI/index.
- `src/i18n/locales/en/translation.json` — new keys.
- (auto) `src/bindings.ts` — regenerated.
