# Transcription Cleanup — LLM Post-Processing with Audit Trail

## Problem

STT models (Whisper, Parakeet, etc.) produce raw transcription that often contains:
- Minor grammar issues, awkward phrasing
- Missing punctuation or capitalization
- Homophone errors ("their/there", "its/it's")
- Inconsistent formatting

The existing `post_process` feature in Handy solves this, but has gaps:
1. **It's opt-in per transcription** — two shortcut bindings (`transcribe` vs `transcribe_with_post_process`). Users forget which to trigger.
2. **Original text is lost** — `transcription_history` stores the raw STT output in `transcription_text`, but after post-processing the cleaned text overwrites `final_text` that gets pasted. The raw text IS preserved in the DB, but there's no separate "always-on" cleanup path.
3. **No lightweight cleanup mode** — existing post-processing uses user-defined prompts that can do arbitrary transformations. What we want is a simple "fix grammar and punctuation, keep meaning intact" pass that's always on.
4. **Audit trail incomplete** — history stores `transcription_text` (raw) and `post_processed_text` (cleaned), but only when post-processing was explicitly triggered. No log of the LLM prompt used for lightweight cleanup.

## Goal

Add an **always-on, lightweight LLM cleanup step** that runs after STT transcription, with:
- Configurable OpenAI-compatible endpoint and model
- Original → cleaned audit trail in history DB
- Independent toggle from existing post-processing
- Fast, cheap model (small model, simple prompt)

## Design

### Where It Fits in the Pipeline

```
Audio → VAD → STT Model → Raw Text → [Cleanup LLM] → Clean Text → Paste
                                                    ↓
                                            History DB (original + cleaned)
```

The cleanup step sits **after** STT but **before** the existing post-processing step. This gives three layers:

| Layer | When | Purpose | Config |
|-------|------|---------|--------|
| **Cleanup** (NEW) | Always on (or per-transcription) | Light grammar/punctuation fix | Endpoint, model, system prompt |
| **Post-processing** (existing) | Opt-in via shortcut or toggle | Arbitrary LLM transformation | User-defined prompts |
| **Chinese variant** (existing) | When language is zh-Hans/zh-Hant | Traditional ↔ Simplified conversion | Language setting |

### New Settings

Add to `AppSettings` in `src-tauri/src/settings.rs`:

```rust
pub struct AppSettings {
    // ... existing fields ...

    // --- NEW: Transcription cleanup ---
    pub cleanup_enabled: bool,                  // Default: false (opt-in until verified)
    pub cleanup_provider_id: String,            // Reuses PostProcessProvider system
    pub cleanup_api_key: String,               // Per-provider, reuses existing key store
    pub cleanup_model: String,                 // e.g. "gpt-4o-mini", "llama-3.1-8b"
    pub cleanup_prompt_id: Option<String>,     // Selected cleanup prompt
    pub cleanup_prompts: Vec<LLMCleanupPrompt>, // Pre-defined + custom cleanup prompts
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMCleanupPrompt {
    pub id: String,
    pub name: String,        // Display name: "Light cleanup", "Heavy grammar fix", etc.
    pub system_prompt: String, // System message sent to LLM
    pub built_in: bool,      // Can't delete built-in prompts
}
```

**Built-in cleanup prompts** (shipped with Handy):

| ID | Name | System Prompt |
|----|------|---------------|
| `light` | Light cleanup | "Fix minor grammar, punctuation, and capitalization errors. Preserve the original meaning, tone, and wording as much as possible. Do not rewrite or rephrase — only fix clear errors." |
| `medium` | Moderate cleanup | "Fix grammar, punctuation, capitalization, and awkward phrasing. Improve readability while preserving the original meaning and tone. Keep technical terms and proper nouns intact." |
| `heavy` | Full rewrite | "Rewrite this transcription for clarity and correctness. Fix all grammar, punctuation, and phrasing issues. Preserve the original meaning but improve the overall quality." |

### Reusing Existing Infrastructure

The cleanup step **reuses** the existing post-processing infrastructure:

| Component | How It's Reused |
|-----------|----------------|
| `PostProcessProvider` | Same provider list (OpenAI, OpenRouter, Custom, etc.). Cleanup uses its own `provider_id` field but same provider configs |
| `post_process_api_keys` | Same `SecretMap` — cleanup reads from the same key store |
| `llm_client.rs` | Same `send_chat_completion()` and `send_chat_completion_with_schema()` functions |
| Settings commands | Same pattern: `#[tauri::command]` getters/setters in `shortcut/mod.rs` |
| Frontend settings | Same settings store pattern, new UI section |

### History Database Changes

Add two new columns to `transcription_history` via `rusqlite_migration`:

```sql
ALTER TABLE transcription_history ADD COLUMN cleanup_original_text TEXT;
-- Stores the raw STT output BEFORE cleanup (for audit)

ALTER TABLE transcription_history ADD COLUMN cleanup_prompt_used TEXT;
-- Stores the system prompt that was applied (for audit/reproducibility)
```

**When cleanup is enabled:**
- `transcription_text` = raw STT output (unchanged)
- `cleanup_original_text` = same as `transcription_text` (redundant but explicit for audit)
- `post_processed_text` = cleanup output (when cleanup runs without post-processing)
- `cleanup_prompt_used` = the system prompt text that was sent to the LLM

**When both cleanup AND post-processing run:**
- `transcription_text` = raw STT output
- `cleanup_original_text` = raw STT output
- `post_processed_text` = final post-processed output (after cleanup + post-process)
- `cleanup_prompt_used` = cleanup system prompt
- `post_process_prompt` = post-processing prompt

Actually, simpler approach: **add a dedicated cleanup output column** to avoid ambiguity with existing `post_processed_text`:

```sql
ALTER TABLE transcription_history ADD COLUMN cleanup_text TEXT;
ALTER TABLE transcription_history ADD COLUMN cleanup_prompt_used TEXT;
```

| Column | Meaning |
|--------|---------|
| `transcription_text` | Raw STT output (never changes) |
| `cleanup_text` | Output after cleanup LLM (NULL if cleanup didn't run) |
| `cleanup_prompt_used` | System prompt for cleanup (NULL if cleanup didn't run) |
| `post_processed_text` | Output after post-processing (NULL if post-process didn't run) |
| `post_process_prompt` | Prompt template for post-processing (NULL if post-process didn't run) |

This makes the audit trail explicit: you can see raw → cleaned → post-processed at each step.

### New Migration

Add to `MIGRATIONS` in `src-tauri/src/managers/history.rs`:

```rust
M::up("ALTER TABLE transcription_history ADD COLUMN cleanup_text TEXT;"),
M::up("ALTER TABLE transcription_history ADD COLUMN cleanup_prompt_used TEXT;"),
```

Update `HistoryEntry` struct:

```rust
pub struct HistoryEntry {
    // ... existing fields ...
    pub cleanup_text: Option<String>,
    pub cleanup_prompt_used: Option<String>,
}
```

Update all SQL queries in `history.rs` to include the new columns (there are ~8 query sites: `save_entry`, `update_transcription`, `get_history_entries`, `get_entry_by_id`, `map_history_entry`, and test helpers).

### Cleanup Function

New function in `src-tauri/src/actions.rs`, parallel to `post_process_transcription()`:

```rust
async fn cleanup_transcription(settings: &AppSettings, transcription: &str) -> Option<String> {
    // 1. Resolve provider (reuses active_post_process_provider() pattern)
    // 2. Resolve model from settings.cleanup_model
    // 3. Resolve prompt from settings.cleanup_prompts by cleanup_prompt_id
    // 4. Call llm_client::send_chat_completion_with_schema() with:
    //    - system_prompt: selected cleanup prompt
    //    - user_content: transcription
    //    - schema: same TRANSCRIPTION_FIELD schema as post-processing
    // 5. Return cleaned text or None on failure
}
```

Uses **structured output** (JSON schema) by default for reliable parsing. Falls back to raw text if structured output fails.

### Integration in `process_transcription_output()`

Modify the existing function in `src-tauri/src/actions.rs`:

```rust
pub(crate) struct ProcessedTranscription {
    pub final_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    pub cleanup_text: Option<String>,        // NEW
    pub cleanup_prompt_used: Option<String>, // NEW
}

pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    let mut text = transcription.to_string();

    // 1. Chinese variant conversion (existing)
    if let Some(converted) = maybe_convert_chinese_variant(&settings, transcription).await {
        text = converted;
    }

    // 2. Cleanup (NEW) — runs BEFORE post-processing
    let (mut cleanup_text, mut cleanup_prompt_used) = (None, None);
    if settings.cleanup_enabled {
        if let Some(cleaned) = cleanup_transcription(&settings, &text).await {
            cleanup_text = Some(cleaned.clone());
            text = cleaned;

            // Capture prompt for audit
            if let Some(prompt_id) = &settings.cleanup_prompt_id {
                if let Some(prompt) = settings.cleanup_prompts.iter()
                    .find(|p| &p.id == prompt_id)
                {
                    cleanup_prompt_used = Some(prompt.system_prompt.clone());
                }
            }
        }
    }

    // 3. Post-processing (existing) — runs AFTER cleanup
    let (mut post_processed_text, mut post_process_prompt) = (None, None);
    if post_process {
        if let Some(processed) = post_process_transcription(&settings, &text).await {
            post_processed_text = Some(processed.clone());
            text = processed;
            // ... existing prompt capture logic ...
        }
    }

    ProcessedTranscription {
        final_text: text,
        post_processed_text,
        post_process_prompt,
        cleanup_text,
        cleanup_prompt_used,
    }
}
```

### Update `TranscribeAction::stop()` to Save Cleanup Data

In the `actions.rs` `stop()` method, update the `save_entry` call:

```rust
hm.save_entry(
    file_name,
    transcription,                    // raw STT text
    post_process,                     // was post-processing requested?
    processed.cleanup_text.clone(),   // NEW: cleanup output
    processed.cleanup_prompt_used.clone(), // NEW: cleanup prompt
    processed.post_processed_text,    // post-processing output
    processed.post_process_prompt,    // post-processing prompt
);
```

And update `HistoryManager::save_entry()` signature to accept the new fields.

### Tauri Commands

Add to `src-tauri/src/shortcut/mod.rs` (follow existing pattern):

```rust
#[tauri::command]
async fn get_cleanup_enabled(state: State<AppState>) -> Result<bool, String> { /* ... */ }
#[tauri::command]
async fn set_cleanup_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn get_cleanup_provider_id(state: State<AppState>) -> Result<String, String> { /* ... */ }
#[tauri::command]
async fn set_cleanup_provider_id(state: State<AppState>, id: String) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn get_cleanup_model(state: State<AppState>) -> Result<String, String> { /* ... */ }
#[tauri::command]
async fn set_cleanup_model(state: State<AppState>, model: String) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn get_cleanup_prompts(state: State<AppState>) -> Result<Vec<LLMCleanupPrompt>, String> { /* ... */ }
#[tauri::command]
async fn set_cleanup_prompt_id(state: State<AppState>, id: Option<String>) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn get_cleanup_prompt_by_id(state: State<AppState>, id: String) -> Result<LLMCleanupPrompt, String> { /* ... */ }
#[tauri::command]
async fn add_cleanup_prompt(state: State<AppState>, prompt: LLMCleanupPrompt) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn update_cleanup_prompt(state: State<AppState>, prompt: LLMCleanupPrompt) -> Result<(), String> { /* ... */ }
#[tauri::command]
async fn delete_cleanup_prompt(state: State<AppState>, id: String) -> Result<(), String> { /* ... */ }
```

Register in `lib.rs` `collect_commands![]` macro.

### Frontend UI

New settings section in `src/components/settings/`:

**`TranscriptionCleanupSettings.tsx`** — placed alongside existing `PostProcessingSettings.tsx`:

- Enable/disable toggle
- Provider selector (dropdown — reuses existing provider list)
- Model selector (dropdown — populated from `fetch_models` API call)
- Prompt selector (dropdown — built-in + custom prompts)
- Prompt editor (view/edit selected prompt's system prompt)
- "Add custom prompt" button
- "Test cleanup" button (send current transcription through cleanup, show result)

**History view updates** — in the existing history panel:
- Show original → cleaned diff when cleanup was applied
- Collapsible "cleanup" section showing the prompt used and the cleaned output
- Side-by-side comparison: raw STT vs cleaned vs post-processed

### Frontend Store Updates

Extend `src/stores/settingsStore.ts`:

```typescript
interface SettingsStore {
  // ... existing ...

  // Cleanup settings
  cleanupEnabled: boolean;
  cleanupProviderId: string;
  cleanupModel: string;
  cleanupPromptId: string | null;
  cleanupPrompts: LLMCleanupPrompt[];

  setCleanupEnabled: (enabled: boolean) => void;
  setCleanupProviderId: (id: string) => void;
  setCleanupModel: (model: string) => void;
  setCleanupPromptId: (id: string | null) => void;
  // ... etc
}
```

### Recommended Default Model

For lightweight cleanup, recommend fast/cheap models:

| Provider | Model | Why |
|----------|-------|-----|
| OpenAI | `gpt-4o-mini` | Fast, cheap, good at grammar |
| OpenRouter | `meta-llama/llama-3.1-8b-instruct` | Free/cheap, capable |
| Groq | `llama-3.1-8b` | Extremely fast inference |
| Cerebras | `llama-3.1-8b` | Sub-100ms latency |
| Custom | Any small model | For local/self-hosted |

The cleanup prompt is simple (grammar fix), so small models work well. No need for large/expensive models.

## Implementation Plan

| Step | What | Files | Effort |
|------|------|-------|--------|
| 1 | Add settings fields + defaults | `settings.rs` | Small |
| 2 | Add built-in cleanup prompts | `settings.rs` | Small |
| 3 | Add DB migration + HistoryEntry fields | `history.rs` | Small |
| 4 | Implement `cleanup_transcription()` function | `actions.rs` | Medium |
| 5 | Update `process_transcription_output()` | `actions.rs` | Small |
| 6 | Update `save_entry()` signature + calls | `history.rs`, `actions.rs` | Small |
| 7 | Add Tauri commands | `shortcut/mod.rs`, `lib.rs` | Medium |
| 8 | Generate TypeScript types | `tauri-specta` codegen | Auto |
| 9 | Frontend: cleanup settings UI | `TranscriptionCleanupSettings.tsx`, `settingsStore.ts` | Medium |
| 10 | Frontend: history view updates | History panel components | Medium |

## Open Questions

1. **Should cleanup use structured output or raw completion?** Structured output is more reliable (JSON schema enforces a `transcription` field), but adds a slight overhead. Recommendation: structured output by default, fall back to raw.

2. **Should cleanup run on `transcribe` (no post-process) shortcut only, or always?** If always-on, it runs even when user triggers `transcribe_with_post_process`. Recommendation: always-on when `cleanup_enabled` is true, regardless of which shortcut is used. The two layers are independent.

3. **Should we show a processing overlay during cleanup?** Existing `show_processing_overlay()` is shown during post-processing. For lightweight cleanup with fast models (Groq, Cerebras), the delay may be <500ms — no overlay needed. But for slower providers, an overlay would be good. Recommendation: skip overlay for cleanup, keep it for post-processing.

4. **Error handling: fallback to raw text?** If the LLM call fails (network error, rate limit, etc.), should we fall back to raw STT text? Recommendation: yes, always fall back to raw. Never block transcription output on LLM failure.

5. **Should cleanup have its own API key or share with post-processing?** They could share (same provider = same key), but some users may want different accounts/quotas. Recommendation: reuse the same `post_process_api_keys` SecretMap — cleanup reads the key for its selected provider. Simpler, fewer settings fields.

6. **Prompt templates: should `${output}` placeholder work in cleanup prompts?** Existing post-processing prompts support `${output}` substitution. For cleanup, we're using system prompts + user message pattern (not template substitution). Recommendation: cleanup prompts are system messages only, no variable substitution. Simpler mental model.

## Audit Trail Example

After a transcription with cleanup enabled:

```
HistoryEntry {
  id: 42,
  transcription_text: "hey so i was thinking like we should probably um meet tomorrow at like three",
  cleanup_text: Some("Hey, so I was thinking we should probably meet tomorrow at three."),
  cleanup_prompt_used: Some("Fix minor grammar, punctuation, and capitalization errors..."),
  post_processed_text: None,
  post_process_prompt: None,
}
```

User can see in the history panel:
- **Original:** "hey so i was thinking like we should probably um meet tomorrow at like three"
- **Cleaned:** "Hey, so I was thinking we should probably meet tomorrow at three."
- **Prompt used:** "Fix minor grammar, punctuation, and capitalization errors..."

## References

- Existing post-processing: `src-tauri/src/actions.rs` (line ~68, `post_process_transcription()`)
- LLM client: `src-tauri/src/llm_client.rs` (`send_chat_completion()`, `send_chat_completion_with_schema()`)
- History manager: `src-tauri/src/managers/history.rs`
- Settings: `src-tauri/src/settings.rs` (`PostProcessProvider`, `AppSettings`)
- Frontend settings: `src/components/settings/post-processing/PostProcessingSettings.tsx`
- Settings store: `src/stores/settingsStore.ts`
