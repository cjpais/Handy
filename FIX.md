# Fix Plan: LLM Sidecar Crash Resilience & Wake Word Activation

## Problem Summary

From the logs (logs/LOGS.md), I identified two issues:

### 1. LLM Sidecar Crashes
- **Line 183-184**: First crash - "Invalid sidecar response: expected value at line 1 column 1"
- **Line 191-197**: Recovery works - sidecar respawns and reloads model
- **Line 232-233**: Crashes again on next request after recovery

**Root Cause**: The error "expected value at line 1 column 1" means the sidecar is outputting non-JSON content to stdout (likely llama.cpp debug/error output or crash dump) before the actual JSON response.

### 2. No Wake Word Filter
Currently, the bot responds to ALL audio in the voice channel. User wants it to only respond when someone says "chan", "omni", or "oni".

---

## Implementation Plan

### Part 1: Improve LLM Crash Resilience

**File**: src-tauri/src/local_llm.rs

**Changes**:

1. **Add retry delay after crash recovery** (lines 233-248)
   - After respawning sidecar and reloading model, add a small delay to let it stabilize
   - This prevents the "crash again immediately after recovery" pattern

2. **Improve error detection in `chat()` and `generate()`** (lines 277-297, 312-332)
   - Current code only retries on "Broken pipe", "empty response", or "crashed"
   - Add detection for "expected value" and "Invalid sidecar response" errors
   - These indicate non-JSON output pollution

3. **Add cooldown between retries**
   - After a crash recovery, wait briefly before the retry request
   - Prevents hammering the newly spawned sidecar

**File**: src-tauri/llm-sidecar/src/main.rs

**Changes**:

4. **Flush stdout after model load** (around line 336-340)
   - Ensure any llama.cpp initialization output doesn't leak into response stream
   - Add explicit `stdout().flush()` after model load completes

5. **Wrap token generation in panic handler** (around lines 175-198)
   - If token generation panics, catch it and return JSON error instead of crashing

---

### Part 2: Add Wake Word Activation

**File**: src-tauri/src/discord_conversation.rs

**Location**: After line 300 (after transcription, before LLM processing)

**Changes**:

```rust
// After line 300: info!("Discord transcription: {}", text);

// Check for wake words - only respond if addressed
const WAKE_WORDS: &[&str] = &["chan", "omni", "oni", "onichan"];
let text_lower = text.to_lowercase();
let has_wake_word = WAKE_WORDS.iter().any(|w| text_lower.contains(w));

if !has_wake_word {
    info!("No wake word detected, skipping response");
    // Still emit the transcription for UI visibility
    let _ = app_handle.emit(
        "discord-user-speech",
        serde_json::json!({
            "user_id": user_id,
            "text": text.clone(),
            "skipped": true
        }),
    );
    return Ok(());
}
```

**Behavior**:
- Transcription still happens (shows in UI)
- LLM is only called if wake word is detected
- TTS is only triggered if LLM responds
- Saves compute and prevents unwanted interruptions

---

## Files to Modify

| File | Changes |
|------|---------|
| src-tauri/src/local_llm.rs | Add retry delay, improve error detection |
| src-tauri/llm-sidecar/src/main.rs | Flush stdout, optional panic handler |
| src-tauri/src/discord_conversation.rs | Add wake word check at line 300 |

---

## Verification

1. **Build and run**: `bun run tauri dev`
2. **Test wake word**:
   - Join Discord voice channel
   - Say something without "chan/omni/oni" - should NOT get response
   - Say "hey onichan what's up" - should get response
3. **Test crash recovery**:
   - Use the bot normally until a crash occurs
   - Verify it respawns and continues working on next request
4. **Check logs**: Look for "No wake word detected" messages confirming filter is working
