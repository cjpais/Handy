# Gemini Transcription Provider Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Google Gemini as a transcription provider that combines transcription + post-processing in a single API call by sending audio directly to Gemini's `generateContent` REST API.

**Architecture:** New `LoadedEngine::Gemini` variant in `transcription.rs` — mirrors the `LoadedEngine::Cloud` pattern. Gemini is selected via `selected_model = "gemini"`. When `transcribe_with_post_process` shortcut fires, the configured `gemini_prompt` is sent as `system_instruction` alongside the audio inline_data in a single HTTP request.

**Tech Stack:** Rust (reqwest, serde_json, base64), React/TypeScript (Tailwind, tauri-specta bindings), Google Gemini REST API v1beta

**Design doc:** `docs/plans/2026-02-24-gemini-transcription-design.md`

---

### Task 1: Settings — add Gemini fields

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Step 1: Add fields to `AppSettings` struct**

In `settings.rs`, after `cloud_transcription_extra_params: String,` (around line 371), add:

```rust
#[serde(default)]
pub gemini_api_key: String,
#[serde(default = "default_gemini_model")]
pub gemini_model: String,
#[serde(default = "default_gemini_prompt")]
pub gemini_prompt: String,
```

**Step 2: Add default functions** (after `default_cloud_transcription_model`)

```rust
fn default_gemini_model() -> String {
    "gemini-2.0-flash".to_string()
}

fn default_gemini_prompt() -> String {
    "You are a transcription assistant. Transcribe the audio accurately. Fix capitalization, punctuation, and remove filler words (um, uh, like). Return only the transcription text, nothing else.".to_string()
}
```

**Step 3: Initialize fields in `get_default_settings()`** (around line 743, after `cloud_transcription_extra_params`):

```rust
gemini_api_key: String::new(),
gemini_model: default_gemini_model(),
gemini_prompt: default_gemini_prompt(),
```

**Step 4: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```
Expected: no errors related to AppSettings

**Step 5: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat: add Gemini settings fields to AppSettings"
```

---

### Task 2: Model Manager — add Gemini engine type and model entry

**Files:**
- Modify: `src-tauri/src/managers/model.rs`

**Step 1: Add `Gemini` to `EngineType` enum** (around line 20)

```rust
pub enum EngineType {
    Whisper,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    Cloud,
    Gemini,  // Add this
}
```

**Step 2: Add Gemini `ModelInfo` entry** in `ModelManager::new()`, after the `"cloud"` entry (around line 422):

```rust
available_models.insert(
    "gemini".to_string(),
    ModelInfo {
        id: "gemini".to_string(),
        name: "Google Gemini".to_string(),
        description: "Transcribe with Google Gemini AI (transcription + post-processing in one step)".to_string(),
        filename: String::new(),
        url: None,
        size_mb: 0,
        is_downloaded: true,
        is_downloading: false,
        partial_size: 0,
        is_directory: false,
        engine_type: EngineType::Gemini,
        accuracy_score: 0.95,
        speed_score: 0.75,
        supports_translation: false,
        is_recommended: false,
        supported_languages: vec![], // Gemini supports all — no badge needed
        is_custom: false,
    },
);
```

**Step 3: Handle Gemini in `is_cloud_engine` / filtering** — search for `EngineType::Cloud` usage in model.rs and add matching `EngineType::Gemini` cases where needed:

Look for line ~491:
```rust
if matches!(model.engine_type, EngineType::Cloud) {
```
Change to:
```rust
if matches!(model.engine_type, EngineType::Cloud | EngineType::Gemini) {
```
(This ensures Gemini isn't treated as a downloadable model)

**Step 4: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```

**Step 5: Commit**

```bash
git add src-tauri/src/managers/model.rs
git commit -m "feat: add EngineType::Gemini and Gemini model entry"
```

---

### Task 3: gemini_client.rs — HTTP client for Gemini API

**Files:**
- Create: `src-tauri/src/gemini_client.rs`

**Step 1: Create the module**

```rust
use base64::Engine as _;
use log::debug;
use serde::{Deserialize, Serialize};

const GEMINI_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models";

// ---- Request types ----

#[derive(Debug, Serialize)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
}

#[derive(Debug, Serialize)]
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

/// Call Gemini generateContent API with audio bytes.
/// - `prompt`: if Some, used as system_instruction (transcribe_with_post_process mode)
/// - `prompt`: if None, plain transcription (basic transcribe mode)
pub async fn call_gemini_api(
    api_key: &str,
    model: &str,
    wav_bytes: Vec<u8>,
    prompt: Option<String>,
) -> anyhow::Result<String> {
    let url = format!(
        "{}/{}:generateContent?key={}",
        GEMINI_BASE_URL, model, api_key
    );

    debug!("Calling Gemini API: model={}", model);

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
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "audio/wav".to_string(),
                        data: audio_data,
                    }),
                },
                Part {
                    text: Some("Transcribe this audio.".to_string()),
                    inline_data: None,
                },
            ],
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Gemini API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
    }

    let parsed: GenerateContentResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Gemini response: {}", e))?;

    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .and_then(|p| p.text)
        .unwrap_or_default();

    debug!("Gemini API returned {} chars", text.len());
    Ok(text)
}

/// Send a minimal test request to verify API key + model.
/// Uses a tiny silent WAV (44 bytes header + 0 samples).
pub async fn test_gemini_connection(api_key: &str, model: &str) -> anyhow::Result<()> {
    // Minimal valid 16kHz mono WAV with 0 samples (44-byte header only)
    let silent_wav: Vec<u8> = vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0x24, 0x00, 0x00, 0x00, // chunk size = 36
        0x57, 0x41, 0x56, 0x45, // "WAVE"
        0x66, 0x6D, 0x74, 0x20, // "fmt "
        0x10, 0x00, 0x00, 0x00, // subchunk1 size = 16
        0x01, 0x00,             // PCM
        0x01, 0x00,             // 1 channel
        0x80, 0x3E, 0x00, 0x00, // 16000 Hz
        0x00, 0x7D, 0x00, 0x00, // byte rate
        0x02, 0x00,             // block align
        0x10, 0x00,             // bits per sample = 16
        0x64, 0x61, 0x74, 0x61, // "data"
        0x00, 0x00, 0x00, 0x00, // data size = 0
    ];

    call_gemini_api(api_key, model, silent_wav, None).await?;
    Ok(())
}
```

**Step 2: Register the `base64` crate** — check if it's already a dependency:

```bash
grep "base64" src-tauri/Cargo.toml
```

If not present, add to `src-tauri/Cargo.toml`:
```toml
base64 = "0.22"
```

**Step 3: Register module in `lib.rs`** — add after `mod llm_client;`:

```rust
mod gemini_client;
```

**Step 4: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -40
```

**Step 5: Commit**

```bash
git add src-tauri/src/gemini_client.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat: add gemini_client module with generateContent API"
```

---

### Task 4: TranscriptionManager — add Gemini engine and prompt param

**Files:**
- Modify: `src-tauri/src/managers/transcription.rs`

**Step 1: Add `Gemini` to `LoadedEngine` enum** (around line 146)

```rust
enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
    Moonshine(MoonshineEngine),
    MoonshineStreaming(MoonshineStreamingEngine),
    SenseVoice(SenseVoiceEngine),
    Cloud,
    Gemini,  // Add this
}
```

**Step 2: Handle `EngineType::Gemini` in `load_model()`** — find the match arm for `EngineType::Cloud => LoadedEngine::Cloud,` (around line 451) and add after it:

```rust
EngineType::Gemini => LoadedEngine::Gemini,
```

**Step 3: Handle unloading Gemini** — find the unload match that has `LoadedEngine::Cloud => { /* nothing to unload */ }` (around line 269) and add the same for Gemini:

```rust
LoadedEngine::Gemini => { /* nothing to unload */ }
```

**Step 4: Add `prompt` parameter to `transcribe()`** — change signature at line ~509:

```rust
pub fn transcribe(&self, audio: Vec<f32>, prompt: Option<String>) -> Result<String> {
```

**Step 5: Add Gemini match arm in the `transcribe()` dispatch** — in the `match &mut engine` block, after `LoadedEngine::Cloud => { ... }` block (after line 689), add:

```rust
LoadedEngine::Gemini => {
    let wav = samples_to_wav_bytes(&audio)?;

    const RETRY_DELAYS_MS: &[u64] = &[0, 300, 800];
    let mut last_error = anyhow::anyhow!("Unknown Gemini transcription error");

    for (attempt, &delay) in RETRY_DELAYS_MS.iter().enumerate() {
        if delay > 0 {
            debug!(
                "Gemini transcription retry {}/{}, waiting {}ms",
                attempt + 1,
                RETRY_DELAYS_MS.len(),
                delay
            );
            thread::sleep(Duration::from_millis(delay));
        }

        let api_result = tokio::task::block_in_place(|| {
            tauri::async_runtime::block_on(crate::gemini_client::call_gemini_api(
                &settings.gemini_api_key,
                &settings.gemini_model,
                wav.clone(),
                prompt.clone(),
            ))
        });

        match api_result {
            Ok(text) => {
                return Ok(transcribe_rs::TranscriptionResult {
                    text,
                    segments: None,
                });
            }
            Err(e) => {
                warn!(
                    "Gemini transcription attempt {}/{} failed: {}",
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
```

**Step 6: Fix all `tm.transcribe(samples)` call sites** — there is one in `actions.rs`. Search for all callers:

```bash
grep -rn "\.transcribe(" src-tauri/src/
```

Update each call to pass `None` as the second argument (will be updated properly in Task 6).

**Step 7: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -40
```

**Step 8: Commit**

```bash
git add src-tauri/src/managers/transcription.rs
git commit -m "feat: add LoadedEngine::Gemini with prompt support in transcribe()"
```

---

### Task 5: Commands — Gemini settings commands

**Files:**
- Modify: `src-tauri/src/shortcut/mod.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Add Gemini commands to `shortcut/mod.rs`** — after the `change_cloud_transcription_extra_params` function (around line 1141):

```rust
#[tauri::command]
#[specta::specta]
pub fn change_gemini_api_key(app: AppHandle, api_key: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.gemini_api_key = api_key;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_gemini_model(app: AppHandle, model: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.gemini_model = model;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_gemini_prompt(app: AppHandle, prompt: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.gemini_prompt = prompt;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn test_gemini_connection(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    crate::gemini_client::test_gemini_connection(
        &settings.gemini_api_key,
        &settings.gemini_model,
    )
    .await
    .map_err(|e| e.to_string())
}
```

**Step 2: Register commands in `lib.rs`** — in `collect_commands![]` (around line 304), after `shortcut::change_cloud_transcription_extra_params,` add:

```rust
shortcut::change_gemini_api_key,
shortcut::change_gemini_model,
shortcut::change_gemini_prompt,
shortcut::test_gemini_connection,
```

**Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -40
```

**Step 4: Regenerate TypeScript bindings** (tauri-specta auto-generates `src/bindings.ts`):

```bash
cd src-tauri && cargo test export_bindings 2>&1 | tail -5
```

Or run the dev build briefly to trigger specta export:
```bash
bun run tauri dev --no-watch 2>&1 | head -5  # just to trigger export, then Ctrl+C
```

Check that `src/bindings.ts` now has `changeGeminiApiKey`, `changeGeminiModel`, `changeGeminiPrompt`, `testGeminiConnection`.

**Step 5: Commit**

```bash
git add src-tauri/src/shortcut/mod.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: add Gemini settings commands and regenerate bindings"
```

---

### Task 6: actions.rs — wire Gemini prompt into transcription

**Files:**
- Modify: `src-tauri/src/actions.rs`

**Step 1: Pass prompt to `tm.transcribe()`** — find the `tm.transcribe(samples)` call (around line 422) and replace with:

```rust
let transcription_time = Instant::now();
let samples_clone = samples.clone();

// For Gemini: pass prompt when post_process is true (combined transcription+processing)
let settings = get_settings(&ah);
let gemini_prompt = if post_process && settings.selected_model == "gemini" {
    Some(settings.gemini_prompt.clone())
} else {
    None
};

match tm.transcribe(samples, gemini_prompt) {
```

Note: `settings` is already fetched below in the original code — reorganize to avoid double fetch. Move the existing `let settings = get_settings(&ah);` (around line 430) to above the `transcribe()` call.

**Step 2: Skip LLM post-process for Gemini** — find the post_process block (around line 447):

```rust
let processed = if post_process {
    post_process_transcription(&settings, &final_text).await
} else {
    None
};
```

Change to:

```rust
// Gemini already handled post-processing in the transcription call
let processed = if post_process && settings.selected_model != "gemini" {
    post_process_transcription(&settings, &final_text).await
} else {
    None
};
```

**Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -40
```

**Step 4: Commit**

```bash
git add src-tauri/src/actions.rs
git commit -m "feat: wire Gemini prompt into transcribe(), skip LLM post-process for Gemini"
```

---

### Task 7: i18n — translation keys

**Files:**
- Modify: `src/i18n/locales/en/translation.json`

**Step 1: Find where cloudTranscription keys live and add Gemini keys after them**

```bash
grep -n "cloudTranscription" src/i18n/locales/en/translation.json | head -5
```

**Step 2: Add Gemini translation keys** — inside `"settings": { "models": { ... } }`, after the `"cloudTranscription"` block:

```json
"gemini": {
  "title": "Google Gemini",
  "description": "Transcription + AI processing in one step via Google Gemini API",
  "apiKeyLabel": "API Key",
  "apiKeyPlaceholder": "AIza...",
  "apiKeyHint": "Get your key at aistudio.google.com",
  "modelLabel": "Model",
  "modelPlaceholder": "gemini-2.0-flash",
  "promptLabel": "Post-Process Prompt",
  "promptHint": "Used when pressing the Transcribe with Post-Processing shortcut",
  "promptPlaceholder": "Transcribe the audio accurately. Fix capitalization and punctuation...",
  "configured": "Configured",
  "notConfigured": "Not configured",
  "test": "Test",
  "testFailed": "Connection test failed",
  "selectButton": "Use Gemini"
}
```

**Step 3: Commit**

```bash
git add src/i18n/locales/en/translation.json
git commit -m "feat: add Gemini i18n keys"
```

---

### Task 8: Frontend — GeminiTranscriptionCard component

**Files:**
- Create: `src/components/settings/models/GeminiTranscriptionCard.tsx`
- Modify: `src/components/settings/models/ModelsSettings.tsx`

**Step 1: Create GeminiTranscriptionCard** — mirror `CloudTranscriptionCard.tsx` structure:

```tsx
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp } from "lucide-react";
import { commands } from "@/bindings";
import Badge from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";

type TestStatus = "idle" | "testing" | "ok" | "error";

interface GeminiTranscriptionCardProps {
  isActive: boolean;
  onSelect: (modelId: string) => void;
}

export const GeminiTranscriptionCard: React.FC<GeminiTranscriptionCardProps> = ({
  isActive,
  onSelect,
}) => {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("gemini-2.0-flash");
  const [prompt, setPrompt] = useState("");
  const [showPrompt, setShowPrompt] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [testStatus, setTestStatus] = useState<TestStatus>("idle");
  const [testError, setTestError] = useState<string | null>(null);
  const loadedRef = useRef(false);
  const okTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (loadedRef.current) return;
    loadedRef.current = true;
    commands.getAppSettings().then((result) => {
      if (result.status === "ok") {
        const s = result.data;
        setApiKey(s.gemini_api_key ?? "");
        setModel(s.gemini_model ?? "gemini-2.0-flash");
        setPrompt(s.gemini_prompt ?? "");
      }
    });
  }, []);

  useEffect(() => {
    if (isActive) setIsExpanded(true);
  }, [isActive]);

  useEffect(
    () => () => {
      if (okTimerRef.current) clearTimeout(okTimerRef.current);
    },
    [],
  );

  const isConfigured = apiKey.trim() !== "" && model.trim() !== "";

  const save = async (fn: () => Promise<unknown>) => {
    setIsSaving(true);
    try {
      await fn();
    } catch (e) {
      console.error("Failed to save Gemini setting:", e);
    } finally {
      setIsSaving(false);
    }
  };

  const handleTest = async () => {
    if (okTimerRef.current) clearTimeout(okTimerRef.current);
    setTestStatus("testing");
    setTestError(null);
    const result = await commands.testGeminiConnection();
    if (result.status === "ok") {
      setTestStatus("ok");
      okTimerRef.current = setTimeout(() => setTestStatus("idle"), 2000);
    } else {
      setTestStatus("error");
      setTestError(result.error ?? t("settings.models.gemini.testFailed"));
    }
  };

  function getTestLabel(): string {
    switch (testStatus) {
      case "ok":
        return "✓";
      case "error":
        return "✗";
      default:
        return t("settings.models.gemini.test");
    }
  }

  function renderBadge() {
    if (isActive) {
      return <Badge variant="primary">{t("modelSelector.active")}</Badge>;
    }
    const labelKey = isConfigured
      ? "settings.models.gemini.configured"
      : "settings.models.gemini.notConfigured";
    return <Badge variant="secondary">{t(labelKey)}</Badge>;
  }

  const borderClass = isActive
    ? "border-logo-primary/50 bg-logo-primary/10"
    : "border-mid-gray/20 hover:border-logo-primary/30";

  return (
    <div
      className={`flex flex-col rounded-xl px-4 py-3 gap-2 border-2 transition-all duration-200 ${borderClass}`}
    >
      <button
        type="button"
        className="flex items-start justify-between w-full text-left"
        onClick={() => setIsExpanded((v) => !v)}
      >
        <div className="flex flex-col items-start flex-1 min-w-0">
          <div className="flex items-center gap-3 flex-wrap">
            <h3 className="text-base font-semibold text-text">
              {t("settings.models.gemini.title")}
            </h3>
            {renderBadge()}
          </div>
          <p className="text-sm text-text/60 leading-relaxed">
            {t("settings.models.gemini.description")}
          </p>
        </div>
        <div className="ml-3 mt-0.5 shrink-0">
          {isExpanded ? (
            <ChevronUp className="w-4 h-4 text-text/40" />
          ) : (
            <ChevronDown className="w-4 h-4 text-text/40" />
          )}
        </div>
      </button>

      {isExpanded && (
        <>
          <hr className="w-full border-mid-gray/20" />
          <div className="flex flex-col gap-3">
            {/* API Key */}
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.gemini.apiKeyLabel")}
              </label>
              <Input
                type="password"
                variant="compact"
                value={apiKey}
                onChange={(e) => {
                  setApiKey(e.target.value);
                  setTestStatus("idle");
                }}
                onBlur={(e) =>
                  save(() => commands.changeGeminiApiKey(e.target.value))
                }
                placeholder={t("settings.models.gemini.apiKeyPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
              <p className="text-xs text-text/30">
                {t("settings.models.gemini.apiKeyHint")}
              </p>
              {testStatus === "error" && (
                <p className="text-xs text-red-400 break-all">{testError}</p>
              )}
            </div>

            {/* Model */}
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-text/60">
                {t("settings.models.gemini.modelLabel")}
              </label>
              <Input
                type="text"
                variant="compact"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                onBlur={(e) =>
                  save(() => commands.changeGeminiModel(e.target.value))
                }
                placeholder={t("settings.models.gemini.modelPlaceholder")}
                className="w-full"
                disabled={isSaving}
              />
            </div>

            {/* Prompt (collapsible) */}
            <div className="flex flex-col gap-1">
              <button
                type="button"
                className="flex items-center gap-1 text-xs text-text/40 hover:text-text/60 transition-colors w-fit"
                onClick={() => setShowPrompt((v) => !v)}
              >
                <span>{showPrompt ? "▾" : "▸"}</span>
                <span>{t("settings.models.gemini.promptLabel")}</span>
              </button>
              {showPrompt && (
                <div className="flex flex-col gap-1">
                  <textarea
                    rows={5}
                    value={prompt}
                    onChange={(e) => setPrompt(e.target.value)}
                    onBlur={(e) =>
                      save(() => commands.changeGeminiPrompt(e.target.value))
                    }
                    placeholder={t("settings.models.gemini.promptPlaceholder")}
                    className="w-full rounded-lg border border-mid-gray/30 bg-background px-3 py-2 text-xs font-mono text-text/80 placeholder:text-text/30 focus:outline-none focus:ring-2 focus:ring-logo-primary/50 resize-none"
                    disabled={isSaving}
                    spellCheck={false}
                  />
                  <p className="text-xs text-text/30">
                    {t("settings.models.gemini.promptHint")}
                  </p>
                </div>
              )}
            </div>

            {/* Bottom row: Activate + Test */}
            <div className="flex items-center justify-end gap-2">
              {!isActive && (
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => {
                    if (isConfigured) onSelect("gemini");
                  }}
                  disabled={!isConfigured}
                >
                  {t("settings.models.gemini.selectButton")}
                </Button>
              )}
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void handleTest()}
                disabled={!isConfigured || testStatus === "testing"}
                className={[
                  "w-16 justify-center shrink-0 transition-colors",
                  testStatus === "ok" ? "!text-green-500" : "",
                  testStatus === "error" ? "!text-red-400" : "",
                ].join(" ")}
              >
                {getTestLabel()}
              </Button>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
```

**Step 2: Add GeminiTranscriptionCard to ModelsSettings** — in `ModelsSettings.tsx`:

Import at the top:
```tsx
import { GeminiTranscriptionCard } from "./GeminiTranscriptionCard";
```

In the `{/* API Section */}` block (around line 362), add after `<CloudTranscriptionCard .../>`:
```tsx
<GeminiTranscriptionCard
  isActive={currentModel === "gemini"}
  onSelect={handleModelSelect}
/>
```

**Step 3: TypeScript check**

```bash
bun run tsc --noEmit 2>&1 | head -30
```

Fix any type errors.

**Step 4: Commit**

```bash
git add src/components/settings/models/GeminiTranscriptionCard.tsx \
        src/components/settings/models/ModelsSettings.tsx
git commit -m "feat: add GeminiTranscriptionCard settings UI"
```

---

### Task 9: Full build and lint check

**Step 1: Run full lint + typecheck**

```bash
bun run lint 2>&1 | tail -20
bun run format:check 2>&1 | tail -10
cd src-tauri && cargo clippy 2>&1 | grep -E "^error" | head -20
```

Fix any issues. For linting errors about missing i18n keys or hardcoded strings, verify all strings use `t()`.

**Step 2: Build production to catch any compilation issues**

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri build 2>&1 | tail -30
```

**Step 3: Fix any issues found**

**Step 4: Commit any fixes**

```bash
git add -p
git commit -m "fix: address lint and build issues"
```

---

### Task 10: Manual End-to-End Test

**Prerequisites:** Google AI Studio API key (aistudio.google.com)

**Step 1: Launch dev build**

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

**Step 2: Configure Gemini**
1. Open Settings → Models
2. Find Google Gemini card — verify it appears
3. Enter API key
4. Set model: `gemini-2.0-flash`
5. Click Test — verify ✓ appears
6. Click "Use Gemini" — verify "Active" badge appears

**Step 3: Test plain transcription**
1. Press `option+space` (transcribe)
2. Say: "hello world this is a test"
3. Verify text appears in clipboard/focused app
4. Check logs: `debug: Calling Gemini API: model=gemini-2.0-flash`

**Step 4: Test transcription with post-processing**
1. Expand the Prompt section
2. Set prompt: "Transcribe the audio. Make everything UPPERCASE."
3. Press `option+shift+space` (transcribe_with_post_process)
4. Say: "hello world"
5. Verify output is "HELLO WORLD" (or similar uppercase result)

**Step 5: Test fallback — verify old post-process still works with non-Gemini model**
1. Switch to a local model
2. Enable post-processing with an OpenAI provider
3. Verify `option+shift+space` still works as before

**Step 6: Final commit if any fixes needed**

```bash
git add -p
git commit -m "fix: manual testing fixes"
```

---

## Summary of Changes

| File | Type | Change |
|------|------|--------|
| `src-tauri/src/settings.rs` | Modify | Add `gemini_api_key`, `gemini_model`, `gemini_prompt` |
| `src-tauri/src/managers/model.rs` | Modify | Add `EngineType::Gemini`, add "gemini" ModelInfo |
| `src-tauri/src/gemini_client.rs` | Create | Gemini REST API client |
| `src-tauri/src/managers/transcription.rs` | Modify | Add `LoadedEngine::Gemini`, `prompt` param to `transcribe()` |
| `src-tauri/src/shortcut/mod.rs` | Modify | Add 4 Gemini commands |
| `src-tauri/src/lib.rs` | Modify | Register module + commands |
| `src-tauri/src/actions.rs` | Modify | Pass Gemini prompt, skip LLM post-process for Gemini |
| `src/bindings.ts` | Regenerate | New Gemini commands |
| `src/components/settings/models/GeminiTranscriptionCard.tsx` | Create | Settings UI component |
| `src/components/settings/models/ModelsSettings.tsx` | Modify | Add GeminiTranscriptionCard |
| `src/i18n/locales/en/translation.json` | Modify | Gemini i18n keys |
