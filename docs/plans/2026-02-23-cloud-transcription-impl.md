# Cloud Transcription Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a configurable "Cloud Transcription" entry to the models list that sends audio to any OpenAI-compatible `/audio/transcriptions` endpoint (e.g. Groq), with 3-retry logic, failure notification via frontend event, and history-based retry for failed recordings.

**Architecture:** New `EngineType::Cloud` + `LoadedEngine::Cloud` variants slot into the existing transcription pipeline. `transcribe()` encodes `Vec<f32>` to WAV bytes in-memory using `hound`, then POSTs multipart to the API endpoint. On all-retries-exhausted, actions.rs saves a pending history entry and emits a frontend event for toast display. History gets a `cloud_pending` DB column (migration M4) and a new `retranscribe_history_entry` command.

**Tech Stack:** Rust (`reqwest 0.12` multipart, `hound` WAV encoding, `rusqlite_migration`), React/TypeScript (event listener for toast, inline config form in model card, retry button in history)

---

## Task 1: Add cloud transcription settings fields

**Files:**
- Modify: `src-tauri/src/settings.rs`

**Step 1: Add 3 fields to `AppSettings` struct** (after `typing_tool` field, around line 362)

```rust
#[serde(default = "default_cloud_transcription_base_url")]
pub cloud_transcription_base_url: String,
#[serde(default)]
pub cloud_transcription_api_key: String,
#[serde(default = "default_cloud_transcription_model")]
pub cloud_transcription_model: String,
```

**Step 2: Add default functions** (after `fn default_typing_tool`, around line 571)

```rust
fn default_cloud_transcription_base_url() -> String {
    "https://api.groq.com/openai/v1".to_string()
}

fn default_cloud_transcription_model() -> String {
    "whisper-large-v3".to_string()
}
```

**Step 3: Initialize fields in `get_default_settings()`** (inside the `AppSettings { ... }` block)

```rust
cloud_transcription_base_url: default_cloud_transcription_base_url(),
cloud_transcription_api_key: String::new(),
cloud_transcription_model: default_cloud_transcription_model(),
```

**Step 4: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

Expected: no errors.

**Step 5: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat: add cloud transcription settings fields"
```

---

## Task 2: Add `EngineType::Cloud` and cloud ModelInfo entry

**Files:**
- Modify: `src-tauri/src/managers/model.rs`

**Step 1: Add `Cloud` variant to `EngineType` enum** (after `SenseVoice` line, around line 26)

```rust
Cloud,
```

**Step 2: Add the cloud model entry to `available_models`** in `ModelManager::new()`, at the very end of the hashmap insertions (just before `Ok(ModelManager { ... })`).

Find the last model insert (search for `available_models.insert`) and add after it:

```rust
available_models.insert(
    "cloud".to_string(),
    ModelInfo {
        id: "cloud".to_string(),
        name: "Cloud Transcription".to_string(),
        description: "Transcribe using an OpenAI-compatible cloud API (e.g. Groq, OpenAI)".to_string(),
        filename: String::new(),
        url: None,
        size_mb: 0,
        is_downloaded: true,
        is_downloading: false,
        partial_size: 0,
        is_directory: false,
        engine_type: EngineType::Cloud,
        accuracy_score: 0.9,
        speed_score: 0.8,
        supports_translation: false,
        is_recommended: false,
        supported_languages: vec!["auto".to_string()],
        is_custom: false,
    },
);
```

**Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

Expected: warning about unhandled `Cloud` match arm in transcription.rs — that's fine, it'll be fixed in Task 3.

**Step 4: Commit**

```bash
git add src-tauri/src/managers/model.rs
git commit -m "feat: add EngineType::Cloud and cloud model entry"
```

---

## Task 3: Add `LoadedEngine::Cloud` — load/unload

**Files:**
- Modify: `src-tauri/src/managers/transcription.rs`

**Step 1: Add Cloud variant to `LoadedEngine` enum** (after `SenseVoice` line, around line 44)

```rust
Cloud {
    base_url: String,
    api_key: String,
    model_name: String,
},
```

**Step 2: Handle `Cloud` in `unload_model()`** — in the `match loaded_engine { ... }` block inside `unload_model()` (around line 163), add:

```rust
LoadedEngine::Cloud { .. } => { /* nothing to unload */ }
```

**Step 3: Handle `Cloud` in `load_model()`** — in the big `match model_info.engine_type { ... }` block (around line 246), add at the end:

```rust
EngineType::Cloud => {
    let settings = get_settings(&self.app_handle);
    LoadedEngine::Cloud {
        base_url: settings.cloud_transcription_base_url.clone(),
        api_key: settings.cloud_transcription_api_key.clone(),
        model_name: settings.cloud_transcription_model.clone(),
    }
}
```

**Step 4: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

Expected: warning about unhandled `Cloud` arm in `transcribe()` — fine for now.

**Step 5: Commit**

```bash
git add src-tauri/src/managers/transcription.rs
git commit -m "feat: add LoadedEngine::Cloud with load/unload"
```

---

## Task 4: Implement WAV-to-bytes encoder + cloud API call

**Files:**
- Modify: `src-tauri/src/managers/transcription.rs`
- Modify: `src-tauri/Cargo.toml`

**Step 1: Add `multipart` feature to reqwest in `Cargo.toml`**

Find: `reqwest = { version = "0.12", features = ["json", "stream"] }`
Replace with: `reqwest = { version = "0.12", features = ["json", "stream", "multipart"] }`

**Step 2: Add `samples_to_wav_bytes` helper** — add this free function near the top of `transcription.rs`, after the imports:

```rust
use std::io::Cursor;

/// Encode f32 audio samples (16 kHz mono) to WAV bytes in memory.
fn samples_to_wav_bytes(samples: &[f32]) -> Result<Vec<u8>> {
    use hound::{WavSpec, WavWriter};
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let cursor = Cursor::new(Vec::new());
    let mut writer = WavWriter::new(cursor, spec)?;
    for s in samples {
        writer.write_sample((*s * i16::MAX as f32) as i16)?;
    }
    let inner = writer.into_inner()?;
    Ok(inner.into_inner())
}
```

Note: `hound` is already a transitive dependency (used in `audio_toolkit`). Add it to `Cargo.toml` explicitly if needed:
```toml
hound = "3"
```

**Step 3: Add `call_cloud_api` async helper** — add after `samples_to_wav_bytes`:

```rust
/// POST audio to an OpenAI-compatible /audio/transcriptions endpoint.
async fn call_cloud_api(
    base_url: &str,
    api_key: &str,
    model_name: &str,
    wav_bytes: Vec<u8>,
    language: Option<String>,
) -> Result<String> {
    use reqwest::multipart;
    let client = reqwest::Client::new();

    let file_part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| anyhow::anyhow!("MIME error: {}", e))?;

    let mut form = multipart::Form::new()
        .part("file", file_part)
        .text("model", model_name.to_string())
        .text("response_format", "json");

    if let Some(lang) = language {
        form = form.text("language", lang);
    }

    let response = client
        .post(format!("{}/audio/transcriptions", base_url.trim_end_matches('/')))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Cloud API {} error: {}", status.as_u16(), error_text));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    json["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No 'text' field in API response"))
        .map(|s| s.to_string())
}
```

**Step 4: Handle `Cloud` in `transcribe()`** — in the big `match &mut engine { ... }` block inside `transcribe()` (around line 466), add:

```rust
LoadedEngine::Cloud { base_url, api_key, model_name } => {
    let wav = samples_to_wav_bytes(&audio)
        .map_err(|e| anyhow::anyhow!("WAV encoding failed: {}", e))?;

    let language = if settings.selected_language == "auto" {
        None
    } else {
        Some(settings.selected_language.clone())
    };

    // Use tokio block_in_place since transcribe() is called from a sync thread
    // but call_cloud_api is async.
    tokio::task::block_in_place(|| {
        tauri::async_runtime::block_on(call_cloud_api(
            base_url,
            api_key,
            model_name,
            wav,
            language,
        ))
    })
    .map(|text| transcribe_rs::TranscriptionResult { text, segments: vec![] })
}
```

**Step 5: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

Expected: 0 errors. May have warnings about unused imports.

**Step 6: Commit**

```bash
git add src-tauri/src/managers/transcription.rs src-tauri/Cargo.toml
git commit -m "feat: implement WAV encoding and cloud API transcription"
```

---

## Task 5: Add retry logic for cloud failures

**Files:**
- Modify: `src-tauri/src/managers/transcription.rs`

**Step 1: Wrap the cloud API call in retry logic** — replace the `LoadedEngine::Cloud` branch from Task 4 with:

```rust
LoadedEngine::Cloud { base_url, api_key, model_name } => {
    let wav = samples_to_wav_bytes(&audio)
        .map_err(|e| anyhow::anyhow!("WAV encoding failed: {}", e))?;

    let language = if settings.selected_language == "auto" {
        None
    } else {
        Some(settings.selected_language.clone())
    };

    const MAX_ATTEMPTS: u32 = 3;
    const DELAYS_MS: [u64; 2] = [300, 800]; // delay before attempt 2 and 3

    let mut last_error = anyhow::anyhow!("Unknown error");
    let mut result: Option<String> = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            let delay = DELAYS_MS[(attempt - 1) as usize];
            debug!("Cloud transcription attempt {}/{}, waiting {}ms", attempt + 1, MAX_ATTEMPTS, delay);
            thread::sleep(Duration::from_millis(delay));
        }

        match tokio::task::block_in_place(|| {
            tauri::async_runtime::block_on(call_cloud_api(
                base_url,
                api_key,
                model_name,
                wav.clone(),
                language.clone(),
            ))
        }) {
            Ok(text) => {
                result = Some(text);
                break;
            }
            Err(e) => {
                warn!("Cloud transcription attempt {}/{} failed: {}", attempt + 1, MAX_ATTEMPTS, e);
                last_error = e;
            }
        }
    }

    match result {
        Some(text) => Ok(transcribe_rs::TranscriptionResult { text, segments: vec![] }),
        None => Err(last_error),
    }
}
```

**Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

**Step 3: Commit**

```bash
git add src-tauri/src/managers/transcription.rs
git commit -m "feat: add 3-retry logic for cloud transcription"
```

---

## Task 6: Add `cloud_pending` to history DB + `save_pending_transcription`

**Files:**
- Modify: `src-tauri/src/managers/history.rs`

**Step 1: Add migration M4** — in the `MIGRATIONS` static (around line 22), append:

```rust
M::up("ALTER TABLE transcription_history ADD COLUMN cloud_pending BOOLEAN NOT NULL DEFAULT 0;"),
```

**Step 2: Add `cloud_pending` to `HistoryEntry` struct** (after `post_process_prompt` field):

```rust
#[serde(default)]
pub cloud_pending: bool,
```

**Step 3: Update `get_history_entries()` SELECT query** to include the new column. Find the SELECT query in `history.rs` and add `cloud_pending` to both the SELECT list and the row mapping.

Search for `SELECT` in `history.rs` and update: add `cloud_pending` to the column list and `row.get(N)?` in the row mapper (check current column count and append).

**Step 4: Add `save_pending_transcription` method** to `HistoryManager`:

```rust
/// Save an audio recording as a pending cloud transcription (failed after all retries).
pub async fn save_pending_transcription(
    &self,
    audio_samples: Vec<f32>,
) -> Result<()> {
    let timestamp = Utc::now().timestamp();
    let file_name = format!("handy-{}.wav", timestamp);
    let title = self.format_timestamp_title(timestamp);

    // Save WAV file
    let file_path = self.recordings_dir.join(&file_name);
    save_wav_file(file_path, &audio_samples).await?;

    // Save to database with cloud_pending = true, empty transcription
    let conn = self.get_connection()?;
    conn.execute(
        "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, cloud_pending) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![file_name, timestamp, false, title, "", true],
    )?;

    if let Err(e) = self.app_handle.emit("history-updated", ()) {
        error!("Failed to emit history-updated event: {}", e);
    }

    Ok(())
}
```

**Step 5: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

**Step 6: Commit**

```bash
git add src-tauri/src/managers/history.rs
git commit -m "feat: add cloud_pending history column and save_pending_transcription"
```

---

## Task 7: Handle cloud failure in `actions.rs` + emit notification event

**Files:**
- Modify: `src-tauri/src/actions.rs`

**Step 1: In the `Err(err)` branch of `tm.transcribe()` in `actions.rs`** (around line 514), add cloud-specific handling:

```rust
Err(err) => {
    debug!("Transcription error: {}", err);
    let settings = get_settings(&ah);
    if settings.selected_model == "cloud" {
        // Save audio as pending history entry for later retry
        let hm_clone = Arc::clone(&hm);
        tauri::async_runtime::spawn(async move {
            if let Err(e) = hm_clone.save_pending_transcription(samples_clone).await {
                error!("Failed to save pending cloud transcription: {}", e);
            }
        });
        // Emit event so frontend can show a notification
        let _ = ah.emit("cloud-transcription-failed", ());
    }
    utils::hide_recording_overlay(&ah);
    change_tray_icon(&ah, TrayIconState::Idle);
}
```

Note: `samples_clone` already exists at this point (it's cloned earlier in the `stop()` function for history saving).

**Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

**Step 3: Commit**

```bash
git add src-tauri/src/actions.rs
git commit -m "feat: save pending history and emit event on cloud failure"
```

---

## Task 8: Add `retranscribe_history_entry` command

**Files:**
- Modify: `src-tauri/src/commands/history.rs`
- Modify: `src-tauri/src/managers/history.rs`

**Step 1: Add `update_transcription` method to `HistoryManager`**:

```rust
/// Update transcription text and clear cloud_pending for a history entry.
pub fn update_transcription(&self, id: i64, transcription_text: &str) -> Result<()> {
    let conn = self.get_connection()?;
    conn.execute(
        "UPDATE transcription_history SET transcription_text = ?1, cloud_pending = 0 WHERE id = ?2",
        params![transcription_text, id],
    )?;
    if let Err(e) = self.app_handle.emit("history-updated", ()) {
        error!("Failed to emit history-updated: {}", e);
    }
    Ok(())
}

/// Get audio samples for a history entry by loading its WAV file.
pub fn get_audio_samples(&self, file_name: &str) -> Result<Vec<f32>> {
    use hound::WavReader;
    let path = self.recordings_dir.join(file_name);
    let mut reader = WavReader::open(&path)
        .map_err(|e| anyhow::anyhow!("Failed to open WAV: {}", e))?;
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to read WAV samples: {}", e))?;
    Ok(samples)
}
```

**Step 2: Add `retranscribe_history_entry` command to `commands/history.rs`**:

```rust
#[tauri::command]
#[specta::specta]
pub async fn retranscribe_history_entry(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    id: i64,
) -> Result<(), String> {
    // Load the entry to get file_name
    let entries = history_manager
        .get_history_entries()
        .await
        .map_err(|e| e.to_string())?;

    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("History entry {} not found", id))?;

    if !entry.cloud_pending {
        return Err("Entry is not a pending cloud transcription".to_string());
    }

    // Load audio from WAV file
    let samples = history_manager
        .get_audio_samples(&entry.file_name)
        .map_err(|e| e.to_string())?;

    // Retranscribe (includes retry logic)
    let text = transcription_manager
        .transcribe(samples)
        .map_err(|e| e.to_string())?;

    // Update history entry
    history_manager
        .update_transcription(id, &text)
        .map_err(|e| e.to_string())?;

    Ok(())
}
```

**Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

**Step 4: Commit**

```bash
git add src-tauri/src/managers/history.rs src-tauri/src/commands/history.rs
git commit -m "feat: add retranscribe_history_entry command"
```

---

## Task 9: Register new command + regenerate bindings

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Step 1: Find the `collect_commands!` macro call in `lib.rs`** (search for `collect_commands`) and add `commands::history::retranscribe_history_entry` to the list.

**Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

**Step 3: Regenerate TypeScript bindings**

Run the app in dev mode briefly to trigger specta binding generation, OR check if there's a dedicated binding generation step:

```bash
cd /Users/ilyanovik/Documents/Projects/oss/Handy
grep -r "specta\|bindings" package.json | head -5
```

If no dedicated script, `bun run tauri dev` (Ctrl+C after bindings regenerate — check `src/bindings.ts` changed).

**Step 4: Verify `src/bindings.ts`** contains `retranscribeHistoryEntry` and `HistoryEntry` has `cloudPending: boolean`.

**Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: register retranscribe command and update bindings"
```

---

## Task 10: Add i18n keys

**Files:**
- Modify: `src/i18n/locales/en/translation.json`

**Step 1: Find the `settings.models` section** in translation.json and add cloud model i18n:

```json
"cloudTranscription": {
  "name": "Cloud Transcription",
  "description": "Transcribe using an OpenAI-compatible API",
  "baseUrlLabel": "Base URL",
  "baseUrlPlaceholder": "https://api.groq.com/openai/v1",
  "apiKeyLabel": "API Key",
  "apiKeyPlaceholder": "Enter API key...",
  "modelLabel": "Model",
  "modelPlaceholder": "whisper-large-v3",
  "configureButton": "Configure"
}
```

**Step 2: Find the `history` section** (or create it) and add:

```json
"cloudPending": {
  "placeholder": "Transcription failed — tap Retry to try again",
  "retryButton": "Retry",
  "retrying": "Retrying..."
}
```

**Step 3: Add notification toast key** at the top-level or in a suitable section:

```json
"notifications": {
  "cloudTranscriptionFailed": "Cloud transcription failed. Recording saved — retry from History."
}
```

**Step 4: Commit**

```bash
git add src/i18n/locales/en/translation.json
git commit -m "feat: add cloud transcription i18n keys"
```

---

## Task 11: Frontend — Cloud model card inline config

**Files:**
- Modify: `src/components/settings/models/ModelsSettings.tsx`
- Create: `src/components/settings/models/CloudModelConfig.tsx`

**Step 1: Create `CloudModelConfig.tsx`**

```tsx
import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";

export const CloudModelConfig: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const [baseUrl, setBaseUrl] = useState<string>(
    () => getSetting("cloud_transcription_base_url") ?? "https://api.groq.com/openai/v1"
  );
  const [apiKey, setApiKey] = useState<string>(
    () => getSetting("cloud_transcription_api_key") ?? ""
  );
  const [model, setModel] = useState<string>(
    () => getSetting("cloud_transcription_model") ?? "whisper-large-v3"
  );

  const handleBlur = (field: "cloud_transcription_base_url" | "cloud_transcription_api_key" | "cloud_transcription_model", value: string) => {
    updateSetting(field, value);
  };

  return (
    <div className="mt-3 space-y-2 p-3 bg-mid-gray/5 rounded-lg border border-mid-gray/20">
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.baseUrlLabel")}
        </label>
        <input
          type="text"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          onBlur={(e) => handleBlur("cloud_transcription_base_url", e.target.value)}
          placeholder={t("settings.models.cloudTranscription.baseUrlPlaceholder")}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.apiKeyLabel")}
        </label>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          onBlur={(e) => handleBlur("cloud_transcription_api_key", e.target.value)}
          placeholder={t("settings.models.cloudTranscription.apiKeyPlaceholder")}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.modelLabel")}
        </label>
        <input
          type="text"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          onBlur={(e) => handleBlur("cloud_transcription_model", e.target.value)}
          placeholder={t("settings.models.cloudTranscription.modelPlaceholder")}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
    </div>
  );
};
```

**Step 2: Modify `ModelsSettings.tsx`** to show `CloudModelConfig` inline when the cloud model is in `downloadedModels`. The cloud model has `is_downloaded: true`, so it already appears in `downloadedModels`.

Find where `downloadedModels.map` renders `ModelCard` and add a conditional after each card:

```tsx
{downloadedModels.map((model: ModelInfo) => (
  <div key={model.id}>
    <ModelCard
      model={model}
      status={getModelStatus(model.id)}
      onSelect={handleModelSelect}
      onDownload={handleModelDownload}
      onDelete={handleModelDelete}
      onCancel={handleModelCancel}
      downloadProgress={getDownloadProgress(model.id)}
      downloadSpeed={getDownloadSpeed(model.id)}
      showRecommended={false}
    />
    {model.id === "cloud" && <CloudModelConfig />}
  </div>
))}
```

Import `CloudModelConfig` at the top of `ModelsSettings.tsx`:
```tsx
import { CloudModelConfig } from "./CloudModelConfig";
```

**Step 3: Verify the frontend builds**

```bash
bun run lint 2>&1 | head -30
```

**Step 4: Commit**

```bash
git add src/components/settings/models/CloudModelConfig.tsx src/components/settings/models/ModelsSettings.tsx
git commit -m "feat: add inline cloud transcription config in model card"
```

---

## Task 12: Frontend — history retry button + notification toast

**Files:**
- Modify: `src/components/settings/history/HistorySettings.tsx`
- Modify: `src/App.tsx` (or appropriate root component for global event listener)

**Step 1: Add retry handler to `HistorySettings.tsx`**

Add state and handler inside `HistorySettings` component:

```tsx
const [retryingIds, setRetryingIds] = useState<Set<number>>(new Set());

const handleRetry = async (id: number) => {
  setRetryingIds((prev) => new Set(prev).add(id));
  try {
    const result = await commands.retranscribeHistoryEntry(id);
    if (result.status === "ok") {
      await loadHistoryEntries();
    }
  } catch (error) {
    console.error("Retry failed:", error);
  } finally {
    setRetryingIds((prev) => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
  }
};
```

**Step 2: In the history entry render** (find where `historyEntries.map` renders each entry), add retry UI for pending entries. Look for where `entry.transcription_text` is rendered and add:

```tsx
{entry.cloud_pending ? (
  <div className="flex items-center gap-2">
    <p className="text-sm text-text/40 italic">
      {t("history.cloudPending.placeholder")}
    </p>
    <button
      onClick={() => handleRetry(entry.id)}
      disabled={retryingIds.has(entry.id)}
      className="px-3 py-1 text-xs font-medium bg-logo-primary text-white rounded-md hover:opacity-90 disabled:opacity-50"
    >
      {retryingIds.has(entry.id)
        ? t("history.cloudPending.retrying")
        : t("history.cloudPending.retryButton")}
    </button>
  </div>
) : (
  // existing transcription_text render
)}
```

**Step 3: Add `cloud-transcription-failed` event listener + toast banner**

In `HistorySettings.tsx` (or `App.tsx` if there's a global notification area), add a listener that shows a dismissable banner:

```tsx
const [showCloudFailedBanner, setShowCloudFailedBanner] = useState(false);

useEffect(() => {
  const setup = async () => {
    const unlisten = await listen("cloud-transcription-failed", () => {
      setShowCloudFailedBanner(true);
      // Auto-dismiss after 8 seconds
      setTimeout(() => setShowCloudFailedBanner(false), 8000);
    });
    return unlisten;
  };
  const unlistenPromise = setup();
  return () => { unlistenPromise.then((fn) => fn && fn()); };
}, []);
```

Render the banner (e.g. at the top of the History page or in a global position):

```tsx
{showCloudFailedBanner && (
  <div className="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg flex items-center justify-between">
    <p className="text-sm text-red-600 dark:text-red-400">
      {t("notifications.cloudTranscriptionFailed")}
    </p>
    <button
      onClick={() => setShowCloudFailedBanner(false)}
      className="ml-2 text-red-400 hover:text-red-600 text-lg leading-none"
    >
      ×
    </button>
  </div>
)}
```

**Step 4: Verify the frontend builds without errors**

```bash
bun run lint 2>&1 | head -30
```

**Step 5: Commit**

```bash
git add src/components/settings/history/HistorySettings.tsx
git commit -m "feat: add cloud transcription retry button and failure notification"
```

---

## Task 13: Full build verification

**Step 1: Run full lint + format check**

```bash
bun run lint && bun run format:check
```

Fix any issues found.

**Step 2: Run Rust checks**

```bash
cd src-tauri && cargo clippy 2>&1 | grep "^error" | head -20
cd src-tauri && cargo fmt --check
```

**Step 3: Build the app**

```bash
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri build 2>&1 | tail -30
```

Expected: successful build.

**Step 4: Manual smoke test (if possible)**
- Open Settings → Models → confirm "Cloud Transcription" entry appears at the bottom
- Expand/config fields: set Groq base URL, API key, model
- Select "Cloud Transcription" as active model
- Record a short phrase → verify transcription appears
- Test with wrong API key → verify 3 retry attempts in logs, failure banner shows, History shows entry with Retry button
- Click Retry → verify transcription completes

**Step 5: Final commit**

```bash
git add -A
git commit -m "feat: cloud transcription implementation complete"
```

---

## Notes for implementer

- `hound` is already a transitive dep. Add `hound = "3"` to `Cargo.toml` `[dependencies]` explicitly if `cargo check` says it's missing.
- `tokio::task::block_in_place` requires the current thread to be a tokio blocking thread — transcription runs in `thread::spawn` which is fine; `block_on` from within `thread::spawn` (non-tokio thread) also works. If you hit a "Cannot start a runtime from within a runtime" panic, use `std::thread::spawn` + channel pattern instead.
- `WavWriter::into_inner()` returns `Result<W, hound::Error>` — if compilation fails due to type mismatch, try `writer.into_inner().map_err(|e| anyhow::anyhow!("{}", e))`.
- The `ModelsSettings.tsx` language filter currently hides models that don't support the selected language. Cloud model has `supported_languages: vec!["auto"]`. This means it'll be hidden when a specific language filter is active. If desired, add special handling: `model.id === "cloud" || modelSupportsLanguage(...)` — but this is optional polish.
- `useSettings` hook's `getSetting` method may not support all field names in TypeScript types until bindings regenerate — if you get TS errors, use `commands.getAppSettings()` directly in `CloudModelConfig` with a local `useEffect`.
