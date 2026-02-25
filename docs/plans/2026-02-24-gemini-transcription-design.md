# Gemini Transcription Provider — Design

**Date:** 2026-02-24
**Status:** Approved
**Branch:** feat/local-server-transcription

## Summary

Add Google Gemini as a native transcription provider. Unlike existing Cloud (OpenAI-compatible `/audio/transcriptions`) or Local (Whisper/Parakeet) modes, Gemini accepts audio directly via its multimodal `generateContent` API and can apply a custom prompt in the same call — combining transcription + post-processing in a single HTTP request.

## User-Facing Behavior

- **`transcribe` shortcut** (`option+space`) with Gemini selected: audio → Gemini → plain transcription text
- **`transcribe_with_post_process` shortcut** (`option+shift+space`) with Gemini selected: audio + `gemini_prompt` as system instruction → Gemini → formatted/processed text
- No separate LLM post-processing step when Gemini is the active provider
- Configured via new `GeminiTranscriptionCard` in Settings → Models

## Architecture

### New Files

- `src-tauri/src/gemini_client.rs` — async HTTP client for Gemini `generateContent` REST API

### Modified Files

| File | Change |
|------|--------|
| `src-tauri/src/settings.rs` | Add `gemini_api_key`, `gemini_model`, `gemini_prompt` to `AppSettings` + defaults |
| `src-tauri/src/managers/model.rs` | Add `EngineType::Gemini` |
| `src-tauri/src/managers/transcription.rs` | Add `LoadedEngine::Gemini`, dispatch to `gemini_client`, accept `prompt: Option<String>` param |
| `src-tauri/src/actions.rs` | Pass `Some(gemini_prompt)` when `post_process && selected_model == "gemini"`; skip LLM post-process for Gemini |
| `src-tauri/src/commands/` | New commands: `change_gemini_api_key`, `change_gemini_model`, `change_gemini_prompt`, `test_gemini_connection` |
| `src/components/settings/models/ModelsSettings.tsx` | Render `GeminiTranscriptionCard` in cloud providers section |
| `src/i18n/locales/en/translation.json` | New i18n keys for Gemini UI |

### New Frontend Files

- `src/components/settings/models/GeminiTranscriptionCard.tsx`

## Data Flow

```
User: option+shift+space
  ↓
actions.rs
  selected_model == "gemini" → gemini_prompt = Some(settings.gemini_prompt)
  ↓
TranscriptionManager::transcribe(audio: Vec<f32>, prompt: Option<String>)
  ↓  LoadedEngine::Gemini
gemini_client::call_gemini_api(api_key, model, wav_bytes, prompt)
  ↓
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}
  ↓
Parse response.candidates[0].content.parts[0].text
  ↓
Return text → skip LLM post-process → clipboard/paste

User: option+space (no post-process)
  → same flow with prompt = None → plain transcription
```

## Gemini API Request Format

```json
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}
Content-Type: application/json

{
  "system_instruction": {
    "parts": [{ "text": "<gemini_prompt>" }]
  },
  "contents": [{
    "parts": [
      {
        "inline_data": {
          "mime_type": "audio/wav",
          "data": "<base64-encoded WAV bytes>"
        }
      },
      {
        "text": "Transcribe this audio."
      }
    ]
  }]
}
```

Notes:
- `system_instruction` only included when `prompt` is `Some`
- Uses `inline_data` (no Files API) — typical recordings are well under the 20MB request limit
- WAV bytes reuse existing `samples_to_wav_bytes()` from `transcription.rs`
- Auth via query param `?key=` (Google AI Studio key format)

## New Settings Fields

```rust
pub gemini_api_key: String,   // Google AI Studio API key
pub gemini_model: String,     // default: "gemini-2.0-flash"
pub gemini_prompt: String,    // system instruction for transcribe_with_post_process
```

**Default gemini_prompt:**
```
You are a transcription assistant. Transcribe the audio accurately.
Fix capitalization, punctuation, and remove filler words.
Return only the transcription text.
```

## Frontend: GeminiTranscriptionCard

Mirrors `CloudTranscriptionCard` structure:

- **API Key** — password input, links to Google AI Studio
- **Model** — text input, placeholder `gemini-2.0-flash`
- **Prompt** — textarea, shown collapsed under "Advanced", used for `transcribe_with_post_process`
- **Test** button — sends minimal test request to verify key+model
- **Activate** button — enabled when `api_key` + `model` are non-empty; sets `selected_model = "gemini"`

## Error Handling

- Missing API key or model → skip Gemini, log warning
- HTTP error from Gemini API → propagate as transcription error (existing error display)
- Retry: same 3-attempt retry strategy as Cloud transcription (`RETRY_DELAYS_MS: [0, 300, 800]`)

## Out of Scope

- Gemini Live API (real-time streaming) — future work
- Files API (for recordings > 20MB) — not needed for typical use
- Multiple Gemini prompts (prompt management UI) — use single prompt field for now
