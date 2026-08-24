# Goal: complete the Handy product description

You are working in the `product-description/` directory of the Handy repository. Read `README.md`, `glossary.md`, `foundations/triggers-and-shortcuts.md`, and `settings/shortcut-recorder.md` first. The README defines the purpose, the document template, the method, the structure, and the coverage table. The other three are the exemplars: match their depth, tone, and structure exactly. Your job is to write every document in the README's structure until the coverage table has no `not started` rows, then run a consistency pass.

## Source of truth

The Handy source is the repository this directory lives in (the parent of `product-description/`). Describe the experience of the desktop app on macOS with a fresh settings store and nothing customized. Windows and Linux branches are named under **Platform differences** but not described in depth. Portable mode, the Linux typing tools, and the Raycast extension are out of scope.

For each document, read in this order before writing:

1. Where the interaction state lives for the feature: `src-tauri/src/transcription_coordinator.rs` and `src-tauri/src/actions.rs` for anything that touches a dictation; `src-tauri/src/managers/audio.rs` for the microphone; `src-tauri/src/managers/transcription.rs` and `src-tauri/src/managers/model.rs` for models and text; `src-tauri/src/clipboard.rs` for delivery; `src-tauri/src/shortcut/mod.rs` for every settings command; the React component under `src/components/` for the screen.
2. The shared pieces: `src-tauri/src/settings.rs` (every setting and its default), `src-tauri/src/lib.rs` (startup, tray menu handlers), `src-tauri/src/overlay.rs` and `src/overlay/RecordingOverlay.tsx` (the overlay), `src-tauri/src/tray.rs`.
3. The tests at the bottom of each Rust file. Key ones: `transcription_coordinator.rs` (auto-repeat and release grace), `settings.rs` (defaults, migrations, salvage), `audio_toolkit/text.rs` (filler and custom words), `managers/transcription.rs` (language evidence, translation plan), `managers/model.rs` (effective language, discovery), `clipboard.rs` (auto-submit, clipboard restore), `paste_tx/mod.rs` (reliable paste), `tray.rs`.
4. UI behavior: `src/App.tsx`, `src/components/settings/**`, `src/components/onboarding/**`, `src/components/model-selector/**`, `src/components/footer/**`, `src/stores/*.ts`, `src/hooks/useSettings.ts`.
5. Strings: `src/i18n/locales/en/translation.json`. Quote UI text exactly as it appears there.

Do not describe code. Describe what the user sees and does. Technical detail goes only in `> Technical note:` block quotes, and only when the mechanism changes what the user would expect.

## Writing rules

- Follow the eight-section template in the README for every feature document. Foundations and cross-cutting documents may drop sections that do not apply but must still cover cancel/interrupt behavior wherever an interaction exists.
- The five phases are always headed **Start**, **Ends at once**, **Becomes active**, **While active**, **Finish**, in that order, and each subsection's first sentence says what the phase means for that feature. Modifiers and cancel/interrupt go in tables, split by phase ("before active" / "while active") as in `settings/shortcut-recorder.md`. The six modifier rows, the eight interrupt rows, and the eight cross-cutting concerns are fixed in the README; do not add, drop, or reorder them in a single document.
- Use the glossary's words. If you need a term the glossary lacks, add it to `glossary.md` in the right section with a one-paragraph definition, then use it.
- Sentence case for all headings. Direct, concrete language. No hedging, no marketing.
- State surprising behavior plainly and say why if the reason is in the code or a comment. If it looks like a bug, say so in "Open questions" rather than smoothing it over.
- Cross-reference other documents with relative links rather than repeating their content. `foundations/triggers-and-shortcuts.md` owns the debounce, release grace, binding, and mode definitions; `foundations/audio-capture.md` owns readiness and VAD numbers; `foundations/models.md` owns the model states and capability words; `foundations/the-settings-model.md` owns how settings save and reset. Do not restate them; link.
- Every document ends with "## Open questions and verification" listing what was read from code but not confirmed by hand, followed by `Verified against Handy commit \`{sha}\`` using the current `git rev-parse --short HEAD` of the repository.
- Mermaid `stateDiagram-v2` for each interaction's states. Keep it to the states the user passes through; omit internal bookkeeping states.

## Things already established (do not re-derive, do not contradict)

Filled in as the foundations are written. Each line is a fact another document may depend on.

### Triggers and shortcuts
- Default bindings on macOS: Transcribe = Option+Space; Transcribe with Post-Processing = Option+Shift+Space; Cancel = Escape. Windows and Linux use Ctrl+Space / Ctrl+Shift+Space.
- Push to talk is on by default. In push to talk, release stops; a release is deferred 50 ms and cancelled by a re-press of the same shortcut within that window (absorbs key auto-repeat). In toggle mode a second press stops; releases do nothing.
- Presses of a transcribe shortcut within 30 ms of the previous press are dropped (both modes).
- The dictation stage is idle → recording → processing → idle. Triggers are honored only in idle (start) and recording (stop, same shortcut only). During processing every trigger is ignored.
- The Cancel shortcut is registered only while recording and unregistered at the stop, so Escape reaches other apps normally when Handy is idle or processing. Cancel via Escape therefore does nothing during processing; the overlay ✕, the tray item, and `--cancel` still do.
- On Linux the Cancel shortcut is never registered (the setting is hidden).
- The post-processing shortcut is registered only while `post_process_enabled` is true.
- While the shortcut recorder is open every binding except Cancel is unregistered.
- Keyboard implementation defaults to handy_keys on macOS and Windows, tauri on Linux. If handy_keys fails to start, Handy falls back to tauri and persists that.
- `push_to_talk` is read on every key event, so toggling it mid-recording changes what the next release or press does.

### Audio capture
- The microphone is opened at the trigger (on-demand mode) and closed at the stop, unless "Keep Mic Open Between Transcriptions" is on (closed 30 s after the last stop) or always-on microphone is on (never closed).
- Capture, readiness, the start chime, and mute all begin on the first chunk of microphone sound, not when the stream opens. A stop before the first chunk suppresses all three.
- VAD: Silero threshold 0.3, 30 ms frames, 2-frame onset, 15-frame pre-roll, 15-frame hangover (55 for streaming models). Off: every frame kept. The policy is fixed at the trigger.
- Captures shorter than 1 s but longer than 0 samples are padded with silence to 1.25 s before transcription. Empty captures end the dictation with no history entry, no file, and no paste.
- Extra recording buffer (debug) keeps capturing N ms after the stop; a cancel during the buffer aborts it.
- "Mute While Recording" mutes system output from readiness until the stop (restored before the stop chime) or the cancel; a system that was already muted stays muted.
- If the selected microphone is missing when capture starts, Handy falls back to the system default and rewrites the setting to Default.

### Models
- A fresh install has no active model; `selected_model` is empty and onboarding must pick one. Once onboarding is complete, a missing selection is auto-filled with the first downloaded model in catalog order.
- Unload timeout default: 5 minutes of idle (checked every 10 s). "Immediately" unloads after each dictation and skips load-on-select.
- Selecting a model writes the setting first, then loads; a failed load reverts the setting and shows a toast.
- Deleting the active model unloads it and clears the selection; the next dictation fails with a "model not loaded" toast until a model is chosen.
- Streaming, translation, and language detection are per-model capabilities; the live panel, the Translate to English toggle, and the Auto language option depend on them.
- Model load is kicked off at every trigger if the model is not loaded; a dictation whose model is still loading waits for it at the stop.

### Settings
- Every control saves immediately; there is no Save or Cancel. Reset arrows restore the per-platform default.
- A settings store that fails to parse is salvaged field by field; only invalid fields fall back to defaults.
- The overlay style default is Live on macOS and Windows, None on Linux.

### Windows and the tray
- Closing the settings window hides it and (macOS, tray shown) removes Handy from the Dock. Quit is the tray's Quit item or Cmd+Q in the window.
- Launching Handy while it is running raises the settings window (and on macOS recreates the tray icon) instead of starting a second copy; with `--toggle-transcription`, `--toggle-post-process`, or `--cancel` it forwards that action and exits.

### Dictation document ownership
- `dictation/starting-and-recording.md` owns idle → recording and the recording stage, including the stop event itself.
- `dictation/transcribing.md` owns processing from the stop to final text (batch path, text cleanup, failures).
- `dictation/live-transcription.md` owns the streaming path during recording and its finalization; it hands the final text to transcribing.
- `dictation/post-processing.md` owns the LLM step between final transcript and delivery.
- `dictation/pasting.md` owns delivery: paste, clipboard restore, auto-submit, copy-to-clipboard, paste failure.
- `dictation/cancelling.md` owns every cancel path at every stage and what each leaves behind.
- `dictation/the-overlay.md` owns what the overlay looks like in each state and how it is positioned; other documents say *which* state is shown and link.

## Order of work

1. `foundations/` first, in this order: triggers-and-shortcuts, audio-capture, models, the-settings-model, windows-and-tray. Everything else links to them.
2. `dictation/` next, all seven documents. Read `transcription_coordinator.rs`, `actions.rs`, `managers/audio.rs`, and `managers/transcription.rs` in full before starting any of them, because the stages hand off to each other and the documents must agree on where one ends and the next begins (ownership above).
3. The remaining `setup/`, `models/`, `settings/`, `history/`, `tray/`, `integration/`, `cross-cutting/` documents. These are independent of each other and can be drafted in parallel with subagents once the foundations and dictation documents exist to link to. If you parallelize, give each subagent this file, the four exemplars, and the specific document to write; then review every result yourself for consistency with the glossary and the established facts above before accepting it.
4. Consistency pass over the whole set: same term for the same thing everywhere, no two documents describing the same behavior differently, every relative link resolves, every document has a verification footer, every glossary term used is defined.
5. Update the coverage table in `README.md` as you go: `drafted` when written, never `verified` (verification by hand is a separate pass you are not doing).

## Working rules

- Commit after each document or coherent group of documents with a message of the form `docs: add product-description/{path}` or `docs: revise product-description/{path}`. Work on the `docs/product-description` branch, never directly on `main`. The Handy repository asks for conventional-commit prefixes and does not require AI attribution trailers in commit messages.
- Do not modify anything outside `product-description/`. The Handy source is read-only reference material.
- Do not add files outside the README's structure without updating the structure and coverage table to match.
- When a behavior cannot be determined from code and tests, write down what you could determine, put the rest in "Open questions", and move on. Do not guess and do not block.
- Depth bar: `settings/shortcut-recorder.md` is roughly 180 lines for a small feature. The dictation documents will be longer; settings-page documents will often be shorter per control but must cover every control on the page. Completeness matters more than length. Every state, every modifier, every cancel/interrupt row must be accounted for, even if the answer is "no effect".
- If you find that the README's structure is wrong for something you discover (a document that should be split, two that should merge), make the change, update the structure and coverage table, and note why in the commit message.

You are done when the coverage table has no `not started` rows, the consistency pass is complete, and everything is committed.
