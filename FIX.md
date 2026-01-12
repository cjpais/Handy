# Fix Plan: LLM Recovery & Parallel Audio Processing

## Problem Summary

From the logs (logs/LOGS.md), I identified two critical issues:

### Issue 1: LLM Doesn't Recover Model After Crash

**Evidence from logs (lines 805-815):**
```
[20:57:16] Sidecar appears to have crashed during chat, attempting recovery...
[20:57:16] Dropping sidecar process
[20:57:17] Error sending shutdown to sidecar: Invalid sidecar response...
[20:57:17] Starting LLM sidecar process...
[20:57:17] Sidecar ready: LLM sidecar ready
[20:57:17] Waiting for model to stabilize after crash recovery...
[20:57:19] Local LLM error: No model loaded    <-- MODEL NOT RELOADED!
[20:57:19] Failed to process transcription from 2393: No model loaded
```

**Root Cause A**: When `chat()` detects a crash and clears the sidecar (`*guard = None`), the subsequent call to `ensure_sidecar()` spawns a NEW sidecar but the model reload logic in `ensure_sidecar()` doesn't run because `was_running` is `false` (the guard was already set to `None`).

**Root Cause B** (discovered later): Even after model reloads successfully, llama.cpp outputs debug text to stdout during model loading. This pollutes the JSON response stream, causing subsequent chat requests to fail with "Invalid sidecar response: expected value at line 1 column 1".

The bug is in `local_llm.rs` lines 343-351:
```rust
// Force respawn by clearing the sidecar
{
    let mut guard = self.sidecar.lock().unwrap();
    *guard = None;  // <-- Sets guard to None
}

// Respawn sidecar - ensure_sidecar will also reload the model from loaded_model_path
self.ensure_sidecar()?;  // <-- But ensure_sidecar sees was_running=false!
```

### Issue 2: Audio Chunks Dropped / Not Transcribed

**Evidence from logs (lines 234-280):**
```
[INFO ] Sending audio chunk for user 2545: 4.76s (76160 mono samples)
[INFO ] Sending audio chunk for user 2545: 0.76s (12160 mono samples)
[INFO ] Sending audio chunk for user 2393: 1.08s (17280 mono samples)
... (many more chunks sent, NO transcription logs)
```

Audio is being sent from Discord sidecar but NOT being processed for transcription. The main loop architecture has a flaw: it only processes ONE event per iteration from the sidecar (`recv_event_timeout`), then checks transcription queue. If audio chunks arrive faster than they're processed, they pile up and some get dropped.

**Root Cause**: The conversation loop processes events sequentially:
1. Receive ONE audio event
2. Check transcription results
3. Process ONE user's audio for transcription
4. Sleep 10ms
5. Repeat

When multiple users are speaking simultaneously, audio events arrive faster than the loop can process them. The `recv_event_timeout(50ms)` only gets one event per call.

---

## Implementation Plan

### Part 1: Fix LLM Model Recovery After Crash

**File**: `src-tauri/src/local_llm.rs`

**Problem**: When `chat()` crashes, it sets `*guard = None` before calling `ensure_sidecar()`, but `ensure_sidecar()` checks `was_running = guard.is_some()` which is now `false`.

**Solution**: Pass a flag to `ensure_sidecar()` indicating crash recovery, or reload the model in `chat()` after `ensure_sidecar()` returns.

**Option A (Recommended)**: Reload model explicitly in `chat()` after recovery:
```rust
// In chat() after ensure_sidecar() for crash recovery:
{
    let model_path_guard = self.loaded_model_path.lock().unwrap();
    if let Some(ref model_path) = *model_path_guard {
        info!("Reloading model after crash recovery: {:?}", model_path);
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(ref mut sidecar) = *guard {
            sidecar.load_model(&model_path.to_string_lossy())?;
        }
    }
}
```

**Option B**: Add `force_reload: bool` parameter to `ensure_sidecar()`.

### Part 1B: Fix llama.cpp stdout pollution

**Problem**: After model reload, llama.cpp outputs debug text to stdout which gets read as the JSON response, causing parse failures.

**Solution**: Modify `send_request()` to skip non-JSON lines:
```rust
// In send_request() - try up to 10 lines to find valid JSON
for attempt in 0..10 {
    let mut line = String::new();
    reader.read_line(&mut line)?;

    match serde_json::from_str::<SidecarResponse>(&line) {
        Ok(response) => return Ok(response),
        Err(e) => {
            let trimmed = line.trim();
            if !trimmed.starts_with('{') {
                // Non-JSON debug output - skip it
                warn!("Skipping non-JSON sidecar output: {}", trimmed);
                continue;
            }
            return Err(format!("Invalid sidecar response: {}", e));
        }
    }
}
```

### Part 2: Fix Audio Processing Architecture

**File**: `src-tauri/src/discord_conversation.rs`

**Problem**: Sequential event processing can't keep up with parallel audio streams.

**Solution**: Use a dedicated audio receiver task that runs in parallel.

**Architecture Change**:
1. Spawn a dedicated tokio task for receiving audio events
2. Use an unbounded channel to buffer incoming audio
3. Main loop only processes from the channel, never directly from sidecar

```rust
// Spawn audio receiver task
let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<(String, Vec<f32>, u32)>();
let dm = discord_manager.clone();
let running = is_running.clone();

tokio::spawn(async move {
    while running.load(Ordering::Relaxed) {
        // Receive ALL available events, not just one
        while let Some(event) = dm.recv_event_timeout(Duration::from_millis(10)) {
            match event {
                SidecarResponse::UserAudio { user_id, audio_base64, sample_rate } => {
                    if let Ok(samples) = decode_audio(&audio_base64, sample_rate) {
                        let _ = audio_tx.send((user_id, samples, sample_rate));
                    }
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
});

// Main loop now reads from audio_rx instead of discord_manager directly
while let Ok((user_id, samples, sample_rate)) = audio_rx.try_recv() {
    // Queue audio for transcription...
}
```

**Benefits**:
- Audio receiver runs independently, never blocked by transcription
- Unbounded channel buffers all incoming audio
- Main loop can process all buffered audio each iteration
- No more dropped audio chunks

---

## Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/local_llm.rs` | Fix model reload after crash in `chat()` and `generate()`, skip non-JSON stdout output |
| `src-tauri/src/discord_conversation.rs` | Add dedicated audio receiver task |

---

## Critical Code Locations

### local_llm.rs
- **Lines 343-364**: `chat()` crash recovery - needs to reload model
- **Lines 233-272**: `ensure_sidecar()` - currently doesn't reload if guard was cleared

### discord_conversation.rs
- **Lines 322-378**: Event receiving loop - needs to be moved to dedicated task
- **Lines 380-398**: Audio queue processing - needs to drain unbounded channel

---

## Verification

1. **Build and run**: `bun run tauri dev`

2. **Test LLM recovery**:
   - Trigger LLM crash (send long/complex prompt)
   - Next wake word should still work
   - Check logs for: "Reloading model after crash recovery"

3. **Test parallel audio**:
   - Have 2+ people speaking in Discord simultaneously
   - All speakers should be transcribed
   - Check logs: Every "Sending audio chunk" should have matching "Starting parallel transcription"

4. **Check no dropped audio**:
   ```bash
   grep "Sending audio chunk" LOGS.md | wc -l    # Count sent
   grep "Starting parallel transcription" LOGS.md | wc -l  # Count processed
   # These numbers should be close (some empty transcriptions filtered)
   ```
