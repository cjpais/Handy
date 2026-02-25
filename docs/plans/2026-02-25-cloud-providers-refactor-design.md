# Cloud Providers Refactor — Design

**Date:** 2026-02-25
**Status:** Approved
**Branch:** feat/local-server-transcription

## Summary

Refactor the Cloud and Gemini transcription backends into a clean `CloudProvider` trait with shared infrastructure. Eliminates duplicated retry logic, removes Gemini-specific concerns from the general transcription interface, and makes it trivial to add future cloud providers.

## Problem

Current implementation has:
- Duplicated 3-attempt retry blocks for Cloud and Gemini in `transcription.rs`
- `prompt: Option<String>` on `transcribe()` leaking Gemini-only concern into the general interface
- Redundant `settings.gemini_prompt` field (actual prompt read from `post_process_prompts[GEMINI_PROMPT_ID]`)
- Magic strings `"cloud"` / `"gemini"` scattered across backend and frontend
- Cloud providers pretending to be local models in `LoadedEngine` enum with no-op load/unload

## Architecture

### New Module: `cloud_providers/`

```
src-tauri/src/cloud_providers/
├── mod.rs          — CloudProvider trait, with_retry(), constants
├── openai.rs       — OpenAI-compatible (/audio/transcriptions)
└── gemini.rs       — Google Gemini (generateContent)
```

### CloudProvider Trait

```rust
pub const MODEL_ID_CLOUD: &str = "cloud";
pub const MODEL_ID_GEMINI: &str = "gemini";

#[async_trait]
pub trait CloudProvider: Send + Sync {
    /// Transcribe WAV audio.
    /// post_process=true: provider MAY apply its built-in prompt
    ///   (e.g. Gemini sends system_instruction)
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        post_process: bool,
        settings: &AppSettings,
    ) -> Result<String>;

    /// Verify credentials with minimal request.
    async fn test_connection(&self, settings: &AppSettings) -> Result<()>;

    /// Provider ID matching model ID constant.
    fn id(&self) -> &'static str;
}
```

### Shared Retry Wrapper

```rust
pub async fn with_retry<F, Fut>(label: &str, f: F) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    const RETRY_DELAYS_MS: &[u64] = &[0, 300, 800];
    // Shared 3-attempt retry with exponential backoff
}
```

Replaces identical retry blocks in both Cloud and Gemini transcription paths.

## Provider Implementations

### GeminiProvider

- Reads `gemini_api_key` and `gemini_model` from settings
- When `post_process=true`: reads prompt from `post_process_prompts[GEMINI_PROMPT_ID]`, sends as `system_instruction`
- When `post_process=false`: plain transcription, no system instruction
- `test_connection()`: sends minimal silent WAV (existing logic from `gemini_client.rs`)

### OpenAiProvider

- Reads `cloud_transcription_base_url`, `cloud_transcription_api_key`, `cloud_transcription_model` from settings
- Ignores `post_process` flag — LLM post-processing handled separately in `actions.rs`
- `test_connection()`: sends minimal silent WAV to `/audio/transcriptions`

## Modified Files

### transcription.rs

```rust
// Before
enum LoadedEngine { ..., Cloud, Gemini }
fn transcribe(&self, audio: Vec<f32>, prompt: Option<String>) -> Result<String>

// After
enum LoadedEngine { ..., Cloud(Box<dyn CloudProvider>) }
fn transcribe(&self, audio: Vec<f32>, post_process: bool) -> Result<String>
```

Cloud dispatch reduces from ~100 lines (two separate branches) to ~5 lines:
```rust
LoadedEngine::Cloud(provider) => {
    let wav = samples_to_wav_bytes(&audio)?;
    tokio::task::block_in_place(|| {
        tauri::async_runtime::block_on(
            with_retry(provider.id(), || provider.transcribe(wav.clone(), post_process, &settings))
        )
    })
}
```

### actions.rs

- Remove `gemini_prompt` computation block (lines 424-433)
- Simplify call: `tm.transcribe(samples, post_process)`
- Remove `if settings.selected_model != "gemini"` guard — handled internally by providers
- Extract fallback logic into `try_fallback_local_transcription()` helper

### settings.rs

**Remove:**
- `gemini_prompt: String` field from `AppSettings`
- `default_gemini_prompt()` function

**Keep:**
- `gemini_api_key`, `gemini_model` — still needed for GeminiProvider
- `GEMINI_PROMPT_ID` prompt in `post_process_prompts` as single source of truth

### model.rs

- `EngineType::Cloud` and `EngineType::Gemini` stay (used for model registry/UI)
- Model constants `MODEL_ID_CLOUD`, `MODEL_ID_GEMINI` imported from `cloud_providers`

### Deleted Files

- `gemini_client.rs` — logic moves into `cloud_providers/gemini.rs`

### Constants

All occurrences of `"cloud"` / `"gemini"` as model IDs replaced with `MODEL_ID_CLOUD` / `MODEL_ID_GEMINI`:
- `model.rs`, `actions.rs`, `transcription.rs`, `tray.rs`
- Frontend: constants file or exported via bindings

## What Stays the Same

- Frontend UI components — no changes needed
- `EngineType` enum — Cloud and Gemini variants remain for model registry
- Post-processing flow in `actions.rs` — still a separate LLM call for non-Gemini providers
- Gemini fallback-to-local logic — stays in `actions.rs`, just cleaner

## Data Flow (After)

```
User: option+space (transcribe)
  → actions.rs: tm.transcribe(samples, post_process=false)
  → TranscriptionManager: LoadedEngine::Cloud(provider)
  → cloud_providers::with_retry(|| provider.transcribe(wav, false, settings))
  → GeminiProvider: POST generateContent (no system_instruction)
  → Return text → clipboard/paste

User: option+shift+space (transcribe + post-process)
  → actions.rs: tm.transcribe(samples, post_process=true)
  → TranscriptionManager: LoadedEngine::Cloud(provider)
  → cloud_providers::with_retry(|| provider.transcribe(wav, true, settings))
  → GeminiProvider: POST generateContent WITH system_instruction from GEMINI_PROMPT_ID
  → Return text → skip LLM post-process → clipboard/paste
```
