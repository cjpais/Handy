# Gaze Detection & Eye Tracking for Handy

## Overview

Add eye-tracking capabilities to Handy so users can control the mouse cursor, switch windows across multiple monitors, and trigger transcription — all through gaze. This extends Handy's existing voice-first model with a second input modality (eyes), enabling powerful voice + gaze fusion commands.

**Primary use case:** Navigate between multiple monitors and windows quickly with eye gaze, without touching the keyboard or mouse.

---

## Architecture

Eye tracking adds two new capabilities to Handy:
1. **Gaze-triggered transcription** — start/stop transcription by looking at a virtual trigger zone (alternative to keyboard shortcuts)
2. **Gaze-driven cursor + window switching** — move cursor and switch windows with eye gaze

Both share the same underlying gaze detection pipeline.

### Module Structure

New Rust module in `src-tauri/src/eyetracking/`:

```
src-tauri/src/eyetracking/
├── mod.rs              # EyeTrackingManager, public API
├── gaze.rs             # GazePoint struct, calibration data
├── providers/
│   ├── mod.rs          # EyeTrackingProvider trait
│   ├── webcam.rs       # GazeTracking (Python subprocess) provider
│   └── pupil.rs        # Pupil Labs API provider (future)
└── calibration.rs      # Calibration state & routines
```

### Provider Trait

Follows the established manager/provider pattern used throughout the codebase (AudioManager, ModelManager, etc.):

```rust
pub trait EyeTrackingProvider: Send + Sync {
    fn init(&mut self) -> Result<()>;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self);
    fn poll_gaze(&self) -> Option<GazePoint>;  // Returns screen (x, y) + confidence
    fn calibrate(&mut self, points: Vec<CalibrationPoint>) -> Result<()>;
}
```

### GazePoint Struct

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GazePoint {
    pub x: f64,              // Normalized 0.0-1.0 or screen pixel coords
    pub y: f64,
    pub confidence: f32,     // 0.0-1.0 from the provider
    pub timestamp: u64,      // Millisecond timestamp
    pub screen_id: Option<u32>, // Which monitor the gaze landed on
}
```

---

## Phase 1: Gaze Detection Backend

### Webcam Provider (MVP)

Launch a Python subprocess using the `GazeTracking` library ([Nicmcd-GazeTracking, 2.6k stars](https://github.com/StephanL/Nicmcd-GazeTracking)), communicating via stdin/stdout JSON.

**Why a subprocess?** Avoids adding Rust OpenCV/DLib dependencies. The Python library is mature, webcam-only, and requires no special hardware.

**Python subprocess script** (bundled in `src-tauri/bin/gaze_server.py`):

```python
#!/usr/bin/env python3
"""Gaze tracking server - communicates with Handy via JSON over stdin/stdout."""
import json
import sys
from gaze_tracking import GazeTracking

gaze = GazeTracking()
gaze.calibration_profile_id = "handy_user"  # Persist calibration

while True:
    command = sys.stdin.readline().strip()
    if command == "start":
        gaze.start()
        print(json.dumps({"status": "started"}))
    elif command == "stop":
        gaze.stop()
        print(json.dumps({"status": "stopped"}))
    elif command == "poll":
        frame, gaze_points = gaze.update()
        if gaze_points:
            gp = gaze_points[0]  # Left eye
            print(json.dumps({
                "x": gp.gaze_x,
                "y": gp.gaze_y,
                "confidence": 1.0,  # GazeTracking doesn't expose confidence
            }))
        else:
            print(json.dumps({"x": None, "y": None}))
    elif command == "calibrate":
        # Handle calibration points
        pass
    sys.stdout.flush()
```

**Rust subprocess management** in `webcam.rs`:
- Spawn Python process on `start()`
- Send `"poll"` commands each frame (e.g., 30Hz loop)
- Parse JSON responses into `GazePoint`
- Kill process on `stop()` or shutdown
- Auto-install `gaze-tracking` pip package on first run if missing

### Alternative: Pupil Labs Provider (Future)

For higher accuracy, integrate with [Pupil Labs](https://github.com/PupilLabs/pupil) hardware (USB camera glasses). Their API exposes gaze data over a network socket. Same provider trait, different implementation.

---

## Phase 2: Settings & Configuration

### New Settings in `AppSettings`

Extend the existing settings system in `src-tauri/src/settings.rs` (follows the pattern of adding fields to `AppSettings`, implementing `#[tauri::command]` getters/setters, and registering commands in `lib.rs`):

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EyeTrackingSettings {
    pub enabled: bool,
    pub provider: String,              // "webcam" | "pupil"
    pub sensitivity: f32,              // Gaze-to-cursor scaling (0.5 - 2.0), default 1.0
    pub dwell_time_ms: u32,           // Time to hold gaze for "click" (500-2000ms), default 1000
    pub trigger_zone: TriggerZone,    // Where gaze triggers transcription
    pub calibration_points: u32,      // 5 or 9 point calibration
    pub smoothing_enabled: bool,      // Temporal gaze smoothing, default true
    pub smoothing_window: u32,        // Number of frames to average, default 5
    pub window_switch_mode: bool,     // Enable gaze window switching, default false
    pub cursor_visibility: String,    // "hide" | "show" | "gaze_only", default "show"
    pub blink_to_click: bool,         // Use blink detection as click trigger, default false
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriggerZone {
    pub x: f64,    // Normalized 0.0-1.0
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
```

### Commands to Add

Follow existing pattern — each setting gets a getter and setter:

```rust
#[tauri::command]
async fn get_eye_tracking_settings(state: State<AppState>) -> Result<EyeTrackingSettings, String> {
    let settings = state.settings.lock().await;
    Ok(settings.eye_tracking.clone())
}

#[tauri::command]
async fn set_eye_tracking_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    let mut settings = state.settings.lock().await;
    settings.eye_tracking.enabled = enabled;
    settings.save().await?;

    // Start/stop the eye tracking manager
    let manager = state.eye_tracking.lock().await;
    if enabled {
        manager.start()?;
    } else {
        manager.stop()?;
    }
    Ok(())
}

// ... repeat for each setting field
```

Register all commands in `src-tauri/src/lib.rs` setup function.

---

## Phase 3: Gaze-to-Cursor + Dwell Click

### Extend Input Module

`src-tauri/src/input.rs` already has cursor control via Enigo. Add:

```rust
// Add to existing input.rs pub fn list:
pub fn move_cursor(x: i32, y: i32) -> Result<(), String> {
    let mut enigo = Enigo::new(&Direction::Inherit)?;
    enigo.mouse_move_to(Absolute::from(x, y))?;
    Ok(())
}

pub fn click_mouse() -> Result<(), String> {
    let mut enigo = Enigo::new(&Direction::Inherit)?;
    enigo.mouse_click(Button::Left)?;
    Ok(())
}

pub fn mouse_down() -> Result<(), String> {
    let mut enigo = Enigo::new(&Direction::Inherit)?;
    enigo.mouse_button_click(Button::Left, 1)?;
    Ok(())
}
```

### Gaze Processing Loop

In `EyeTrackingManager`, run a background task (Tokio task, similar to how `TranscriptionCoordinator` works):

1. Poll gaze coordinates from provider at ~30Hz
2. Apply calibration offset + smoothing (exponential moving average)
3. Map normalized gaze (0-1) to screen pixel coordinates
4. Handle multi-monitor: determine which display the gaze maps to, convert to that display's coordinate space
5. Move virtual cursor via `move_cursor()`
6. Track dwell time — if gaze stays within a threshold radius (e.g., 20px) for `dwell_time_ms`, trigger `click_mouse()`

### Smoothing

Simple exponential moving average to reduce jitter:

```rust
fn smooth_gaze(&mut self, new: GazePoint) -> GazePoint {
    let alpha = 0.3; // Smoothing factor
    GazePoint {
        x: self.smoothed_x * alpha + new.x * (1.0 - alpha),
        y: self.smoothed_y * alpha + new.y * (1.0 - alpha),
        ..new
    }
}
```

### Midas Touch Prevention

The "Midas Touch Problem" — you don't want to click everything you look at. Multiple strategies:

| Strategy | How It Works | Config |
|----------|-------------|--------|
| **Dwell time** | Must hold gaze on a point for X ms before clicking | `dwell_time_ms` (default 1000ms) |
| **Blink to click** | Use blink detection as explicit click trigger | `blink_to_click` toggle |
| **Voice confirm** | Look + say "click" to confirm | Uses existing Handy voice pipeline |
| **Click mode** | Toggle a "click enabled" state via voice/keyboard | Future enhancement |

---

## Phase 4: Multi-Monitor + Window Switching

### New Window Module

`src-tauri/src/window.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub bounds: Rect,  // x, y, width, height
    pub screen_id: u32,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitorFrame {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

pub fn get_window_at_point(x: i32, y: i32) -> Option<WindowInfo>;
pub fn focus_window(window_id: u32) -> Result<(), String>;
pub fn get_all_windows() -> Vec<WindowInfo>;
pub fn get_monitors() -> Vec<MonitorFrame>;
```

### Platform-Specific Implementation

- **macOS:** Use `core-graphics` crate for `CGDisplay` APIs (monitor frames), `AppKit`/`pyobjc` bridge or `cocoa` crate for `CGWindowListCopyWindowInfo` and window focus
- **Windows:** `EnumWindows` + `GetWindowRect` + `SetForegroundWindow` via `windows` crate
- **Linux:** `xdotool` subprocess or `wl-clipboard`/Wayland protocols

### Window Switching Modes

| Mode | Behavior | Config |
|------|----------|--------|
| **Auto-focus** | Cursor moves to window, auto-focuses on entry | May be too aggressive |
| **Dwell-to-focus** | Gaze on window for X seconds switches focus | `dwell_time_ms` |
| **Gaze + voice** | Look at window, say "switch" or "focus" | Best UX, leverages Handy voice |

---

## Phase 5: Gaze-Triggered Transcription

### Integration with TranscriptionCoordinator

The existing `TranscriptionCoordinator` in `src-tauri/src/transcription/coordinator.rs` already handles input from keyboard, voice, and clipboard. Add a new input source:

```rust
// Extend InputSource enum
#[derive(Debug, Clone, PartialEq)]
pub enum InputSource {
    Voice,
    Keyboard,
    Clipboard,
    EyeTracking,  // NEW
}

// New method on TranscriptionCoordinator
impl TranscriptionCoordinator {
    pub fn on_gaze_in_trigger_zone(&self, in_zone: bool) {
        if in_zone && self.settings.eye_tracking.enabled {
            self.send_input("transcribe", InputSource::EyeTracking, true, false);
        }
    }
}
```

### Trigger Zones

Define screen regions where gazing starts/stops transcription. Configurable via `TriggerZone` struct in settings. Multiple zones supported (e.g., top-center of primary monitor, floating overlay button).

---

## Phase 6: Frontend UI

### New Settings Components

In `src/components/settings/`:

- **`EyeTrackingSettings.tsx`** — Main settings panel:
  - Enable/disable toggle
  - Provider selection (webcam / pupil)
  - Sensitivity slider (0.5x - 2.0x)
  - Dwell time slider (500ms - 2000ms)
  - Smoothing toggle
  - Window switch mode toggle
  - Cursor visibility dropdown
  - Blink-to-click toggle

- **`CalibrationWizard.tsx`** — Step-by-step calibration:
  - 5-point or 9-point calibration grid
  - Animated dot moves to each point, user looks at it
  - Samples gaze position at each point
  - Computes calibration offset
  - Saves to persistent storage

- **`TriggerZoneEditor.tsx`** — Visual editor:
  - Drag-to-resize rectangle on a screen map
  - Preview trigger zone boundaries
  - Add/remove multiple zones

### Overlay Enhancements

In `src/overlay/`:

- **Gaze indicator** — Small semi-transparent circle following eye gaze position
- **Dwell progress ring** — Circular progress indicator that fills as you hold gaze on a point (visual feedback for dwell-to-click)
- **Trigger zone boundaries** — Show dotted rectangles around active trigger zones
- **Status indicator** — Small icon in overlay showing eye tracking active/inactive/calibrating

### Tauri Events

Frontend subscribes to gaze events via Tauri's event system:

```typescript
// In the overlay or a gaze indicator component
import { listen } from '@tauri-apps/api/event';

listen<'eyetracking|gaze-update'>('eyetracking|gaze-update', (event) => {
  const { x, y, confidence } = event.payload;
  setGazePosition({ x, y });
  setConfidence(confidence);
});

listen<'eyetracking|dwell-progress'>('eyetracking|dwell-progress', (event) => {
  const { progress } = event.payload; // 0.0 - 1.0
  setDwellProgress(progress);
});
```

Rust side emits events from the gaze processing loop:

```rust
// In EyeTrackingManager gaze loop
app.emit_all("eyetracking|gaze-update", &gaze_point)?;
app.emit_all("eyetracking|dwell-progress", &dwell_progress)?;
```

---

## Phase 7: Voice + Gaze Fusion Commands (Handy-Specific)

Since Handy already handles voice, the killer differentiator is **gaze + voice fusion**:

| Gaze Action | Voice Command | Result |
|-------------|---------------|--------|
| Look at window | "switch" / "focus" | Switch to that window |
| Look at text field | "type" / "transcribe" | Start transcription targeting that field |
| Look at button | "click" | Click the element |
| Look at URL bar | "go to ..." | Navigate to URL |
| Look at app dock icon | "open" | Launch the app |
| Look at trigger zone | (nothing) | Start/stop transcription |

### Implementation

Extend the existing command system in `src-tauri/src/commands/`:

```rust
// In the command execution pipeline, when a voice command is ambiguous,
// resolve it using current gaze position:

pub fn resolve_command_with_gaze(command: &str, gaze: &GazePoint) -> ExecutedCommand {
    let window_under_gaze = get_window_at_point(gaze.screen_x, gaze.screen_y);

    match command {
        "switch" | "focus" => {
            if let Some(window) = window_under_gaze {
                focus_window(window.id);
            }
        }
        "click" => {
            click_mouse(); // Cursor is already at gaze position
        }
        "transcribe" | "type" => {
            // Focus window under gaze, then start transcription
            if let Some(window) = window_under_gaze {
                focus_window(window.id);
            }
            transcription_coordinator.start();
        }
        _ => {} // Fall through to existing command handling
    }
}
```

This requires the gaze position to be available when voice commands are processed. Store the latest gaze point in `AppState` and reference it during command resolution.

---

## Implementation Order

| Phase | What | Depends On | Est. Effort |
|-------|------|------------|-------------|
| 1 | Gaze detection backend (webcam provider) | — | Medium |
| 2 | Settings & configuration | 1 | Small |
| 3 | Gaze-to-cursor + dwell click | 1, 2 | Medium |
| 4 | Multi-monitor + window switching | 3 | Medium |
| 5 | Gaze-triggered transcription | 3, existing transcription | Small |
| 6 | Frontend UI (settings + overlay) | 2, 3 | Medium |
| 7 | Voice + gaze fusion commands | 4, 5, existing voice | Medium |

## Key Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Webcam gaze accuracy** (~20-40px error) | Cursor feels imprecise | Smoothing + calibration + large click targets + sensitivity tuning |
| **Midas touch problem** | Accidental clicks on everything | Dwell time (default 1s), blink-to-click, voice confirmation |
| **Multi-monitor calibration drift** | Gaze jumps between screens | Per-monitor calibration zones, separate calibration offsets |
| **CPU load from gaze processing** | Drops transcription quality | Run gaze detection in separate Python process, Rust side is lightweight |
| **Privacy concerns** (webcam always on) | User trust | Visual indicator in overlay, easy toggle off, no video stored |
| **Lighting conditions** | Gaze tracking degrades in poor light | Document requirements, fallback to keyboard/mouse |

## Dependencies to Add

### Rust (Cargo.toml)
- No new heavy dependencies — subprocess management uses existing `std::process`
- `core-graphics` (macOS monitor/window APIs) — may already be available via Tauri deps
- `cocoa` or `objc2` for macOS window management

### Python (bundled script requirements)
- `gaze-tracking` — webcam-based eye tracking
- `opencv-python-headless` — required by gaze-tracking
- `dlib` — required by gaze-tracking (may need system-level installation)

### Frontend
- No new npm packages needed — use existing Tailwind + shadcn/ui + Tauri event system

## Open Questions

1. **Calibration persistence** — Should calibration profiles be per-user and persisted across sessions? (Recommended: yes, use `gaze_tracking`'s built-in profile system)
2. **Blink detection** — Does `GazeTracking` expose blink data? If not, may need a different library for blink-to-click feature
3. **Virtual cursor vs. real cursor** — Should we hide the real OS cursor and show a custom gaze cursor in the overlay? Or move the real cursor?
4. **Gaze smoothing algorithm** — Exponential moving average is simple but may lag. Consider Kalman filter for better tracking
5. **Multi-user calibration** — Should multiple users have separate calibration profiles?
6. **Fallback behavior** — What happens when gaze tracking loses the face? Pause cursor movement? Return to last known position?

## References

- [Nicmcd-GazeTracking](https://github.com/StephanL/Nicmcd-GazeTracking) — Python webcam eye tracking library (2.6k ⭐)
- [OptiKey](https://github.com/OptiKey/OptiKey) — Full eye-controlled computer access (4.4k ⭐) — inspiration for dwell-click and trigger zones
- [Pupil Labs](https://github.com/PupilLabs/pupil) — Research-grade eye tracking (1.7k ⭐) — future hardware provider
- [Talon Voice + talon-gaze-ocr](https://github.com/wolfmanstout/talon-gaze-ocr) — Voice + gaze fusion inspiration
