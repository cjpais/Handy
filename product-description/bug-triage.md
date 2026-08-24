# Bug triage

A consolidated list of the defects and inconsistencies that the feature documents raised in their "Open questions and verification" sections and in their bodies. Each entry is read from the Handy source at commit `af48dd6` and its tests; none has yet been confirmed in the running product, so no entry carries a **Status** line. The list exists so the product team can decide, item by item, whether to fix, to document as intended, or to leave.

## Summary

The forty-one documents raised about ninety suspected problems. Merged by root cause they come to 39 entries: 7 high, 17 medium, 15 low. The high ones share one shape — a feature that silently does nothing, or keeps going after the user has told it to stop: post-processing is inert on a fresh install until a prompt is chosen, a cancel during transcription leaves the shortcut dead until the abandoned work finishes, a request to a slow LLM has no timeout, and the shortcut recorder commits Escape and keys typed into other applications. The largest clusters are *failures that are only logged* (B-10, B-13, B-30, B-36: seven documents raised some form of "the user sees nothing when this fails"), *history housekeeping* (B-03, B-07, B-09, B-20, B-24), and *the Debug section* (B-06, B-26, B-31). Entries B-37 and B-38 gather the copy and cosmetic slips.

| ID | Title | Severity | Area | Decision needed |
| --- | --- | --- | --- | --- |
| B-01 | Post-processing is silently inert until a prompt is chosen | high | post-processing | fix |
| B-02 | A cancel during transcription leaves the shortcut dead until the abandoned work finishes | high | dictation | fix |
| B-03 | History Limit saves and deletes on every keystroke | high | settings, history | fix |
| B-04 | The shortcut recorder commits Escape and captures keys typed in other applications | high | settings | fix |
| B-05 | An LLM request has no timeout, so one slow provider hangs the dictation | high | post-processing | fix |
| B-06 | `--debug` does not reveal the Debug section the README says it enables | high | command line | fix |
| B-07 | A dictation cancelled during transcription leaves an orphaned recording that cleanup never removes | high | history | fix |
| B-08 | Speech the model hears as nothing becomes a "Transcription failed" history entry | medium | dictation, history | product call |
| B-09 | Retention cleanup does not refresh an open History page | medium | history | fix |
| B-10 | Post-processing and update failures are reported only to the log | medium | post-processing, updates | product call |
| B-11 | `--toggle-post-process` and the tray run post-processing even when the feature is off | medium | command line, tray | fix |
| B-12 | Installing an update relaunches Handy in the middle of a dictation | medium | updates | fix |
| B-13 | Cancelling a download during Verifying or Extracting does not stop it | medium | models | fix |
| B-14 | Secure Input engaging while a push-to-talk key is held loses the release | medium | secure input | fix |
| B-15 | A model with a deleted file stays selected and only fails at the next dictation | medium | models | fix |
| B-16 | Deleting a catalog model removes the whole Hugging Face repo folder | medium | models | product call |
| B-17 | A recordings folder or database deleted while Handy runs is not recreated until relaunch | medium | history, data | fix |
| B-18 | The system going to sleep mid-recording leaves a dead stream that still counts as recording | medium | audio | fix |
| B-19 | Two dictations finishing in the same second share one recording filename | medium | history | fix |
| B-20 | The permissions step can dead-end: a denied microphone waits forever and three failed polls stop polling | medium | setup | fix |
| B-21 | Toasts are the only error channel and they only render inside the settings window | medium | cross-cutting | product call |
| B-22 | Turning Experimental Features off hides the Post Processing toggle without disabling post-processing | medium | settings | product call |
| B-23 | Re-selecting the active model from the footer reloads it | medium | models | fix |
| B-24 | Auto-Delete Recordings and History Limit clean by database rows, not by files | medium | history | fix |
| B-25 | Chinese script conversion is stored as post-processed text | low | language | product call |
| B-26 | The Debug page stays open after debug mode is turned off, and its Unload Model value shows "Select an option…" | low | settings | fix |
| B-27 | Launch on Startup stays on when login-item registration is refused | low | settings | fix |
| B-28 | The tray menu is not rebuilt when a model is downloaded or deleted from the Models page | low | tray | fix |
| B-29 | The legacy Parakeet V3 language picker has no effect | low | language | product call |
| B-30 | Model-list fetch failures on the Post Processing page are invisible | low | post-processing | fix |
| B-31 | The Keyboard Diagnostic's hint starts a real dictation | low | settings | fix |
| B-32 | The permissions banner's button only requests on its first click and never re-checks by itself | low | setup | fix |
| B-33 | Choosing a model from the tray during onboarding completes onboarding but leaves the window on the model step | low | setup | fix |
| B-34 | The copy icon and the tray's last-transcript entry copy different text | low | history, tray | product call |
| B-35 | The History page's "Failed to delete" toast is unreachable and re-transcribe errors are generic | low | history | fix |
| B-36 | A microphone change writes the setting before rebuilding the stream | low | audio | fix |
| B-37 | Fallback shadow shortcuts stay registered while the recorder is open | low | secure input | fix |
| B-38 | Small copy and rendering slips | low | various | fix |
| B-39 | `--start-hidden` hides the window but leaves the Dock icon, unlike the Start Hidden setting | low | command line | fix |

## High

### B-01: Post-processing is silently inert until a prompt is chosen

- **Where the user meets it:** Turning on Post Processing in Settings › Advanced › Experimental, filling in a provider and key, and dictating with the post-processing shortcut.
- **What happens / what was expected:** The raw transcript is pasted, with no toast and no change. A fresh install has no selected prompt, and with none selected the pipeline skips post-processing with a debug-level log line. The user expected the transcript to be rewritten, or a message saying why it was not.
- **Reproduce:** Fresh settings. Enable Experimental Features and Post Processing. Set a provider and a key. Do not touch the Prompts section. Press the post-processing shortcut and say a sentence. The raw transcript is pasted.
- **Why (from the code):** `src-tauri/src/settings.rs:916` sets `post_process_selected_prompt_id: None` by default; `src-tauri/src/actions.rs:149-155` returns the raw text when no prompt is selected, logging at `debug!` only. The Post Processing page shows an unchecked prompt list with no indication that one must be chosen.
- **Severity:** `high`. A feature the user turned on and keyed does nothing, silently.
- **Decision needed:** `fix`. Either select the first built-in prompt by default, or show a warning on the Post Processing page and a toast at the dictation when no prompt is selected.
- **Raised by:** [Post-processing](dictation/post-processing.md#open-questions-and-verification), [The Post Processing page](settings/post-processing-page.md#open-questions-and-verification), [The settings model](foundations/the-settings-model.md#defaults-and-reset)

### B-02: A cancel during transcription leaves the shortcut dead until the abandoned work finishes

- **Where the user meets it:** Pressing the Cancel shortcut (or Escape on the overlay) while "Transcribing..." is showing, then trying to dictate again.
- **What happens / what was expected:** The overlay disappears and the tray goes idle, but the transcribe shortcut does nothing until the abandoned transcription (and any post-processing) finishes in the background. With a long recording, a large model, or a slow LLM that can be many seconds. The user expected cancel to return Handy to idle at once.
- **Reproduce:** Record 30 s with a large Whisper model. Release. While "Transcribing..." shows, press Cancel. Immediately press the transcribe shortcut. Nothing happens until the old transcription finishes.
- **Why (from the code):** `src-tauri/src/transcription_coordinator.rs:195-205` — on Cancel during Processing the coordinator sets the cancel flag and the overlay/tray to idle but deliberately does not reset its stage ("Don't reset during processing — wait for the pipeline to finish"); new inputs are dropped while the stage is Processing. The transcription itself cannot be interrupted, and LLM requests have no timeout (B-05), so the dead period is unbounded.
- **Severity:** `high`. The product looks idle but is not, and the user cannot tell why.
- **Decision needed:** `fix`. Either let a new trigger queue behind the abandoned work, or show the busy state honestly (keep the tray busy icon) until the work finishes, or abort the engine where the backend allows it.
- **Raised by:** [Cancelling](dictation/cancelling.md#open-questions-and-verification), [Transcribing](dictation/transcribing.md#open-questions-and-verification), [Triggers and shortcuts](foundations/triggers-and-shortcuts.md#open-questions-and-verification), [The overlay](dictation/the-overlay.md#open-questions-and-verification)

### B-03: History Limit saves and deletes on every keystroke

- **Where the user meets it:** Typing a new number into Settings › Advanced › History Limit.
- **What happens / what was expected:** Every keystroke saves the field and runs the cleanup. Typing "50" over a "5" first passes through whatever intermediate value the field holds; backspacing to empty or to "0" deletes every unsaved entry immediately. The user expected the value to apply when they finished typing.
- **Reproduce:** With 20 unsaved entries, set History Limit to 5. Click into the field, press Backspace (field becomes empty/0). The History page is now empty except saved entries.
- **Why (from the code):** `src/components/settings/HistoryLimit.tsx:20-26` calls `updateSetting("history_limit", value)` in `onChange`; `src-tauri/src/commands/history.rs:115-125` runs `cleanup_old_entries()` on every update, which deletes rows and recording files. The same shape applies to Auto-Delete Recordings, though that one is a dropdown.
- **Severity:** `high`. Loses data on an ordinary editing gesture.
- **Decision needed:** `fix`. Save on blur or Enter, or debounce, and treat an empty field as "no change". Consider a confirmation when the new limit would delete entries.
- **Raised by:** [Advanced](settings/advanced.md#open-questions-and-verification), [The history page](history/the-history-page.md#open-questions-and-verification), [The settings model](foundations/the-settings-model.md#edge-cases)

### B-04: The shortcut recorder commits Escape and captures keys typed in other applications

- **Where the user meets it:** Recording a new shortcut on Settings › General.
- **What happens / what was expected:** (a) Pressing Escape to back out of the recorder commits "Escape" as the shortcut. (b) While the recorder is open it listens system-wide: switching to another application and typing commits whatever was typed there, and every Handy shortcut is suspended meanwhile. The user expected Escape to cancel and the recorder to only hear keys typed into Handy's window.
- **Reproduce:** (a) Click the transcribe shortcut chip; press Escape; the chip reads "Escape". (b) Click the chip; Cmd+Tab to another app; type "a"; switch back — the chip reads "A" (or the last key seen).
- **Why (from the code):** `src/components/settings/HandyKeysShortcutInput.tsx:94-160` commits whatever key string the backend last reported, with no Escape special case; `src-tauri/src/shortcut/handy_keys.rs:263-300, 546-548` starts a global key listener for the recording and calls `suspend_all_shortcuts` for its duration. The pilot document describes both as they are.
- **Severity:** `high`. One common gesture (Escape) does the opposite of what it means everywhere else, and the global listener makes the product misbehave while the user is elsewhere.
- **Decision needed:** `fix`. Treat Escape as cancel; stop recording when the settings window loses focus (and restore shortcuts).
- **Raised by:** [Shortcut recorder](settings/shortcut-recorder.md#open-questions-and-verification), [General](settings/general.md#open-questions-and-verification), [Triggers and shortcuts](foundations/triggers-and-shortcuts.md#open-questions-and-verification)

### B-05: An LLM request has no timeout, so one slow provider hangs the dictation

- **Where the user meets it:** Dictating with post-processing against a provider that is slow, overloaded, or unreachable through a half-open connection.
- **What happens / what was expected:** The overlay says "Transcribing..." indefinitely; the paste never comes. Cancel clears the overlay but the shortcut stays dead (B-02) until the request finally returns or the socket dies. The user expected the request to give up after some seconds and paste the raw transcript.
- **Reproduce:** Point a custom provider at a server that accepts the connection and never responds (for example `nc -l 8080`). Dictate with the post-processing shortcut. The overlay never leaves "Transcribing...".
- **Why (from the code):** `src-tauri/src/llm_client.rs:175-181` builds the `reqwest::Client` without `.timeout()`; nothing upstream bounds the await. The Apple Intelligence path has its own bound; the HTTP path has none.
- **Severity:** `high`. One dictation hangs the product until a network-level failure.
- **Decision needed:** `fix`. A request timeout (30–60 s) that falls back to the raw transcript with a toast.
- **Raised by:** [Post-processing](dictation/post-processing.md#open-questions-and-verification), [The Post Processing page](settings/post-processing-page.md#open-questions-and-verification), [Cancelling](dictation/cancelling.md#open-questions-and-verification)

### B-06: `--debug` does not reveal the Debug section the README says it enables

- **Where the user meets it:** Launching `Handy --debug` to get the Debug settings section, as the README's CLI table suggests ("Enable debug mode").
- **What happens / what was expected:** Logging goes to Trace and log lines stream to the window, but the sidebar keeps reading the saved `debug_mode`, so the Debug section does not appear. The user expected the flag to do what Cmd+Shift+D does for one run.
- **Reproduce:** Fresh settings. `open -a Handy --args --debug`. Open Settings; no Debug section.
- **Why (from the code):** `src-tauri/src/lib.rs:920-930` raises the log level from `cli_args.debug` and never touches the settings' `debug_mode`; `src/components/Sidebar.tsx:69` gates the section on `settings.debug_mode`. The README and the CLI help text both describe the flag as enabling debug mode.
- **Severity:** `high`. A documented flag does something different from what is documented, and the difference is invisible.
- **Decision needed:** `fix`. Either expose the runtime flag to the frontend so the section shows without saving, or correct the README and help text to "verbose logging".
- **Raised by:** [Command line](integration/command-line.md#open-questions-and-verification), [Debug](settings/debug.md#open-questions-and-verification), [First launch](setup/first-launch.md#edge-cases), [The settings model](foundations/the-settings-model.md#debug-mode-and-hidden-controls)

### B-07: A dictation cancelled during transcription leaves an orphaned recording that cleanup never removes

- **Where the user meets it:** Cancelling while "Transcribing..." shows, then looking at the recordings folder or at disk use over months.
- **What happens / what was expected:** The WAV was written before transcription started; the cancel returns before a history row is created, so the file has no row. Retention cleanup walks rows, not files, so the orphan stays forever. The user expected a cancelled dictation to leave nothing behind (the cancel document says "no recording file", which is only true for a cancel while recording).
- **Reproduce:** Record 20 s, release, press Cancel during "Transcribing...". Open the recordings folder: a new `handy-*.wav` exists; the History page has no entry for it.
- **Why (from the code):** `src-tauri/src/actions.rs:705-750` writes the WAV before transcription; `actions.rs:755-760` and `789-794` return on the cancel flag before `save_entry`. `src-tauri/src/managers/history.rs:330-440` selects files to delete from the table only.
- **Severity:** `high` for the document's promise (cancel leaves a file) though the disk cost is slow; graded high because it is data the user believed discarded.
- **Decision needed:** `fix`. Delete the WAV on the cancel path, and have retention cleanup also remove files in the folder with no row.
- **Raised by:** [Cancelling](dictation/cancelling.md#open-questions-and-verification), [Data on disk](cross-cutting/data-on-disk.md#open-questions-and-verification), [The history page](history/the-history-page.md#open-questions-and-verification)

## Medium

### B-08: Speech the model hears as nothing becomes a "Transcription failed" history entry

- **Where the user meets it:** A short "um", a cough, or a noisy room with VAD keeping some frames: the overlay flashes and nothing is pasted, and the History page shows an entry labelled "Transcription failed".
- **What happens / what was expected:** A non-empty capture that transcribes to an empty string is saved as an entry with empty text; the History page renders empty text as a failure. Nothing failed. The user expected either no entry or an honest "no speech detected".
- **Reproduce:** Tap the shortcut and cough once. Open History.
- **Why (from the code):** `src-tauri/src/actions.rs:796-807` saves the entry whatever the text; `src/components/settings/history/HistorySettings.tsx:441` shows `transcriptionFailed` whenever the text is empty, with no separate state for "empty result".
- **Severity:** `medium`. Misleading, recoverable.
- **Decision needed:** `product call`. Skip the entry (loses the recording for re-transcribe) or label it "No speech" (keeps it).
- **Raised by:** [Transcribing](dictation/transcribing.md#open-questions-and-verification), [The history page](history/the-history-page.md#open-questions-and-verification)

### B-09: Retention cleanup does not refresh an open History page

- **Where the user meets it:** Leaving the History page open while dictating past the History Limit.
- **What happens / what was expected:** Entries deleted by the limit stay on the page until the next manual refresh or navigation; clicking play or delete on one fails. The user expected the list to drop them.
- **Reproduce:** History Limit 5, History page open with 5 unsaved entries. Dictate once. The oldest entry stays listed; click its play button — nothing plays.
- **Why (from the code):** `src-tauri/src/managers/history.rs:330-440` (`cleanup_old_entries`) deletes rows and files without emitting `history-updated`; the page listens only for that event (`HistorySettings.tsx`).
- **Severity:** `medium`. Stale UI with dead controls.
- **Decision needed:** `fix`. Emit `history-updated` after cleanup.
- **Raised by:** [The history page](history/the-history-page.md#open-questions-and-verification), [Advanced](settings/advanced.md#open-questions-and-verification)

### B-10: Post-processing and update failures are reported only to the log

- **Where the user meets it:** A wrong API key, a provider that returns 401 or 500, a model name that does not exist; or Check for Updates failing.
- **What happens / what was expected:** Post-processing falls back to the raw transcript with no toast; the user cannot tell whether the feature ran. The update checker logs to the console and shows nothing. The user expected a message.
- **Reproduce:** Set an invalid key. Dictate with post-processing. The raw text is pasted; no toast. Disconnect the network; click Check for Updates; nothing.
- **Why (from the code):** `src-tauri/src/actions.rs:330-345` logs `error!` and returns `None`; `src/components/update-checker/UpdateChecker.tsx:100-104, 155-158` `console.error` only. Only the "Test Connection" button on the Post Processing page surfaces provider errors.
- **Severity:** `medium`. The fallback is deliberate (the code says so) but invisible.
- **Decision needed:** `product call`. Silent fallback keeps dictation flowing; a toast tells the user their key is wrong. A toast once per failure mode, or a badge on the Post Processing page, would serve both.
- **Raised by:** [Post-processing](dictation/post-processing.md#open-questions-and-verification), [The Post Processing page](settings/post-processing-page.md#open-questions-and-verification), [Updates](integration/updates.md#open-questions-and-verification), [About](settings/about.md#open-questions-and-verification)

### B-11: `--toggle-post-process` and the tray run post-processing even when the feature is off

- **Where the user meets it:** A script or window-manager binding that sends `--toggle-post-process` (or SIGUSR2) while Post Processing is disabled in Settings.
- **What happens / what was expected:** The dictation runs through the post-processing pipeline anyway (and, with no prompt, silently does nothing — B-01). The in-app shortcut for the same action is only registered when the feature is on, so the two channels disagree.
- **Reproduce:** Post Processing off, a prompt selected, a valid provider. `Handy --toggle-post-process`, speak, again. The text is post-processed.
- **Why (from the code):** `src-tauri/src/signal_handle.rs:18-40` sends the binding id straight to the coordinator; the coordinator's action map accepts `transcribe_with_post_process` without consulting `post_process_enabled`. Only shortcut registration (`src-tauri/src/shortcut/mod.rs`) is gated.
- **Severity:** `medium`. An off switch that one entry point ignores.
- **Decision needed:** `fix`. Check the setting in `send_transcription_input` and fall back to plain transcription.
- **Raised by:** [Command line](integration/command-line.md#open-questions-and-verification), [Post-processing](dictation/post-processing.md#open-questions-and-verification)

### B-12: Installing an update relaunches Handy in the middle of a dictation

- **Where the user meets it:** Clicking "Install" on the update banner (or a script doing so) while a recording or transcription is in progress.
- **What happens / what was expected:** The app relaunches at once; the capture and any pending paste are lost with no warning.
- **Reproduce:** Needs a pending update. Start recording, click Install.
- **Why (from the code):** `src/components/update-checker/UpdateChecker.tsx:140-156` downloads, installs, and calls `relaunch()` without asking the coordinator whether it is idle.
- **Severity:** `medium`. Uncommon path; loses work when hit.
- **Decision needed:** `fix`. Disable Install while busy, or defer the relaunch until idle.
- **Raised by:** [Updates](integration/updates.md#open-questions-and-verification)

### B-13: Cancelling a download during Verifying or Extracting does not stop it

- **Where the user meets it:** Clicking Cancel on a model card after the transfer has finished and the card reads "Verifying..." or "Extracting...".
- **What happens / what was expected:** The progress UI clears, but verification/extraction completes in the background, the model appears as downloaded, and, if it was the onboarding choice or the store auto-selects, becomes active. The user expected nothing to land.
- **Reproduce:** Download a directory model (for example a Parakeet legacy). Click Cancel when "Extracting..." shows. Wait; the model appears under Downloaded Models.
- **Why (from the code):** `src-tauri/src/managers/model.rs:2542-2560` (`cancel_download`) only triggers the transfer's cancellation token; the verify/extract stage at `model.rs:2230-2300` does not check it and emits completion normally.
- **Severity:** `medium`. Harmless result, wrong feedback.
- **Decision needed:** `fix`. Either honour the token after the transfer or hide Cancel once verification starts.
- **Raised by:** [Downloading a model](models/downloading-a-model.md#open-questions-and-verification), [Choosing a model](setup/choosing-a-model.md#open-questions-and-verification)

### B-14: Secure Input engaging while a push-to-talk key is held loses the release

- **Where the user meets it:** Holding the transcribe key while a password field (or Terminal's Secure Keyboard Entry) takes focus.
- **What happens / what was expected:** The handy_keys listener stops receiving events; the fallback shortcuts register, but the release of the already-held key never arrives, so the recording runs until the user presses and releases the key again. The user expected the release to stop it.
- **Reproduce:** Hold the transcribe key, click into a password field in Safari, release. Recording continues.
- **Why (from the code):** `src-tauri/src/secure_input.rs:380-430` swaps listeners on the status change; the handler in `src-tauri/src/shortcut/handler.rs` has no synthetic release for keys held at the swap.
- **Severity:** `medium`. Uncommon; recoverable by pressing again.
- **Decision needed:** `fix`. Send a release for every held binding when the listener is swapped.
- **Raised by:** [Secure input](cross-cutting/secure-input.md#open-questions-and-verification), [Triggers and shortcuts](foundations/triggers-and-shortcuts.md#open-questions-and-verification)

### B-15: A model with a deleted file stays selected and only fails at the next dictation

- **Where the user meets it:** Deleting a model's file from the Hugging Face cache or models folder by hand (or with another tool), then dictating.
- **What happens / what was expected:** The catalog entry remains, so the selection is kept at launch and Rescan; the footer shows the name with a grey dot, and the next dictation fails with "Failed to load model". The user expected Handy to notice the file is gone.
- **Reproduce:** With Whisper Small active, delete its `.gguf` from `~/.cache/huggingface/hub`. Click Rescan. The footer still names it. Dictate: load error.
- **Why (from the code):** `src-tauri/src/managers/model.rs:1495-1520` clears the selection only when the id is absent from the model list; catalog ids are always present regardless of `is_downloaded`.
- **Severity:** `medium`. Uncommon path, clear error when it happens.
- **Decision needed:** `fix`. Check `is_downloaded` as well as presence.
- **Raised by:** [Models](foundations/models.md#the-models-states), [The Models page](models/the-models-page.md#open-questions-and-verification)

### B-16: Deleting a catalog model removes the whole Hugging Face repo folder

- **Where the user meets it:** Deleting a model from the Models page when other tools share the Hugging Face cache.
- **What happens / what was expected:** The entire `models--<org>--<repo>` folder is removed, including files Handy never downloaded (other quantizations, config files used by another application). The user expected only Handy's file to go.
- **Reproduce:** Have another tool download a second quant of the same repo. Delete the model in Handy. The other quant is gone.
- **Why (from the code):** `src-tauri/src/managers/model.rs:2290-2300` — `remove_dir_all` on the repo directory for catalog models; alternate quants inside Handy's own list delete by file.
- **Severity:** `medium`. Data loss, but in a shared cache the user chose to share.
- **Decision needed:** `product call`. Per-file deletion leaves cache metadata behind; per-repo deletion can remove other tools' files. A confirmation naming the folder would make either acceptable.
- **Raised by:** [The Models page](models/the-models-page.md#open-questions-and-verification), [Data on disk](cross-cutting/data-on-disk.md#open-questions-and-verification)

### B-17: A recordings folder or database deleted while Handy runs is not recreated until relaunch

- **Where the user meets it:** Clearing the app data folder by hand while Handy is running, then dictating.
- **What happens / what was expected:** Every dictation logs "Failed to save WAV file" and the history row is written without a file (or fails if the database is gone); the text still pastes. The user expected Handy to recreate what it needs.
- **Reproduce:** Delete `recordings/` while Handy runs; dictate; open History — no play button, log shows the error.
- **Why (from the code):** `src-tauri/src/managers/history.rs:80-90` creates the folder once at manager construction; `src-tauri/src/actions.rs:740-748` logs the write failure and continues.
- **Severity:** `medium`. Self-inflicted but silent.
- **Decision needed:** `fix`. `create_dir_all` before each write.
- **Raised by:** [Data on disk](cross-cutting/data-on-disk.md#open-questions-and-verification)

### B-18: The system going to sleep mid-recording leaves a dead stream that still counts as recording

- **Where the user meets it:** Closing the lid while holding or after toggling the shortcut.
- **What happens / what was expected:** On wake the overlay is still in the recording state and the microphone indicator may be off; the stream delivers nothing. The stop hands on whatever was captured before sleep. The user expected the recording to end on sleep or resume on wake.
- **Reproduce:** Toggle recording on, sleep the Mac, wake, speak, toggle off. Only the pre-sleep speech transcribes.
- **Why (from the code):** `src-tauri/src/audio_toolkit/audio/recorder.rs:180-200` sets `stream_error` on a device error but nothing watches it during a recording; the coordinator has no sleep/wake hook.
- **Severity:** `medium`. Uncommon, confusing.
- **Decision needed:** `fix`. Stop or cancel the dictation on a stream error, with a toast.
- **Raised by:** [Audio capture](foundations/audio-capture.md#open-questions-and-verification), [Starting and recording](dictation/starting-and-recording.md#open-questions-and-verification)

### B-19: Two dictations finishing in the same second share one recording filename

- **Where the user meets it:** Very quick successive taps, or a re-transcribe racing a new dictation.
- **What happens / what was expected:** The second WAV overwrites the first; two history rows point at one file, and deleting either removes the other's recording.
- **Reproduce:** Hard by hand; two stops within one wall-clock second.
- **Why (from the code):** `src-tauri/src/actions.rs:707` names files `handy-{unix seconds}.wav`.
- **Severity:** `medium`. Rare, loses a recording when it happens.
- **Decision needed:** `fix`. Millisecond timestamp or a uniqueness check.
- **Raised by:** [Data on disk](cross-cutting/data-on-disk.md#open-questions-and-verification), [The history page](history/the-history-page.md#edge-cases)

### B-20: The permissions step can dead-end: a denied microphone waits forever and three failed polls stop polling

- **Where the user meets it:** Onboarding on a Mac where the user clicked "Don't Allow" on the microphone prompt, or where the accessibility check throws.
- **What happens / what was expected:** The microphone row stays "Waiting…" with no path forward other than a trip to System Settings the page does not mention; after three polling errors the page stops checking and never advances even when permission is later granted. The user expected guidance and a retry.
- **Reproduce:** Deny the microphone at the prompt. The row never changes.
- **Why (from the code):** `src/components/onboarding/AccessibilityOnboarding.tsx:47, 220-230` stop after `MAX_POLLING_ERRORS = 3`; `:260-270` requests once and has no denied state.
- **Severity:** `medium`. First-run path; recoverable by relaunch.
- **Decision needed:** `fix`. A "denied — open System Settings" state and a retry button.
- **Raised by:** [Permissions](setup/permissions.md#open-questions-and-verification), [First launch](setup/first-launch.md#open-questions-and-verification)

### B-21: Toasts are the only error channel and they only render inside the settings window

- **Where the user meets it:** Dictating with the window hidden (the normal case) when the microphone is denied, no model is loaded, or transcription fails.
- **What happens / what was expected:** The overlay flashes and nothing is pasted; the explanation is a toast that was rendered into a hidden window and has expired by the time the window is opened. The user expected to be told.
- **Reproduce:** Close the window. Revoke microphone access. Dictate.
- **Why (from the code):** `src/App.tsx:103-120` listens for `recording-error` and calls `toast`; there is no native notification path and no overlay error state.
- **Severity:** `medium`. Affects every failure mode; each is individually uncommon.
- **Decision needed:** `product call`. A system notification, an overlay error flash with text, or bringing the window forward on error each have a cost.
- **Raised by:** [Audio capture](foundations/audio-capture.md#open-questions-and-verification), [Starting and recording](dictation/starting-and-recording.md#open-questions-and-verification), [Transcribing](dictation/transcribing.md#open-questions-and-verification), [Windows and tray](foundations/windows-and-tray.md#open-questions-and-verification)

### B-22: Turning Experimental Features off hides the Post Processing toggle without disabling post-processing

- **Where the user meets it:** Turning Experimental Features off after enabling Post Processing.
- **What happens / what was expected:** The sidebar's Post Processing section and the post-processing shortcut stay active; the toggle that controls them is no longer visible. The user expected either the feature to switch off with its parent or the toggle to stay reachable.
- **Reproduce:** Enable both; turn Experimental off; the sidebar still shows Post Processing.
- **Why (from the code):** `src/components/settings/advanced/AdvancedSettings.tsx:64-80` renders the toggle only when `experimental_enabled`; nothing writes `post_process_enabled = false`.
- **Severity:** `medium`. Inconsistency between two controls that should nest.
- **Decision needed:** `product call`. Cascade off, or keep the child visible.
- **Raised by:** [Advanced](settings/advanced.md#open-questions-and-verification), [The settings window](settings/the-settings-window.md#open-questions-and-verification)

### B-23: Re-selecting the active model from the footer reloads it

- **Where the user meets it:** Opening the footer dropdown and clicking the model that is already active.
- **What happens / what was expected:** The model is unloaded and loaded again (the footer dot goes yellow); a dictation started meanwhile waits. Selecting the same thing was expected to be a no-op, as it is from the tray.
- **Reproduce:** Footer dropdown → click the active model. The dot pulses yellow for a second.
- **Why (from the code):** `src/components/model-selector/ModelSelector.tsx:143-160` calls the switch command without comparing to the current id; `src-tauri/src/commands/models.rs:74` has the early return, but only for the tray path — the footer path reaches `load_model` unconditionally.
- **Severity:** `medium`. Harmless except for the delay.
- **Decision needed:** `fix`. Compare ids before switching.
- **Raised by:** [Switching models](models/switching-models.md#open-questions-and-verification), [Models](foundations/models.md#open-questions-and-verification)

### B-24: Auto-Delete Recordings and History Limit clean by database rows, not by files

- **Where the user meets it:** Disk use after months, or after B-07 orphans accumulate; and the Data on disk claim that recordings follow the settings.
- **What happens / what was expected:** Files with no row (orphans, files from a restored backup, files left after a database reset) are never deleted by either setting. The user expected "Keep latest 5" to describe the folder.
- **Reproduce:** Copy a WAV into `recordings/`; change Auto-Delete to "Keep latest 1"; the file stays.
- **Why (from the code):** `src-tauri/src/managers/history.rs:380-440` selects `file_name` from rows only.
- **Severity:** `medium`. Same family as B-07; kept separate because the fix is a folder sweep rather than a cancel-path fix.
- **Decision needed:** `fix`. Sweep the folder for files with no row after each cleanup.
- **Raised by:** [Data on disk](cross-cutting/data-on-disk.md#open-questions-and-verification), [Advanced](settings/advanced.md#open-questions-and-verification)

## Low

### B-25: Chinese script conversion is stored as post-processed text

- **Where the user meets it:** Dictating with a Chinese model and a Traditional/Simplified conversion set; the History page shows two versions, the tray copies the converted one, the copy icon the raw one.
- **What happens / what was expected:** The converted text lands in the `post_processed_text` column, so the UI treats a script conversion like an LLM rewrite. The user probably expected one transcript.
- **Reproduce:** Chinese model, conversion on, dictate; open History.
- **Why (from the code):** `src-tauri/src/actions.rs:457-459` stores `final_text` as post-processed whenever it differs from the raw transcription.
- **Severity:** `low`. Cosmetic; the texts are both correct.
- **Decision needed:** `product call`. Store the converted text as the transcript, or keep both and label the second "Converted".
- **Raised by:** [Language and translation](cross-cutting/language-and-translation.md#open-questions-and-verification), [The history page](history/the-history-page.md#open-questions-and-verification)

### B-26: The Debug page stays open after debug mode is turned off, and its Unload Model value shows "Select an option…"

- **Where the user meets it:** Pressing Cmd+Shift+D while on the Debug page; or choosing "After 15 seconds (Debug)" and then leaving debug mode.
- **What happens / what was expected:** The sidebar entry disappears but the page remains until navigation. The Unload Model dropdown on Advanced shows "Select an option..." because the saved 15 s value is not in the non-debug list; the timeout still applies. Expected: navigate away; show the value.
- **Reproduce:** As above.
- **Why (from the code):** `src/App.tsx:76-95` toggles the setting without changing the current section; `src/components/settings/ModelUnloadTimeout.tsx:51-75` swaps option lists by `debug_mode`.
- **Severity:** `low`.
- **Decision needed:** `fix`. Navigate to General on toggle-off; always include the saved value in the option list.
- **Raised by:** [Debug](settings/debug.md#open-questions-and-verification), [Advanced](settings/advanced.md#open-questions-and-verification)

### B-27: Launch on Startup stays on when login-item registration is refused

- **Where the user meets it:** Turning on Launch on Startup on a managed Mac where login items are blocked.
- **What happens / what was expected:** The toggle shows on; the log has a warning; Handy does not launch at login. Expected: the toggle snaps back with a message.
- **Reproduce:** Needs a managed profile blocking login items.
- **Why (from the code):** `src-tauri/src/autostart.rs:70-90` logs `warn!` and returns Ok.
- **Severity:** `low`. Rare.
- **Decision needed:** `fix`. Propagate the error so the toggle reverts.
- **Raised by:** [General](settings/general.md#open-questions-and-verification)

### B-28: The tray menu is not rebuilt when a model is downloaded or deleted from the Models page

- **Where the user meets it:** Downloading a second model, then opening the tray's model submenu.
- **What happens / what was expected:** The submenu lists the models as of the last rebuild (launch, selection change, language change); the new model is missing until one of those happens. Expected: the list to be current.
- **Reproduce:** Download a model; open the tray submenu.
- **Why (from the code):** `src-tauri/src/lib.rs:300-330` rebuilds on `model-state-changed` (selection/load) and settings events; download-complete and delete emit other events that are not listened for.
- **Severity:** `low`.
- **Decision needed:** `fix`. Rebuild on download-complete and delete.
- **Raised by:** [The tray menu](tray/the-tray-menu.md#open-questions-and-verification), [Downloading a model](models/downloading-a-model.md#open-questions-and-verification)

### B-29: The legacy Parakeet V3 language picker has no effect

- **Where the user meets it:** Choosing a language on the General page with the legacy ONNX Parakeet V3 active (the catalog GGUF Parakeet V3 is believed to honor the hint; not tested).
- **What happens / what was expected:** The picker is shown because the model advertises 25 languages, but the engine auto-detects regardless; the choice is saved and ignored.
- **Reproduce:** Parakeet V3 active; choose German; dictate in English — English transcript.
- **Why (from the code):** The Parakeet path in `src-tauri/src/managers/transcription.rs` does not pass the language to the engine; only Whisper-family and some ONNX engines do.
- **Severity:** `low`.
- **Decision needed:** `product call`. Hide the picker for models that cannot take a language, or mark it "auto-detect only".
- **Raised by:** [Language and translation](cross-cutting/language-and-translation.md#open-questions-and-verification)

### B-30: Model-list fetch failures on the Post Processing page are invisible

- **Where the user meets it:** Opening the model dropdown on the Post Processing page with a bad key or no network.
- **What happens / what was expected:** The dropdown is empty or stale; the error goes to the console. Expected: a message in the dropdown.
- **Reproduce:** Invalid key; open the Post Processing page.
- **Why (from the code):** `src/stores/settingsStore.ts:555-570` `console.error` only.
- **Severity:** `low`.
- **Decision needed:** `fix`. Show the error inline.
- **Raised by:** [The Post Processing page](settings/post-processing-page.md#open-questions-and-verification)

### B-31: The Keyboard Diagnostic's hint starts a real dictation

- **Where the user meets it:** Running the Keyboard Diagnostic on the Debug page, which asks the user to press the transcribe shortcut.
- **What happens / what was expected:** The shortcut is live during the diagnostic, so pressing it starts a recording (overlay, chime) while the diagnostic counts events. Expected: the diagnostic to swallow the keys.
- **Reproduce:** Debug › Keyboard Diagnostic › Start; press Option+Space.
- **Why (from the code):** `src-tauri/src/secure_input.rs:69-100` counts events from a parallel listener and does not suspend shortcuts.
- **Severity:** `low`. Debug-only.
- **Decision needed:** `fix`. Suspend shortcuts for the diagnostic's duration.
- **Raised by:** [Debug](settings/debug.md#open-questions-and-verification)

### B-32: The permissions banner's button only requests on its first click and never re-checks by itself

- **Where the user meets it:** The accessibility banner at the top of Settings after permission was lost (a rebuilt app, a reset TCC database).
- **What happens / what was expected:** The first click requests permission; the banner moves to a "verify" state where the same button only re-checks. Granting permission in System Settings is not noticed until the button is clicked again. Expected: the banner to clear on its own.
- **Reproduce:** Revoke accessibility; open Settings; grant it in System Settings; the banner stays until clicked.
- **Why (from the code):** `src/components/AccessibilityPermissions.tsx:25-50` — three states, no polling, one request.
- **Severity:** `low`.
- **Decision needed:** `fix`. Poll on window focus as onboarding does.
- **Raised by:** [Permissions](setup/permissions.md#open-questions-and-verification), [The settings window](settings/the-settings-window.md#open-questions-and-verification)

### B-33: Choosing a model from the tray during onboarding completes onboarding but leaves the window on the model step

- **Where the user meets it:** During the model step, if a model is already on disk, picking it from the tray's model submenu.
- **What happens / what was expected:** `onboarding_completed` is written, but the window keeps showing onboarding until a setting change refreshes it. Expected: the window to move on.
- **Reproduce:** Reset settings with a model on disk; at the model step, select from the tray.
- **Why (from the code):** `src-tauri/src/commands/models.rs:120-128` sets the flag; the frontend reads it from its cached settings and is not told to refresh.
- **Severity:** `low`. Unusual path.
- **Decision needed:** `fix`. Emit a settings-changed event after the write.
- **Raised by:** [Choosing a model](setup/choosing-a-model.md#open-questions-and-verification), [The tray menu](tray/the-tray-menu.md#open-questions-and-verification)

### B-34: The copy icon and the tray's last-transcript entry copy different text

- **Where the user meets it:** With post-processing on, copying from the History page versus the tray.
- **What happens / what was expected:** The History page's copy icon copies the raw transcript; the tray's "Copy last transcription" copies the post-processed text when there is one. Expected: the same text.
- **Reproduce:** Post-process a dictation; compare the two copies.
- **Why (from the code):** `src-tauri/src/tray.rs:580-595` prefers `post_processed_text`; the History entry's copy button uses the raw field.
- **Severity:** `low`.
- **Decision needed:** `product call`. Either is defensible; pick one and offer the other explicitly.
- **Raised by:** [The history page](history/the-history-page.md#open-questions-and-verification), [The tray menu](tray/the-tray-menu.md#open-questions-and-verification)

### B-35: The History page's "Failed to delete" toast is unreachable and re-transcribe errors are generic

- **Where the user meets it:** Deleting an entry whose file is already gone (works fine, the toast never fires) or re-transcribing one (a failure reads "Failed to re-transcribe" whatever the cause, including "Recording contains no speech" which is not a failure).
- **What happens / what was expected:** The delete path ignores a missing file, so the error toast is dead code; re-transcribe collapses every cause to one message.
- **Reproduce:** Re-transcribe a silent recording.
- **Why (from the code):** `src-tauri/src/commands/history.rs:85-95` returns "Recording contains no speech" as an error; `HistorySettings.tsx` shows one message for all errors.
- **Severity:** `low`.
- **Decision needed:** `fix`. Pass the backend message through; treat "no speech" as a result, not an error.
- **Raised by:** [The history page](history/the-history-page.md#open-questions-and-verification)

### B-36: A microphone change writes the setting before rebuilding the stream

- **Where the user meets it:** Choosing a microphone that then fails to open (a Bluetooth device that just disconnected).
- **What happens / what was expected:** The setting is saved, the rebuild fails, the dropdown snaps back to the old value, but the file now names the failed device; at the next launch Handy falls back to Default and rewrites it again. Expected: a refused change to leave the file alone.
- **Reproduce:** Hard by hand; needs a device that enumerates but will not open.
- **Why (from the code):** `src-tauri/src/commands/audio.rs:215-240` writes `selected_microphone` and then rebuilds.
- **Severity:** `low`. Self-healing at launch.
- **Decision needed:** `fix`. Rebuild first, then write.
- **Raised by:** [General](settings/general.md#open-questions-and-verification), [Audio capture](foundations/audio-capture.md#open-questions-and-verification)

### B-37: Fallback shadow shortcuts stay registered while the recorder is open

- **Where the user meets it:** Recording a new shortcut while Secure Input is active (the fallback listener is in use).
- **What happens / what was expected:** handy_keys shortcuts are suspended for the recording, but the fallback registrations are not, so pressing the current shortcut while recording both records the keys and starts a dictation.
- **Reproduce:** Terminal with Secure Keyboard Entry on; open the recorder; press the current transcribe shortcut.
- **Why (from the code):** `src-tauri/src/shortcut/handy_keys.rs:546` suspends its own listener; `src-tauri/src/secure_input.rs:397-430` keeps the fallback bindings.
- **Severity:** `low`. Rare combination.
- **Decision needed:** `fix`. Suspend both.
- **Raised by:** [Secure input](cross-cutting/secure-input.md#open-questions-and-verification), [Shortcut recorder](settings/shortcut-recorder.md#open-questions-and-verification)

### B-38: Small copy and rendering slips

- **Where the user meets it:** Scattered across the settings window and the README.
- **What happens / what was expected:** Each item below is a single wrong or inconsistent string or a harmless leftover.
  - The Apple Intelligence minimum-OS message disagrees with itself: the backend says "macOS 15 or later" (`src-tauri/src/shortcut/mod.rs:1197`), the UI says "macOS Tahoe (26)" (`src/i18n/locales/en/translation.json:396`).
  - Two catalog entries are named "Cohere Transcribe" (`src-tauri/src/catalog/catalog.json:111, 474`), so the Models page shows two cards with one name.
  - The README's Linux data path (`~/.config/com.pais.handy/`, `README.md:342`) and the `WEBKIT_DISABLE_DMABUF_RENDERER` advice (`README.md:176, 468`) describe a variable the app already sets itself (`src-tauri/src/main.rs:14`).
  - The footer's "Downloading…" status strings exist in several forms (`src/components/model-selector/ModelSelector.tsx:195-200`); one ("downloadingMultiple") is not consistently pluralised across locales.
  - `DebugPaths.tsx` is an unused component (`src/components/settings/debug/DebugPaths.tsx`).
  - The What's New dialog's title shows the version of the note it found rather than the running version when the two differ.
  - The "Show all N" count on the Post Processing prompts list stays visible when N equals the shown count.
  - The `tray.rs` test fixture hard-codes `update_checks_enabled: true` (`src-tauri/src/tray.rs:684`) so the "Check for Updates" item is never tested hidden.
- **Severity:** `low`.
- **Decision needed:** `fix`. Each is a one-line change.
- **Raised by:** [About](settings/about.md#open-questions-and-verification), [The Models page](models/the-models-page.md#open-questions-and-verification), [Platform differences](cross-cutting/platform-differences.md#open-questions-and-verification), [Switching models](models/switching-models.md#open-questions-and-verification), [Debug](settings/debug.md#open-questions-and-verification), [First launch](setup/first-launch.md#open-questions-and-verification), [The Post Processing page](settings/post-processing-page.md#open-questions-and-verification), [The tray menu](tray/the-tray-menu.md#open-questions-and-verification)

### B-39: `--start-hidden` hides the window but leaves the Dock icon, unlike the Start Hidden setting

- **Where the user meets it:** Launching `Handy --start-hidden` from a login script or window-manager config on macOS.
- **What happens / what was expected:** The window stays hidden and the tray icon appears, but a Dock icon also appears, as if the window were open. The Start Hidden setting, which the README presents as the flag's equivalent, starts Handy as an accessory with no Dock icon.
- **Reproduce:** Start Hidden off. `open -a Handy --args --start-hidden`. A Dock icon shows.
- **Why (from the code):** `src-tauri/src/lib.rs:196-204` switches to `ActivationPolicy::Accessory` only when `settings.start_hidden && settings.show_tray_icon`; the CLI flag is checked separately when deciding whether to show the window and never reaches this branch.
- **Severity:** `low`. Cosmetic, but it is the visible difference between two things documented as the same.
- **Decision needed:** `fix`. Include `cli_args.start_hidden` in the condition.
- **Raised by:** [Command line](integration/command-line.md#open-questions-and-verification), [Windows and tray](foundations/windows-and-tray.md#open-questions-and-verification)

Verified against Handy commit `af48dd6`.
