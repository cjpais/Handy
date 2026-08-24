# Platform differences

## Summary

Handy ships one product for macOS, Windows, and Linux, and the rest of this description is written from the macOS seat. This document is the consolidated list of every place a user on another platform would notice something different: a different default, a control that exists only on one platform, a permission step, a window that behaves differently, a keystroke that is sent differently, or a mechanism that is unreliable in one environment. Each row names the document that owns the behavior; this document does not re-describe it. Windows and Linux rows are read from the code and the README and are not verified by hand.

## The simple case

A user on any platform installs Handy, opens it, is asked for whatever permissions that platform requires (two on macOS, one on Windows, none on Linux), picks a model, and dictates with the default shortcut: Option+Space on a Mac, Ctrl+Space on Windows and Linux. The text is pasted with Cmd+V or Ctrl+V. On macOS and Windows a floating overlay shows the recording; on Linux there is none by default. Everything after that — models, history, settings, the tray — looks and works the same, with the exceptions tabled below.

## Defaults

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Transcribe shortcut | Option+Space | Ctrl+Space | Ctrl+Space | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |
| Transcribe with Post-Processing shortcut | Option+Shift+Space | Ctrl+Shift+Space | Ctrl+Shift+Space | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |
| Cancel shortcut | Escape | Escape | Escape in settings, but never registered (row hidden) | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |
| Keyboard Implementation | handy_keys | handy_keys | tauri | [The settings model](../foundations/the-settings-model.md#defaults-and-reset) |
| Paste Method | "Clipboard (Cmd+V)" | "Clipboard (Ctrl+V)" | "Direct" | [Pasting](../dictation/pasting.md) |
| Overlay | "Live" | "Live" | "None" | [The overlay](../dictation/the-overlay.md) |
| Overlay Position | "Bottom" | "Bottom" | "Bottom" | [The overlay](../dictation/the-overlay.md) |
| Application Language | System locale, else English | Same | Same | [About](../settings/about.md) |
| Application Theme | "System"; also colors the native title bar | "System"; also colors the native title bar | "System"; in-page colors only | [The settings window](../settings/the-settings-window.md) |
| Modifier names in shortcut chips | "Option", "Command" | "Alt", "Win" | "Alt", "Super" | [The shortcut recorder](../settings/shortcut-recorder.md) |
| Debug mode shortcut | Cmd+Shift+D | Ctrl+Shift+D | Ctrl+Shift+D | [Debug](../settings/debug.md) |
| Tray menu hints | "Cmd+," on Settings…, "Cmd+Q" on Quit | "Ctrl+,", "Ctrl+Q" | "Ctrl+,", "Ctrl+Q" | [The tray menu](../tray/the-tray-menu.md) |

## Permissions

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Onboarding permissions step ("Permissions Required") | Two cards: "Microphone Access" and "Accessibility Access", each with "Grant Permission"; the step completes only when both show "Granted" | One card, "Microphone Access", whose button reads "Open System Settings" and opens the Windows microphone privacy page; the step completes when access is no longer denied | Step skipped entirely | [Permissions](../setup/permissions.md) |
| How the grant is detected | The system prompt is shown, then Handy polls every second until both are granted | Handy reads the Windows consent store and polls every second; it cannot prompt | — | [Permissions](../setup/permissions.md) |
| Returning-user check at launch | Missing Accessibility or Microphone access forces the window visible on the permissions step | Denied microphone access (with at least one model downloaded) forces the window visible on the permissions step | None | [Windows and the tray](../foundations/windows-and-tray.md) |
| Accessibility banner in the settings window ("Handy needs accessibility permissions to type transcribed text." with "Open System Settings") | Shown while Accessibility is missing | Never | Never | [Permissions](../setup/permissions.md) |
| Shortcuts before permission | Nothing is registered until Accessibility is granted | Registered at launch | Registered at launch | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |
| "Microphone Access Denied" toast text | "Grant microphone access in System Settings → Privacy & Security → Microphone." | "Enable microphone access in Settings → Privacy & security → Microphone (including desktop app access)." | "Grant microphone access in your system's sound or privacy settings." | [Audio capture](../foundations/audio-capture.md) |

> Technical note: the Windows check reads the consent-store registry keys for the microphone (device-wide, and the "desktop apps" scope Handy falls under). A system tool that sets the app-wide key to deny without touching the desktop-app key is treated as allowed, so Handy's "denied" matches what actually blocks it.

## Shortcuts and triggers

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| fn / Globe key in a shortcut | Works only on Apple keyboards; a third-party keyboard never sends it | Not applicable (handy_keys accepts it, nothing sends it) | Refused by the tauri implementation | [The shortcut recorder](../settings/shortcut-recorder.md) |
| Cancel shortcut registration | Registered while recording | Registered while recording | Never registered; the "Cancel Shortcut" row is hidden; Escape never cancels, only the overlay ✕, the tray Cancel item, and `--cancel` | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |
| Shortcut recorder | System-wide key tap; captures keys typed in other apps | Same | Reads keys only while the settings window is focused; needs a main key; refuses fn and modifier-only | [The shortcut recorder](../settings/shortcut-recorder.md) |
| Secure Input | Detected, fallback registrations, tray badge, banner, recorder refusal, "Keyboard Diagnostic" in Debug | None of it exists | None of it exists | [Secure Input](secure-input.md) |
| Signals | SIGUSR2 = Transcribe, SIGUSR1 = Transcribe with Post-Processing | No signals | SIGUSR2 only; SIGUSR1 is left to the browser engine (it caused phantom recordings) | [Command line](../integration/command-line.md) |
| Command-line remote control (`--toggle-transcription`, `--toggle-post-process`, `--cancel`) | Same | Same | Same; the recommended way to bind shortcuts under Wayland, where Handy's own global shortcuts are not delivered | [Command line](../integration/command-line.md) |
| handy_keys startup failure | Falls back to tauri and saves it | Same | Linux already defaults to tauri | [Triggers and shortcuts](../foundations/triggers-and-shortcuts.md) |

## The overlay

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Default style | Live | Live | None (the README: some compositors treat the overlay as the active window, which breaks pasting) | [The overlay](../dictation/the-overlay.md) |
| Window kind | Non-activating panel at status level, joins all Spaces and full-screen apps | Ordinary always-on-top window, forced topmost again after every show | A layer-shell surface anchored to the screen edge when the compositor supports it; otherwise an always-on-top window | [Windows and the tray](../foundations/windows-and-tray.md) |
| Bottom placement | 15 points above the work area, so it rides above the Dock | 40 points above the screen's bottom edge (clears the taskbar) | 40 points above the bottom edge (layer-shell margin when anchored) | [Windows and the tray](../foundations/windows-and-tray.md) |
| Top placement | 46 points below the top edge | 4 points below the top edge | 4 points below the top edge | [Windows and the tray](../foundations/windows-and-tray.md) |
| Which monitor | The one under the mouse pointer | Same; sized in that monitor's own pixel scale | Same, but detection is unreliable under Wayland; with layer shell the anchor makes exact coordinates unnecessary | [Windows and the tray](../foundations/windows-and-tray.md) |
| Creation at launch | Always | Skipped if no monitor position can be determined (no overlay that run) | Always | [Windows and the tray](../foundations/windows-and-tray.md) |
| `HANDY_NO_GTK_LAYER_SHELL=1` | — | — | Skips the layer-shell surface; the overlay becomes a regular always-on-top window (KDE Plasma on Wayland reportedly needs this) | [The overlay](../dictation/the-overlay.md) |
| Focus side effect | None | None | A visible overlay can take focus on some compositors, so the paste lands in the wrong window or fails | [Pasting](../dictation/pasting.md) |

## The tray

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Click | Left click opens the menu | Left click or double click opens the settings window; right click opens the menu | Left click opens the menu | [The tray menu](../tray/the-tray-menu.md) |
| Icon style | Template (monochrome) icons that follow the menu bar's appearance | Light or dark icon chosen from the taskbar's own theme (not the app theme) | Colored (pink) icons: `handy`, `recording`, `transcribing` | [The tray menu](../tray/the-tray-menu.md) |
| Secure Input warning badge | Yes | No | No | [Secure Input](secure-input.md) |
| Dock icon | Removed when the window is closed with the tray shown; back when the window is shown | No equivalent | No equivalent | [Windows and the tray](../foundations/windows-and-tray.md) |
| Vanished icon recovery | Relaunching Handy (Spotlight, Finder, Dock, shell) recreates the tray icon before showing the window | Relaunching only shows the window | Relaunching only shows the window | [Windows and the tray](../foundations/windows-and-tray.md) |
| Runtime requirement | None | None | An app-indicator library; the packaged builds depend on it | [First launch](../setup/first-launch.md) |

## Pasting and the clipboard

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Paste keystroke | Cmd+V, with the V key resolved for the current layout | Ctrl+V (virtual key V) | Ctrl+V (character v) | [Pasting](../dictation/pasting.md) |
| "Direct" in Paste Method | Not offered; an existing "direct" selection is shown disabled | Offered | Offered and the default | [Pasting](../dictation/pasting.md) |
| "Clipboard (Ctrl+Shift+V)" and "Clipboard (Shift+Insert)" | Not offered | Offered | Offered | [Pasting](../dictation/pasting.md) |
| "External Script" | Not offered | Not offered | Offered; a path field ("/path/to/your/script.sh") appears below the dropdown | [Pasting](../dictation/pasting.md) |
| "Typing Tool" row | Hidden | Hidden | Shown only with Paste Method "Direct": "Auto (Recommended)" plus whichever of wtype, kwtype, dotool, ydotool, xdotool are installed | [Advanced](../settings/advanced.md) |
| Clipboard write | System clipboard | System clipboard | Under Wayland, `wl-copy` when installed; else the system clipboard | [Pasting](../dictation/pasting.md) |
| Keystroke delivery | Handy's own key injection (needs Accessibility) | Handy's own key injection | Wayland: wtype (skipped on KDE), then dotool, then ydotool; KDE Wayland: kwtype for typing; X11: xdotool, then ydotool; otherwise Handy's own injection, which the README calls limited on Wayland | [Pasting](../dictation/pasting.md) |
| "Reliable Paste (Beta)" (Debug) | Shown | Shown | Hidden | [Pasting](../dictation/pasting.md) |
| Auto Submit third option label | "Cmd+Enter" | "Super+Enter" (sends the Windows key) | "Super+Enter" | [Pasting](../dictation/pasting.md) |

## Audio

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| "Mute While Recording" mechanism | An AppleScript volume command; may be refused silently on unusual setups | The default output endpoint's mute switch | `wpctl`, then `pactl`, then `amixer`, whichever is installed; none installed means no mute | [Audio capture](../foundations/audio-capture.md) |
| "Clamshell Microphone" row (Debug) | Shown on laptops only (battery detected); the lid state is read from the system registry each time a stream opens | Hidden | Hidden | [Debug](../settings/debug.md) |
| Bluetooth headset microphone | Playback quality drops while recording because the headset switches to its bidirectional profile; the README recommends a wired or built-in microphone | Not called out | Not called out | [Audio capture](../foundations/audio-capture.md) |
| History playback | Streams the file | Streams the file | Reads the whole file into memory first (the streaming path is not used) | [The history page](../history/the-history-page.md) |

## Acceleration and post-processing

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| Whisper-family (transcribe.cpp) acceleration | Metal, with CPU fallback | Vulkan on x64 builds, with CPU fallback; the ARM build is CPU-only | Vulkan, with CPU fallback (OpenBLAS) | [Models](../foundations/models.md) |
| x64 build running under emulation on Windows ARM | — | GPU is disabled: the "transcribe.cpp Acceleration" dropdown offers only "CPU" and the saved setting is treated as CPU | — | [Advanced](../settings/advanced.md) |
| "transcribe.cpp Acceleration" options | "Auto", each GPU by name with its memory, "CPU" | Same | Same | [Advanced](../settings/advanced.md) |
| "ONNX Acceleration" row | Hidden unless the engine reports more than Auto and CPU; shipped builds are CPU-only, so it is not expected to appear | Same (the description still mentions "DirectML on Windows is experimental") | Same | [Advanced](../settings/advanced.md) |
| Whisper crashes on some configurations | — | Known issue in the README | Known issue in the README | [Transcribing](../dictation/transcribing.md) |
| "Apple Intelligence" post-processing provider | Listed on Apple-silicon Macs only; needs macOS 26 with Apple Intelligence enabled, else the section shows "Apple Intelligence is not available on this device. Requires an Apple Silicon Mac running macOS Tahoe (26.0) or later with Apple Intelligence enabled in System Settings." | Not listed | Not listed | [Post-processing](../dictation/post-processing.md) |

## Launch, autostart, and updates

| Behavior | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| "Launch on Startup" mechanism | macOS 13+: a login item attributed to Handy (System Settings › General › Login Items); any older launch-agent file from previous versions is removed. macOS 12 and earlier: a launch agent attributed to the developer name | A Run entry in the registry | `~/.config/autostart/Handy.desktop` | [Advanced](../settings/advanced.md) |
| Start Hidden | Runs as an accessory app (no Dock icon) when the tray is shown | Window hidden | Window hidden | [Windows and the tray](../foundations/windows-and-tray.md) |
| Forced window at launch | Missing permissions | Denied microphone with models downloaded | Never for permissions | [Windows and the tray](../foundations/windows-and-tray.md) |
| Portable mode | Not offered | Installer option "Portable Installation"; a `portable` marker next to the executable moves all data into `Data\` beside it | Not offered | [Data on disk](data-on-disk.md#portable-mode) |
| Installing an update | In place | In place; a portable install instead shows "Manual update required" with "Download installer" | In place for packages the updater supports; not verified per package format | [Updates](../integration/updates.md) |
| Package managers named in the README | Homebrew cask | winget | deb, rpm, AppImage | [First launch](../setup/first-launch.md) |

## Data locations

| Path | macOS | Windows | Linux | Owner |
| --- | --- | --- | --- | --- |
| App data directory | `~/Library/Application Support/com.pais.handy` | `%APPDATA%\com.pais.handy` | `~/.local/share/com.pais.handy` | [Data on disk](data-on-disk.md) |
| Log directory | `~/Library/Logs/com.pais.handy` | `%LOCALAPPDATA%\com.pais.handy\logs` | `~/.local/share/com.pais.handy/logs` | [Data on disk](data-on-disk.md) |
| Hugging Face cache | `~/.cache/huggingface/hub` | `C:\Users\<user>\.cache\huggingface\hub` | `~/.cache/huggingface/hub` | [Data on disk](data-on-disk.md) |

## Environment variables

| Variable | Platform | Effect | Owner |
| --- | --- | --- | --- |
| `HANDY_NO_GTK_LAYER_SHELL=1` | Linux | Overlay uses a regular always-on-top window instead of a layer-shell surface | [The overlay](../dictation/the-overlay.md) |
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | Linux | Works around window rendering crashes on some GPU and driver combinations. Handy sets it itself at every launch, so setting it by hand (as the README suggests) changes nothing | [First launch](../setup/first-launch.md) |
| `HF_HOME` | All | Moves the Hugging Face cache to `$HF_HOME/hub` | [Data on disk](data-on-disk.md) |
| `HANDY_METAL_RESIDENCY=1` | macOS | Restores the Metal engine's default memory residency behavior that Handy otherwise disables to avoid a shutdown crash | [Models](../foundations/models.md) |
| `RUST_LOG` | All | Console log filter only; the file log follows the "Log Level" setting | [Debug](../settings/debug.md) |

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | Same on every platform; on Linux the "Cancel Shortcut" row stays hidden even with push to talk off, because Cancel is never registered there. | Same. |
| Binding | Defaults differ (Option vs Ctrl). The Cancel binding is inert on Linux. | Same. |
| Overlay style | Default Live on macOS and Windows, None on Linux; the Linux README advises leaving it None. | Same. |
| Streaming model | No platform difference. | No platform difference. |
| Voice activity detection | No platform difference. | No platform difference. |
| Always-on microphone | No platform difference in the setting; the mute it interacts with uses a different mechanism per platform. | Same. |

## Cancel and interrupt

| Event | Before a dictation | During a dictation |
| --- | --- | --- |
| Cancel | No platform difference. | Escape works on macOS and Windows only; on Linux the overlay ✕ (if an overlay is shown at all), the tray Cancel item, and `--cancel` are the only cancels. |
| Another trigger | SIGUSR1 exists on macOS only; SIGUSR2 on macOS and Linux; Windows has flags only. | Same. |
| A setting changed mid-way | Paste Method and Typing Tool offer different options per platform; switching Keyboard Implementation on Linux from tauri to handy_keys is possible but not the default. | Same. |
| Microphone lost | The denied-access toast wording differs; Windows can also force the window open at launch. | The stop still works everywhere. |
| Model or processing failure | GPU backends differ, so a model that loads on one platform may fall back to CPU or crash on another (README: Whisper on some Windows and Linux configurations). | Same. |
| The active application changes | No difference. | On Linux a visible overlay may itself become the active window and receive the paste. |
| Handy quits or the system sleeps | Autostart is re-applied at every launch through a different mechanism per platform. | No difference. |
| Keyboard channel changes | Secure Input exists only on macOS; the tauri implementation on Linux has no fallback to switch to. | Same. |

## Interactions with other systems

**Permissions.** Two permissions on macOS, one on Windows (read, not requested), none on Linux. See the Permissions table above and [Permissions](../setup/permissions.md).

**History and recordings.** Identical files and behavior on every platform; only the playback path and the folder locations differ.

**Clipboard.** Wayland uses `wl-copy` for the write when installed; the restore and Copy to Clipboard use the system clipboard everywhere.

**Model state.** The same models run everywhere; acceleration differs per platform and per build, and the accelerator dropdown adapts to what the host reports.

**Tray and overlay.** See the two tables above: click behavior, icon style, and Dock handling for the tray; window kind, placement offsets, and default style for the overlay.

**Sounds and system audio.** Chimes play through the same engine everywhere; mute uses a platform-specific mechanism that can be silently absent on Linux.

**Settings persistence.** The same file with platform-dependent defaults for the two shortcuts, Paste Method, Keyboard Implementation, and Overlay. A settings file copied from one platform to another keeps its values, so a macOS file on Linux brings the overlay back on (the README warns about this).

**Platform differences.** This document.

## Edge cases

- The Windows ARM build and the x64 build under emulation behave differently: the ARM build simply has no GPU option, while the emulated x64 build hides "Auto" and the GPU entries and forces CPU even if the settings file says otherwise.
- On Linux the Cancel binding is still stored and still has a default, so a settings file edited by hand can carry a Cancel shortcut that does nothing.
- The Post-Processing Hotkey and Transcribe Shortcut chips format the same stored combination with different words per platform; a settings file moved between platforms shows "Option" as "Alt".
- On Windows, a left click on the tray icon while the settings window is already in front does nothing visible beyond focusing it.
- On macOS the Apple Intelligence provider appears in the dropdown on every Apple-silicon Mac, including those running a macOS older than 26; only selecting it reveals the unavailable notice.
- The README's Linux note names the overlay setting as "Overlay Position: None"; the setting is now "Overlay" with the option "None".
- `--debug` and Ctrl/Cmd+Shift+D behave the same on every platform; only the key differs.

## Open questions and verification

- No Windows or Linux row in this document was verified by hand; every one was read from `cfg(target_os)` branches, the frontend's platform checks, and the README.
- Handy sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` unconditionally at launch on Linux, so the README's instruction to set it manually cannot change anything. Suspected stale documentation (or the variable should be made conditional on the user not having set it).
- The Linux app data path the README gives (`~/.config/com.pais.handy/`) does not match the path library's `~/.local/share/com.pais.handy`. Suspected documentation bug; see [Data on disk](data-on-disk.md).
- The "ONNX Acceleration" row is hidden unless more than two options are reported; with the shipped CPU-only ONNX runtime it is expected never to appear on any platform, while its description still mentions DirectML. Suspected stale UI.
- The two Apple Intelligence error strings disagree on the minimum macOS: the settings window says "macOS Tahoe (26.0) or later", the backend's model-list error says "macOS 15 or later". Suspected bug in one of the strings.
- Whether the overlay's 40-point bottom offset on Windows actually clears a taskbar at every scale was not measured.
- Whether the tauri keyboard implementation on Linux delivers global shortcuts at all under Wayland (the README says system-level shortcuts must be configured in the desktop environment) was not tested; the description treats the command-line flags as the supported path there.
- The Linux updater path (which package formats can self-update) was not determined from the code.
- Whether a left click on the Linux tray icon opens the menu on every desktop (app-indicator implementations vary) was not checked.

Verified against Handy commit `af48dd6`.
