---
Title: Streaming Output Architecture Analysis
Created: 2025-12-07
Last Updated: 2025-12-07
Tags: architecture, streaming, transcription, feature-design
---

# Streaming Output Architecture Analysis

## Executive Summary

This document analyzes the feasibility and approach for implementing "eager streaming mode" - incrementally outputting transcription results while the user holds down the hotkey, with intelligent updates at pause points.

**Verdict: Feasible, but requires significant architectural changes**

The current architecture processes audio as a batch after button release. Implementing streaming requires:
1. A streaming-capable transcription backend (current `transcribe-rs` doesn't support this)
2. New pause detection logic during recording
3. Text replacement infrastructure with backspace fallback
4. Settings integration for local-only mode

---

## Current Architecture Overview

### Flow: Button Press → Text Output

```
┌──────────────┐     ┌─────────────────────┐     ┌─────────────────────┐
│ Button Press │────▶│ AudioRecordingMgr   │────▶│ Vec<f32> (batch)    │
│ (shortcut)   │     │ accumulates samples │     │ after button release│
└──────────────┘     └─────────────────────┘     └─────────────────────┘
                                                            │
                     ┌─────────────────────┐                ▼
                     │ TranscriptionMgr    │◀───────────────┘
                     │ transcribe(Vec<f32>)│
                     └─────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Post-Processing Pipeline                                              │
│ 1. Chinese variant conversion (if zh-Hans/zh-Hant)                   │
│ 2. LLM post-processing (if enabled)                                  │
│ 3. Context-aware capitalization (reads text before cursor via a11y) │
└──────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
                     ┌─────────────────────┐
                     │ clipboard.rs::paste │
                     │ (single operation)  │
                     └─────────────────────┘
```

### Key Components

| Component | File | Purpose |
|-----------|------|---------|
| TranscribeAction | `actions.rs:208-426` | Handles start/stop button events |
| AudioRecordingManager | `managers/audio.rs:335-416` | Audio capture, VAD, level emission |
| TranscriptionManager | `managers/transcription.rs:305-430` | Batch transcription via transcribe-rs |
| paste() | `clipboard.rs:233-259` | Text insertion (clipboard + keystroke) |
| Context APIs | `context.rs:44-152` | macOS accessibility text reading |

---

## Streaming Mode Requirements

Based on the user's notes:

1. **Incremental output while holding button** - Don't wait for button release
2. **Local model only** - Fast enough for streaming (20-30x realtime)
3. **Pause-triggered output** - When user pauses, output what we have
4. **Resume on speech** - If user speaks again, update the text
5. **Accessibility-aware replacement** - Use exact text replacement if possible
6. **Backspace fallback** - Queue recent updates, use backspace to undo for non-accessible apps

---

## Technical Challenges

### Challenge 1: No Streaming Transcription API

**Problem**: `transcribe-rs` v0.1.4 only supports batch transcription:
```rust
// Current API - requires complete audio
fn transcribe_samples(audio: Vec<f32>, params: Option<Params>) -> Result<TranscriptionResult>
```

**Solutions**:

| Option | Description | Complexity |
|--------|-------------|------------|
| A. Fork transcribe-rs | Add streaming API using whisper.cpp's streaming mode | High |
| B. Use whisper-stream-rs | Different crate with native streaming | Medium |
| C. Chunked batch calls | Send overlapping windows of audio | Low |

**Recommendation**: Start with **Option C** (chunked batch) for MVP, consider Option A for production.

#### Option C: Chunked Batch Architecture

```
Audio Stream
    │
    ▼
┌───────────────────────────────────────────────┐
│ Ring Buffer (e.g., last 30 seconds)           │
│ [─────────────────────────────────────────]   │
│      ▲                               ▲        │
│   overlap                        current      │
└───────────────────────────────────────────────┘
    │
    ▼ (on pause detection)
┌───────────────────────────────────────────────┐
│ transcribe() with accumulated audio           │
│ Compare with previous output                  │
│ Emit delta (new text since last output)       │
└───────────────────────────────────────────────┘
```

### Challenge 2: Pause Detection During Recording

**Problem**: Current VAD is used for post-recording trimming, not real-time pause detection.

**Current VAD Stack**:
- `SileroVad` (ONNX model) - processes 30ms frames
- `SmoothedVad` - 15-frame history smoothing
- Runs per-frame during recording

**What we need**:
- Detect sustained silence (e.g., 300-500ms) during recording
- Trigger incremental transcription on pause
- Track speech resumption

**Implementation**:
```rust
// Add to AudioRecordingManager
struct PauseDetector {
    silence_frames: u32,
    pause_threshold_frames: u32,  // e.g., 10 frames = 300ms at 30ms/frame
    callback: Box<dyn Fn()>,
}

impl PauseDetector {
    fn on_vad_result(&mut self, is_speech: bool) {
        if !is_speech {
            self.silence_frames += 1;
            if self.silence_frames >= self.pause_threshold_frames {
                (self.callback)();  // Trigger transcription
                self.silence_frames = 0;  // Reset after trigger
            }
        } else {
            self.silence_frames = 0;
        }
    }
}
```

### Challenge 3: Text Replacement Infrastructure

**Two scenarios**:

#### Scenario A: Accessible Apps (macOS)
- Can read text before cursor via `AXUIElement`
- Know exactly what we previously inserted
- Can select and replace precisely

**Approach**:
```rust
// Track what we've inserted
struct StreamingState {
    inserted_text: String,      // What we've output so far
    insertion_point: usize,     // Where we started inserting
}

// To replace:
// 1. Select previous text: Shift+Cmd+Left (word by word) or direct AX selection
// 2. Type new text (overwrites selection)
```

#### Scenario B: Non-Accessible Apps (terminals, some apps)
- Cannot read text context
- Must use backspace to remove previous output

**Approach**:
```rust
struct StreamingStateNonAccessible {
    output_history: VecDeque<String>,  // Last N outputs
    max_history: usize,                 // How many to keep (e.g., 3)
}

impl StreamingStateNonAccessible {
    fn replace_last(&self, new_text: &str) -> String {
        let prev = self.output_history.back().unwrap_or(&String::new());
        let backspaces = prev.chars().count();
        // Generate: N backspaces + new_text
        format!("{}{}", "\x08".repeat(backspaces), new_text)
    }
}
```

**Complexity**: Backspace behavior varies by application:
- Most apps: `\x08` (ASCII backspace) or `Key::Backspace`
- Some terminals: May need `\x7F` (DEL)
- Some apps: May not respond to programmatic backspace

### Challenge 4: Context-Aware Capitalization with Streaming

**Problem**: Current implementation reads text before cursor ONCE at the end.

**For streaming**: Need to either:
1. Read context only on FIRST output (recommended)
2. Re-read context on each update (expensive, may race)

**Recommendation**: Read context once when first pause detected, apply to initial output. Subsequent outputs maintain relative capitalization.

### Challenge 5: Post-Processing Incompatibility

**Problem**: LLM post-processing is slow (500ms-2s per call) and designed for complete text.

**Options**:
1. **Disable during streaming** - Output raw transcription while streaming, apply LLM only on final release
2. **Queue final post-processing** - When button released, do one final LLM pass and replace
3. **Skip entirely in streaming mode** - Keep it simple

**Recommendation**: Option 1 - disable during streaming, apply on button release.

---

## Proposed Architecture

### New Components

```
┌─────────────────────────────────────────────────────────────────────┐
│                        StreamingController                          │
│                                                                     │
│  ┌───────────────┐     ┌─────────────────┐     ┌───────────────┐   │
│  │ PauseDetector │────▶│ ChunkedTranscr. │────▶│ TextReplacer  │   │
│  │ (VAD-based)   │     │ (batch windows) │     │ (a11y/backsp) │   │
│  └───────────────┘     └─────────────────┘     └───────────────┘   │
│         ▲                                              │           │
│         │                                              ▼           │
│  ┌──────────────────────────────────────────────────────────┐     │
│  │                    AudioBuffer                           │     │
│  │  [ring buffer with overlap, tracks sample positions]     │     │
│  └──────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────┘
```

### State Machine

```
                    ┌─────────────┐
                    │    Idle     │
                    └──────┬──────┘
                           │ button press
                           ▼
                    ┌─────────────┐
           ┌───────▶│  Recording  │◀──────┐
           │        └──────┬──────┘       │
           │               │              │
      speech starts        │ pause        │ speech resumes
           │               │ detected     │
           │               ▼              │
           │        ┌─────────────┐       │
           └────────│ Outputting  │───────┘
                    │ (transcribe │
                    │  + insert)  │
                    └──────┬──────┘
                           │ button release
                           ▼
                    ┌─────────────┐
                    │ Finalizing  │ (optional LLM post-process,
                    │             │  final text replacement)
                    └──────┬──────┘
                           │
                           ▼
                    ┌─────────────┐
                    │    Idle     │
                    └─────────────┘
```

### Settings Changes

```rust
// Add to AppSettings
pub struct AppSettings {
    // ... existing fields ...

    /// Enable streaming mode (only for local models)
    pub streaming_mode_enabled: bool,

    /// Pause detection threshold in milliseconds
    pub streaming_pause_threshold_ms: u32,  // default: 400

    /// How many previous outputs to track for backspace replacement
    pub streaming_history_size: usize,  // default: 3
}
```

---

## Implementation Plan

### Phase 1: Foundation (MVP)

1. **Add pause detection to AudioRecordingManager**
   - Hook into existing VAD callback
   - Count consecutive silence frames
   - Emit `pause-detected` event

2. **Create StreamingController**
   - Manage state machine
   - Coordinate between audio, transcription, and text output
   - Track inserted text for replacement

3. **Implement basic text replacement**
   - Accessibility path: Select previous text, type new
   - Non-accessible path: Backspace + retype

4. **Settings integration**
   - Add streaming mode toggle (UI)
   - Local model detection

### Phase 2: Refinement

5. **Optimize chunked transcription**
   - Implement overlapping windows
   - Cache partial model state if possible

6. **Improve text replacement reliability**
   - Handle edge cases (selection, cursor movement)
   - Test across different applications

7. **Add final post-processing pass**
   - On button release, run LLM post-process
   - Replace streamed text with final version

### Phase 3: Polish

8. **UI feedback during streaming**
   - Show real-time transcription in overlay
   - Indicate streaming vs final state

9. **Performance tuning**
   - Measure latency from pause to output
   - Optimize buffer sizes and overlap

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| transcribe-rs can't stream fast enough | Medium | High | Benchmark first; fork if needed |
| Backspace fallback unreliable | High | Medium | Test widely; make it opt-in |
| Context reading races with output | Medium | Low | Read once at start |
| Users enable for slow models | Low | Medium | Auto-detect local models |
| Complexity explosion | Medium | High | MVP first, iterate |

---

## Open Questions

1. **What's the minimum pause duration that feels natural?** (300ms? 500ms?)
2. **Should we show streaming text in the overlay before insertion?**
3. **How to handle errors mid-stream?** (e.g., transcription fails)
4. **Should streaming mode be default or opt-in?**
5. **Do we need to handle selection being active when we try to insert?**

---

## Next Steps

1. **Benchmark `transcribe-rs` with chunked audio** - Measure latency and accuracy of transcribing 2-5 second chunks
2. **Prototype pause detection** - Add silence tracking to VAD callback
3. **Test text replacement approaches** - Try backspace method in various apps
4. **Create settings UI** - Add streaming mode toggle

---

## References

- [transcribe-rs GitHub](https://github.com/cjpais/transcribe-rs) - Current transcription library (no streaming)
- [whisper-stream-rs](https://crates.io/crates/whisper-stream-rs) - Alternative with streaming support
- [rwhisper](https://docs.rs/rwhisper) - Another streaming-capable Whisper wrapper
- macOS Accessibility APIs (`context.rs`) - Text reading infrastructure
