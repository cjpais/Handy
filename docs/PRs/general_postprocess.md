# General Command Filter for Post-Processing (stdin/stdout)

## Why

This PR adds a local, provider-agnostic post-processing path so users can transform transcriptions with any command-line tool, not just LLM providers.

The goal is to support workflows like formatting, cleanup, custom replacement logic, and language/domain-specific transforms while keeping processing local and scriptable.

## What changed

### 1) New settings

Added new persisted settings in Rust + TS bindings:

- `command_filter_enabled: bool`
- `command_filter_scope: "transcribe" | "post_process" | "both"`
- `command_filter_order: "before_llm" | "after_llm"`
- `command_filter_executable: string`
- `command_filter_args: string[]`
- `command_filter_timeout_ms: number`

Defaults:

- enabled: `false`
- scope: `both`
- order: `after_llm`
- executable: empty
- args: empty
- timeout: `10000`

### 2) New backend executor

Added `src-tauri/src/command_filter.rs`.

- Runs `executable + args` directly (no shell string execution).
- Expands args/executable beginning with `~/` (or `~`) to the current user's home directory before execution.
- Writes transcription text to child `stdin` exactly (no forced newline).
- Reads `stdout`/`stderr`.
- Enforces timeout.
- Returns one of:
  - `Applied(text)`
  - `CancelledEmpty` (when trimmed stdout is empty)
  - `Failed(reason)`

### 3) Pipeline order behavior

In `src-tauri/src/actions.rs`, command filtering is integrated into the transcription pipeline:

1. transcription result
2. optional Chinese variant conversion
3. command filter (if scope applies and order=`before_llm`)
4. optional LLM post-processing (only when post-process mode is enabled and secondary hotkey used)
5. command filter (if scope applies and order=`after_llm`)

Failure behavior:

- Filter failures log and fallback to previous text.

Empty-output behavior:

- If filter succeeds but returns trimmed-empty stdout, paste is canceled.
- History still saves the original transcription.

### 4) Shortcut registration behavior

Secondary shortcut (`transcribe_with_post_process`) is now registered when either:

- AI post-processing is enabled, **or**
- command filter is enabled and scope includes `post_process`.

This logic is centralized via `should_register_secondary_shortcut()` in settings.

### 5) UI changes

- Post Process sidebar item is now always visible.
- Removed old Post Processing toggle from Advanced/Experimental section.
- Added **Modes** section in Post Process page:
  - AI Post-Processing toggle
  - Command Filter toggle
- Added **Command Filter** section:
  - scope selector
  - order selector
  - executable input
  - args textarea (one arg per line)
  - timeout input (ms)

## Behavior matrix

### Scope × hotkey

| Scope          | Normal transcribe hotkey | Secondary post-process hotkey |
| -------------- | ------------------------ | ----------------------------- |
| `transcribe`   | runs filter              | does not run filter           |
| `post_process` | does not run filter      | runs filter                   |
| `both`         | runs filter              | runs filter                   |

### Order relative to LLM

- `before_llm`: filter runs before LLM post-processing.
- `after_llm`: filter runs after LLM post-processing (default).

(Chinese conversion still runs before either filter order.)

### Failure fallback

- Spawn/IO/non-zero/timeout failures: fallback to previous text and continue.

### Empty-output cancel

- Trimmed-empty stdout: cancel paste.
- Keep history entry with original transcription text.

## Compatibility

- Backward-compatible with existing settings files.
- New settings are defaulted via serde defaults.
- No DB migration required.
- Existing behavior unchanged until Command Filter is enabled.

## Testing performed

### Rust

- Added unit tests for:
  - scope matching
  - secondary shortcut enable logic
  - command filter success/failure/timeout/empty-output

Commands run:

- `cargo check -q` (pass)
- `cargo test -q` (pass)

### Frontend / i18n

Commands run:

- `bun run lint` (pass)
- `bun run check:translations` (pass)
- `bun run build` (pass)

## Known limitations / follow-ups

- Only one command filter is supported in this PR (no filter chain yet).
- Errors are log-only (no user-facing toast).
- Non-English locale strings for newly added keys are currently English placeholders where untranslated.
