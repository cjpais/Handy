# The settings model

## Summary

This document is how settings behave as a whole: where they are shown, when they are saved, what a reset does, what "default" means, how a damaged or out-of-date settings file is handled, and which settings take effect immediately versus at the next dictation or the next launch. Every settings-page document links here instead of repeating it. There is no Save button, no Apply, no Cancel, and no confirmation anywhere in the settings window except for deleting a model.

## The simple case

The user opens Settings › Advanced and flips "Append Trailing Space" on. The switch moves immediately, a tiny spinner may flash over it, and the change is written to disk before the spinner goes. The next dictation ends with a space after the pasted text. If they change their mind they flip it back; there is nothing to confirm. A dropdown works the same way: choosing a value closes the menu and saves. Text fields (an API key, a custom word) save when the field loses focus or when Enter or the adjacent button is pressed. The reset arrow beside a control puts back the value a fresh install would have, also immediately.

## How a change is saved

Every control writes its setting the moment it changes. The control shows the new value at once (optimistically), asks the backend to apply and persist it, and, if the backend refuses, snaps back to the old value. While the request is in flight the control is disabled and most show a small spinner; this is usually too quick to notice except for controls that do real work — changing the microphone (the stream is rebuilt), switching the keyboard implementation (every shortcut is re-registered), changing the always-on microphone mode.

Some changes are applied live as well as saved: the tray icon appears or disappears, the overlay moves between top and bottom, the theme changes the window and overlay colors, autostart registers or unregisters the login item, the post-processing shortcut is registered or unregistered, the interface language re-renders the window and rebuilds the tray menu. Others are read only at the next dictation: push to talk (at the next key event), VAD, filler-word removal, custom words, paste method, auto-submit, clipboard handling, the overlay style's effect on level updates. A few need the next model load: the accelerators.

> Technical note: settings are one JSON object in `settings_store.json` in the app data directory, rewritten in full on every change. Reads also write, because migrations and missing fields are filled in on load.

## Defaults and reset

"Default" means the value a fresh install has. Some defaults depend on the platform: the transcribe shortcut (Option+Space on macOS, Ctrl+Space elsewhere), the paste method (Clipboard on macOS and Windows, Direct on Linux), the keyboard implementation (handy_keys on macOS and Windows, tauri on Linux), the overlay style (Live on macOS and Windows, None on Linux), the interface language (the system locale, else English).

The reset arrow writes the default through the same path as any change, so its side effects are the same (a shortcut is re-registered, the microphone stream is rebuilt). Not every control has one: toggles and most dropdowns do not; the shortcut chips, microphone, output device, clamshell microphone, and language picker do. There is no "reset everything".

The full list of defaults that matter to the experience, for reference:

| Setting | Default |
| --- | --- |
| Push To Talk | on |
| Audio Feedback | off (volume 100%, theme Marimba) |
| Microphone / Output Device | Default (system) |
| Language | Auto |
| Translate to English | off |
| Overlay | Live, Bottom |
| Unload Model | After 5 minutes |
| Paste Method | Clipboard (Cmd+V) |
| Clipboard Handling | Don't Modify Clipboard |
| Auto Submit | Off (key Enter) |
| Voice Activity Detection | on |
| Remove Filler Words | on |
| Custom Words | none (correction threshold 0.18) |
| Append Trailing Space | off |
| History Limit | 5 entries; Auto-Delete Recordings: Keep latest 5 |
| Start Hidden, Launch on Startup | off |
| Show Tray Icon | on |
| Experimental Features, Post Processing | off |
| Always-On Microphone, Mute While Recording, Keep Mic Open | off |
| Check for Updates, Show What's New | on |
| Theme | System |
| Log Level | Debug |
| Paste Delay (Before / After) | 60 ms / 60 ms |
| Extra Recording Buffer | 0 ms |

## Damaged and out-of-date settings files

If the settings file cannot be read as a whole (a value written by a newer or older version, a hand edit), Handy keeps every field that is individually valid and resets only the broken ones to their defaults, so one bad value never wipes the configuration. If the file is not an object at all, everything resets. A field missing from the file gets its default. Migrations run on first read: a file from before the overlay style existed gets Live (or None if the old position was "none"), a pre-0.9 GPU device index is cleared, and a file without the What's New marker is treated as an upgrade so the release notes show once. Bindings missing from the file are added from defaults.

## Debug mode and hidden controls

Debug mode (Cmd+Shift+D in the settings window) is itself a saved setting, so it survives relaunch. The `--debug` flag turns it on for one run without saving. Experimental Features (Advanced) and Post Processing (Advanced › Experimental) are ordinary saved toggles that reveal more controls and, for Post Processing, a sidebar section and a shortcut.

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | A saved toggle; read per key event. | Same. |
| Binding | Saved shortcuts; registered on change. | Same. |
| Overlay style | A saved dropdown; applied live to the next overlay show. | Same. |
| Streaming model | Not a setting; a capability of the active model. | — |
| Voice activity detection | A saved toggle; read at the trigger. | Same. |
| Always-on microphone | A saved toggle (Debug section); applied live. | Same. |

## Cancel and interrupt

For a single control's change:

| Event | Before active (control opened, no change) | While active (change in flight) |
| --- | --- | --- |
| Cancel | Escape closes the language picker and the models-page language filter; other dropdowns close on a click outside. | No way to cancel an in-flight save. |
| Another trigger | A dictation can start while a dropdown is open; the dropdown stays open. | Same. Changes that need the microphone idle (input channel) are refused with an error. |
| A setting changed mid-way | Opening one dropdown closes another. | Two controls can be in flight at once; each resolves independently. |
| Microphone lost | Device dropdowns refresh when opened. | A microphone change that fails to open the device is reported as an error and the control snaps back. |
| Model or processing failure | None. | A model selection that fails reverts (see [Models](models.md)). |
| The active application changes | The settings window keeps its state; dropdowns stay open. | Same. |
| Handy quits or the system sleeps | Nothing unsaved exists. | A save not yet written is lost; the file is written atomically per change. |
| Keyboard channel changes | Switching the keyboard implementation resets shortcuts it cannot express and shows "Keyboard shortcuts were incompatible and reset to defaults". | Same. |

## Interactions with other systems

**Permissions.** None.

**History and recordings.** History Limit and Auto-Delete Recordings trigger a cleanup as soon as they change, which can delete unsaved entries immediately.

**Clipboard.** None.

**Model state.** Selecting a model and changing the unload timeout are settings; the accelerators mark the loaded model for reload at next use.

**Tray and overlay.** Show Tray Icon, Overlay, Overlay Position, Application Theme, and Application Language all apply live.

**Sounds and system audio.** Audio Feedback, Volume, Sound Theme, Output Device apply at the next chime; the play button on the Sound Theme row previews immediately.

**Settings persistence.** This document.

**Platform differences.** See Defaults above. The Typing Tool and External Script controls exist only on Linux; Direct paste is not offered on macOS; Shift+Insert and Ctrl+Shift+V only on Windows and Linux; the clamshell microphone only on MacBooks.

## Edge cases

- Editing the settings file by hand while Handy runs: the next read merges defaults and may rewrite the file; the UI does not watch the file and shows stale values until it next refreshes (model state changes and some setting commands trigger a refresh).
- Setting the history limit to 0 deletes every unsaved entry at once.
- The API key fields are saved when the field loses focus; switching provider before blurring discards the typed key.
- Two Handy processes cannot run, so there is no concurrent-write case.

## Open questions and verification

- Whether every toggle visibly shows the spinner, or only slow ones, was not observed.
- The settings file is rewritten on every read that performed a migration; whether a read-only app data directory breaks startup was not checked.
- Which controls refresh the UI after a backend-initiated change (for example the microphone falling back to Default) was read from the event list and not observed.

Verified against Handy commit `af48dd6`.
