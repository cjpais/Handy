# GNOME Shell Status Extension Plan

## Goal

Add an optional GNOME Shell integration that shows Handy's live activity state in the GNOME top bar without relying on AppIndicator/tray support or a focusable overlay window.

This should be a follow-up feature/UX PR, separate from the current Linux bug-fix PR.

## Current Context

- Handy is a Tauri 2 desktop app with a Rust backend and React frontend.
- The current Linux bug-fix PR suppresses the recording overlay on Wayland when GTK Layer Shell cannot initialize.
- That fix is intentionally capability-based:
  - X11 still shows the overlay.
  - Hyprland/Sway/wlroots compositors with GTK Layer Shell support still show the overlay.
  - GNOME Wayland/Mutter, which does not support `zwlr_layer_shell_v1`, suppresses the overlay to avoid a focus race that can drop the first pasted character.
- GNOME AppIndicator/tray support is not reliable or visually good enough for this use case.
- Users may disable the GNOME AppIndicator extension, which also hides Handy's tray icon.
- Handy already has status transitions internally through tray and overlay updates.

Relevant existing files:

- `src-tauri/src/actions.rs`: recording/transcribing/processing lifecycle transitions.
- `src-tauri/src/tray.rs`: `TrayIconState::{Idle, Recording, Transcribing}` and tray icon updates.
- `src-tauri/src/overlay.rs`: overlay show/hide state, including `recording`, `transcribing`, and `processing` states.
- `src-tauri/src/settings.rs`: settings including tray visibility and overlay position.
- `src-tauri/src/lib.rs`: app startup and tray/overlay initialization.

## Non-Goals

- Do not change paste behavior.
- Do not change clipboard behavior.
- Do not change `dotool`, `dotoolc`, `wl-copy`, `ydotool`, `xdotool`, `wtype`, or `kwtype` selection.
- Do not depend on AppIndicator.
- Do not make the GNOME extension mandatory.
- Do not auto-enable the extension from the app or package.
- Do not mix this with the current Linux bug-fix PR.

## Recommended Architecture

Use a small GNOME Shell extension plus a minimal Handy Linux status API over the user session D-Bus.

Handy publishes activity state on D-Bus. The GNOME Shell extension listens for status changes and renders a native GNOME top-bar indicator.

This keeps the integration status-only and avoids interaction with windows, focus, input injection, or clipboard tooling.

## D-Bus Design

Suggested names:

- Bus name: `com.pais.Handy`
- Object path: `/com/pais/Handy`
- Interface: `com.pais.Handy.Status`

Initial API:

- Property: `Status: string`
- Method: `GetStatus() -> string`
- Signal: `StatusChanged(status: string)`

Initial status values:

- `idle`
- `recording`
- `transcribing`
- `processing`
- `error` can be added later if useful.

Implementation notes:

- Add a small Linux-only D-Bus status service in Rust.
- Prefer a direct Rust dependency such as `zbus` for D-Bus service support.
- Keep the service optional and non-fatal. If D-Bus registration fails, log a warning and let Handy continue normally.
- Store current status in one app-level state object, not duplicated across tray/overlay/extension code.
- Emit `StatusChanged` only when the state actually changes.
- Reset to `idle` on finish, cancel, start failure, transcription failure, paste completion, or app shutdown paths.

## Backend Implementation Plan

1. Add an activity status type.
   - Example enum: `ActivityStatus::{Idle, Recording, Transcribing, Processing}`.
   - Serialize to lowercase strings for D-Bus.
   - This should be independent from `TrayIconState`, because `TrayIconState` currently has no `Processing` variant.

2. Add a status manager.
   - Suggested file: `src-tauri/src/status.rs` or `src-tauri/src/managers/status.rs`.
   - Responsibilities:
     - hold current activity status
     - expose `set_status(status)`
     - emit D-Bus signal on Linux when available
     - optionally emit a Tauri event later if frontend status UI needs it

3. Wire the status manager into existing lifecycle points in `actions.rs`.
   - Start recording:
     - current code calls `change_tray_icon(app, TrayIconState::Recording)` and `show_recording_overlay(app)`.
     - also set status to `recording`.
   - Stop recording/transcription starts:
     - current code calls `change_tray_icon(app, TrayIconState::Transcribing)` and `show_transcribing_overlay(app)`.
     - also set status to `transcribing`.
   - Post-processing starts:
     - current code calls `show_processing_overlay(&ah)` when post-processing is enabled.
     - also set status to `processing`.
   - Empty audio, transcription errors, paste completion, cancel, or start failure:
     - existing code hides overlay and returns tray to idle.
     - also set status to `idle`.

4. Keep tray and overlay behavior unchanged.
   - Do not replace `change_tray_icon` or overlay calls yet.
   - The GNOME extension should be an additional status consumer, not a replacement for other platforms.

5. Initialize D-Bus service on Linux startup.
   - Initialize during Tauri setup in `src-tauri/src/lib.rs` after app state is available.
   - Use session bus, not system bus.
   - If another Handy process already owns the bus name, handle gracefully. Handy already uses single-instance behavior, so this should normally not happen.

## GNOME Shell Extension Plan

Suggested location in this repository:

- `gnome-extension/`

Suggested files:

- `gnome-extension/metadata.json`
- `gnome-extension/extension.js`
- `gnome-extension/stylesheet.css`
- optional later: `gnome-extension/prefs.js`

Extension behavior for v1:

- Show a small top-bar indicator when Handy is active.
- Hide or dim when Handy is `idle`. Product decision still needed.
- Listen to `StatusChanged` over D-Bus.
- On extension enable, call `GetStatus()` once to sync initial state.
- If Handy is not running or D-Bus name is unavailable, show nothing.
- If D-Bus disconnects, return to hidden/idle state.

Visual states:

- `recording`: red/pink dot or pill, label optional.
- `transcribing`: neutral spinner/pulse or text.
- `processing`: different spinner/pulse or label.
- `idle`: hidden by default, unless we decide always-visible is better.

Optional later actions from top-bar menu:

- Open Handy settings window.
- Toggle transcription.
- Cancel current operation.

Do not add actions in v1 unless the status-only implementation is stable.

## Packaging Strategy

Phase 1: manual development install.

- Copy/symlink extension into `~/.local/share/gnome-shell/extensions/handy-status@pais.com/`.
- Reload GNOME Shell if on X11, or log out/in on Wayland.
- Enable with `gnome-extensions enable handy-status@pais.com`.

Phase 2: optional repo packaging.

- Include the extension in source tree.
- Add a small install script or documentation.

Phase 3: optional `.deb` packaging.

- Install extension files under `/usr/share/gnome-shell/extensions/handy-status@pais.com/`.
- Do not auto-enable it.
- Document that GNOME users can enable it using Extensions app or `gnome-extensions enable`.

## Compatibility Requirements

- X11 must not regress.
- Hyprland/Sway/wlroots must not regress.
- GNOME users without the extension must still be able to use Handy.
- Tray support must remain optional.
- Overlay support must remain controlled by existing settings and compositor capability.
- D-Bus failure must not block recording, transcription, paste, tray, or settings UI.
- The extension must not create windows or request focus.
- The extension must not interact with clipboard or input injection.

## Testing Plan

Backend:

- `cargo fmt --check` from `src-tauri`.
- `cargo check` from `src-tauri`.
- `bun run build`.
- Confirm status transitions with a D-Bus inspection tool.
- Confirm Handy still runs if D-Bus registration fails.

Possible D-Bus inspection commands:

```bash
busctl --user introspect com.pais.Handy /com/pais/Handy
busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Status GetStatus
busctl --user monitor com.pais.Handy
```

GNOME extension:

- Install extension manually.
- Enable it with `gnome-extensions enable handy-status@pais.com`.
- Verify indicator shows `recording` when recording starts.
- Verify indicator changes to `transcribing` after recording stops.
- Verify indicator changes to `processing` when post-processing is enabled.
- Verify indicator returns to idle/hidden after paste completes.
- Verify no first-character paste regression on GNOME Wayland.
- Disable the AppIndicator extension and confirm Handy status is still visible through the new extension.
- Disable the Handy extension and confirm Handy still works normally.

Regression checks:

- Existing tray behavior still works where tray/AppIndicator is available.
- Existing overlay behavior still works on X11 and wlroots with GTK Layer Shell.
- Existing overlay remains suppressed on GNOME Wayland without GTK Layer Shell.
- `dotoolc`/`dotool` paste behavior remains unchanged.

## Product Decisions Still Needed

1. Should the top-bar indicator be hidden when Handy is idle, or always visible?
2. Should v1 be status-only, or include actions like Toggle/Cancel/Open Settings?
3. Which GNOME Shell versions should be supported initially?
4. Should the extension live in this repository long-term or become a separate repository later?
5. Should the `.deb` eventually install the extension globally, or should installation stay manual?

Recommended answers for v1:

- Hide when idle.
- Status-only first.
- Support the GNOME Shell version used on the test machine first, then broaden if needed.
- Keep it in this repository initially for fast iteration.
- Manual install first; package globally only after the extension is stable.

## Implementation Order for Subagent

1. Confirm GNOME Shell version with `gnome-shell --version`.
2. Add backend status manager and Linux D-Bus publisher.
3. Wire status transitions into `actions.rs` where tray/overlay states already change.
4. Add manual D-Bus verification commands to docs or PR notes.
5. Create minimal GNOME extension that listens to Handy status.
6. Test on GNOME Wayland with AppIndicator disabled.
7. Run Rust/frontend checks.
8. Keep the change as a separate follow-up PR.
