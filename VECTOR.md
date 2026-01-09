# Discord Bot Long-Term Memory System

Add vector database for per-user conversation memory with 30-day TTL expiration.

## Overview

**Goal**: Enable the Discord voice bot to remember conversations with users long-term using semantic search.

**User Preferences**:
- Memory scope: **Per User** (memory follows users across servers)
- Retention: **Unlimited with 30-day TTL** (auto-expire old messages)

---

## Architecture

### New Sidecar: `memory-sidecar`

Following the existing sidecar pattern (llm-sidecar, tts-sidecar, discord-sidecar), create a new memory service that handles:
1. **Embedding generation** - Convert text to 384-dim vectors using all-MiniLM-L6-v2
2. **Vector storage** - LanceDB for embedded, serverless vector search
3. **Semantic retrieval** - Find relevant past conversations by similarity

```
src-tauri/
  memory-sidecar/           # NEW
    Cargo.toml
    src/
      main.rs               # JSON IPC server (stdin/stdout)
      embeddings.rs         # ONNX Runtime + MiniLM model
      vector_store.rs       # LanceDB wrapper
```

### IPC Protocol

```json
// Store a message
{"type": "store", "user_id": "123", "content": "hey what's up", "is_bot": false}
→ {"type": "ok", "id": "uuid-abc"}

// Query relevant context
{"type": "query", "user_id": "123", "text": "what did we talk about?", "limit": 5}
→ {"type": "results", "messages": [{"content": "...", "similarity": 0.87, "timestamp": 1704067200}]}

// Cleanup expired messages
{"type": "cleanup", "ttl_days": 30}
→ {"type": "ok", "deleted": 42}
```

---

## Implementation Plan

### Phase 1: Memory Sidecar Foundation

**New file: `src-tauri/memory-sidecar/Cargo.toml`**
```toml
[package]
name = "memory-sidecar"
version = "0.1.0"
edition = "2021"

[dependencies]
# Vector database
lancedb = "0.17"
arrow = { version = "54", default-features = false }
arrow-schema = "54"

# Embeddings (ONNX Runtime)
ort = { version = "2", features = ["download-binaries"] }
tokenizers = "0.21"
hf-hub = "0.4"

# IPC
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }

# Utils
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
log = "0.4"
env_logger = "0.11"
anyhow = "1"
```

**New file: `src-tauri/memory-sidecar/src/embeddings.rs`**
- Load all-MiniLM-L6-v2 ONNX model from HuggingFace Hub
- Cache model to `~/.cache/huggingface/` or app data dir
- Generate 384-dim embeddings for text

**New file: `src-tauri/memory-sidecar/src/vector_store.rs`**
- Initialize LanceDB in app data dir (`onichan_memory/`)
- Table schema: `id, user_id, content, embedding, is_bot, timestamp`
- Store messages with embeddings
- Query by user_id + vector similarity
- TTL cleanup (delete where timestamp < now - 30 days)

**New file: `src-tauri/memory-sidecar/src/main.rs`**
- JSON IPC over stdin/stdout (same pattern as llm-sidecar)
- Handle store/query/cleanup/shutdown commands
- Initialize embedding model + vector store on startup

### Phase 2: Main App Integration

**New file: `src-tauri/src/memory.rs`**
```rust
pub struct MemoryManager {
    process: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_reader: Option<BufReader<ChildStdout>>,
}

impl MemoryManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self>;
    pub fn store_message(&self, user_id: &str, content: &str, is_bot: bool) -> Result<()>;
    pub fn query_context(&self, user_id: &str, text: &str, limit: usize) -> Result<Vec<MemoryMessage>>;
    pub fn cleanup_expired(&self, ttl_days: u32) -> Result<u32>;
}
```

**Modify: `src-tauri/src/lib.rs`**
- Add `mod memory;`
- Initialize MemoryManager in `run()` function
- Register as Tauri state

**Modify: `src-tauri/src/onichan.rs`**
```rust
pub struct OnichanManager {
    // ... existing fields ...
    memory_manager: Arc<Mutex<Option<Arc<MemoryManager>>>>,
}

// In process_local():
// 1. Query memory for relevant past context
let memory_context = memory_manager.query_context(&user_id, &user_text, 3)?;

// 2. Build enhanced prompt with memory
let system_prompt = format!(
    "You're a sarcastic gamer friend...\n\nYou remember these past conversations:\n{}",
    format_memory_context(&memory_context)
);

// 3. After getting response, store both user message and bot response
memory_manager.store_message(&user_id, &user_text, false)?;
memory_manager.store_message(&user_id, &response, true)?;
```

**Modify: `src-tauri/src/discord_conversation.rs`**
- Pass user_id from Discord to onichan for memory association
- Store messages after successful processing

### Phase 3: Build Configuration

**Modify: `src-tauri/tauri.conf.json`**
Add memory-sidecar to externalBin:
```json
"bundle": {
  "externalBin": [
    "binaries/llm-sidecar",
    "binaries/tts-sidecar",
    "binaries/discord-sidecar",
    "binaries/memory-sidecar"
  ]
}
```

**Add build script** for memory-sidecar (similar to existing sidecars)

### Phase 4: Periodic Cleanup

**Modify: `src-tauri/src/lib.rs`**
- Spawn background task on app startup
- Run `cleanup_expired(30)` every 24 hours
- Clean up on app shutdown

---

## Data Schema (LanceDB)

```rust
struct MemoryEntry {
    id: String,           // UUID
    user_id: String,      // Discord user ID (memory follows user across servers)
    content: String,      // Message text
    embedding: Vec<f32>,  // 384-dim MiniLM embedding
    is_bot: bool,         // true if bot's response, false if user message
    timestamp: i64,       // Unix timestamp for TTL
}
```

---

## Embedding Model

**Model**: `sentence-transformers/all-MiniLM-L6-v2`
- Size: ~22M parameters (~90MB on disk)
- Dimensions: 384
- Latency: ~15-50ms per message
- Quality: Good enough for conversational similarity

**Download**: Auto-download from HuggingFace Hub on first run, cache locally.

---

## Critical Files Summary

| File | Changes |
|------|---------|
| `src-tauri/memory-sidecar/` | NEW - Entire sidecar directory |
| `src-tauri/src/memory.rs` | NEW - Memory manager wrapper |
| `src-tauri/src/lib.rs` | Add memory module, initialize manager |
| `src-tauri/src/onichan.rs` | Integrate memory queries + storage |
| `src-tauri/src/discord_conversation.rs` | Pass user_id for memory association |
| `src-tauri/tauri.conf.json` | Add memory-sidecar to externalBin |

---

## Performance Expectations

- **Embedding generation**: 15-50ms per message
- **Vector search**: <5ms for 10,000 messages
- **Storage**: ~2KB per message (embedding + metadata)
- **Model memory**: ~120MB loaded

---

## Verification

1. Build memory-sidecar: `cd src-tauri/memory-sidecar && cargo build`
2. Test IPC manually: pipe JSON commands to stdin
3. Start Discord bot, have conversation
4. Verify messages are stored in LanceDB
5. Ask bot "what did we talk about?" - should retrieve relevant context
6. Wait 30+ days (or manually trigger cleanup) - verify old messages deleted
