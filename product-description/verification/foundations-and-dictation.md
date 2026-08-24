# Verification: foundations and dictation

How to run this file: start from a clean state with onboarding completed and one model downloaded and active (Parakeet Unified EN 0.6B unless an item says **batch-model**). Every setting at its default: push to talk on, Audio Feedback off, Overlay Live / Bottom, VAD on, Unload Model 5 minutes. Keep the settings window visible unless told otherwise. TextEdit with a new plain-text document is the paste target; click into it before each dictation. Device column values are defined in [README.md](README.md).

## foundations/triggers-and-shortcuts.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TRIG-01 | P1 | mac, mic | Holding Option+Space records and releasing stops ([The simple case](../foundations/triggers-and-shortcuts.md#the-simple-case)). | Defaults. | 1. Click into TextEdit.<br>2. Hold Option+Space, say "testing one two three", release. | The overlay appears while held, switches to "Transcribing..." on release, text is pasted. | — |
| TRIG-02 | P1 | mac, mic | In toggle mode a second press stops and releases do nothing ([While active](../foundations/triggers-and-shortcuts.md#while-active)). | Push To Talk off. | 1. Tap Option+Space and release.<br>2. Wait 2 s, say a phrase.<br>3. Tap Option+Space again. | Recording continues after the first release; the second tap stops it; text pasted. | — |
| TRIG-03 | P1 | mac, mic | A press within 30 ms of the previous press is dropped ([Start](../foundations/triggers-and-shortcuts.md#start)). | Push To Talk off. | 1. Double-tap Option+Space as fast as possible. | Recording starts once and stays running (a single start, not start+stop). Record what happens. | — |
| TRIG-04 | P1 | mac, mic | Push-to-talk release is deferred 50 ms and absorbed by key repeat ([While active](../foundations/triggers-and-shortcuts.md#while-active)). | Defaults; macOS key repeat at its fastest. | 1. Hold Option+Space for 5 s while speaking. | Recording is continuous (one start chime if audio feedback is on; one pill), never toggles. | — |
| TRIG-05 | P1 | mac, mic | Escape cancels only while recording ([While active](../foundations/triggers-and-shortcuts.md#while-active)). | Defaults. | 1. Hold Option+Space, press Escape while holding.<br>2. Release.<br>3. Start another dictation, release, and press Escape during "Transcribing...". | Step 1: overlay disappears, nothing pasted. Step 3: Escape goes to TextEdit; the dictation completes and pastes. | — |
| TRIG-06 | P1 | mac, mic | Triggers during processing are ignored ([Finish](../foundations/triggers-and-shortcuts.md#finish)). | batch-model, Unload Model set to Immediately (forces a slow load). | 1. Dictate a 15 s passage, release.<br>2. During "Transcribing..." press Option+Space twice. | No second dictation starts; after the paste, the next press works. | — |
| TRIG-07 | P2 | mac, mic | The other transcribe shortcut is ignored while recording ([While active](../foundations/triggers-and-shortcuts.md#while-active)). | Post Processing enabled. | 1. Hold Option+Space.<br>2. Press Option+Shift+Space while holding.<br>3. Release both. | One dictation, plain transcribe (no "Processing..." state). | — |
| TRIG-08 | P1 | mac, mic, shell | `--toggle-transcription` starts and stops like toggle mode regardless of push to talk ([Start](../foundations/triggers-and-shortcuts.md#start)). | Defaults (push to talk on). | 1. Run `/Applications/Handy.app/Contents/MacOS/Handy --toggle-transcription`.<br>2. Speak, run it again. | First run starts recording; second stops and pastes; the second process exits immediately each time. | — |
| TRIG-09 | P2 | mac, mic, shell | SIGUSR2 is the same trigger ([Start](../foundations/triggers-and-shortcuts.md#start)). | Handy running. | 1. `kill -USR2 $(pgrep -x Handy)` twice with speech between. | Same as TRIG-08. | — |
| TRIG-10 | P1 | mac, mic | Toggling Push To Talk mid-recording changes what the release does ([Modifiers](../foundations/triggers-and-shortcuts.md#modifiers)). | Defaults. | 1. Hold Option+Space.<br>2. With the other hand click Push To Talk off.<br>3. Release. | Recording continues after the release; a further tap stops it. (Record what happens.) | — |
| TRIG-11 | P2 | mac | Re-recording a shortcut suspends the others; a remote toggle still works ([Cancel and interrupt](../foundations/triggers-and-shortcuts.md#cancel-and-interrupt)). | Shortcut recorder open. | 1. Press Option+Space. 2. Run `--toggle-transcription`. | Step 1 is captured as the candidate, no dictation. Step 2 starts a dictation with the recorder still open. | — |
| TRIG-12 | P3 | mac, mic, log | The stop is logged about 50 ms after the release. | Defaults. | 1. Dictate; read the log for the press/release timestamps. | The stop follows the release by ~50 ms. | — |

Not checkable by hand:

- Which binding wins when two bindings share identical keys ([Edge cases](../foundations/triggers-and-shortcuts.md#edge-cases)).

## foundations/audio-capture.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| AUD-01 | P1 | mac, mic | Readiness (pink dot) follows the first chunk of sound, not the trigger ([Becomes active](../foundations/audio-capture.md#becomes-active)). | Defaults. | 1. Hold Option+Space and watch the overlay dot. | A brief grey arming state, then pink. | — |
| AUD-02 | P1 | mac, mic | The start chime plays at readiness and is suppressed by a stop before it ([Becomes active](../foundations/audio-capture.md#becomes-active)). | Audio Feedback on. | 1. Hold Option+Space for 2 s.<br>2. Tap Option+Space as briefly as possible. | Step 1: start chime then stop chime. Step 2: stop chime only (record what you hear). | — |
| AUD-03 | P1 | mac, mic | VAD drops silence: a 5 s pause mid-sentence is absent from the recording ([While active](../foundations/audio-capture.md#while-active)). | Defaults. | 1. Dictate "one" … 5 s silence … "two".<br>2. Play the entry on the History page. | The recording is much shorter than the hold; the pause is gone. | — |
| AUD-04 | P1 | mac, mic | With VAD off the whole hold is kept ([Modifiers](../foundations/audio-capture.md#modifiers)). | Voice Activity Detection off. | 1. Same as AUD-03. | The recording is about as long as the hold, pause included. | — |
| AUD-05 | P1 | mac, mic | A stop before any sound produces no history entry and no paste ([Ends at once](../foundations/audio-capture.md#ends-at-once)). | Defaults. | 1. Tap Option+Space for under 50 ms. | Overlay flashes; nothing pasted; History unchanged; no new file in the recordings folder. | — |
| AUD-06 | P1 | mac | A denied microphone shows the "Microphone Access Denied" toast and no dictation ([Start](../foundations/audio-capture.md#start)). | System Settings › Privacy › Microphone: Handy off. | 1. Hold Option+Space. | Overlay and tray flash to recording and back; toast with the macOS instruction text. | — |
| AUD-07 | P1 | usb-mic | Unplugging the selected microphone mid-recording still allows the stop to complete ([Cancel and interrupt](../foundations/audio-capture.md#cancel-and-interrupt)). | Microphone set to the USB device. | 1. Hold Option+Space, speak, unplug, release. | Bars flatten; "Transcribing..." runs; the text spoken before the unplug is pasted or the entry is empty. Record what happens. | — |
| AUD-08 | P1 | usb-mic | A missing selected microphone falls back to Default and rewrites the setting ([Start](../foundations/audio-capture.md#start)). | Microphone set to the USB device; unplug it. | 1. Hold Option+Space and speak.<br>2. Open General. | Recording works from the built-in mic; the Microphone dropdown now reads Default. | — |
| AUD-09 | P2 | mac, mic | Mute While Recording mutes output from readiness to stop and restores it ([Finish](../foundations/audio-capture.md#finish)). | Mute While Recording on; play music. | 1. Hold Option+Space 3 s, release. | Music mutes after the dot turns pink; unmutes at release. | — |
| AUD-10 | P2 | mac, mic | Mute leaves an already-muted system muted ([Sounds and system audio](../foundations/audio-capture.md#interactions-with-other-systems)). | As AUD-09, system muted beforehand. | 1. Dictate. | System still muted after. | — |
| AUD-11 | P3 | mac, mic | Captures under 1 s are padded to 1.25 s ([Finish](../foundations/audio-capture.md#finish)). | Defaults. | 1. Say one short word, release immediately.<br>2. Play the entry. | The player's duration reads 0:01. | — |
| AUD-12 | P2 | debug, mac, mic | Extra Recording Buffer keeps capturing after release ([Finish](../foundations/audio-capture.md#finish)). | Buffer 2000 ms. | 1. Hold, release, and keep speaking for 2 s. | The words after release are in the transcript. | — |
| AUD-13 | P2 | debug, mac, mic | Always-On Microphone removes the arming phase ([Modifiers](../foundations/audio-capture.md#modifiers)). | Always-On on; System Settings shows the mic indicator. | 1. Hold Option+Space. | The dot is pink at once; the menu-bar mic indicator is on between dictations. | — |
| AUD-14 | P3 | bt-mic | Readiness on Bluetooth is visibly later ([Becomes active](../foundations/audio-capture.md#becomes-active)). | Bluetooth headset selected. | 1. Hold Option+Space. | Arming lasts noticeably longer than the built-in mic. | — |

Not checkable by hand:

- Whether the capture survives a microphone change mid-recording ([Open questions](../foundations/audio-capture.md#open-questions-and-verification)).

## foundations/models.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| MODL-01 | P1 | mac | Selecting a model loads it; the footer dot goes yellow then green ([The model's states](../foundations/models.md#the-models-states)). | Two models downloaded. | 1. Pick the other model in the footer dropdown. | "Loading {name}..." with yellow dot, then the name with green dot. | — |
| MODL-02 | P1 | mac | After 5 minutes idle the model unloads (grey dot) ([The unload timeout](../foundations/models.md#the-unload-timeout)). | Default timeout; or debug "After 15 seconds". | 1. Dictate once, wait. | Dot turns grey within the timeout + 10 s; the next dictation reloads. | — |
| MODL-03 | P1 | mac, mic | "Immediately" unloads after each dictation and does not load on select ([The unload timeout](../foundations/models.md#the-unload-timeout)). | Unload Model Immediately. | 1. Select a model. 2. Dictate. | Grey dot after select; green during the dictation; grey after. | — |
| MODL-04 | P1 | no-model, mic | With no model selected a dictation fails with a toast and an empty entry ([Edge cases](../foundations/models.md#edge-cases)). | Delete the active model. | 1. Dictate. | Toast "Transcription Failed"; History shows a failed entry. Footer "No Model - Download Required". | — |
| MODL-05 | P2 | mac | A failed load reverts the selection ([The model's states](../foundations/models.md#the-models-states)). | Corrupt a downloaded model file (truncate it). | 1. Select it. | Toast "Failed to load model: {name}"; the previous model is still active. | — |
| MODL-06 | P2 | mac | The tray's Unload Model is enabled only while loaded ([Interactions](../foundations/models.md#interactions-with-other-systems)). | Model loaded. | 1. Open the tray menu; click Unload Model; reopen. | Enabled, then disabled after unloading; footer dot grey. | — |
| MODL-07 | P2 | mac | Capability tags follow the model ([Capabilities](../foundations/models.md#capabilities)). | Parakeet Unified EN active. | 1. Open the Models page and General. | Card shows "English only" and "Streaming"; General shows no language picker and no Translate toggle. | — |
| MODL-08 | P2 | mac | A `.gguf` dropped into the models folder appears after Rescan as Custom ([Where models live](../foundations/models.md#where-models-live)). | A spare GGUF speech model file. | 1. Copy it into `~/Library/Application Support/com.pais.handy/models`. 2. Click Rescan. | It appears under Downloaded Models with a Custom badge and "Not officially supported". | — |
| MODL-09 | P1 | mac, mic | The first dictation after a load waits at the stop rather than failing ([The model's states](../foundations/models.md#the-models-states)). | Model unloaded (tray). | 1. Dictate immediately. | "Transcribing..." lasts longer; text is pasted. | — |

Not checkable by hand:

- Which model a dictation started during a switch uses ([Open questions](../foundations/models.md#open-questions-and-verification)).

## foundations/the-settings-model.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| SETM-01 | P1 | mac | Every change saves immediately and survives relaunch ([How a change is saved](../foundations/the-settings-model.md#how-a-change-is-saved)). | Defaults. | 1. Turn Append Trailing Space on. 2. Quit and relaunch. | Still on. | — |
| SETM-02 | P1 | mac | The reset arrow restores the platform default ([Defaults and reset](../foundations/the-settings-model.md#defaults-and-reset)). | Transcribe shortcut changed to Ctrl+Space. | 1. Click its reset arrow. | Chip reads "Option + Space"; the shortcut works. | — |
| SETM-03 | P1 | mac | A broken settings file is salvaged field by field ([Damaged and out-of-date settings files](../foundations/the-settings-model.md#damaged-and-out-of-date-settings-files)). | Quit; edit settings_store.json: set `"sound_theme": "theremin"`, keep `"push_to_talk": false`. | 1. Launch. | Push To Talk still off; Sound Theme back to Marimba; no other setting changed. | — |
| SETM-04 | P2 | mac | Live-applied settings: Show Tray Icon, Overlay Position, Theme ([How a change is saved](../foundations/the-settings-model.md#how-a-change-is-saved)). | Defaults. | 1. Toggle Show Tray Icon. 2. Set Overlay Position Top and dictate. 3. Set Theme Dark. | Tray icon disappears/reappears; overlay at top; window and overlay go dark. | — |
| SETM-05 | P1 | mac, shell | Debug mode is a saved setting; `--debug` does not reveal the Debug section (suspected bug) ([Debug mode and hidden controls](../foundations/the-settings-model.md#debug-mode-and-hidden-controls)). | Defaults. | 1. Cmd+Shift+D, relaunch. 2. Turn it off, relaunch with `--debug`. | Step 1: Debug section persists. Step 2: no Debug section; the log file logs at Trace. Record what happens. | — |
| SETM-06 | P2 | mac | History Limit 0 deletes every unsaved entry at once ([Edge cases](../foundations/the-settings-model.md#edge-cases)). | Three unsaved entries, one starred. | 1. Set History Limit to 0. | Only the starred entry remains. | — |
| SETM-07 | P3 | mac | The defaults table matches a fresh install ([Defaults and reset](../foundations/the-settings-model.md#defaults-and-reset)). | Clean state. | 1. Read every page. | Each control shows the listed default. | — |

## foundations/windows-and-tray.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| WIN-01 | P1 | mac | Closing the window hides it and removes the Dock icon; shortcuts keep working ([Closing, hiding, and showing](../foundations/windows-and-tray.md#closing-hiding-and-showing)). | Defaults. | 1. Close the window. 2. Cmd+Tab. 3. Dictate. | Handy absent from Cmd+Tab and the Dock; dictation works. | — |
| WIN-02 | P1 | mac | "Settings…" in the tray shows the window and the Dock icon returns ([Closing, hiding, and showing](../foundations/windows-and-tray.md#closing-hiding-and-showing)). | Window hidden. | 1. Tray › Settings…. | Window in front and focused; Dock icon back. | — |
| WIN-03 | P1 | mac | Start Hidden launches without the window; relaunching shows it ([Launch](../foundations/windows-and-tray.md#launch)). | Start Hidden on. | 1. Quit, launch. 2. Launch again from Spotlight. | Step 1: tray only. Step 2: window appears, one process. | — |
| WIN-04 | P1 | mac | Start Hidden is ignored when the tray icon is off ([Launch](../foundations/windows-and-tray.md#launch)). | Start Hidden on, Show Tray Icon off. | 1. Quit, launch. | Window shown. | — |
| WIN-05 | P1 | mac | Show Tray Icon off with the window hidden leaves no way back except relaunch ([Edge cases](../foundations/windows-and-tray.md#edge-cases)). | Defaults. | 1. Turn Show Tray Icon off. 2. Close the window. 3. Look for Handy in the Dock and menu bar. 4. Relaunch from Spotlight. | No icon anywhere; relaunch shows the window. (Suspected trap; record what happens.) | — |
| WIN-06 | P2 | mac | Quit releases the model and ends the process ([Quitting](../foundations/windows-and-tray.md#quitting)). | Model loaded. | 1. Tray › Quit. | Handy gone from Activity Monitor. | — |
| WIN-07 | P2 | mac | Cmd+Q quits only with the settings window focused ([Quitting](../foundations/windows-and-tray.md#quitting)). | Window shown. | 1. Focus it, press Cmd+Q. | Handy quits. (Open question: confirm it works.) | — |
| WIN-08 | P2 | two-monitors, mic | The overlay appears on the monitor under the pointer ([The overlay window](../foundations/windows-and-tray.md#the-overlay-window)). | Pointer on the second display, TextEdit on the first. | 1. Dictate. | Overlay on the second display; text pasted on the first. | — |
| WIN-09 | P3 | mac, mic | The overlay sits 15 points above the Dock-adjusted bottom, centered ([The overlay window](../foundations/windows-and-tray.md#the-overlay-window)). | Dock shown. | 1. Dictate and take a screenshot. | The pill clears the Dock and is centered. | — |
| WIN-10 | P2 | mac, shell | A second launch with `--start-hidden` is ignored ([Second launches and remote control](../foundations/windows-and-tray.md#second-launches-and-remote-control)). | Running. | 1. Run the binary with `--start-hidden`. | The window is shown (the flag does nothing). | — |

## dictation/starting-and-recording.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| REC-01 | P1 | mac, mic | The tray icon and overlay switch at the trigger and at the stop ([Start](../dictation/starting-and-recording.md#start), [Finish](../dictation/starting-and-recording.md#finish)). | Defaults. | 1. Hold, speak, release. | Recording glyph + pill on press; transcribing glyph + "Transcribing..." on release; idle after. | — |
| REC-02 | P1 | mac, mic, streaming-model | With Live and a streaming model the overlay is the wider form and opens into a panel ([The simple case](../dictation/starting-and-recording.md#the-simple-case)). | Defaults. | 1. Hold and speak two sentences. | The pill grows into a panel with text and a timer. | — |
| REC-03 | P1 | mac, mic, batch-model | With a non-streaming model under Live the overlay stays a pill ([Modifiers](../dictation/starting-and-recording.md#modifiers)). | Whisper Medium active. | 1. Hold and speak. | Pill only; no panel, no timer. | — |
| REC-04 | P1 | mac, mic | Overlay None shows nothing; the tray still changes ([Modifiers](../dictation/starting-and-recording.md#modifiers)). | Overlay None. | 1. Hold and speak. | No overlay; tray glyph changes; text pasted. | — |
| REC-05 | P2 | mac, mic | The stop chime plays even when nothing was captured ([Ends at once](../dictation/starting-and-recording.md#ends-at-once)). | Audio Feedback on. | 1. Tap the shortcut very briefly. | Stop chime heard; no start chime. | — |
| REC-06 | P1 | mac, mic | Switching apps mid-recording sends the paste to the newly focused app ([While active](../dictation/starting-and-recording.md#while-active)). | TextEdit and Notes open. | 1. Hold in TextEdit, click into Notes, release. | Text appears in Notes. | — |
| REC-07 | P1 | mac, mic | A second dictation is ignored until the first finishes processing ([Edge cases](../dictation/starting-and-recording.md#edge-cases)). | batch-model. | 1. Dictate 20 s; release; immediately hold again and speak. | The second hold does nothing until the first paste; then it works. Record the gap. | — |
| REC-08 | P2 | mac, mic | There is no maximum recording length ([While active](../dictation/starting-and-recording.md#while-active)). | Push To Talk off. | 1. Record for 3 minutes. | Still recording; stop transcribes all of it. | — |
| REC-09 | P2 | mac, mic | Changing the microphone during a recording restarts the stream ([Cancel and interrupt](../dictation/starting-and-recording.md#cancel-and-interrupt)). | Two microphones. | 1. Hold; change Microphone; keep speaking; release. | Record whether the early words survive (open question). | — |
| REC-10 | P3 | mac, mic | Level bars move with the voice and flatten in pauses ([While active](../dictation/starting-and-recording.md#while-active)). | Defaults. | 1. Speak, pause, speak. | Bars rise and fall; flat during the pause. | — |

## dictation/transcribing.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TRNS-01 | P1 | mac, mic | A history entry with the recording appears for every transcribed dictation ([Finish](../dictation/transcribing.md#finish)). | Defaults. | 1. Dictate; open History. | New top entry with the text and a play button. | — |
| TRNS-02 | P1 | mac, mic | Filler words are removed ("um", "uh") for English ([While active](../dictation/transcribing.md#while-active)). | English speech, Language Auto or English. | 1. Dictate "so um I think uh this works". | Pasted text has no "um"/"uh". | — |
| TRNS-03 | P1 | mac, mic | Remove Filler Words off keeps them ([While active](../dictation/transcribing.md#while-active)). | Toggle off. | 1. Same as TRNS-02. | Fillers present (as the model heard them). | — |
| TRNS-04 | P1 | mac, mic | Custom Words correct near-misses ([While active](../dictation/transcribing.md#while-active)). | Custom Words: "ChargeBee". | 1. Dictate "send it to charge bee". | Pasted text contains "ChargeBee". | — |
| TRNS-05 | P1 | mac, mic | Three repeats collapse to one; two do not ([While active](../dictation/transcribing.md#while-active)). | Defaults. | 1. Dictate "no no no no" then "no no". | First becomes "no"; second stays "no no" (record the model's raw output too). | — |
| TRNS-06 | P1 | mac, mic | A silent-but-non-empty capture leaves a "Transcription failed" entry (suspected bug) ([Edge cases](../dictation/transcribing.md#edge-cases)). | VAD off. | 1. Hold 3 s in silence, release. | Nothing pasted; History shows "Transcription failed. You can re-transcribe using the retry icon." Record what happens. | — |
| TRNS-07 | P1 | no-model, mic | Transcription failure shows a toast and an empty entry ([Finish](../dictation/transcribing.md#finish)). | No model. | 1. Dictate. | Toast "Transcription Failed" with the message; failed entry. | — |
| TRNS-08 | P2 | mac, mic | Transcription waits for a loading model ([Start](../dictation/transcribing.md#start)). | Unload via tray, then dictate. | 1. Dictate. | Longer "Transcribing..."; correct text. | — |
| TRNS-09 | P2 | mac, mic | Translate to English translates non-English speech ([Becomes active](../dictation/transcribing.md#becomes-active)). | A translation-capable model (Whisper Medium), toggle on. | 1. Dictate a Spanish sentence. | English text pasted. | — |
| TRNS-10 | P2 | mac, mic | Chinese script conversion follows the language setting ([While active](../dictation/transcribing.md#while-active)). | Whisper Medium; Language Chinese (Traditional). | 1. Dictate Mandarin. | Traditional characters pasted. | — |
| TRNS-11 | P3 | mac, mic | A recommended model transcribes faster than real time ([Becomes active](../dictation/transcribing.md#becomes-active)). | Parakeet Unified. | 1. Dictate 10 s. | "Transcribing..." lasts well under 10 s. | — |

## dictation/pasting.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PASTE-01 | P1 | mac, mic | The previous clipboard text is restored after a paste ([Finish](../dictation/pasting.md#finish)). | Copy "hello" in TextEdit. | 1. Dictate. 2. Cmd+V by hand. | Dictated text inserted; "hello" pasted by hand afterwards. | — |
| PASTE-02 | P1 | mac, mic | A previous clipboard image is restored ([Finish](../dictation/pasting.md#finish)). | Take a screenshot to the clipboard (Ctrl+Shift+Cmd+4). | 1. Dictate into TextEdit. 2. Paste into Preview › New from Clipboard. | The screenshot is still on the clipboard. | — |
| PASTE-03 | P1 | mac, mic | An empty clipboard stays empty afterwards ([Finish](../dictation/pasting.md#finish)). | `pbcopy < /dev/null`. | 1. Dictate. 2. `pbpaste`. | Empty. | — |
| PASTE-04 | P1 | mac, mic | Copy to Clipboard leaves the transcript on the clipboard ([Finish](../dictation/pasting.md#finish)). | Clipboard Handling: Copy to Clipboard. | 1. Dictate. 2. `pbpaste`. | The transcript. | — |
| PASTE-05 | P1 | mac, mic | Paste Method None pastes nothing but saves history ([Start](../dictation/pasting.md#start)). | Paste Method None. | 1. Dictate. | Nothing in TextEdit; entry in History. | — |
| PASTE-06 | P1 | mac, mic | Auto Submit presses Enter after the paste ([Finish](../dictation/pasting.md#finish)). | Auto Submit: Enter; a Messages or Terminal prompt as target. | 1. Dictate a command into Terminal. | The command runs. | — |
| PASTE-07 | P2 | mac, mic | Append Trailing Space adds a space to the paste but not to history ([Edge cases](../dictation/pasting.md#edge-cases)). | Toggle on. | 1. Dictate twice in a row. | Two phrases separated by a space; History entries have no trailing space. | — |
| PASTE-08 | P1 | mac, mic | Pasting into Handy's own window (suspected sharp edge) ([Cancel and interrupt](../dictation/pasting.md#cancel-and-interrupt)). | Settings window frontmost, cursor in the Custom Words field. | 1. Dictate. | Record where the text lands. | — |
| PASTE-09 | P2 | mac, mic | The paste works on a non-US layout ([Becomes active](../dictation/pasting.md#becomes-active)). | Input source: Dvorak. | 1. Dictate. | Text pasted. | — |
| PASTE-10 | P2 | mac, mic | Accessibility denied → "Failed to Paste Text" toast ([Interactions](../dictation/pasting.md#interactions-with-other-systems)). | Remove Handy from Accessibility after granting (shortcuts may also stop — use `--toggle-transcription`). | 1. Dictate via the flag. | Toast appears; entry in History. Record what happens. | — |
| PASTE-11 | P3 | mac, mic, log | The transcript occupies the clipboard for roughly 220 ms ([While active](../dictation/pasting.md#while-active)). | Defaults. | 1. Dictate; read the log timing lines. | Paste completes within ~250 ms of starting. | — |
| PASTE-12 | P2 | debug, mac, mic | Reliable Paste restores after the target reads ([Finish](../dictation/pasting.md#finish)). | Reliable Paste on; copy "hello". | 1. Dictate. 2. Cmd+V by hand. | Same outcome as PASTE-01; log shows "[reliable-paste] clipboard read". | — |

## dictation/cancelling.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| CANC-01 | P1 | mac, mic | Cancel during recording keeps nothing ([Ends at once](../dictation/cancelling.md#ends-at-once)). | Note the recordings folder count. | 1. Hold, speak, press Escape. | No paste, no entry, no new file, no stop chime. | — |
| CANC-02 | P1 | mac, mic | The overlay ✕ cancels during recording and during transcribing ([Start](../dictation/cancelling.md#start)). | Defaults. | 1. Hold; click ✕. 2. Dictate 15 s; click ✕ during "Transcribing...". | Both: overlay gone, nothing pasted. | — |
| CANC-03 | P1 | mac, mic | The tray's Cancel item appears during a dictation ([Start](../dictation/cancelling.md#start)). | Push To Talk off. | 1. Tap to start; open the tray menu. | "Cancel" present; model submenu absent. Click it: recording ends. | — |
| CANC-04 | P1 | mac, mic | Cancel during transcription orphans the recording file (suspected bug) ([Finish](../dictation/cancelling.md#finish)). | Count files in the recordings folder. | 1. Dictate 20 s; click ✕ during "Transcribing...". | A new `handy-*.wav` exists; no History entry. Record. | — |
| CANC-05 | P1 | mac, mic | After a cancel during transcription the shortcut is dead until the work finishes (suspected bug) ([While active](../dictation/cancelling.md#while-active)). | batch-model. | 1. Dictate 30 s; ✕ immediately; try to dictate again at once. | The second hold does nothing for several seconds. Record the gap. | — |
| CANC-06 | P1 | mac, mic, shell | `--cancel` works at every stage ([Start](../dictation/cancelling.md#start)). | Defaults. | 1. Start with `--toggle-transcription`; run `--cancel`. | Recording ends, nothing pasted. | — |
| CANC-07 | P2 | mac, mic | Cancel with Mute While Recording restores audio immediately ([Interactions](../dictation/cancelling.md#interactions-with-other-systems)). | Mute on; music playing. | 1. Hold; Escape. | Music back at once. | — |
| CANC-08 | P2 | mac, mic | Cancel after the history entry is saved keeps the entry ([Finish](../dictation/cancelling.md#finish)). | bad-llm, post-processing configured, prompt selected. | 1. Dictate with Option+Shift+Space; click ✕ during "Processing...". | No paste; record whether an entry exists (expected: none). | — |

## dictation/live-transcription.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| LIVE-01 | P1 | mac, mic, streaming-model | Text appears while speaking with a caret; the timer counts ([While active](../dictation/live-transcription.md#while-active)). | Defaults. | 1. Hold and speak three sentences. | Words appear live; timer increments from 0:00. | — |
| LIVE-02 | P1 | mac, mic, streaming-model | The panel keeps its text under the spinner at the stop ([Finish](../dictation/live-transcription.md#finish)). | Defaults. | 1. Release after speaking. | Text stays; control row shows spinner + "Transcribing..."; then fade and paste. | — |
| LIVE-03 | P1 | mac, mic, streaming-model | Minimal style disables live transcription ([Modifiers](../dictation/live-transcription.md#modifiers)). | Overlay Minimal. | 1. Speak. | Pill only; "Transcribing..." at the stop lasts as long as a batch run. | — |
| LIVE-04 | P2 | mac, mic, streaming-model | Custom words are applied to the final text but not shown live ([While active](../dictation/live-transcription.md#while-active)). | Custom Words: "ChargeBee". | 1. Say "charge bee". | Panel shows "charge bee"-ish; paste shows "ChargeBee". | — |
| LIVE-05 | P2 | mac, mic, streaming-model | A long pause freezes the text until speech resumes ([Edge cases](../dictation/live-transcription.md#edge-cases)). | Defaults. | 1. Speak, pause 5 s, speak. | Text stops then continues. | — |
| LIVE-06 | P2 | mac, mic, streaming-model | Scroll-back inside the panel pauses auto-follow ([While active](../dictation/live-transcription.md#while-active)). | Defaults. | 1. Speak a paragraph; scroll up in the panel; keep speaking. | The view stays where scrolled until scrolled back down. | — |
| LIVE-07 | P2 | mac, mic, streaming-model | Overlay Position Top flips the panel layout ([Edge cases](../dictation/live-transcription.md#edge-cases)). | Overlay Position Top. | 1. Speak a paragraph. | Control row on top, text flowing downward. | — |

## dictation/post-processing.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| POST-01 | P1 | mac, mic | A fresh install has no selected prompt, so post-processing silently does nothing (suspected bug) ([Start](../dictation/post-processing.md#start)). | Clean state; enable Post Processing; configure OpenAI key and model; do NOT pick a prompt. | 1. Dictate with Option+Shift+Space. | No "Processing..." state; raw transcript pasted. Record. | — |
| POST-02 | P1 | llm, mic | With a prompt selected the reply is pasted and both texts saved ([Finish](../dictation/post-processing.md#finish)). | Prompt "Improve Transcriptions" selected. | 1. Dictate "um the meeting is at three thirty". | "Processing..." shown; cleaned text pasted; tray › Copy Last Transcript yields the cleaned text. | — |
| POST-03 | P1 | bad-llm, mic | A failing provider falls back silently to the raw transcript ([Finish](../dictation/post-processing.md#finish)). | Custom provider at http://localhost:1/v1. | 1. Dictate with Option+Shift+Space. | Raw transcript pasted; no toast. | — |
| POST-04 | P1 | mic, network-off or a hanging endpoint | A hung provider holds "Processing..." until cancelled (suspected bug) ([While active](../dictation/post-processing.md#while-active)). | Custom provider pointed at a port that accepts but never replies (e.g. `nc -l 9999`). | 1. Dictate with Option+Shift+Space; wait 60 s; click ✕. | "Processing..." persists; ✕ ends it; nothing pasted. | — |
| POST-05 | P1 | llm, mic | The plain Transcribe shortcut never post-processes ([Modifiers](../dictation/post-processing.md#modifiers)). | As POST-02. | 1. Dictate with Option+Space. | No "Processing..."; raw transcript. | — |
| POST-06 | P2 | llm, mic | Blank transcripts skip the request ([Ends at once](../dictation/post-processing.md#ends-at-once)). | VAD off. | 1. Hold 2 s in silence with Option+Shift+Space. | No "Processing..." state. | — |

## dictation/the-overlay.md

| ID | P | Device | Claim | Setup | Steps | Expected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OVL-01 | P1 | mac, mic | Arming: grey dot and faint travelling bars; ready: pink pulsing dot and live bars ([The states](../dictation/the-overlay.md#the-states)). | Defaults. | 1. Hold and watch. | As described. | — |
| OVL-02 | P1 | mac, mic | The working pill shows a spinner, "Transcribing...", and ✕ ([The states](../dictation/the-overlay.md#the-states)). | batch-model. | 1. Release after speaking. | As described; pill widens. | — |
| OVL-03 | P1 | mac, mic | The overlay never takes focus and clicking it does not raise Handy ([Placement and behavior](../dictation/the-overlay.md#placement-and-behavior)). | Push To Talk off. | 1. Tap to start; click the pill away from ✕. | TextEdit stays frontmost; recording continues. Record whether the click passed through. | — |
| OVL-04 | P2 | mac, mic | Overlay Position moves it live ([Cancel and interrupt](../dictation/the-overlay.md#cancel-and-interrupt)). | Push To Talk off; recording. | 1. Change Overlay Position to Top. | The pill jumps to the top edge. | — |
| OVL-05 | P2 | mac, mic | The overlay appears over a full-screen app ([Placement and behavior](../dictation/the-overlay.md#placement-and-behavior)). | Safari full screen. | 1. Dictate. | Pill visible over Safari. | — |
| OVL-06 | P2 | mac, mic | Theme Dark darkens the overlay ([Placement and behavior](../dictation/the-overlay.md#placement-and-behavior)). | Theme Dark, macOS light. | 1. Dictate. | Dark pill. | — |
| OVL-07 | P3 | mac, mic | The fade-out takes about 300 ms ([The states](../dictation/the-overlay.md#the-states)). | Defaults. | 1. Dictate; watch the end. | A short fade, not an instant disappearance. | — |
| OVL-08 | P3 | mac, mic | Labels follow the interface language ([Placement and behavior](../dictation/the-overlay.md#placement-and-behavior)). | Application Language: Deutsch. | 1. Dictate. | German label in the working pill. | — |
