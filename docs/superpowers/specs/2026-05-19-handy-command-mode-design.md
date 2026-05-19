# Handy — Command Mode (design)

**Date:** 2026-05-19
**Status:** Approved design, pre-implementation
**Target:** A fork of [`cjpais/Handy`](https://github.com/cjpais/Handy) based on current `main`.

## Problem

Handy is an offline speech-to-text app: press a hotkey, speak, and the transcript
is pasted into the focused text field. There is no way to *act* on speech — to say
something and have it routed to an AI agent or an arbitrary command rather than typed
out verbatim.

Upstream has rejected every attempt at this (PRs #739, #1411; issue #207) and is under
a feature freeze, so the work lives in a fork. The closest sanctioned extension point
is the unmerged transcription hook (PR #930): a `hooks/transcription` script that
receives the transcript on stdin and whose stdout replaces the pasted text.

## Goal

Add a **command mode**: a second global shortcut that records and transcribes speech
exactly like normal STT, then — instead of pasting — pipes the transcript to a
user-provided script. The script is the versatility layer: it can route to an AI
agent (`claude -p`), run a shell command, launch an app, etc. This keeps the feature
script-driven (no hardcoded agent integration) while being a real, separate mode.

## Non-goals (v1)

- **Paste-back.** Handy does not capture or insert the command script's stdout. The
  script owns all output (TTS, notifications, launching apps, or inserting text by its
  own means). Deferred to a possible v2.
- **Queueing.** Overlapping command invocations are not queued.
- **Result-display widget.** No UI to show what the script returned. Possible v2.
- No changes to the existing `transcribe` flow or the `hooks/transcription` hook.

## Design

### Behavior

1. A new `command` action with its own configurable global shortcut, alongside the
   existing `transcribe` action.
2. Pressing it records + transcribes through Handy's **existing** record → VAD →
   Whisper/Parakeet pipeline. No new audio code.
3. The flow diverges at the end of `TranscriptionManager`. For command mode, instead
   of post-processing + paste, Handy invokes `run_command_hook(transcript)`:
   - Feeds the **raw transcript** — post-VAD, *before* LLM post-processing. The script
     is itself the processing layer; an LLM should not rewrite "open my briefing"
     before the script sees it.
   - Passes the transcript via the script's **stdin**.
   - Passes context via environment variables so the script can route conditionally:
     - `HANDY_ACTIVE_APP` — name of the frontmost application.
     - `HANDY_CLIPBOARD` — current clipboard text contents.
     - `HANDY_SELECTED_TEXT` — selected text, if available cheaply on the platform;
       omitted otherwise.
   - Spawns the script **detached** (non-blocking). Handy does not `wait_with_output()`
     on the main path and does not read stdout for pasting.

### Concurrency

- The regular `transcribe` hotkey stays **fully usable** while a command script runs —
  the command process is detached and independent.
- A `command_running: AtomicBool` flag guards command mode against itself. Pressing the
  command hotkey while the flag is set is a **no-op for spawning** — it only surfaces
  the busy widget. No second script is started; invocations are not queued.

### Lifecycle / watcher thread

- After spawning the detached child, Handy starts a lightweight **watcher thread** that
  calls `child.wait()`, then:
  - clears `command_running`,
  - dismisses the busy overlay,
  - logs the exit code and stderr.
- A configurable **timeout** (default 120s) kills a hung child and clears the flag, so
  the busy widget can never get permanently stuck.

### Overlay / busy widget

Reuse the existing recording-overlay component. Add states:

- `command-recording` — shown while recording in command mode.
- `command-running` — busy spinner shown while the script executes.

When the command hotkey is pressed while in `command-running`, Handy surfaces this
busy state instead of starting a new recording.

### Cross-platform script resolution

`Command::new` on a bare, extensionless `hooks/command` will not run a shebang script
on Windows. The hook is resolved by first existing match:

- **Windows:** `hooks/command.cmd`, `hooks/command.bat`, `hooks/command.ps1`,
  `hooks/command.exe` (in that order).
- **Unix (macOS/Linux):** `hooks/command` with the executable bit set.

All resolved relative to the app data directory, mirroring `hooks/transcription`.

### Error handling

Every failure path logs and clears `command_running`; none disrupts normal STT:

- No `hooks/command` script found → log at debug, do nothing.
- Spawn fails → log error, clear flag, dismiss widget.
- Timeout exceeded → kill child, log warning, clear flag, dismiss widget.
- Non-zero exit → log error with exit code + stderr, clear flag, dismiss widget.

## Affected code

- `src-tauri/src/shortcut/` + `src-tauri/src/actions.rs` — register the new `command`
  action and its binding.
- `src-tauri/src/settings.rs` — persist the command-mode keybinding and the timeout
  setting; backfill defaults on existing installs.
- `src-tauri/src/managers/transcription.rs` — branch the end-of-pipeline path for
  command mode; call `run_command_hook`.
- `src-tauri/src/command_hook.rs` *(new)* — hook resolution, env assembly, detached
  spawn, watcher thread, timeout.
- `src-tauri/src/overlay.rs` + the overlay React component — `command-recording` and
  `command-running` states.

## Testing

- **Rust unit tests:** hook path resolution per-platform; environment-variable
  assembly from a known context.
- **Manual test script:** a `hooks/command` that appends its stdin and env vars to a
  log file — verifies the script is invoked, receives stdin + env, that the regular
  transcribe hotkey still works during execution, and that the busy flag toggles on
  spawn and clears on exit.
- **Timeout test:** a script that sleeps past the timeout — verifies the child is
  killed and the busy widget clears.

## Open questions

None blocking. v2 candidates: paste-back / result-display widget, invocation queueing.
