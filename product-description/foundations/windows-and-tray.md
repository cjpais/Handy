# Windows and the tray

## Summary

This document is the window model: the three things Handy puts on screen — the settings window, the overlay, and the menu-bar icon — and the rules for showing, hiding, focusing, and quitting. It covers what a launch does, what closing the window does, how the Dock icon comes and goes, and how a second launch behaves. The contents of the settings window are in `settings/`; the overlay's states are in [The overlay](../dictation/the-overlay.md); the tray menu's items are in [The tray menu](../tray/the-tray-menu.md).

## The simple case

The user opens Handy. A menu-bar icon appears and, because this is not a hidden start, the settings window opens in front. They close the window with the red button; the window disappears, Handy's icon leaves the Dock, and the menu-bar icon stays. Handy keeps running and shortcuts keep working. Later they click the menu-bar icon and choose "Settings…": the window comes back and the Dock icon returns. To quit they choose "Quit" from the menu, or press Cmd+Q while the settings window is in front.

## Launch

At launch Handy creates the settings window hidden, builds the tray icon, and then decides whether to show the window:

- Shown if Start Hidden is off and `--start-hidden` was not passed.
- Shown anyway if the tray icon is hidden (Show Tray Icon off, or `--no-tray`), because without a tray there would be no way back in.
- Shown anyway on Windows when models are downloaded but microphone access is denied, so the permissions step can be seen.
- Otherwise hidden; on macOS Handy then runs as an accessory app with no Dock icon.

The window is 680×570 points and cannot be made smaller; it can be resized larger and maximized. Its title is "Handy". On first launch it shows onboarding (see [First launch](../setup/first-launch.md)); afterwards it opens on the General section.

> Technical note: shortcuts and text injection are not started until the window's page has loaded and, on macOS, Accessibility access has been confirmed. With a hidden start that happens as soon as the hidden page loads, so shortcuts work without the window ever being shown.

## Closing, hiding, and showing

Closing the settings window never quits Handy. The window is hidden; on macOS, if the tray icon is shown, Handy also switches to an accessory app so its Dock icon disappears and Cmd+Tab skips it. If the tray icon is hidden the Dock icon stays so the app can be reopened from there.

The window is shown again by: the tray's "Settings…" item; the tray's Secure Input warning line; "Check for Updates…" in the tray (which also starts a check); launching Handy again while it is running (from Spotlight, the Dock, Finder, or a shell); or, on Windows, a left click or double click on the tray icon. Showing restores the window if minimized, brings it to front, focuses it, and on macOS returns Handy to a regular app with a Dock icon.

Relaunching while running is also the recovery for a vanished menu-bar icon (a known macOS problem): if the window is hidden at that moment Handy recreates the tray icon before showing the window.

## The tray icon

The menu-bar icon has three states — idle, recording, transcribing — drawn as template images that follow the menu bar's light or dark appearance. On macOS an idle icon with a warning badge is used while Secure Input is blocking a shortcut. The tooltip is "Handy v{version}" (with " (Dev)" in development builds). A click opens the menu. Show Tray Icon (Advanced) hides the icon live; `--no-tray` hides it for one run.

## The overlay window

The overlay is a separate, borderless, transparent, non-focusable window created hidden at launch and reused for every dictation. It floats above all windows and all Spaces (and full-screen apps), never takes focus, and is positioned at show time on the monitor under the mouse pointer: horizontally centered, 15 points above the bottom of the work area (so it tracks the Dock) or 46 points from the top when Overlay Position is Top. Its size is 256×46 points as a pill and 400×120 as a panel. It is shown at the trigger, resized and repositioned for each state change, and hidden 300 ms after the dictation ends to let it fade. With Overlay set to None it is never shown, though the window still exists.

## Quitting

"Quit" in the tray menu, or Cmd+Q with the settings window focused, exits Handy. The loaded model is released first. A dictation in progress is abandoned; nothing partial is written. Launch on Startup (Advanced) registers Handy as a login item (on macOS 13+ through the system's login-items mechanism, so it appears under System Settings › General › Login Items as Handy).

## Second launches and remote control

Handy enforces a single instance. A second launch with no flags sends "show the window" to the running copy and exits. With `--toggle-transcription`, `--toggle-post-process`, or `--cancel` it sends that action and exits, without showing the window. With `--start-hidden`, `--no-tray`, or `--debug` on a second launch, the flags are ignored (they only apply to the first process). The headless flags (`--transcribe-file`, `--list-models`, `--list-devices`) run a separate process that never creates a window or tray; see [Command line](../integration/command-line.md).

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | No effect on windows. | No effect. |
| Binding | No effect on windows. | No effect. |
| Overlay style | None: the overlay window is never shown. Minimal: pill only. Live: panel with a streaming model, pill otherwise. | A change takes effect at the next show; an overlay already showing stays until the dictation ends. Position changes move the overlay live. |
| Streaming model | Decides pill vs panel under Live. | Fixed at the trigger. |
| Voice activity detection | No effect on windows. | No effect. |
| Always-on microphone | No effect on windows. | No effect. |

## Cancel and interrupt

For the window lifecycle (the "interaction" being show → use → close):

| Event | Before active (window hidden) | While active (window shown) |
| --- | --- | --- |
| Cancel | — | Closing the window hides it; Escape does nothing to the window. |
| Another trigger | A dictation runs without the window; toasts are not seen. | A dictation runs; toasts appear in the window; the Handy window itself can be the paste target if it is frontmost (see [Pasting](../dictation/pasting.md)). |
| A setting changed mid-way | Show Tray Icon off with the window hidden: the window is not auto-shown until relaunch (the Dock icon is also gone on macOS) — the only way back is relaunching Handy. | Show Tray Icon off: the icon disappears; closing the window then keeps the Dock icon. |
| Microphone lost | No effect. | No effect. |
| Model or processing failure | Toasts are missed. | Toasts shown. |
| The active application changes | — | The window stays where it is; the overlay follows the mouse pointer's monitor, not the focused window. |
| Handy quits or the system sleeps | Quit ends the process; on the next launch the window follows the Start Hidden rule, not its previous state. | Same. |
| Keyboard channel changes | A Secure Input warning is visible only as the tray badge and menu line. | The banner appears at the top of the window content. |

## Interactions with other systems

**Permissions.** On macOS the returning-user permission check runs at every launch; if Accessibility or Microphone access is missing the window is forced visible and shows the permissions step.

**History and recordings.** None.

**Clipboard.** None.

**Model state.** The model is released at quit.

**Tray and overlay.** This document.

**Sounds and system audio.** None.

**Settings persistence.** `start_hidden`, `show_tray_icon`, `autostart_enabled`, `overlay_style`, `overlay_position`, `theme`.

**Platform differences.** The Dock/accessory switch is macOS-only. On Windows a left click on the tray icon opens the window and a right click opens the menu; elsewhere a left click opens the menu. On Linux the overlay uses a layer-shell surface anchored to the screen edge when the compositor supports it (set `HANDY_NO_GTK_LAYER_SHELL=1` to disable), monitor detection is unreliable under Wayland, and the overlay defaults to None. The theme setting colors the native title bar on macOS and Windows only. On Windows the overlay is forced topmost after each show. Autostart on macOS 12 and older uses a launch agent attributed to the developer name rather than the app.

## Edge cases

- Start Hidden with Show Tray Icon off is a combination Handy refuses to honor: the window shows anyway.
- Turning Show Tray Icon off cannot strand the user: the toggle is only reachable with the window shown, and closing the window while the tray is hidden keeps the Dock icon, so the window can always be reopened from the Dock. The only way to end up with neither icon is the tray icon vanishing on its own (see [The tray menu](../tray/the-tray-menu.md)); relaunching from Spotlight recreates it.
- The overlay appears on the monitor with the mouse pointer, which may not be the monitor with the text field being dictated into.
- With the window minimized to the Dock, "Settings…" unminimizes it.
- Cmd+Q only works while the settings window (not the overlay) is focused; the overlay can never be focused.
- A full-screen app: the overlay still appears over it; the settings window, when shown, opens on the desktop Space as macOS decides.

## Open questions and verification

- Whether "Check for Updates…" from the tray shows the window before or after the check starts was read from the code (show, then check); not observed.
- The exact overlay offsets (15 points above the work area, 46 below the top) and whether the bottom placement clears the Dock on the current macOS were not measured.
- Whether the settings window remembers its size and position between launches (the code does not save them, so it should not) was not confirmed.

Verified against Handy commit `af48dd6`.
