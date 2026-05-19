# Command Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a second global shortcut to Handy that records and transcribes speech, then pipes the raw transcript to a user-provided script instead of pasting it — making speech actionable (route to an AI agent, run a command, launch apps).

**Architecture:** Reuse Handy's existing record → VAD → transcribe pipeline. A new `RecordingMode::Command` variant on the recording action diverges at the end: instead of post-processing + paste, it spawns a user script (`hooks/command`) **detached** (non-blocking), passing the transcript on stdin. A watcher thread enforces a timeout and clears a global busy flag. The recording overlay gains `command-recording` and `command-running` states. The regular transcribe hotkey stays fully usable while a command script runs.

**Tech Stack:** Rust (Tauri v2 backend), React + TypeScript (overlay frontend), `cargo test` for unit tests.

**Spec:** `docs/superpowers/specs/2026-05-19-handy-command-mode-design.md`

---

## Scope note (v1 vs spec)

The spec lists three context environment variables. **v1 implements `HANDY_CLIPBOARD` only** (trivially available via the Tauri clipboard plugin). `HANDY_ACTIVE_APP` and `HANDY_SELECTED_TEXT` require platform-specific window/selection APIs and are deferred to v2 — the script can query those itself in the meantime. The `CommandContext` struct and `build_env` function are designed for all three fields so v2 is purely additive.

## File Structure

**New files:**
- `src-tauri/src/command_hook.rs` — hook discovery, cross-platform command construction, context env assembly, the detached runner + watcher thread, and the global busy flag. One module, one responsibility: "run the user's command hook."

**Modified files:**
- `src-tauri/src/lib.rs` — register `mod command_hook;`.
- `src-tauri/src/settings.rs` — add the `command` shortcut binding to defaults and a `command_hook_timeout_secs` setting.
- `src-tauri/src/overlay.rs` — add `show_command_recording_overlay` and `show_command_running_overlay`.
- `src-tauri/src/transcription_coordinator.rs` — rename `is_transcribe_binding` → `is_recording_binding` and include `"command"`.
- `src-tauri/src/shortcut/handler.rs` — update the call site for the rename.
- `src-tauri/src/actions.rs` — refactor `TranscribeAction` → `RecordingAction` with a `RecordingMode` enum; add the command branch; register `command` in `ACTION_MAP`.
- `src/overlay/RecordingOverlay.tsx` + `RecordingOverlay.css` — render the two new overlay states.
- `src/i18n/locales/en/translation.json` — add the `overlay.commandRunning` string.
- `README.md` — document the `hooks/command` extension point.

## Conventions

- Commit messages use Conventional Commits (`feat:`, `refactor:`, `docs:`) — matches the repo history.
- Rust unit tests live in a `#[cfg(test)] mod tests` block at the bottom of the same file (the established pattern in `portable.rs`, `settings.rs`).
- Run Rust commands from `src-tauri/`: `cd src-tauri`.

---

## Task 1: command_hook module — hook discovery

**Files:**
- Create: `src-tauri/src/command_hook.rs`
- Modify: `src-tauri/src/lib.rs` (add module declaration)

- [ ] **Step 1: Register the module**

In `src-tauri/src/lib.rs`, the module declarations are an alphabetical-ish block at the top (lines ~1-21). Add this line after `mod clipboard;`:

```rust
mod command_hook;
```

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/command_hook.rs` with only the test module and a stub:

```rust
//! Command mode: runs a user-provided script with the transcript on stdin.
//!
//! When the user triggers the `command` shortcut, Handy records and transcribes
//! as normal, then — instead of pasting — pipes the raw transcript to an
//! executable `hooks/command` script in the app data directory. The script runs
//! detached; Handy does not read its output.

use std::path::{Path, PathBuf};

/// Candidate hook file names inside the `hooks/` directory, in resolution order.
///
/// Windows cannot execute an extensionless shebang script, so platform-specific
/// extensions are tried; the first existing file wins.
#[cfg(windows)]
const HOOK_CANDIDATES: &[&str] = &["command.cmd", "command.bat", "command.ps1", "command.exe"];
#[cfg(not(windows))]
const HOOK_CANDIDATES: &[&str] = &["command"];

/// Returns the first existing command-hook file inside `hooks_dir`, or `None`.
pub fn resolve_hook(hooks_dir: &Path) -> Option<PathBuf> {
    HOOK_CANDIDATES
        .iter()
        .map(|name| hooks_dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hook_returns_none_when_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_hook(dir.path()), None);
    }

    #[test]
    fn resolve_hook_finds_an_existing_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let name = HOOK_CANDIDATES[0];
        let hook = dir.path().join(name);
        std::fs::write(&hook, b"#!/bin/sh\ncat\n").unwrap();
        assert_eq!(resolve_hook(dir.path()), Some(hook));
    }
}
```

- [ ] **Step 3: Ensure the `tempfile` dev-dependency exists**

Run: `cd src-tauri && cargo metadata --format-version=1 --no-deps | grep -o '"tempfile"' | head -1`
Expected: prints `"tempfile"`. If it prints nothing, add to `src-tauri/Cargo.toml` under `[dev-dependencies]` (create the section if absent):

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test command_hook -- --nocapture`
Expected: `resolve_hook_returns_none_when_dir_is_empty` and `resolve_hook_finds_an_existing_candidate` both PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/command_hook.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(command-hook): add hook file discovery"
```

---

## Task 2: command_hook module — cross-platform command builder

A resolved hook path cannot always be executed directly: on Windows, `.cmd`/`.bat` need `cmd.exe /C` and `.ps1` needs `powershell.exe -File`. This task adds `build_command`.

**Files:**
- Modify: `src-tauri/src/command_hook.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/command_hook.rs`:

```rust
    #[test]
    fn build_command_runs_unix_script_directly() {
        let cmd = build_command(Path::new("/data/hooks/command"));
        assert_eq!(cmd.get_program(), "/data/hooks/command");
    }

    #[cfg(windows)]
    #[test]
    fn build_command_wraps_windows_script_types() {
        assert_eq!(
            build_command(Path::new("C:\\h\\command.ps1")).get_program(),
            "powershell.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.cmd")).get_program(),
            "cmd.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.bat")).get_program(),
            "cmd.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.exe")).get_program(),
            "C:\\h\\command.exe"
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test command_hook`
Expected: FAIL — `cannot find function build_command in this scope`.

- [ ] **Step 3: Implement `build_command`**

Add to `src-tauri/src/command_hook.rs` after `resolve_hook` (and add `use std::process::Command;` to the imports at the top):

```rust
use std::process::Command;

/// Builds the `Command` that executes `hook_path`.
///
/// On Unix the script is run directly (it needs the executable bit + a shebang).
/// On Windows, `.cmd`/`.bat` are run via `cmd.exe /C` and `.ps1` via PowerShell,
/// because the OS cannot execute those file types directly. `.exe` runs directly.
fn build_command(hook_path: &Path) -> Command {
    #[cfg(windows)]
    {
        let ext = hook_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "ps1" => {
                let mut cmd = Command::new("powershell.exe");
                cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                    .arg(hook_path);
                return cmd;
            }
            "cmd" | "bat" => {
                let mut cmd = Command::new("cmd.exe");
                cmd.arg("/C").arg(hook_path);
                return cmd;
            }
            _ => {}
        }
    }
    Command::new(hook_path)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test command_hook`
Expected: all `command_hook` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/command_hook.rs
git commit -m "feat(command-hook): add cross-platform command builder"
```

---

## Task 3: command_hook module — context env vars

**Files:**
- Modify: `src-tauri/src/command_hook.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/command_hook.rs`:

```rust
    #[test]
    fn build_env_emits_only_present_fields() {
        let ctx = CommandContext {
            active_app: None,
            clipboard: Some("hello".to_string()),
            selected_text: None,
        };
        assert_eq!(
            build_env(&ctx),
            vec![("HANDY_CLIPBOARD".to_string(), "hello".to_string())]
        );
    }

    #[test]
    fn build_env_is_empty_when_no_context() {
        let ctx = CommandContext {
            active_app: None,
            clipboard: None,
            selected_text: None,
        };
        assert!(build_env(&ctx).is_empty());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test command_hook`
Expected: FAIL — `cannot find type CommandContext` / `cannot find function build_env`.

- [ ] **Step 3: Implement `CommandContext` and `build_env`**

Add to `src-tauri/src/command_hook.rs` after `build_command`:

```rust
/// Context handed to the command hook as environment variables.
///
/// v1 populates `clipboard` only; `active_app` and `selected_text` are kept so
/// v2 can fill them without changing this interface.
pub struct CommandContext {
    pub active_app: Option<String>,
    pub clipboard: Option<String>,
    pub selected_text: Option<String>,
}

/// Builds the `(name, value)` environment pairs for the hook. Only fields that
/// are `Some` are emitted, so a script can distinguish "absent" from "empty".
pub fn build_env(ctx: &CommandContext) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(v) = &ctx.active_app {
        env.push(("HANDY_ACTIVE_APP".to_string(), v.clone()));
    }
    if let Some(v) = &ctx.clipboard {
        env.push(("HANDY_CLIPBOARD".to_string(), v.clone()));
    }
    if let Some(v) = &ctx.selected_text {
        env.push(("HANDY_SELECTED_TEXT".to_string(), v.clone()));
    }
    env
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test command_hook`
Expected: all `command_hook` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/command_hook.rs
git commit -m "feat(command-hook): add context environment assembly"
```

---

## Task 4: command_hook module — detached runner + busy flag

This task adds the runner that spawns the hook detached, writes the transcript to its stdin, and a watcher thread that enforces a timeout and clears the busy flag. There is no unit test for the spawn itself (it depends on a live `AppHandle`); it is verified by `cargo build` here and by the manual test in Task 11.

**Files:**
- Modify: `src-tauri/src/command_hook.rs`

- [ ] **Step 1: Add imports**

At the top of `src-tauri/src/command_hook.rs`, replace the existing `use` lines with:

```rust
use crate::settings::get_settings;
use log::{debug, error, warn};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
```

- [ ] **Step 2: Add the busy flag and `is_command_running`**

Add after the `HOOK_CANDIDATES` constants in `src-tauri/src/command_hook.rs`:

```rust
/// True while a command hook process is running. Guards command mode against
/// re-entry: pressing the command shortcut again while set is a no-op.
static COMMAND_RUNNING: AtomicBool = AtomicBool::new(false);

/// Returns true while a command hook is executing.
pub fn is_command_running() -> bool {
    COMMAND_RUNNING.load(Ordering::SeqCst)
}
```

- [ ] **Step 3: Implement `run_command_hook`**

Add to the bottom of `src-tauri/src/command_hook.rs` (above the `#[cfg(test)] mod tests` block):

```rust
/// Runs the user's command hook with `transcript` on stdin, detached.
///
/// Non-blocking: the hook runs on its own process and a watcher thread waits
/// for it. Handy does not read the hook's stdout/stderr (they go to the null
/// device, which also prevents a full-pipe deadlock). The watcher kills the
/// process if it exceeds `command_hook_timeout_secs` and always clears the
/// busy flag and overlay when the process ends.
///
/// If no hook file exists, this just hides the overlay and returns.
pub fn run_command_hook(app: &AppHandle, transcript: String) {
    let hooks_dir = match crate::portable::app_data_dir(app) {
        Ok(dir) => dir.join("hooks"),
        Err(e) => {
            error!("Command hook: failed to resolve app data dir: {}", e);
            crate::overlay::hide_recording_overlay(app);
            return;
        }
    };

    let Some(hook_path) = resolve_hook(&hooks_dir) else {
        debug!("Command hook: no hook file in {:?}", hooks_dir);
        crate::overlay::hide_recording_overlay(app);
        return;
    };

    let ctx = CommandContext {
        active_app: None,
        clipboard: app.clipboard().read_text().ok(),
        selected_text: None,
    };

    let timeout = Duration::from_secs(get_settings(app).command_hook_timeout_secs);

    let mut command = build_command(&hook_path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in build_env(&ctx) {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            error!("Command hook: failed to spawn {:?}: {}", hook_path, e);
            crate::overlay::hide_recording_overlay(app);
            return;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(transcript.as_bytes()) {
            error!("Command hook: failed to write transcript to stdin: {}", e);
        }
        // stdin is dropped here, closing the pipe so the script sees EOF.
    }

    COMMAND_RUNNING.store(true, Ordering::SeqCst);
    crate::overlay::show_command_running_overlay(app);
    debug!("Command hook: started {:?}", hook_path);

    let app = app.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    debug!("Command hook: exited with {}", status);
                    break;
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        warn!("Command hook: timed out after {:?}, killing", timeout);
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    error!("Command hook: error waiting for process: {}", e);
                    break;
                }
            }
        }
        COMMAND_RUNNING.store(false, Ordering::SeqCst);
        crate::overlay::hide_recording_overlay(&app);
    });
}
```

> Note: `get_settings`, `crate::overlay::show_command_running_overlay`, and the
> `command_hook_timeout_secs` setting do not exist yet — they are added in
> Tasks 5 and 6. This task will not compile on its own; build verification
> happens after Task 6. Commit the code now so the history stays granular.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/command_hook.rs
git commit -m "feat(command-hook): add detached runner with timeout watcher"
```

---

## Task 5: Settings — command binding + timeout setting

**Files:**
- Modify: `src-tauri/src/settings.rs`

- [ ] **Step 1: Add the `command_hook_timeout_secs` field to `AppSettings`**

In `src-tauri/src/settings.rs`, find the `AppSettings` struct — it contains the line `pub bindings: HashMap<String, ShortcutBinding>,` (around line 339). Immediately below that line, add:

```rust
    #[serde(default = "default_command_hook_timeout_secs")]
    pub command_hook_timeout_secs: u64,
```

- [ ] **Step 2: Add the default function**

In `src-tauri/src/settings.rs`, find the block of `default_*` functions (e.g. `fn default_paste_delay_ms() -> u64 { ... }` near line 482). Add next to them:

```rust
fn default_command_hook_timeout_secs() -> u64 {
    120
}
```

- [ ] **Step 3: Add the `command` binding to defaults**

In `src-tauri/src/settings.rs`, find the `cancel` binding insert (the `bindings.insert("cancel".to_string(), ShortcutBinding { ... });` block, around lines 756-765). Immediately **after** that block and **before** the `AppSettings {` literal, add:

```rust
    #[cfg(target_os = "windows")]
    let default_command_shortcut = "ctrl+alt+space";
    #[cfg(target_os = "macos")]
    let default_command_shortcut = "option+ctrl+space";
    #[cfg(target_os = "linux")]
    let default_command_shortcut = "ctrl+alt+space";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let default_command_shortcut = "alt+ctrl+space";

    bindings.insert(
        "command".to_string(),
        ShortcutBinding {
            id: "command".to_string(),
            name: "Command".to_string(),
            description: "Sends your speech to the hooks/command script instead of typing it."
                .to_string(),
            default_binding: default_command_shortcut.to_string(),
            current_binding: default_command_shortcut.to_string(),
        },
    );
```

- [ ] **Step 4: Add the field to the `AppSettings` literal**

In the same function, in the `AppSettings { ... }` struct literal that follows (it starts `AppSettings {` with `bindings,` then `push_to_talk: true,`), add this line immediately after `push_to_talk: true,`:

```rust
        command_hook_timeout_secs: default_command_hook_timeout_secs(),
```

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles. (Pre-existing warnings are fine; `command_hook.rs` errors about `show_command_running_overlay` are expected and resolved in Task 6.)

If `cargo check` reports errors **only** from `command_hook.rs` referencing `show_command_running_overlay` / `hide_recording_overlay`, that is expected at this stage — proceed. Any error originating in `settings.rs` must be fixed before committing.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/settings.rs
git commit -m "feat(settings): add command shortcut binding and hook timeout"
```

---

## Task 6: Overlay (Rust) — command overlay states

**Files:**
- Modify: `src-tauri/src/overlay.rs`

- [ ] **Step 1: Add the two overlay helper functions**

In `src-tauri/src/overlay.rs`, find `show_processing_overlay` (around line 353):

```rust
/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}
```

Immediately after it, add:

```rust
/// Shows the overlay in the command-mode recording state
pub fn show_command_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "command-recording");
}

/// Shows the overlay in the command-running (busy) state
pub fn show_command_running_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "command-running");
}
```

- [ ] **Step 2: Verify the backend compiles end-to-end**

Run: `cd src-tauri && cargo build`
Expected: build succeeds. This is the first point where `command_hook.rs` (Task 4) compiles fully, because `show_command_running_overlay`, `hide_recording_overlay`, `get_settings`, and `command_hook_timeout_secs` now all exist.

If the build fails, fix the reported error before continuing — do not proceed with a broken build.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/overlay.rs
git commit -m "feat(overlay): add command-mode overlay states"
```

---

## Task 7: Overlay (frontend) — command states

**Files:**
- Modify: `src/overlay/RecordingOverlay.tsx`
- Modify: `src/overlay/RecordingOverlay.css`
- Modify: `src/i18n/locales/en/translation.json`

- [ ] **Step 1: Add the i18n string**

In `src/i18n/locales/en/translation.json`, find the `"overlay"` object — it contains `"transcribing"` and `"processing"` keys. Add a `commandRunning` key. Change:

```json
    "transcribing": "Transcribing...",
    "processing": "Processing..."
```

to:

```json
    "transcribing": "Transcribing...",
    "processing": "Processing...",
    "commandRunning": "Running command..."
```

(Other locales fall back to English automatically; translating them is out of scope.)

- [ ] **Step 2: Extend the `OverlayState` type**

In `src/overlay/RecordingOverlay.tsx`, change line 14:

```tsx
type OverlayState = "recording" | "transcribing" | "processing";
```

to:

```tsx
type OverlayState =
  | "recording"
  | "transcribing"
  | "processing"
  | "command-recording"
  | "command-running";
```

- [ ] **Step 3: Update `getIcon`**

In `src/overlay/RecordingOverlay.tsx`, replace the `getIcon` function:

```tsx
  const getIcon = () => {
    if (state === "recording") {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };
```

with:

```tsx
  const isRecordingState =
    state === "recording" || state === "command-recording";

  const getIcon = () => {
    if (isRecordingState) {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };
```

- [ ] **Step 4: Update the rendered body**

In `src/overlay/RecordingOverlay.tsx`, replace the whole `return ( ... )` JSX block (lines ~73-116) with:

```tsx
  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""} ${
        state === "command-recording" || state === "command-running"
          ? "command-mode"
          : ""
      }`}
    >
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {isRecordingState && (
          <div className="bars-container">
            {levels.map((v, i) => (
              <div
                key={i}
                className="bar"
                style={{
                  height: `${Math.min(20, 4 + Math.pow(v, 0.7) * 16)}px`,
                  transition: "height 60ms ease-out, opacity 120ms ease-out",
                  opacity: Math.max(0.2, v * 1.7),
                }}
              />
            ))}
          </div>
        )}
        {state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
        {state === "processing" && (
          <div className="transcribing-text">{t("overlay.processing")}</div>
        )}
        {state === "command-running" && (
          <div className="transcribing-text">
            {t("overlay.commandRunning")}
          </div>
        )}
      </div>

      <div className="overlay-right">
        {isRecordingState && (
          <div
            className="cancel-button"
            onClick={() => {
              commands.cancelOperation();
            }}
          >
            <CancelIcon />
          </div>
        )}
      </div>
    </div>
  );
```

- [ ] **Step 5: Add a CSS accent for command mode**

Append to the end of `src/overlay/RecordingOverlay.css`:

```css
/* Command mode: tint the overlay so the user knows speech will be sent to
   the command hook instead of typed. */
.recording-overlay.command-mode {
  box-shadow: inset 0 0 0 1.5px rgba(120, 170, 255, 0.9);
}
```

- [ ] **Step 6: Verify the frontend builds**

Run (from the repo root): `npm run build` (or `bun run build` if the repo uses Bun — check `package.json` `packageManager` field; the repo's CI uses Bun).
Expected: TypeScript compiles with no errors.

- [ ] **Step 7: Commit**

```bash
git add src/overlay/RecordingOverlay.tsx src/overlay/RecordingOverlay.css src/i18n/locales/en/translation.json
git commit -m "feat(overlay): render command-mode states in the frontend"
```

---

## Task 8: Coordinator routing — recognise the command binding

The `command` binding records audio, so it must be routed through the `TranscriptionCoordinator` (which serialises push-to-talk / toggle lifecycle), not handled directly. `is_transcribe_binding` is renamed to `is_recording_binding` to reflect that it now gates all recording bindings.

**Files:**
- Modify: `src-tauri/src/transcription_coordinator.rs`
- Modify: `src-tauri/src/shortcut/handler.rs`

- [ ] **Step 1: Rename and extend the predicate**

In `src-tauri/src/transcription_coordinator.rs`, replace:

```rust
pub fn is_transcribe_binding(id: &str) -> bool {
    id == "transcribe" || id == "transcribe_with_post_process"
}
```

with:

```rust
/// True for bindings whose lifecycle (record → stop → process) is owned by the
/// coordinator: the two transcribe bindings and the command binding.
pub fn is_recording_binding(id: &str) -> bool {
    id == "transcribe" || id == "transcribe_with_post_process" || id == "command"
}
```

- [ ] **Step 2: Update the call site**

In `src-tauri/src/shortcut/handler.rs`, change the import on line 13:

```rust
use crate::transcription_coordinator::is_transcribe_binding;
```

to:

```rust
use crate::transcription_coordinator::is_recording_binding;
```

Then change line ~38:

```rust
    if is_transcribe_binding(binding_id) {
```

to:

```rust
    if is_recording_binding(binding_id) {
```

- [ ] **Step 3: Verify there are no other references**

Run: `cd src-tauri && grep -rn "is_transcribe_binding" src/`
Expected: no output (all references renamed).

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles. (`command_hook.rs` will warn that `run_command_hook` / `is_command_running` are unused — expected until Task 9.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/transcription_coordinator.rs src-tauri/src/shortcut/handler.rs
git commit -m "refactor(coordinator): rename predicate and route the command binding"
```

---

## Task 9: Wire the command action

This refactors `TranscribeAction` into `RecordingAction` with a `RecordingMode` enum so the recording/transcription scaffolding is shared, and the command path diverges only at the end (run hook instead of paste).

**Files:**
- Modify: `src-tauri/src/actions.rs`

- [ ] **Step 1: Add the `RecordingMode` enum**

In `src-tauri/src/actions.rs`, replace the struct definition (lines ~47-50):

```rust
// Transcribe Action
struct TranscribeAction {
    post_process: bool,
}
```

with:

```rust
/// What a recording action does with the transcript once it is produced.
#[derive(Clone, Copy)]
enum RecordingMode {
    /// Post-process (optionally) and paste the text into the focused field.
    Transcribe { post_process: bool },
    /// Pipe the raw transcript to the user's `hooks/command` script.
    Command,
}

// Recording Action — records, transcribes, then either pastes or runs the hook.
struct RecordingAction {
    mode: RecordingMode,
}
```

- [ ] **Step 2: Extract the shared recording-start logic**

The body of the current `TranscribeAction::start` does not depend on `post_process`. Extract it into a free function. In `src-tauri/src/actions.rs`, add this function immediately **above** `impl ShortcutAction for TranscribeAction` (which you will rename in Step 3). Use the **exact existing body** of `TranscribeAction::start` (lines ~391-489), with one change: the overlay call.

```rust
/// Begins recording for a recording action (shared by transcribe and command).
/// `is_command` only affects which overlay state is shown.
fn begin_recording(app: &AppHandle, binding_id: &str, is_command: bool) {
    let start_time = Instant::now();
    debug!("begin_recording called for binding: {}", binding_id);

    // Load model in the background
    let tm = app.state::<Arc<TranscriptionManager>>();
    let rm = app.state::<Arc<AudioRecordingManager>>();

    // Load ASR model and VAD model in parallel
    tm.initiate_model_load();
    let rm_clone = Arc::clone(&rm);
    std::thread::spawn(move || {
        if let Err(e) = rm_clone.preload_vad() {
            debug!("VAD pre-load failed: {}", e);
        }
    });

    let binding_id = binding_id.to_string();
    change_tray_icon(app, TrayIconState::Recording);
    if is_command {
        crate::overlay::show_command_recording_overlay(app);
    } else {
        show_recording_overlay(app);
    }

    // Get the microphone mode to determine audio feedback timing
    let settings = get_settings(app);
    let is_always_on = settings.always_on_microphone;
    debug!("Microphone mode - always_on: {}", is_always_on);

    let mut recording_error: Option<String> = None;
    if is_always_on {
        debug!("Always-on mode: Playing audio feedback immediately");
        let rm_clone = Arc::clone(&rm);
        let app_clone = app.clone();
        std::thread::spawn(move || {
            play_feedback_sound_blocking(&app_clone, SoundType::Start);
            rm_clone.apply_mute();
        });

        if let Err(e) = rm.try_start_recording(&binding_id) {
            debug!("Recording failed: {}", e);
            recording_error = Some(e);
        }
    } else {
        debug!("On-demand mode: Starting recording first, then audio feedback");
        let recording_start_time = Instant::now();
        match rm.try_start_recording(&binding_id) {
            Ok(()) => {
                debug!("Recording started in {:?}", recording_start_time.elapsed());
                let app_clone = app.clone();
                let rm_clone = Arc::clone(&rm);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    debug!("Handling delayed audio feedback/mute sequence");
                    play_feedback_sound_blocking(&app_clone, SoundType::Start);
                    rm_clone.apply_mute();
                });
            }
            Err(e) => {
                debug!("Failed to start recording: {}", e);
                recording_error = Some(e);
            }
        }
    }

    if recording_error.is_none() {
        shortcut::register_cancel_shortcut(app);
    } else {
        utils::hide_recording_overlay(app);
        change_tray_icon(app, TrayIconState::Idle);
        if let Some(err) = recording_error {
            let error_type = if is_microphone_access_denied(&err) {
                "microphone_permission_denied"
            } else if is_no_input_device_error(&err) {
                "no_input_device"
            } else {
                "unknown"
            };
            let _ = app.emit(
                "recording-error",
                RecordingErrorEvent {
                    error_type: error_type.to_string(),
                    detail: Some(err),
                },
            );
        }
    }

    debug!("begin_recording completed in {:?}", start_time.elapsed());
}
```

- [ ] **Step 3: Rewrite the `impl ShortcutAction`**

In `src-tauri/src/actions.rs`, replace the entire `impl ShortcutAction for TranscribeAction { ... }` block (from `impl ShortcutAction for TranscribeAction {` on line ~389 through its closing `}` on line ~661) with the block below. The `start` method is new (small); the `stop` method is the existing one with two changes: the captured variable, and the `Ok(transcription)` arm now branches on `mode`.

```rust
impl ShortcutAction for RecordingAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        // Command mode is single-flight: if a hook is still running, just show
        // the busy overlay and do not start a new recording.
        if matches!(self.mode, RecordingMode::Command)
            && crate::command_hook::is_command_running()
        {
            debug!("Command hook already running; ignoring command shortcut");
            crate::overlay::show_command_running_overlay(app);
            return;
        }
        begin_recording(app, binding_id, matches!(self.mode, RecordingMode::Command));
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("RecordingAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);
        show_transcribing_overlay(app);

        rm.remove_mute();
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string();
        let mode = self.mode;

        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            debug!("Starting async transcription task for binding: {}", binding_id);

            // Whether to record this run as post-processed in history.
            let post_process = matches!(mode, RecordingMode::Transcribe { post_process: true });

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if samples.is_empty() {
                    debug!("Recording produced no audio samples; skipping persistence");
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                } else {
                    let sample_count = samples.len();
                    let file_name = format!("handy-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    let transcription_time = Instant::now();
                    let transcription_result = tm.transcribe(samples);

                    let wav_saved = match wav_handle.await {
                        Ok(Ok(())) => {
                            match crate::audio_toolkit::verify_wav_file(
                                &wav_path_for_verify,
                                sample_count,
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    error!("WAV verification failed: {}", e);
                                    false
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Failed to save WAV file: {}", e);
                            false
                        }
                        Err(e) => {
                            error!("WAV save task panicked: {}", e);
                            false
                        }
                    };

                    match transcription_result {
                        Ok(transcription) => {
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                transcription
                            );

                            if let RecordingMode::Command = mode {
                                // Command mode: save the raw transcript to
                                // history, then hand off to the hook. The hook
                                // owns the overlay (command-running) from here.
                                if wav_saved {
                                    if let Err(err) = hm.save_entry(
                                        file_name,
                                        transcription.clone(),
                                        false,
                                        None,
                                        None,
                                    ) {
                                        error!("Failed to save history entry: {}", err);
                                    }
                                }

                                if transcription.trim().is_empty() {
                                    debug!("Command mode: empty transcript, not running hook");
                                    utils::hide_recording_overlay(&ah);
                                } else {
                                    crate::command_hook::run_command_hook(&ah, transcription);
                                }
                                change_tray_icon(&ah, TrayIconState::Idle);
                            } else {
                                if post_process {
                                    show_processing_overlay(&ah);
                                }
                                let processed = process_transcription_output(
                                    &ah,
                                    &transcription,
                                    post_process,
                                )
                                .await;

                                if wav_saved {
                                    if let Err(err) = hm.save_entry(
                                        file_name,
                                        transcription,
                                        post_process,
                                        processed.post_processed_text.clone(),
                                        processed.post_process_prompt.clone(),
                                    ) {
                                        error!("Failed to save history entry: {}", err);
                                    }
                                }

                                if processed.final_text.is_empty() {
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                } else {
                                    let ah_clone = ah.clone();
                                    let paste_time = Instant::now();
                                    let final_text = processed.final_text;
                                    ah.run_on_main_thread(move || {
                                        match utils::paste(final_text, ah_clone.clone()) {
                                            Ok(()) => debug!(
                                                "Text pasted successfully in {:?}",
                                                paste_time.elapsed()
                                            ),
                                            Err(e) => {
                                                error!("Failed to paste transcription: {}", e);
                                                let _ = ah_clone.emit("paste-error", ());
                                            }
                                        }
                                        utils::hide_recording_overlay(&ah_clone);
                                        change_tray_icon(&ah_clone, TrayIconState::Idle);
                                    })
                                    .unwrap_or_else(|e| {
                                        error!("Failed to run paste on main thread: {:?}", e);
                                        utils::hide_recording_overlay(&ah);
                                        change_tray_icon(&ah, TrayIconState::Idle);
                                    });
                                }
                            }
                        }
                        Err(err) => {
                            debug!("Global Shortcut Transcription error: {}", err);
                            if wav_saved {
                                if let Err(save_err) = hm.save_entry(
                                    file_name,
                                    String::new(),
                                    post_process,
                                    None,
                                    None,
                                ) {
                                    error!("Failed to save failed history entry: {}", save_err);
                                }
                            }
                            utils::hide_recording_overlay(&ah);
                            change_tray_icon(&ah, TrayIconState::Idle);
                        }
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                utils::hide_recording_overlay(&ah);
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!("RecordingAction::stop completed in {:?}", stop_time.elapsed());
    }
}
```

- [ ] **Step 4: Update `ACTION_MAP`**

In `src-tauri/src/actions.rs`, replace the `transcribe` and `transcribe_with_post_process` inserts inside `ACTION_MAP` (lines ~702-711) with three inserts:

```rust
    map.insert(
        "transcribe".to_string(),
        Arc::new(RecordingAction {
            mode: RecordingMode::Transcribe {
                post_process: false,
            },
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(RecordingAction {
            mode: RecordingMode::Transcribe { post_process: true },
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "command".to_string(),
        Arc::new(RecordingAction {
            mode: RecordingMode::Command,
        }) as Arc<dyn ShortcutAction>,
    );
```

- [ ] **Step 5: Verify the backend builds**

Run: `cd src-tauri && cargo build`
Expected: build succeeds with no errors. (Pre-existing warnings are acceptable.)

- [ ] **Step 6: Run all Rust tests**

Run: `cd src-tauri && cargo test`
Expected: all tests pass, including the `command_hook` tests from Tasks 1-3.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/actions.rs
git commit -m "feat(actions): add command mode via RecordingAction"
```

---

## Task 10: Documentation — README + example script

**Files:**
- Modify: `README.md`
- Create: `docs/examples/command-hook.ps1`
- Create: `docs/examples/command`

- [ ] **Step 1: Create the Windows example hook**

Create `docs/examples/command-hook.ps1`:

```powershell
# Handy command-mode hook (Windows / PowerShell).
#
# Install: copy this file to
#   %APPDATA%\com.pais.handy\hooks\command.ps1
# Handy pipes the spoken transcript to this script on stdin. Handy ignores
# whatever the script writes to stdout — the script owns all side effects.

$transcript = [Console]::In.ReadToEnd().Trim()
if (-not $transcript) { exit 0 }

# Example: forward the transcript to an AI agent. Replace with your own tool.
# The result is shown as a desktop notification rather than typed.
$reply = $transcript | claude -p 2>$null

if ($reply) {
    [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null
    $notify = New-Object System.Windows.Forms.NotifyIcon
    $notify.Icon = [System.Drawing.SystemIcons]::Information
    $notify.Visible = $true
    $notify.ShowBalloonTip(8000, 'Handy command', $reply, 'Info')
    Start-Sleep -Seconds 9
    $notify.Dispose()
}
exit 0
```

- [ ] **Step 2: Create the Unix example hook**

Create `docs/examples/command` (Unix/macOS/Linux):

```bash
#!/usr/bin/env bash
# Handy command-mode hook (macOS / Linux).
#
# Install: copy this file to the hooks directory and make it executable:
#   macOS:  ~/Library/Application Support/com.pais.handy/hooks/command
#   Linux:  ~/.local/share/com.pais.handy/hooks/command
#   chmod +x .../hooks/command
#
# Handy pipes the spoken transcript on stdin and ignores stdout.

transcript="$(cat)"
[ -z "$transcript" ] && exit 0

# Example: forward to an AI agent and announce the reply.
reply="$(printf '%s' "$transcript" | claude -p 2>/dev/null)"
[ -n "$reply" ] && printf '%s' "$reply" | (command -v say >/dev/null && say || cat)
exit 0
```

- [ ] **Step 3: Add the README section**

In `README.md`, find the `## Known Issues & Current Limitations` heading. Immediately **before** it, insert:

```markdown
### Command mode

Command mode is a second shortcut (default `Ctrl+Alt+Space`, configurable in
Settings) that records and transcribes speech like normal, but instead of typing
the text it pipes the raw transcript to a script you provide. Use it to send
speech to an AI agent, run a command, or launch apps by voice.

Create an executable hook in the `hooks/` folder of Handy's app data directory:

- **Windows:** `hooks\command.cmd`, `.bat`, `.ps1`, or `.exe`
- **macOS / Linux:** `hooks/command` (with the executable bit set)

Handy passes the transcript to the script on **stdin**. The script's stdout is
**not** used — the script is responsible for all output (notifications, speech,
launching apps, or inserting text itself). A script that produces no visible
effect simply runs silently.

If the script runs longer than `command_hook_timeout_secs` (default 120s) it is
terminated. Example scripts: see [`docs/examples/`](docs/examples/).
```

- [ ] **Step 4: Commit**

```bash
git add README.md docs/examples/command-hook.ps1 docs/examples/command
git commit -m "docs: document command mode and add example hooks"
```

---

## Task 11: Full build + manual end-to-end test

**Files:** none (verification only)

- [ ] **Step 1: Clean release build**

Run (from repo root): `npm run tauri build` (or `bun run tauri build`).
Expected: the app builds and produces an installer/binary with no errors.

- [ ] **Step 2: Install the test hook**

On Windows, create `%APPDATA%\com.pais.handy\hooks\command.ps1` with:

```powershell
$t = [Console]::In.ReadToEnd()
Add-Content -Path "$env:APPDATA\com.pais.handy\hooks\command-test.log" -Value "[$(Get-Date -Format o)] env=$env:HANDY_CLIPBOARD :: $t"
```

- [ ] **Step 3: Verify command mode end to end**

Run the built app. Press the command shortcut (default `Ctrl+Alt+Space`), speak a sentence, release/press to stop.
Expected:
- The overlay shows the command-mode accent while recording, then `Transcribing...`, then `Running command...`, then hides.
- `%APPDATA%\com.pais.handy\hooks\command-test.log` gains a line containing the spoken transcript and the `HANDY_CLIPBOARD` value.
- No text is pasted into the focused window.

- [ ] **Step 4: Verify the regular transcribe shortcut still works**

Press the normal transcribe shortcut, speak, stop.
Expected: text is pasted as before — command mode did not regress transcription.

- [ ] **Step 5: Verify concurrency**

Replace the test hook body with `Start-Sleep -Seconds 10` followed by the logging line. Trigger command mode, then press the command shortcut again within those 10 seconds.
Expected: the second press shows the `Running command...` busy overlay and does **not** start a second recording. The regular transcribe shortcut still works during the 10 seconds.

- [ ] **Step 6: Verify the timeout**

Set the test hook body to `Start-Sleep -Seconds 600`. Set `command_hook_timeout_secs` low (e.g. edit `settings.json` to `5`) and restart the app. Trigger command mode.
Expected: after ~5 seconds the busy overlay clears and the log shows the watcher killed the process (`Command hook: timed out` in Handy's log).

- [ ] **Step 7: Run the verification skill**

Use `superpowers:verification-before-completion` to confirm every task's tests and builds passed, then report completion.

---

## Self-Review

**Spec coverage:**
- Second shortcut / `command` action → Tasks 5, 8, 9. ✓
- Raw transcript (pre-post-processing) on stdin → Task 9 (Command arm uses `transcription` directly). ✓
- Detached, non-blocking spawn → Task 4. ✓
- No paste-back → Task 4 (`Stdio::null()` for stdout). ✓
- Context env vars → Task 3 (`build_env`); v1 ships `HANDY_CLIPBOARD` only (see Scope note). ✓
- `command_running` flag + single-flight guard → Tasks 4, 9. ✓
- Watcher thread + timeout → Task 4. ✓
- `command-recording` / `command-running` overlay states → Tasks 6, 7. ✓
- Regular transcribe stays usable → verified Task 11 Step 4-5. ✓
- Cross-platform script resolution → Tasks 1, 2. ✓
- Error paths log + clear flag → Task 4. ✓

**Known limitation (acceptable for v1):** Handy has a single overlay window. If the regular transcribe shortcut is used while a command hook is running, the two flows share that window and the last writer wins (e.g. a transcribe may hide the command-running overlay early). Functionality is unaffected; only the busy indicator may disappear sooner than the hook finishes.

**Placeholder scan:** none — every code step contains complete code.

**Type consistency:** `RecordingMode` / `RecordingAction` / `run_command_hook` / `is_command_running` / `is_recording_binding` / `command_hook_timeout_secs` / `show_command_recording_overlay` / `show_command_running_overlay` are used consistently across tasks.
