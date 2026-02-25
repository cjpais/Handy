# Cloud Providers Refactor — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract Cloud and Gemini transcription backends into a `CloudProvider` trait with shared retry, eliminating duplication and Gemini-specific leaks.

**Architecture:** New `cloud_providers/` module with `CloudProvider` trait. `transcription.rs` dispatches cloud providers via a single `LoadedEngine::Cloud(Box<dyn CloudProvider>)` variant. Each provider reads its own config from `AppSettings`. Shared `with_retry()` wrapper eliminates duplicated retry loops.

**Tech Stack:** Rust, async-trait, reqwest, serde, Tauri 2.x

---

### Task 1: Create `cloud_providers` Module with Trait and Retry Helper

**Files:**
- Create: `src-tauri/src/cloud_providers/mod.rs`

**Step 1: Create the module file**

Create `src-tauri/src/cloud_providers/mod.rs`:

```rust
pub mod gemini;
pub mod openai;

use anyhow::Result;
use log::{debug, warn};
use std::future::Future;
use std::time::Duration;

use crate::settings::AppSettings;

pub const MODEL_ID_CLOUD: &str = "cloud";
pub const MODEL_ID_GEMINI: &str = "gemini";

/// Trait for cloud-based transcription providers (Gemini, OpenAI-compatible, etc.)
#[async_trait::async_trait]
pub trait CloudProvider: Send + Sync {
    /// Transcribe WAV audio bytes.
    /// `post_process` — if true, provider may apply its built-in prompt (e.g. Gemini system_instruction).
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        post_process: bool,
        settings: &AppSettings,
    ) -> Result<String>;

    /// Verify credentials with a minimal request.
    async fn test_connection(&self, settings: &AppSettings) -> Result<()>;

    /// Provider ID matching the model ID constant (e.g. "cloud", "gemini").
    fn id(&self) -> &'static str;
}

const RETRY_DELAYS_MS: &[u64] = &[0, 300, 800];

/// Shared retry wrapper for cloud transcription calls.
/// Retries up to 3 times with exponential backoff [0ms, 300ms, 800ms].
pub async fn with_retry<F, Fut>(label: &str, f: F) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut last_error = anyhow::anyhow!("Unknown {} transcription error", label);

    for (attempt, &delay) in RETRY_DELAYS_MS.iter().enumerate() {
        if delay > 0 {
            debug!(
                "{} transcription retry {}/{}, waiting {}ms",
                label,
                attempt + 1,
                RETRY_DELAYS_MS.len(),
                delay
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        match f().await {
            Ok(text) => return Ok(text),
            Err(e) => {
                warn!(
                    "{} transcription attempt {}/{} failed: {}",
                    label,
                    attempt + 1,
                    RETRY_DELAYS_MS.len(),
                    e
                );
                last_error = e;
            }
        }
    }

    Err(last_error)
}

/// Resolve a model ID to its `CloudProvider` implementation.
/// Returns `None` for local model IDs.
pub fn provider_for_model(model_id: &str) -> Option<Box<dyn CloudProvider>> {
    match model_id {
        MODEL_ID_CLOUD => Some(Box::new(openai::OpenAiProvider)),
        MODEL_ID_GEMINI => Some(Box::new(gemini::GeminiProvider)),
        _ => None,
    }
}
```

**Step 2: Register the module**

Modify `src-tauri/src/lib.rs` — add `mod cloud_providers;` near the top alongside existing `mod` declarations.

**Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: Errors about missing `gemini` and `openai` submodules (that's fine — we create them next).

**Step 4: Commit**

```bash
git add src-tauri/src/cloud_providers/mod.rs src-tauri/src/lib.rs
git commit -m "refactor: add cloud_providers module with trait and retry helper"
```

---

### Task 2: Create OpenAI Provider

**Files:**
- Create: `src-tauri/src/cloud_providers/openai.rs`
- Reference: `src-tauri/src/managers/transcription.rs:71-129` (existing `call_cloud_api`)

**Step 1: Create the provider**

Move the existing `call_cloud_api()` and `parse_extra_params()` logic from `transcription.rs` into a new `OpenAiProvider` struct implementing `CloudProvider`.

Create `src-tauri/src/cloud_providers/openai.rs`:

```rust
use super::{CloudProvider, MODEL_ID_CLOUD};
use anyhow::Result;
use log::{debug, warn};
use serde::Deserialize;

/// Parse a JSON string into a serde_json::Value object, ignoring invalid input.
fn parse_extra_params(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str(trimmed) {
        Ok(v @ serde_json::Value::Object(_)) => Some(v),
        _ => {
            warn!("cloud_transcription_extra_params is not a valid JSON object, ignoring");
            None
        }
    }
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: Option<String>,
}

pub struct OpenAiProvider;

#[async_trait::async_trait]
impl CloudProvider for OpenAiProvider {
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        _post_process: bool,
        settings: &crate::settings::AppSettings,
    ) -> Result<String> {
        use reqwest::multipart;

        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", settings.cloud_transcription_model.clone())
            .text("response_format", "json");

        let language = match settings.selected_language.as_str() {
            "auto" => None,
            lang => Some(lang),
        };
        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }

        // Merge extra_params into form fields
        if let Some(serde_json::Value::Object(map)) =
            parse_extra_params(&settings.cloud_transcription_extra_params)
        {
            for (k, v) in map {
                let val = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                form = form.text(k, val);
            }
        }

        let url = format!(
            "{}/audio/transcriptions",
            settings.cloud_transcription_base_url.trim_end_matches('/')
        );
        let response = reqwest::Client::new()
            .post(&url)
            .bearer_auth(&settings.cloud_transcription_api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Network error: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Cloud API {}: {body}", status.as_u16()));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {e}"))?;

        json["text"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("No 'text' field in API response"))
    }

    async fn test_connection(&self, settings: &crate::settings::AppSettings) -> Result<()> {
        // Minimal silent WAV (44-byte header, 0 samples)
        let silent_wav: Vec<u8> = vec![
            0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00,
            0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74, 0x20,
            0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
            0x80, 0x3E, 0x00, 0x00, 0x00, 0x7D, 0x00, 0x00,
            0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61,
            0x00, 0x00, 0x00, 0x00,
        ];
        self.transcribe(silent_wav, false, settings).await?;
        Ok(())
    }

    fn id(&self) -> &'static str {
        MODEL_ID_CLOUD
    }
}
```

**Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: May still error on missing `gemini` module — that's fine.

**Step 3: Commit**

```bash
git add src-tauri/src/cloud_providers/openai.rs
git commit -m "refactor: extract OpenAI provider into cloud_providers module"
```

---

### Task 3: Create Gemini Provider

**Files:**
- Create: `src-tauri/src/cloud_providers/gemini.rs`
- Reference: `src-tauri/src/gemini_client.rs` (existing implementation)

**Step 1: Create the provider**

Move existing `gemini_client.rs` logic into `GeminiProvider`. Key change: when `post_process=true`, read the Gemini prompt from `settings.post_process_prompts` using `GEMINI_PROMPT_ID` and add it as `system_instruction`.

Create `src-tauri/src/cloud_providers/gemini.rs`:

```rust
use super::{CloudProvider, MODEL_ID_GEMINI};
use anyhow::Result;
use base64::Engine as _;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::settings::{AppSettings, GEMINI_PROMPT_ID};

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ---- Request types ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    contents: Vec<Content>,
}

// ---- Response types ----

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: ContentResponse,
}

#[derive(Debug, Deserialize)]
struct ContentResponse {
    parts: Vec<PartResponse>,
}

#[derive(Debug, Deserialize)]
struct PartResponse {
    text: Option<String>,
}

/// Resolve the Gemini system instruction prompt from settings.
/// Returns `Some` only when `post_process` is true and the prompt is non-empty.
fn resolve_prompt(post_process: bool, settings: &AppSettings) -> Option<String> {
    if !post_process {
        return None;
    }
    settings
        .post_process_prompts
        .iter()
        .find(|p| p.id == GEMINI_PROMPT_ID)
        .map(|p| p.prompt.replace("${output}", "").trim().to_string())
        .filter(|p| !p.is_empty())
}

async fn call_gemini_api(
    api_key: &str,
    model: &str,
    wav_bytes: Vec<u8>,
    prompt: Option<String>,
) -> Result<String> {
    let url = format!("{}/{}:generateContent", GEMINI_BASE_URL, model);

    info!(
        "Gemini API call: model={}, audio_bytes={}, has_prompt={}",
        model,
        wav_bytes.len(),
        prompt.is_some()
    );

    let audio_data = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

    let system_instruction = prompt.map(|p| SystemInstruction {
        parts: vec![Part {
            text: Some(p),
            inline_data: None,
        }],
    });

    let request = GenerateContentRequest {
        system_instruction,
        contents: vec![Content {
            parts: vec![
                Part {
                    text: Some(
                        "Please transcribe this audio file. Provide only the transcribed text, with no introductory phrases, labels, or formatting.".to_string(),
                    ),
                    inline_data: None,
                },
                Part {
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "audio/wav".to_string(),
                        data: audio_data,
                    }),
                },
            ],
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-goog-api-key", api_key)
        .json(&request)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Gemini API request failed: {}", e))?;

    let status = response.status();
    info!("Gemini API response status: {}", status);
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        warn!("Gemini API error body: {}", &body[..body.len().min(500)]);
        return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
    }

    let raw_body = response
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read Gemini response: {}", e))?;

    let parsed: GenerateContentResponse = serde_json::from_str(&raw_body).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse Gemini response: {}. Body: {}",
            e,
            &raw_body[..raw_body.len().min(300)]
        )
    })?;

    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .and_then(|p| p.text)
        .unwrap_or_default();

    info!("Gemini API returned {} chars", text.len());
    Ok(text)
}

pub struct GeminiProvider;

#[async_trait::async_trait]
impl CloudProvider for GeminiProvider {
    async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        post_process: bool,
        settings: &AppSettings,
    ) -> Result<String> {
        let prompt = resolve_prompt(post_process, settings);
        call_gemini_api(&settings.gemini_api_key, &settings.gemini_model, wav_bytes, prompt).await
    }

    async fn test_connection(&self, settings: &AppSettings) -> Result<()> {
        // Minimal valid 16kHz mono WAV with 0 samples (44-byte header only)
        let silent_wav: Vec<u8> = vec![
            0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00,
            0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74, 0x20,
            0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
            0x80, 0x3E, 0x00, 0x00, 0x00, 0x7D, 0x00, 0x00,
            0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61,
            0x00, 0x00, 0x00, 0x00,
        ];
        call_gemini_api(&settings.gemini_api_key, &settings.gemini_model, silent_wav, None).await?;
        Ok(())
    }

    fn id(&self) -> &'static str {
        MODEL_ID_GEMINI
    }
}
```

**Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: Clean compile for the `cloud_providers` module (may still have warnings from old code).

**Step 3: Commit**

```bash
git add src-tauri/src/cloud_providers/gemini.rs
git commit -m "refactor: extract Gemini provider into cloud_providers module"
```

---

### Task 4: Wire `LoadedEngine::Cloud` into `transcription.rs`

**Files:**
- Modify: `src-tauri/src/managers/transcription.rs`

**Step 1: Update `LoadedEngine` enum**

Replace `Cloud` and `Gemini` variants with a single:

```rust
Cloud(Box<dyn crate::cloud_providers::CloudProvider>),
```

Remove: `LoadedEngine::Cloud` and `LoadedEngine::Gemini`.

**Step 2: Update `transcribe()` signature**

Change from:
```rust
pub fn transcribe(&self, audio: Vec<f32>, prompt: Option<String>) -> Result<String>
```
to:
```rust
pub fn transcribe(&self, audio: Vec<f32>, post_process: bool) -> Result<String>
```

**Step 3: Replace Cloud and Gemini dispatch blocks**

Remove both `LoadedEngine::Cloud => { ... }` (lines ~635-692) and `LoadedEngine::Gemini => { ... }` (lines ~693-742) blocks. Replace with a single:

```rust
LoadedEngine::Cloud(ref provider) => {
    let wav = samples_to_wav_bytes(&audio)?;
    let provider_id = provider.id().to_string();
    let settings_clone = settings.clone();
    tokio::task::block_in_place(|| {
        tauri::async_runtime::block_on(async {
            crate::cloud_providers::with_retry(&provider_id, || {
                // provider is behind &, but we need to call async method.
                // Clone wav per attempt for retry.
                let wav_clone = wav.clone();
                let settings_ref = &settings_clone;
                async move {
                    // Reborrow provider inside the async block
                    // We can't move provider into the closure since it's behind a ref.
                    // Use the API directly.
                    provider.transcribe(wav_clone, post_process, settings_ref).await
                }
            })
            .await
        })
    })
    .map(|text| transcribe_rs::TranscriptionResult {
        text,
        segments: None,
    })?
}
```

Note: The exact retry closure may need adjustment since `provider` is behind `&mut engine`. The implementer should handle the borrow correctly — the key idea is `with_retry` wraps the call, and the single `LoadedEngine::Cloud(provider)` arm handles all cloud providers.

**Step 4: Remove old helper functions from `transcription.rs`**

Remove:
- `parse_extra_params()` function (moved to `openai.rs`)
- `call_cloud_api()` function (moved to `openai.rs`)

Keep:
- `samples_to_wav_bytes()` (still needed — called before passing to provider)

**Step 5: Update `unload_model()` match arm**

Replace:
```rust
LoadedEngine::Cloud => { /* nothing to unload */ }
LoadedEngine::Gemini => { /* nothing to unload */ }
```
With:
```rust
LoadedEngine::Cloud(_) => { /* nothing to unload */ }
```

**Step 6: Update `load_model()` match arm**

Replace:
```rust
EngineType::Cloud => LoadedEngine::Cloud,
EngineType::Gemini => LoadedEngine::Gemini,
```
With:
```rust
EngineType::Cloud | EngineType::Gemini => {
    match crate::cloud_providers::provider_for_model(model_id) {
        Some(provider) => LoadedEngine::Cloud(provider),
        None => return Err(anyhow::anyhow!("Unknown cloud provider: {}", model_id)),
    }
}
```

**Step 7: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -30`

**Step 8: Commit**

```bash
git add src-tauri/src/managers/transcription.rs
git commit -m "refactor: wire CloudProvider trait into TranscriptionManager"
```

---

### Task 5: Update `actions.rs` — Remove Gemini Prompt Logic, Simplify Call

**Files:**
- Modify: `src-tauri/src/actions.rs`

**Step 1: Remove gemini_prompt computation**

Remove lines 424-433 (the `let gemini_prompt = if settings.selected_model == "gemini" { ... }` block).

**Step 2: Update transcribe call**

Change:
```rust
match tm.transcribe(samples, gemini_prompt) {
```
to:
```rust
match tm.transcribe(samples, post_process) {
```

**Step 3: Simplify post-processing guard**

Change:
```rust
let processed = if post_process && settings.selected_model != "gemini" {
    post_process_transcription(&settings, &final_text).await
} else {
    None
};
```
to:
```rust
let processed = if post_process && crate::cloud_providers::provider_for_model(&settings.selected_model).is_none() {
    post_process_transcription(&settings, &final_text).await
} else {
    None
};
```

This way, ANY cloud provider that handles its own post-processing is automatically excluded. No need to hardcode `"gemini"`.

Alternatively, simpler: use `MODEL_ID_GEMINI` constant instead of the string:
```rust
use crate::cloud_providers::MODEL_ID_GEMINI;
let processed = if post_process && settings.selected_model != MODEL_ID_GEMINI {
    post_process_transcription(&settings, &final_text).await
} else {
    None
};
```

Prefer the constant approach — simpler and explicit.

**Step 4: Replace magic strings in fallback logic**

Replace all `"gemini"` and `"cloud"` string literals in the error-handling block (lines 532-591) with `MODEL_ID_GEMINI` and `MODEL_ID_CLOUD` constants:

```rust
use crate::cloud_providers::{MODEL_ID_CLOUD, MODEL_ID_GEMINI};

if settings.selected_model == MODEL_ID_GEMINI {
    let fallback = mm.get_available_models().into_iter().find(|m| {
        m.is_downloaded && m.id != MODEL_ID_CLOUD && m.id != MODEL_ID_GEMINI && !m.is_custom
    });
    // ... rest of fallback logic
} else if settings.selected_model == MODEL_ID_CLOUD {
    // ... cloud failure handling
}
```

**Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -30`

**Step 6: Commit**

```bash
git add src-tauri/src/actions.rs
git commit -m "refactor: simplify actions.rs with CloudProvider constants"
```

---

### Task 6: Update `tray.rs` — Replace Magic Strings

**Files:**
- Modify: `src-tauri/src/tray.rs:162`

**Step 1: Replace magic string**

Change:
```rust
if settings.selected_model == "cloud" {
```
to:
```rust
use crate::cloud_providers::MODEL_ID_CLOUD;
// Hide "Unload Model" for cloud providers — nothing to unload
if crate::cloud_providers::provider_for_model(&settings.selected_model).is_some() {
```

This makes the tray menu work correctly for any cloud provider (Cloud, Gemini, future ones) — all of them should hide the "Unload Model" menu item.

**Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`

**Step 3: Commit**

```bash
git add src-tauri/src/tray.rs
git commit -m "refactor: use cloud provider check in tray menu"
```

---

### Task 7: Update Commands — Wire Through `CloudProvider`

**Files:**
- Modify: `src-tauri/src/shortcut/mod.rs:1163-1180`
- Modify: `src-tauri/src/lib.rs:308`

**Step 1: Remove `change_gemini_prompt` command**

Delete the `change_gemini_prompt` function from `src-tauri/src/shortcut/mod.rs` (lines 1161-1168).

Remove it from the command registration in `src-tauri/src/lib.rs:308`:
```rust
// Remove this line:
shortcut::change_gemini_prompt,
```

**Step 2: Update `test_gemini_connection` to use provider**

Change `test_gemini_connection` in `shortcut/mod.rs`:

```rust
#[tauri::command]
#[specta::specta]
pub async fn test_gemini_connection(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    let provider = crate::cloud_providers::gemini::GeminiProvider;
    provider
        .test_connection(&settings)
        .await
        .map_err(|e| e.to_string())
}
```

Similarly update `test_cloud_transcription_connection` to use `OpenAiProvider`:

```rust
#[tauri::command]
#[specta::specta]
pub async fn test_cloud_transcription_connection(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    let provider = crate::cloud_providers::openai::OpenAiProvider;
    provider
        .test_connection(&settings)
        .await
        .map_err(|e| e.to_string())
}
```

**Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`

**Step 4: Commit**

```bash
git add src-tauri/src/shortcut/mod.rs src-tauri/src/lib.rs
git commit -m "refactor: wire test commands through CloudProvider trait"
```

---

### Task 8: Clean Up Settings — Remove `gemini_prompt` Field

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Step 1: Remove `gemini_prompt` field and default function**

In `AppSettings` struct, remove:
```rust
#[serde(default = "default_gemini_prompt")]
pub gemini_prompt: String,
```

Remove the `default_gemini_prompt()` function (line 608-610).

Remove `gemini_prompt: default_gemini_prompt(),` from `get_default_settings()` (line 785).

**Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -30`
Expected: Clean compile (the `change_gemini_prompt` command was already removed in Task 7).

**Step 3: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "refactor: remove redundant gemini_prompt setting field"
```

---

### Task 9: Delete Old `gemini_client.rs`

**Files:**
- Delete: `src-tauri/src/gemini_client.rs`
- Modify: `src-tauri/src/lib.rs` — remove `mod gemini_client;`

**Step 1: Remove the module declaration**

In `src-tauri/src/lib.rs`, remove the `mod gemini_client;` line.

**Step 2: Delete the file**

```bash
trash src-tauri/src/gemini_client.rs
```

**Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: Clean compile. All `crate::gemini_client::` references should already be gone (replaced in Tasks 3 and 7).

**Step 4: Commit**

```bash
git add -u src-tauri/src/gemini_client.rs src-tauri/src/lib.rs
git commit -m "refactor: delete old gemini_client.rs (moved to cloud_providers)"
```

---

### Task 10: Replace Frontend Magic Strings with Constants

**Files:**
- Modify: `src/components/settings/models/ModelsSettings.tsx:157,369,373`
- Modify: `src/components/settings/models/GeminiTranscriptionCard.tsx:207`
- Modify: `src/components/settings/models/CloudTranscriptionCard.tsx:256`
- Modify: `src/components/settings/post-processing/PostProcessingSettings.tsx:147,154,444`
- Modify: `src/components/model-selector/ModelDropdown.tsx:23`

**Step 1: Create constants file**

Check if `MODEL_ID_CLOUD` and `MODEL_ID_GEMINI` are already exported via `bindings.ts` (specta auto-generation). If not, create a small constants file:

Create `src/lib/constants/models.ts` if not generated:
```typescript
export const MODEL_ID_CLOUD = "cloud";
export const MODEL_ID_GEMINI = "gemini";
```

**Step 2: Replace hardcoded strings in each file**

Replace all `"cloud"` and `"gemini"` model ID string literals with the constants. For example in `ModelsSettings.tsx`:

```typescript
import { MODEL_ID_CLOUD, MODEL_ID_GEMINI } from "@/lib/constants/models";

// line 157
if (model.id === MODEL_ID_CLOUD || model.id === MODEL_ID_GEMINI) return false;

// line 369
isActive={currentModel === MODEL_ID_CLOUD}

// line 373
isActive={currentModel === MODEL_ID_GEMINI}
```

Apply same pattern to all other files listed above.

**Step 3: Verify frontend builds**

Run: `bun run lint && bun run format:check`

**Step 4: Commit**

```bash
git add src/lib/constants/models.ts src/components/
git commit -m "refactor: replace frontend magic strings with model ID constants"
```

---

### Task 11: Regenerate Bindings and Full Build Verification

**Step 1: Regenerate Tauri type bindings**

The removal of `change_gemini_prompt` command will affect `bindings.ts`. Run:

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

Wait for it to start, then check if `bindings.ts` was regenerated. If the `changeGeminiPrompt` function is still referenced in frontend code, remove those references (GeminiTranscriptionCard doesn't use it, so this should be clean).

**Step 2: Run full lint + format gate**

```bash
bun run lint && bun run format:check && cd src-tauri && cargo clippy -- -D warnings && cargo fmt --check
```

Fix any issues.

**Step 3: Commit any fixups**

```bash
git add -A
git commit -m "chore: regenerate bindings and fix lint"
```

---

### Task 12: Model Registry Constants

**Files:**
- Modify: `src-tauri/src/managers/model.rs:402-445`

**Step 1: Replace hardcoded model ID strings in model registry**

In the `ModelManager::new()` where models are registered, replace:

```rust
available_models.insert(
    "cloud".to_string(),
    ModelInfo {
        id: "cloud".to_string(),
```

with:

```rust
use crate::cloud_providers::{MODEL_ID_CLOUD, MODEL_ID_GEMINI};

available_models.insert(
    MODEL_ID_CLOUD.to_string(),
    ModelInfo {
        id: MODEL_ID_CLOUD.to_string(),
```

Same for the Gemini entry.

**Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`

**Step 3: Commit**

```bash
git add src-tauri/src/managers/model.rs
git commit -m "refactor: use MODEL_ID constants in model registry"
```

---

### Task 13: Final Full Gate

**Step 1: Run full verification**

```bash
cd src-tauri && cargo clippy -- -D warnings && cargo fmt --check
cd .. && bun run lint && bun run format:check
```

**Step 2: Manual smoke test**

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

Verify:
- Settings → Models page loads, shows Cloud and Gemini cards
- Cloud card: can configure and test
- Gemini card: can configure and test
- Select Gemini, record audio → transcription works
- Select Cloud (if server available) → transcription works
- Post-processing settings page shows correct Gemini notice when Gemini is active

**Step 3: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore: final cleanup after cloud providers refactor"
```
