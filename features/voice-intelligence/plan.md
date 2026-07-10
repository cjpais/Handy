# Voice intelligence layer: Ollama engine, MCP tool calls, vocabulary learning, voice edits

## Context

Building on the just-landed wake-word feature, Handy graduates from "dictation → paste" to a voice intelligence platform:

- **F1 — Ollama engine**: first-class local LLM provider and the default brain for F2–F4. (Handy's `custom` provider already defaults to `http://localhost:11434/v1` — this formalizes it.)
- **F2 — Voice → MCP tool calls**: speak a command, an LLM picks an MCP tool + arguments from connected servers (e.g. macOS-control server), Handy executes it.
- **F3 — Personal vocabulary learning**: mine transcription history with Ollama for names/jargon/project names; suggest them for the existing `custom_words` fuzzy-correction dictionary.
- **F4 — Voice-editable outputs**: "scratch that", "make it shorter", "bullet it" spoken within ~30 s of a dictation operate on the last pasted output.

**User-confirmed decisions**: separate trigger for command mode (new `voice_command` binding; second wake word as follow-up) · confirm-risky/auto-run-safe tool safety with growing allowlist · vocabulary is suggest-and-approve (nothing auto-added) · edit commands only within a time-boxed window after a paste.

**Verified foundation**: `llm_client.rs` is OpenAI-compatible with strict JSON-schema output but no `tools` field (added in Phase D); providers migrate via `ensure_post_process_defaults`; `apply_custom_words` fuzzy correction exists; history has `get_latest_completed_entry()`; paste knows the final text but has no last-paste tracking; the new-binding recipe is verified (ACTION_MAP + `is_transcribe_binding` + default `ShortcutBinding` auto-merged + generic `<ShortcutInput>`); `tauri-plugin-dialog` already present for confirmations; rmcp (official MCP Rust SDK) provides stdio client transport + `list_tools`/`call_tool`; tokio + reqwest already in-tree.

## Phase order: A (F1) → B (F4) → C (F3) → D (F2)
A first because B–D consume the intelligence-provider resolution + structured-output helper. B second: zero new crates, highest daily value. C third: pure consumer of A + history. D last: only phase needing `tools` support, the rmcp dep, process lifecycle, and the largest UI. Each phase is an independently shippable PR.

---

## Phase A — Ollama provider + intelligence layer (~1.5–2 d)

1. **`ollama` provider entry** in `settings.rs::default_post_process_providers()` (inserted before `custom`): base_url `http://localhost:11434/v1`, no key, `allow_base_url_edit: true`, `supports_structured_output: true`. `ensure_post_process_defaults` migrates it into existing stores. Extend the reasoning-disable match in `actions.rs:175` to `"custom" | "ollama"`.
2. **Separate intelligence selection** (post-processing can stay on OpenAI while intent/vocab/edit run local): AppSettings `intelligence_provider_id: String` (default `"ollama"`), `intelligence_model: String` (default `""`, UI recommends `qwen2.5:7b-instruct`).
3. **New module `src-tauri/src/intelligence/mod.rs`**: `IntelligenceContext { provider, api_key, model }`, `resolve_context(settings)`, `complete_structured(ctx, system, user, schema) -> Result<Value, IntelligenceError>` (wraps `send_chat_completion_with_schema`, reasoning "none", defensive JSON parse — strip fences), `health_check(ctx) -> Vec<String>` (wraps `fetch_models`).
4. **Commands** (`src-tauri/src/commands/intelligence.rs`): `change_intelligence_provider`, `change_intelligence_model`, `test_intelligence_connection`. **UI**: `src/components/settings/intelligence/IntelligenceSettings.tsx` (provider select, base-url edit, model dropdown from test connection, live status). i18n `settings.intelligence.*`.

## Phase B — Voice-editable outputs (~3–4 d)

1. **Last-output tracking** — `src-tauri/src/intelligence/last_output.rs`: `LastOutput { text, char_count, pasted_at: Instant, history_id, paste_method }` in a managed `LastOutputState(Mutex<Option<…>>)`. Change `clipboard::paste` to return the actually-typed text (`PastedText`) — the trailing-space setting means char_count must come from what was really typed; single call site in `actions.rs`. Never record when auto_submit fired or PasteMethod is None/ExternalScript.
2. **Edit-intent detection** — `src-tauri/src/intelligence/edit.rs`: `EditIntent { Delete, Rewrite(RewriteStyle{Shorter,Bullets,Formal,Casual,Expand,Custom}) }`; `detect_fast` (normalized phrase table: "scratch that"/"delete that"/"undo that"/"never mind" → Delete; "make it shorter", "bullet it", "make it formal"… → Rewrite; <1 ms); `is_edit_candidate` (<8 words AND edit-ish verb start) gates the LLM fallback `detect_llm` (schema `{intent, style}`); `rewrite(ctx, last_text, style)`. Any IntelligenceError falls through to plain dictation — a dead Ollama never blocks pasting.
3. **Deletion mechanics** — `clipboard.rs::delete_chars(app, char_count)`: Backspace clicks via `EnigoState` (input.rs), chunked ~50 keys with ~5 ms sleeps.
4. **Pipeline hook** — in `TranscribeAction::stop` after transcription, before post-process/paste: if `voice_edit_enabled` && LastOutput fresh (< `voice_edit_window_secs`) && !auto_submit → detect. Delete → main-thread `delete_chars`, clear state. Rewrite → processing overlay → `rewrite()` inside `complete_unless_cancelled` → delete_chars + paste replacement → update LastOutput + history. None → normal flow. Documented limitation: cursor moves since paste can mis-delete — mitigated by opt-in + window.
5. **Settings/UI**: `voice_edit_enabled: bool` (false), `voice_edit_window_secs: u64` (30); commands; `VoiceEdit.tsx` with auto-submit incompatibility hint; i18n `settings.voiceEdit.*`.

## Phase C — Vocabulary learning (~2–3 d)

1. **Miner** — `src-tauri/src/intelligence/vocab.rs`: `VocabSuggestion { word, kind, evidence_count, first_seen, status: Pending|Dismissed }` (specta::Type). `VocabMiner::maybe_run()` fire-and-forget after `hm.save_entry` and ~60 s after launch; triggers when ≥25 new entries since `last_scanned_id` OR >24 h. `mine()`: `get_history_entries` batch → `complete_structured` with schema `{"words":[{word,kind,evidence_count}]}` → filter existing custom_words (case-insensitive), dismissed words, len ≥3; dedup; cap 50 pending. Storage: dedicated `vocab_store.json` (tauri_plugin_store: suggestions, last_scanned_id, last_run_ms) — churny data stays out of AppSettings; dismissals persist.
2. **Commands/UI**: `get_vocab_suggestions`, `resolve_vocab_suggestion(word, accept)` (accept routes through existing `update_custom_words` — nothing auto-applied), `run_vocab_scan_now`; tauri_specta event `VocabSuggestionsUpdated`. UI `VocabSuggestions.tsx` beside CustomWords.tsx: Add/Dismiss chips, Scan-now, Ollama-unavailable state. i18n `settings.vocabSuggestions.*`.

## Phase D — Voice → MCP tool calls (~1.5–2 wk)

1. **Tool calling in `llm_client.rs`** (additive; `skip_serializing_if` keeps existing requests byte-identical): `ToolDefinition`/`ToolCall` structs (OpenAI function-calling shape), `ChatCompletionRequest += tools, tool_choice`, response `tool_calls` parsing, `ChatOutcome { Text, ToolCalls }`, `send_chat_completion_with_tools(...)`.
2. **`src-tauri/src/managers/mcp.rs`** — dep `rmcp` (features `client`, `transport-child-process`, pinned): `McpServerConfig { id, name, command, args, env, enabled }` (in settings), `McpToolInfo { server_id, name, description, input_schema, read_only_hint }` (from `annotations.readOnlyHint`), `McpManager { connections, catalog }` with `sync_with_settings()` (spawn/kill/restart via `TokioChildProcess` + `list_tools`), `call_tool`, `test_server`, `shutdown()` on exit. Stdio transport only in v1. Managed `Arc` in lib.rs; initial sync when `mcp_enabled`.
3. **Intent pipeline** — `src-tauri/src/intelligence/intent.rs`: `run_voice_command(app, transcript) -> CommandOutcome { Executed, Denied, NoToolMatched, Failed }`. Catalog → tools named `{server_id}__{tool}` → `send_chat_completion_with_tools` (system prompt frames transcript as untrusted data, pick exactly one tool or none) → validate returned name against catalog (reject hallucinations) + required-args check against input_schema → safety gate: auto-run iff `read_only_hint == Some(true)` OR in `mcp_auto_approved_tools`; else main-thread `tauri-plugin-dialog` confirmation ("Run once" / "Always allow" → persists / "Cancel") showing exact tool + args JSON → `call_tool` → feedback sound + result toast event + history entry. NoToolMatched → toast only; never paste command speech as text.
4. **Trigger wiring**: refactor `TranscribeAction { post_process }` → `{ mode: TranscribeMode { Paste{post_process}, VoiceCommand } }` (recording/transcription halves shared; VoiceCommand branch replaces paste with `run_voice_command` inside `complete_unless_cancelled`). ACTION_MAP += `"voice_command"`; `is_transcribe_binding` (transcription_coordinator.rs:52) += it; default binding (`option+ctrl+space` macOS / `ctrl+alt+space` elsewhere — auto-merged into old stores); registration gated on `mcp_enabled` (mirror shortcut/mod.rs:396 + the change-setting live-register pattern at :880). History: rusqlite migration `ADD COLUMN entry_kind TEXT NOT NULL DEFAULT 'dictation'`; commands saved as `'command'`. Follow-up (not v1): second wake word routed to voice_command via a `COMMAND_WAKE_SOURCE` tag — wake-word machinery reused wholesale.
5. **Settings/UI**: `mcp_enabled`, `mcp_servers`, `mcp_auto_approved_tools`; commands `get/upsert/remove_mcp_server`, `set_mcp_server_enabled`, `test_mcp_server`, `get_mcp_tool_catalog`, `remove_auto_approved_tool`, `change_mcp_enabled`. UI `src/components/settings/mcp/`: McpSettings.tsx (toggle + `<ShortcutInput shortcutId="voice_command" grouped />`), McpServerForm.tsx (structured Claude-Desktop-style form: name/command/args/env + Test showing discovered tools), McpToolCatalog.tsx (read-only vs risky badges), AutoApprovedTools.tsx (revokable chips). i18n `settings.mcp.*`.

## Cross-cutting (every phase)
Register commands in `collect_commands!` (lib.rs ~548) → debug build regenerates `src/bindings.ts` → `settingUpdaters` in `src/stores/settingsStore.ts` → en i18n keys + all 21 locales (`bun run check:translations` enforces) → `bun run lint` + prettier + `cargo fmt`/clippy.

## Verification
- **A**: `test_intelligence_connection` against a running Ollama lists models; post-process with provider=ollama works end-to-end (`bun run tauri dev`, dictate with post-process binding).
- **B**: dictate → say "scratch that" within 30 s → pasted text deleted; "make it shorter" → replaced with shorter text; edit phrases after the window paste as normal text; auto_submit users unaffected; Ollama stopped → edit phrases paste as plain dictation. cargo unit tests for the phrase table + `is_edit_candidate`.
- **C**: seed history with jargon-heavy entries → `run_vocab_scan_now` → suggestions appear; Add moves the word into custom_words (visible fuzzy correction on next dictation); Dismiss persists across restarts.
- **D**: configure a benign MCP server (e.g. filesystem or applescript server); "voice command" trigger + "list my downloads folder" → read-only tool auto-runs with result toast; a write tool prompts confirmation; "Always allow" skips the prompt next time; hallucinated tool names rejected; killing the server shows a clear error. cargo tests for tool-name validation + allowlist matching.
- Regression: full `cargo test`, existing dictation + wake-word flows unaffected when all new toggles are off.

## Risks
1. **rmcp API instability** — pin exact version; confine rmcp types to managers/mcp.rs; smoke-test 2–3 popular servers.
2. **Ollama down** — health checks; B/C degrade silently (plain dictation / skipped scan); D shows explicit "Ollama unreachable at {base_url}" toast; settings show live status.
3. **Tool hallucination / prompt injection via speech** — validate against catalog + input_schema; transcript framed as untrusted data; risky tools always confirmed until allowlisted; dialog shows exact args.
4. **Backspace mismatch (F4)** — exact char count from paste return incl. trailing space; opt-in, windowed, disabled for auto_submit/None/ExternalScript; cursor-move limitation documented.
5. **Latency** — F4 fast path <1 ms; LLM only for short edit-shaped utterances; all LLM calls cancellable via `complete_unless_cancelled`.
6. **Model quality** — recommend qwen2.5:7b-instruct / llama3.1:8b (tool calling + json_schema verified on Ollama); parse defensively.

## Effort
A 1.5–2 d · B 3–4 d · C 2–3 d · D 1.5–2 wk · **total ~4–5 weeks** incl. i18n + QA.

## Step 0 when implementing
Copy this plan to `features/voice-intelligence/plan.md` (user's global planning convention).
