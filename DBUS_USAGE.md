# D-Bus Interface for Handy

## Overview

Handy now supports D-Bus messaging for transcription control on Linux, particularly useful for Wayland/Hyperland where global keyboard shortcuts have limitations.

## Automatic Behavior

- **Wayland Detection**: The app automatically detects Wayland sessions (via `WAYLAND_DISPLAY` environment variable)
- **Keyboard Shortcuts**: Automatically disabled on Wayland, remain active on X11
- **D-Bus Service**: Always available on Linux regardless of display server

## D-Bus Interface Details

- **Service Name**: `com.pais.Handy`
- **Object Path**: `/com/pais/Handy`
- **Interface**: `com.pais.Handy.Transcription`

### Available Methods

1. **StartTranscription()**: Begin recording audio for transcription
2. **StopTranscription()**: Stop recording and process transcription

## Usage Examples

### Using `busctl`

```bash
# Start transcription
busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StartTranscription

# Stop transcription
busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StopTranscription
```

### Using `dbus-send`

```bash
# Start transcription
dbus-send --session --print-reply \
  --dest=com.pais.Handy \
  /com/pais/Handy \
  com.pais.Handy.Transcription.StartTranscription

# Stop transcription
dbus-send --session --print-reply \
  --dest=com.pais.Handy \
  /com/pais/Handy \
  com.pais.Handy.Transcription.StopTranscription
```

### Integration with Hyperland

Add keybindings to your Hyperland config (`~/.config/hypr/hyprland.conf`):

```conf
# Handy transcription with push-to-talk
bind = SUPER, R, exec, busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StartTranscription
bindrt = SUPER, R, exec, busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StopTranscription
```

Or use a single toggle keybinding with a script:

```bash
#!/bin/bash
# ~/.config/hypr/scripts/handy-toggle.sh

# Simple toggle - assumes you're using push-to-talk style
if [ "$1" == "start" ]; then
    busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StartTranscription
else
    busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StopTranscription
fi
```

Then in your Hyperland config:

```conf
bind = SUPER, R, exec, ~/.config/hypr/scripts/handy-toggle.sh start
bindrt = SUPER, R, exec, ~/.config/hypr/scripts/handy-toggle.sh stop
```

## Verification

Check if the D-Bus service is running:

```bash
# List all user services (should show com.pais.Handy)
busctl --user list | grep com.pais.Handy

# Introspect the service to see available methods
busctl --user introspect com.pais.Handy /com/pais/Handy
```

## Logs

When running on Wayland, check the application logs for confirmation:

```
[INFO] Wayland session detected - keyboard shortcuts are disabled.
[INFO] Use D-Bus interface for transcription control:
[INFO]   busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StartTranscription
[INFO]   busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Transcription StopTranscription
[INFO] Initializing D-Bus service...
[INFO] D-Bus service registered at com.pais.Handy on /com/pais/Handy
```

## GUI Settings

The GUI settings window remains fully functional and can be accessed via the system tray icon, regardless of whether you're on Wayland or X11.

## Focus Handling on Wayland/Hyperland

Handy automatically handles window focus to ensure pasting works correctly:

1. **Captures Focus**: When transcription starts, the app captures which window currently has focus using `hyprctl activewindow`
2. **Non-Stealing Overlay**: The overlay window is created with `.focused(false)` to prevent stealing focus
3. **Focus Restoration**: Before pasting, the app automatically restores focus to the original window using `hyprctl dispatch focuswindow`

### Additional Hyperland Configuration (Optional)

If you still experience focus issues, you can add this to your `~/.config/hypr/hyprland.conf`:

```conf
# Prevent Handy overlay from stealing focus (usually not needed)
windowrulev2 = nofocus, title:^(Recording)$
windowrulev2 = noinitialfocus, title:^(Recording)$
```

### Console Output

When focus is captured and restored, you'll see:

```
Captured focused window: 0x12345678
Restoring focus to previous window...
Attempting to restore focus to window: 0x12345678
Successfully restored focus to previous window
```
