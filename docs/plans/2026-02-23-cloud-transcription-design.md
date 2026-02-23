# Cloud Transcription Design

**Date:** 2026-02-23
**Status:** Approved

## Summary

Add a single configurable "Cloud Transcription" entry to the models list that sends audio to any OpenAI-compatible `/audio/transcriptions` endpoint (e.g. Groq, OpenAI, custom). The entry appears after all local models and has inline settings for Base URL, API key, and model name.

---

## Section 1 — Settings & Model Entry

### New AppSettings fields

```rust
#[serde(default = "default_cloud_transcription_base_url")]
pub cloud_transcription_base_url: String,  // "https://api.groq.com/openai/v1"

#[serde(default)]
pub cloud_transcription_api_key: String,   // ""

#[serde(default = "default_cloud_transcription_model")]
pub cloud_transcription_model: String,     // "whisper-large-v3"
```

### ModelManager entry

Hardcoded at the end of the model list:

```
id:           "cloud"
name:         "Cloud Transcription"
engine_type:  EngineType::Cloud
is_downloaded: true  (always, no download needed)
size_mb:      0
url:          None
filename:     ""
is_recommended: false
is_custom:    false
```

No download flow; `is_downloaded = true` always.

---

## Section 2 — Cloud Engine

### New variants

```rust
// model.rs
pub enum EngineType {
    Whisper, Parakeet, Moonshine, MoonshineStreaming, SenseVoice,
    Cloud,  // new
}

// transcription.rs
enum LoadedEngine {
    Whisper(..), Parakeet(..), Moonshine(..), MoonshineStreaming(..), SenseVoice(..),
    Cloud { base_url: String, api_key: String, model_name: String },  // new
}
```

### load_model("cloud")

Reads `cloud_transcription_*` from `AppSettings`, creates `LoadedEngine::Cloud { ... }`, emits `loading_completed` immediately. No file I/O, no model download.

### transcribe() for Cloud branch

1. Encode `Vec<f32>` (16 kHz mono) → WAV bytes via existing `save_wav_file` util
2. Build `reqwest` multipart form:
   - `file`: WAV bytes, filename `audio.wav`, content-type `audio/wav`
   - `model`: model_name
   - `language`: if `selected_language != "auto"`, pass it
   - `response_format`: `json`
3. POST to `{base_url}/audio/transcriptions` with `Authorization: Bearer {api_key}`
4. Parse `{"text": "..."}` response
5. Apply `custom_words` + `filter_transcription_output` (same as local engines)

**Cargo.toml change:** add `multipart` to reqwest features.

---

## Section 3 — Retry, Notifications, History

### Retry logic

3 attempts with increasing delays (300ms → 800ms → 2000ms) in the Cloud branch of `transcribe()`. Uses `thread::sleep` (transcription already runs in a background thread).

### On all retries exhausted

- Save history entry: `transcription_text = ""`, `cloud_pending = true` (new DB column)
- Emit Tauri native notification: _"Cloud transcription failed. Recording saved — retry from History."_
- Return `Err` (caller handles UI error state as usual)

### DB migration (M4)

```sql
ALTER TABLE transcription_history ADD COLUMN cloud_pending BOOLEAN NOT NULL DEFAULT 0;
```

### Retry from History — new Tauri command

```rust
retranscribe_history_entry(id: i64) -> Result<HistoryEntry>
```

1. Load WAV from `recordings_dir / entry.file_name`
2. Decode WAV → `Vec<f32>`
3. Call `transcription_manager.transcribe(audio)`
4. On success: update DB row (`transcription_text`, `cloud_pending = false`), emit `history-updated`
5. On failure: retry same 3-attempt logic, keep `cloud_pending = true`

---

## Section 4 — Frontend

### Models list (ModelsSettings.tsx / model-selector)

Cloud card appears last. Shows ☁️ icon + "Cloud Transcription" name + no size badge.

When selected (or via "Configure" expand button), inline settings appear:

```
Base URL   [https://api.groq.com/openai/v1      ]
API Key    [••••••••••••••••••••••••••••••••••••]
Model      [whisper-large-v3                    ]
```

Each field saves via existing settings commands on `onBlur`. No new settings page needed.

### History (history components)

Entries with `cloud_pending = true`:
- Show placeholder text instead of transcription: _"Transcription failed — tap Retry"_
- Show "Retry" button → calls `retranscribe_history_entry(id)`
- On success: replace placeholder with transcription text
- On in-progress: show spinner on the button

---

## Files to touch

| File | Change |
|------|--------|
| `src-tauri/src/settings.rs` | 3 new AppSettings fields + defaults |
| `src-tauri/src/managers/model.rs` | `EngineType::Cloud`, cloud ModelInfo entry |
| `src-tauri/src/managers/transcription.rs` | `LoadedEngine::Cloud`, load/unload/transcribe impl |
| `src-tauri/src/managers/history.rs` | `cloud_pending` field + migration M4 |
| `src-tauri/src/commands/transcription.rs` | `retranscribe_history_entry` command |
| `src-tauri/Cargo.toml` | add `multipart` to reqwest features |
| `src/components/settings/models/ModelsSettings.tsx` | inline cloud config UI |
| `src/components/history/` | Retry button for pending entries |
| `src/i18n/locales/en/translation.json` | new i18n keys |
| `src/bindings.ts` | regenerated (tauri-specta) |

---

## Non-goals

- Multiple cloud providers in transcription (one configurable entry only)
- Automatic fallback to local model on cloud failure
- Streaming transcription from cloud
