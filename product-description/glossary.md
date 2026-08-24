# Glossary

The vocabulary used across these documents. When a document uses one of these words, it means exactly this.

## The app and its windows

**Handy.** The desktop app as a whole: a background process with a menu-bar icon, a settings window, and a floating overlay. It has no document model; its only product is text pasted into some other application.

**Settings window.** Handy's one ordinary window, titled "Handy", 680×570 points at minimum, with a sidebar of sections on the left and a footer along the bottom. Opened from the tray's "Settings…" item, by relaunching Handy, or at startup unless *start hidden* is on. Closing it hides it rather than quitting; on macOS it also removes Handy from the Dock while the tray icon is shown.

**Section.** One page of the settings window, chosen from the sidebar: General, History, Models, Advanced, Post Process (only when *post-processing* is enabled), Debug (only in *debug mode*), About.

**Footer.** The strip along the bottom of the settings window holding the *model selector* on the left and the update status and version on the right.

**Overlay.** The small floating window Handy shows during a *dictation*. It is never focusable, appears above every other window and on every Space, and is centered at the bottom (or top) of the screen the mouse pointer is on. Its three forms are the *pill* (Minimal style, and the transcribing and processing states), the *panel* (Live style while a *streaming model* is recording), and none (style None). See *overlay style*.

**Pill.** The compact overlay form: a 40-point-high rounded bar holding a dot, a nine-bar waveform, and a ✕ button while recording, or a spinner, a label, and a ✕ button while working.

**Panel.** The larger overlay form used by the Live style with a *streaming model*: the pill grows into a card of up to ~394×118 points that shows live text above the waveform, an elapsed-time counter, and the ✕ button.

**Tray icon.** Handy's icon in the macOS menu bar (the notification area on Windows, the system tray on Linux). It has three states — idle, recording, transcribing — and, on macOS, an idle-with-warning variant for *Secure Input*. Clicking it opens the *tray menu* (on Windows, a left click opens the settings window and a right click opens the menu).

**Tray menu.** The menu under the tray icon: version, (a Secure Input warning line when relevant), Copy Last Transcript, the *model submenu* and Unload Model when idle or a Cancel item when busy, Settings…, Check for Updates…, Quit.

**Toast.** A transient message in the bottom corner of the settings window used for errors (microphone denied, paste failed, transcription failed, model load failed, download failed). Toasts appear only inside the settings window; if it is hidden the user does not see them.

**Start hidden.** The setting (and `--start-hidden` flag) that launches Handy without showing the settings window. Ignored when the tray icon is disabled, because the window would otherwise be unreachable.

**Debug mode.** A hidden mode toggled with Cmd+Shift+D (Ctrl+Shift+D on Windows and Linux) anywhere in the settings window. It adds the Debug section, a "15 seconds" model-unload option, a quantization label on model cards, and streams log lines to the Debug section.

## Models

**Model.** A speech-to-text model Handy can run. Each has an id, a display name, a size, a set of *supported languages*, and three capabilities: *streaming*, *translation*, and *language detection*. A model is one of: a catalog model (downloadable from Hugging Face, with a mirror fallback), a legacy model (an older direct download, shown only if already on disk), a custom model (a `.bin` or `.gguf` file the user put in the models folder), or a cache model (a compatible file found in the shared Hugging Face cache).

**Catalog.** The list of downloadable models compiled into Handy. Sorted by an editorial rank, then recommended-first, then accuracy, speed, and name. Five catalog models are *recommended* and are the ones onboarding shows first.

**Downloaded.** A model whose file (or directory) is complete on disk. Only downloaded models can be made *active*.

**Active model.** The model named in the `selected_model` setting: the one the next dictation will use. Shown in the footer, ticked in the tray's model submenu, and badged "Active" on the Models page. There is at most one. Selecting a model makes it active and, unless *unload* is "Immediately", loads it straight away.

**Loaded model.** The active model when it is resident in memory and ready to transcribe. The footer dot is green when the active model is loaded, grey when it is active but unloaded, yellow while loading, red on error. A model becomes loaded on selection, at the start of a dictation, or on re-transcribe; it becomes unloaded after the *unload timeout*, from the tray's Unload Model, when deleted, when a load fails, or when the transcription engine crashes.

**Unload timeout.** The "Unload Model" setting: how long the loaded model may sit idle before it is released. Never, Immediately, 2, 5 (default), 10, 15 minutes, 1 hour, or 15 seconds in debug mode. "Immediately" unloads after every dictation and skips the load-on-select. The idle check runs every 10 seconds and a recording in progress counts as activity.

**Streaming model.** A model whose capability flags say it can transcribe live as audio arrives. Only streaming models use the Live overlay's *panel* and the *live transcription* path; every other model uses the *pill* and batch transcription even when the overlay style is Live. The flag is read from the catalog or the model file before loading and corrected from the real model once loaded.

**Supported languages.** The list of language codes a model advertises. A model with one supported language shows "<Language> only" and no language picker; a model with more shows "N languages" and a picker. Empty means language-agnostic.

**Language intent.** The `selected_language` setting: "auto" or a language code. It is the user's stated preference, not necessarily what the model receives; see *effective language*.

**Effective language.** The language actually given to the active model for a dictation: the intent if the model supports it, otherwise "auto" if the model can detect languages, otherwise English if supported, otherwise the model's first language. Computed fresh each time and never written back to settings, so switching models and back restores the original intent.

**Recommended.** A badge on the handful of catalog models curated for new users. Distinct from the catalog's sort rank.

## The dictation

**Dictation.** Handy's unit of interaction: everything between a *trigger* and the moment text is delivered (or the attempt is abandoned). Exactly one dictation can be in progress at a time; triggers arriving during one are ignored, except that the same shortcut pressed again in *toggle mode* stops it.

**Trigger.** What starts a dictation: a *transcribe shortcut* pressed, `handy --toggle-transcription` or `--toggle-post-process`, or the SIGUSR2 / SIGUSR1 signals. A trigger that arrives while Handy is idle starts a dictation; in toggle mode the same trigger arriving while recording stops it.

**Binding.** One of the three named shortcuts: **Transcribe** (default Option+Space), **Transcribe with Post-Processing** (default Option+Shift+Space, registered only while *post-processing* is enabled), and **Cancel** (default Escape, registered only while recording). The first two are *transcribe shortcuts*.

**Push to talk.** The default shortcut mode (`push_to_talk` is on): holding the transcribe shortcut records, releasing it stops. A release is honored only after a 50 ms grace period during which a repeated press of the same shortcut cancels it, which is how key auto-repeat is absorbed. The Cancel shortcut setting is hidden in this mode because releasing the key is the way to stop.

**Toggle mode.** The alternative (`push_to_talk` off): one press starts recording, the next press of the same shortcut stops it, and releases are ignored. The Cancel shortcut is the only way to abandon a recording without transcribing it.

**Debounce.** Two presses of a transcribe shortcut within 30 ms are treated as one; the second is dropped. Applies in both modes.

**Stage.** Where a dictation is: *idle* (nothing in progress), *recording* (the microphone is open for this dictation), or *processing* (the microphone has been released and Handy is transcribing, post-processing, or pasting). Triggers are only honored in idle, and — to stop — in recording; nothing a trigger does can interrupt processing.

**Capture.** The stretch of a dictation during which microphone sound is being kept. Capture begins when the microphone delivers its first chunk of sound after the start (not when the stream is opened), which is the moment the overlay turns *ready* and the start chime plays; it ends when the stop is processed. Before capture begins the overlay is *arming*.

**Arming / ready.** The two looks of the overlay while recording: *arming* (grey dot, flat waveform) from the trigger until the microphone delivers sound; *ready* (pink dot, live waveform) from then on. Readiness means sound is flowing, not that speech has been detected.

**Voice activity detection (VAD).** The default-on filter that keeps only the stretches of capture that contain speech, plus a short run-in before speech (15 frames, 450 ms) and a tail after it (15 frames, 450 ms, or 55 frames, 1.65 s, for streaming models). Speech is recognized after 2 consecutive speech frames of 30 ms. With VAD off every frame is kept. VAD is decided once at the trigger and cannot change during a dictation.

**Stop.** The event that ends capture: the shortcut released (push to talk), the shortcut pressed again (toggle mode), or a remote toggle arriving while recording. After a stop the dictation is in *processing* and can no longer be stopped, only *cancelled*.

**Cancel.** Abandoning a dictation without delivering text: Escape while recording, the overlay's ✕, the tray's Cancel item, or `handy --cancel`. A cancel during recording discards the capture entirely (no history entry, no recording file); a cancel during processing stops the pipeline at its next checkpoint and discards the result, but a recording file already written stays on disk and a history entry already saved stays. The stop chime does not play on cancel.

**Transcribe.** Turning the captured sound into text with the active model. Batch transcription runs after the stop; *live transcription* runs during capture and is finalized at the stop.

**Live transcription.** The streaming path: with a streaming model, sound is fed to the model as it arrives and the Live panel shows *committed* text (fixed) followed by *tentative* text (still changing) with a blinking caret. At the stop the stream is finalized and its full text is used; if it produced nothing, Handy falls back to batch transcription of the same sound.

**Text cleanup.** The fixed edits applied to every transcript after the model: custom-word correction (fuzzy, ASCII-only terms, threshold 0.18 by default), filler-word removal (on by default; a universal list plus an English/German/French list gated on knowing the language), collapsing three or more repeated words to one, collapsing runs of spaces, trimming. Then, for Chinese, a Simplified/Traditional conversion when the effective language is zh-Hans or zh-Hant.

**Post-processing.** Sending the cleaned transcript to an LLM provider with a prompt and pasting the reply instead. Off by default; enabled under Advanced › Experimental, after which the Post Process section and the second shortcut appear. Runs only for dictations started with the Transcribe with Post-Processing shortcut. Any failure falls back silently to the unprocessed transcript.

**Deliver.** Getting the final text into the application the user was in: the *paste method* (Cmd+V through the clipboard by default), then optional auto-submit, then optional copy-to-clipboard. A dictation whose final text is empty delivers nothing and shows nothing.

**Paste method.** How text is inserted: Clipboard (Cmd+V) (default on macOS; the previous clipboard contents are put back about 60 ms after the paste), Direct typing (not offered on macOS), None (nothing is pasted; the text still goes to history and, if chosen, the clipboard), and on Windows/Linux Ctrl+Shift+V, Shift+Insert, or an external script.

**Auto-submit.** The setting that presses Enter (or Ctrl+Enter, or Cmd+Enter) 50 ms after a successful paste. Off by default; skipped when the paste method is None.

**History entry.** One row on the History page: a timestamp title, the transcript (and the post-processed text if any), a saved star, and the recording. Written at the end of every dictation that captured sound, including failed transcriptions (with empty text, so the user can retry). Kept to the last 5 unsaved entries by default; saved entries are never auto-deleted.

**Recording.** The WAV file (16 kHz mono) of a dictation's captured sound, in the recordings folder, named `handy-<unix seconds>.wav`. Deleted with its history entry.

## Settings

**Setting.** One value in Handy's settings store. Every control in the settings window writes its setting the moment it is changed; there is no Save button and no Cancel. Most controls have a reset arrow that puts the default back.

**Default.** The value a setting has in a fresh install, as defined in `src-tauri/src/settings.rs`. Some defaults depend on platform (the shortcut, the paste method, the keyboard implementation, whether the overlay shows).

**Overlay style.** The setting that chooses None (no overlay; the Linux default), Minimal (the pill), or Live (the default on macOS and Windows: the panel with a streaming model, the pill otherwise).

**Always-on microphone.** A debug-section setting that keeps the microphone stream open between dictations so capture starts faster. Off by default: the microphone is opened at the trigger and closed at the stop (or 30 s later if "Keep Mic Open Between Transcriptions" is on).

**Keyboard implementation.** Which shortcut engine listens for keys: "handy_keys" (default on macOS and Windows; allows modifier-only and fn shortcuts) or "tauri" (default on Linux; needs a main key and rejects fn). Switchable under Advanced › Experimental; shortcuts the new engine cannot express are reset to defaults.

**Experimental.** The Advanced toggle that reveals Post Processing, Keyboard Implementation, acceleration, and Keep Mic Open.

## Events that end or interrupt

**Complete.** A dictation completes when text has been delivered (or the final text was empty) and Handy has returned to idle: the overlay fades out over 300 ms and the tray icon returns to idle.

**Cancel.** See *cancel* under The dictation. Cancel is the only user action that ends a dictation without transcribing.

**Interrupt.** Something other than the user ending a dictation early: the microphone failing to open (a toast, no dictation), capture dying mid-way (the stop still runs, with whatever was captured), the model failing to load or transcribe (a toast, an empty history entry), the post-processing request failing (silent fallback to the plain transcript), the paste failing (a toast; the text is still in history).

**Ignored.** A trigger that arrives while a dictation is processing, a second shortcut while another is recording, or any press within 30 ms of the last. Nothing happens and nothing is shown.

**Secure Input.** A macOS state in which some process (a password field, Terminal's Secure Keyboard Entry) stops other apps from seeing key presses. While it is held for 3 s or more Handy re-registers keyed shortcuts through a fallback that still works, shows a warning in the tray and a banner in the settings window when a shortcut cannot be covered, and refuses to open the shortcut recorder.

**Checkpoint.** One of the five points in a dictation's processing at which Handy looks for a cancel before continuing: after the capture is collected; after the model returns and the recording file is written; every 25 ms during post-processing; before the history entry is saved; and immediately before the paste keystroke. Work between checkpoints cannot be interrupted.

**Orphaned recording.** A recording file in the recordings folder with no history entry, left by a cancel that arrived while the file was being written or the model was running. The History page never shows it and retention never deletes it.

## Units

Times are wall-clock milliseconds unless stated. Audio is measured in 30 ms frames at 16 kHz. Window sizes are in logical points. Model sizes are in megabytes (MiB, shown as MB).
