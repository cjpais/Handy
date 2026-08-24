# Permissions

## Summary

On macOS Handy needs two system permissions: Microphone access to hear the user and Accessibility access to see global shortcuts and to type or paste into other applications. The permissions step asks for both during onboarding on a screen headed "Permissions Required", with one card per permission and a "Grant Permission" button on each; after a button is clicked Handy polls the system once a second until the permission appears. The same step is re-used at every later launch if either permission has gone missing. Inside the main window a one-line banner, "Handy needs accessibility permissions to type transcribed text.", stands in for the Accessibility half when the window is shown without it. Windows has a reduced version of the step (microphone only, driven by a registry consent check); Linux has no step at all. The step is the second thing a new user sees at [first launch](first-launch.md), before [choosing a model](choosing-a-model.md).

## The simple case

The settings window shows the Handy logo, "Permissions Required", and "Handy needs a couple of permissions to work properly." Below are two cards. The first has a microphone icon, "Microphone Access", "Required to hear your voice for transcription.", and a "Grant Permission" button. The second has a keyboard icon, "Accessibility Access", "Required to type transcribed text into your applications.", and its own "Grant Permission".

The user clicks the microphone button. macOS shows its own dialog asking whether Handy may access the microphone; the card's button is replaced by a spinner and "Waiting...". The user clicks OK in the system dialog and within a second the card reads "Granted" with a check mark. They click the Accessibility button. macOS shows its dialog offering to open System Settings; the user opens it, finds Handy in Privacy & Security › Accessibility, and turns it on. The card, still reading "Waiting...", flips to "Granted" within a second, and Handy's shortcuts come alive at that instant. With both granted the screen is replaced by a green check and "All set!", and 300 ms later the window moves on to the model step.

## The interaction, event by event

```mermaid
stateDiagram-v2
    [*] --> checking : step appears (spinner)
    checking --> needed : one or both permissions missing
    checking --> all_set : both already granted
    needed --> waiting : "Grant Permission" clicked (system prompt, polling starts)
    waiting --> waiting : poll every 1 s; a card flips to "Granted" when macOS says so
    waiting --> all_set : both granted (polling stops, devices refreshed)
    waiting --> needed : three consecutive check failures (polling stops, toast)
    all_set --> [*] : 300 ms later, next step
```

### Start

The step starts when the window shows it: for a new install, right after the page loads; for a returning user, only when the launch check found Accessibility or Microphone access missing, in which case the window is brought to the front even under a hidden start. The screen is a centered spinner while Handy asks macOS for the state of both permissions. Each card then shows "Granted" or its "Grant Permission" button. If Accessibility is already granted at this moment, Handy initializes text injection and registers its shortcuts immediately, before the step is finished. If the check itself fails, a toast reads "Failed to check permissions. Please try again." and both cards show their buttons.

> Technical note: the Accessibility check is the system's "is this process trusted" query; the Microphone check is whether the app's microphone authorization is exactly "authorized", so "not determined" (never asked) and "denied" both count as missing. Text injection and shortcuts are initialized here rather than at process start so that no permission dialog appears before the user reaches this screen.

### Ends at once

The step ends without any click when both permissions are already granted at the start: the spinner is replaced by "All set!", the microphone and output device lists are refreshed, and 300 ms later the window moves on — to the model step for a new install, to the main window for a returning user. This is what a reinstall or a returning user whose check failed for some other reason sees. On Linux the step ends before it is drawn: the page skips straight to the next step. On Windows only the microphone card exists, and if the registry consent is not denied (allowed or unknown) the step ends the same way.

### Becomes active

The step becomes active on the first "Grant Permission" click. For the microphone, Handy asks macOS to request access, which shows the system's microphone dialog if the user has never been asked, and does nothing visible if the user answered before. For Accessibility, Handy asks macOS to prompt, which shows the system's dialog offering to open System Settings; the permission itself can only be granted there. Either click replaces that card's button with a spinner and "Waiting..." and starts the poll: once a second Handy re-reads both permissions. If the request call fails, a toast reads "Failed to request permission. Please try again." and the card keeps its button.

### While active

Every second, each card whose permission has appeared flips from "Waiting..." (or from its button, if the user granted it by hand) to "Granted". When Accessibility appears, text injection is initialized and shortcuts are registered on the spot, so Option+Space works from then on even though the step is still on screen. The other card's "Grant Permission" button stays clickable; clicking it requests that permission and keeps the same poll running. The poll tolerates two consecutive failures; on the third it stops and a toast reads "Failed to check permissions. Please try again.". The step has no timeout and no other controls.

### Finish

The step finishes the first poll on which both permissions are granted: the poll stops, the microphone and output device lists are refreshed, the screen becomes the green check with "All set!", and 300 ms later the window moves on. Nothing is written to settings by this step. For a new install the next screen is the model step; for a returning user it is the main window, where shortcuts and text injection are initialized again if the step did not already do so (the calls are harmless when repeated).

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | No effect. | No effect. |
| Binding | Decides which shortcuts are registered the moment Accessibility is confirmed: the post-processing shortcut only if post-processing is enabled. | No effect. |
| Overlay style | No effect. | No effect. |
| Streaming model | No effect. | No effect. |
| Voice activity detection | No effect. | No effect. |
| Always-on microphone | On (a returning user only): the microphone was opened at process start, so the system microphone prompt has already appeared before this step, and a denial there is what brings the user here. | No effect. |

## Cancel and interrupt

| Event | Before active (cards showing, no click yet) | While active (waiting, polling) |
| --- | --- | --- |
| Cancel | There is no skip. Escape does nothing; `handy --cancel` does nothing. Closing the window hides the step; "Settings…" in the tray brings it back unchanged. | Same; the poll keeps running in the hidden window and the step advances unseen when both are granted. |
| Another trigger | Before Accessibility is granted a shortcut press is not seen at all. A remote toggle or signal starts a dictation regardless; if the microphone is not yet authorized, macOS shows its microphone prompt at that moment, which is a second way to grant it. | After Accessibility is granted, shortcuts work: a dictation runs under the step and, with no model, fails at the stop with a toast. A microphone prompt triggered this way is noticed by the poll within a second. |
| A setting changed mid-way | No controls are visible. | No controls are visible. |
| Microphone lost | No effect; the step checks authorization, not whether a device exists. | No effect. |
| Model or processing failure | No effect. | No effect. |
| The active application changes | System Settings is the expected foreground app. The step waits. | The poll continues while Handy is in the background; the step completes and moves on without bringing the window forward. |
| Handy quits or the system sleeps | Granted permissions are macOS state and persist. The next launch re-runs the check; for a new install the step reappears and passes at once if both were granted. | Same; an in-flight system dialog is dismissed with the process. Sleep pauses the poll with the machine. |
| Keyboard channel changes | Secure Input has no effect on the permission checks. The Secure Input fallback is reconciled when shortcuts are registered. | Same. |

## Interactions with other systems

**Permissions.** This document. After onboarding, a denied microphone surfaces as the "Microphone Access Denied" toast with "Grant microphone access in System Settings → Privacy & Security → Microphone." at the next trigger (see [Audio capture](../foundations/audio-capture.md)); a missing Accessibility permission surfaces as shortcuts that do nothing, a paste that fails, and the banner below.

**History and recordings.** None.

**Clipboard.** None here; pasting with Cmd+V needs Accessibility, which is why it is asked for (see [Pasting](../dictation/pasting.md)).

**Model state.** None.

**Tray and overlay.** The tray icon and menu are present throughout. No overlay.

**Sounds and system audio.** None.

**Settings persistence.** The step writes nothing. The returning-user re-check reads only `onboarding_completed`.

**Platform differences.** Windows shows only the microphone card; its button reads "Open System Settings" and opens Settings › Privacy & security › Microphone, after which the same one-second poll reads the consent from the registry (the device-wide key, the desktop-app key, and the store-app key; "unknown" counts as not denied). Windows also forces the window visible at launch when models are downloaded but the microphone is denied. Linux skips the step entirely and never shows the banner. Text injection on Windows and Linux is initialized when the main window appears, not here.

## The banner in the main window

Above the content of every section on macOS, when Accessibility access is missing at the moment the main window appears, a bordered banner reads "Handy needs accessibility permissions to type transcribed text." with an "Open System Settings" button on the right. The first click asks macOS to prompt, which shows the system dialog offering to open System Settings. The second and every later click only re-checks the permission: if it has been granted the banner disappears; if not, nothing visible happens. The banner does not poll and does not re-check on its own, so a user who grants the permission in System Settings sees the banner until they click its button again or relaunch.

In practice the banner is rarely reached, because the launch check routes a missing permission to the permissions step before the main window is shown. It appears when that check itself failed, or on a Mac where the system reports trust inconsistently.

## Edge cases

- A user who denied the microphone earlier (in a previous install, or to the prompt a dictation triggered) gets no system dialog from "Grant Permission": the card goes to "Waiting..." and stays there. The step offers no "Open System Settings" button on macOS, so the only way on is System Settings › Privacy & Security › Microphone by hand. Suspected gap.
- After three consecutive check failures the poll stops. If both cards are already at "Waiting..." there is no button left to restart it; the step is stuck until the window is reloaded by a relaunch. Suspected bug.
- Granting a permission by hand without clicking its button is picked up only while the poll is running (after any button click). With no click yet, the card keeps its button; clicking it then returns within a second.
- Accessibility confirmed while the microphone is still pending makes shortcuts live on the permissions screen; a dictation started then can trigger the microphone prompt and, with no model, ends in a "Transcription Failed" toast.
- A returning user whose microphone authorization is "not determined" (for example after a macOS privacy reset) is sent to the permissions step with the Accessibility card already "Granted".
- The step's "Open System Settings" label exists only on Windows; on macOS the same label is used only by the banner.
- Nothing in the step or the banner can detect Accessibility being revoked while Handy runs; shortcuts simply stop arriving.

## Open questions and verification

- Whether macOS shows the microphone dialog again after an earlier denial (it should not), and therefore whether the "Waiting..." dead end is reachable on a fresh install, was read from the system API, not reproduced.
- The three-failure dead end with both cards waiting is inferred from the code; which errors the system calls can actually throw was not determined.
- Whether the banner is ever shown on a real machine, given the launch check, was not confirmed; it was read as reachable only through the check's failure path.
- The banner's second click re-checking without opening System Settings, under a button still labelled "Open System Settings", looks unintended. Suspected bug.
- Whether shortcuts registered the instant Accessibility is granted work immediately on current macOS, or need the process to relaunch (a known behaviour of the trust cache in some builds), was not tested.
- The 300 ms "All set!" pause and the one-second poll interval were read from the code, not timed.

Verified against Handy commit `af48dd6`.
