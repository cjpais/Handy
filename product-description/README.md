# Handy product description

A written description of the user experience of Handy: what the user sees, what they can do, and exactly what happens when they do it.

## Purpose

Handy is, from the user's point of view, a large state chart. The user moves through it with a global keyboard shortcut (pressed, held, released, pressed again), the Escape key, clicks on a floating overlay, a menu-bar icon, a settings window, and occasionally a command-line flag. Most of that behavior is defined implicitly, spread across a coordinator thread, a handful of Rust managers, a set of Tauri commands, React settings components, and the unit tests next to them. There is no single place that says, in plain language, "when the user does X, this is what happens, and this is what happens if they do Y halfway through."

This project is that place. It describes the full experience a user has with the Handy desktop app on macOS, launched with a fresh settings store, in the default configuration with nothing customized, through onboarding and every screen and action after it.

The documents are for people who need to understand or change the product: designers, engineers, writers, testers, and anyone evaluating whether a behavior is intentional. They are written from the outside in. They describe the experience, not the implementation.

### What this is not

- Not API documentation. The Tauri commands and events are listed in `src/bindings.ts` and are not described here.
- Not organized by package. `managers/`, `commands/`, `shortcut/`, and the React component tree are not described separately. A single behavior is described once, wherever the user encounters it.
- Not a technical design document. Where a technical detail is critical to understanding the experience, it appears in a block quote labeled `Technical note:` and nowhere else.

## Conventions

- Describe the experience, not the code. "The overlay turns from grey to pink once the microphone is actually delivering sound" rather than "the frontend sets captureReady on the recording-ready event".
- Technical detail goes in block quotes, prefixed with `Technical note:`. Use it only when the mechanism changes what the user would expect.
- Use sentence case for headings.
- Name the vocabulary consistently. The [glossary](glossary.md) is the source of truth for terms like *dictation*, *trigger*, *push to talk*, *toggle mode*, *capture*, *overlay*, *active model*, *loaded model*, *cancel*, and *deliver*.
- Every document ends with the commit of the Handy repository it was verified against and a list of open questions.
- When a behavior is surprising, say so and say why it is that way if the reason is known. Do not smooth it over.

## The work to be done

Each document describes one feature. Features are large things (a dictation from shortcut press to pasted text) or small things (the "Copy Last Transcript" item in the tray menu), but each is described in full, including its edge cases and its interactions with other features.

### Document template

Every feature document follows the same skeleton so that documents are comparable and nothing is skipped.

1. **Summary.** One paragraph describing the feature abstractly. For example: "The shortcut recorder lets the user replace a keyboard shortcut by clicking its current value and pressing a new combination."
2. **The simple case.** The common path in prose.
3. **The interaction, event by event.** The five phases of an interaction: **Start**, **Ends at once**, **Becomes active**, **While active**, **Finish**. For a dictation these are: the trigger (the shortcut is pressed, or a tray item, signal, or command-line flag fires); the dictation ending before anything was captured (a refused microphone, a release before the microphone was ready, a cancel, silence only); capture beginning (the microphone delivers its first sound and the overlay turns ready); recording; and the stop, transcription, and delivery of text. For a settings interaction the same five slots are: the control is opened or clicked; it is left without a change; the first change is made; editing continues; the change is committed. Each subsection begins by saying what the phase means for that feature. Include a small state diagram (Mermaid `stateDiagram-v2`) of the states the user passes through.
4. **Modifiers.** A table of the six settings that change the outcome of the same interaction, what each does when set before the start and when changed while active: **Push to talk**, **Binding** (Transcribe vs Transcribe with Post-Processing), **Overlay style** (None, Minimal, Live), **Streaming model** (whether the active model can stream), **Voice activity detection**, **Always-on microphone**. Same six rows, same order, in every document.
5. **Cancel and interrupt.** The same eight rows in the same order in every document, two columns (before active, while active):
   1. **Cancel** — Escape, the overlay's ✕, the tray's Cancel item, or `handy --cancel`.
   2. **Another trigger** — the same shortcut pressed again, the other transcribe shortcut, a signal or command-line toggle arriving mid-way.
   3. **A setting changed mid-way** — the model switched, the microphone changed, the overlay style changed, push to talk toggled, a shortcut re-recorded.
   4. **Microphone lost** — the device unplugged or capture fails, or permission denied.
   5. **Model or processing failure** — the model is not loaded or fails to load, transcription errors, the post-processing endpoint is unreachable.
   6. **The active application changes** — focus moves to another window (including Handy's own) between the trigger and delivery.
   7. **Handy quits or the system sleeps** — the process ends, the Mac sleeps, the user logs out.
   8. **Keyboard channel changes** — macOS Secure Input engages, key auto-repeat, or the keyboard implementation is switched.
6. **Interactions with other systems.** One bold-led paragraph per concern, in this order: **Permissions.** **History and recordings.** **Clipboard.** **Model state.** **Tray and overlay.** **Sounds and system audio.** **Settings persistence.** **Platform differences.**
7. **Edge cases.** Anything a user could notice that is not covered above.
8. **Open questions and verification.** The Handy commit the document was verified against, and any behavior that could not be confirmed.

Item 5 matters most. Asking the same interrupt questions of every feature is how gaps and inconsistencies are found.

### Method

For each document:

1. Read where the interaction state lives: `src-tauri/src/transcription_coordinator.rs` (the Idle → Recording → Processing lifecycle), `src-tauri/src/actions.rs` (what start and stop actually do), `src-tauri/src/managers/audio.rs` (capture, readiness, cancel), `src-tauri/src/managers/transcription.rs` (model load, streaming, text cleanup), `src-tauri/src/clipboard.rs` (delivery), `src-tauri/src/shortcut/` (triggers and the shortcut recorder), and the relevant React components under `src/components/`.
2. Read the matching tests. Rust unit tests sit at the bottom of each file; the ones that read as executable specifications are in `transcription_coordinator.rs` (push-to-talk release grace, auto-repeat bursts), `settings.rs` (defaults, migrations, salvage), `audio_toolkit/text.rs` (filler words, custom words), `managers/transcription.rs` (language evidence), `managers/model.rs` (language resolution, custom model discovery), `clipboard.rs` (auto-submit, clipboard restore), and `paste_tx/mod.rs` (reliable paste).
3. Draft the document.
4. Try anything ambiguous in the running app: `bun run tauri dev` from the repository root (with `CMAKE_POLICY_VERSION_MINIMUM=3.5` if cmake complains), or the installed `/Applications/Handy.app`. Tests settle "what happens"; the running product settles how it feels, what is visible while the interaction is in progress, and what the timing is like.
5. Record the commit verified against.

### Verification

Drafting reads the code; verification watches the product. The `verification/` directory holds one checklist per cluster of documents, each item a single observable claim with setup, steps, expected result, a priority, and the device it needs. A tester runs them on a Mac with a fresh settings store, records `pass`, `fail`, or `blocked` in the Result column, and files every failure in `bug-triage.md` with the item's ID. A document moves from `drafted` to `verified` in the coverage table only when every P1 and P2 item for it has passed or been filed.

`bug-triage.md` is the other half: every behavior the documents flagged as a likely defect, deduplicated, with reproduction steps, the reason in the code, a severity, and the decision the product team needs to make. Entries confirmed in the running app carry a Status line.

### Order of work

1. **Pilot: the shortcut recorder.** Small and self-contained, with a real multi-phase interaction (click, press, hold, release, click-outside). Used to settle the template, tone, and depth.
2. **Foundations: triggers and shortcuts, audio capture, models, the settings model, windows and the tray.** Everything else refers to them.
3. **The dictation.** The bulk of the experience and the hardest part: seven documents that hand off to each other. Written third so the template is already proven.
4. **Everything else.** Once the template and the exemplars exist, the remaining documents can be drafted in parallel, followed by a consistency pass and a verification pass across the whole set.

Progress is tracked in the [coverage table](#coverage) below.

### Scope decisions

- **Surface.** The desktop app as a macOS user meets it: English interface, a fresh settings store, nothing customized, the shortcut defaults (Option+Space to transcribe, Option+Shift+Space to transcribe with post-processing, Escape to cancel). Where the code branches for Windows or Linux the branch is named under **Platform differences**, but those branches are not verified here.
- **Windows and Linux.** Not separately described. Portable mode (a Windows-only install layout), the Linux typing tools (wtype, dotool, ydotool, xdotool, kwtype), Wayland, and the Windows microphone-consent registry are mentioned only where a macOS user would notice a difference in the UI.
- **The headless command line** (`--transcribe-file`, `--list-models`, `--list-devices`) is described in one short document alongside the remote-control flags, because it shares the binary and the model store; it is not verified by hand.
- **Post-processing providers.** The LLM providers are described as one feature. Which provider gives the best results is out of scope; what happens when a provider fails is in scope.
- **Translations of the UI.** The interface language setting is described; the content of the non-English translations is not.
- **Debug mode** is described (it is reachable with Cmd+Shift+D by any user), but its controls are described at lower depth than the rest.
- **Concerns described inside each document rather than separately:** permissions, history, clipboard, model state, tray and overlay, sounds, settings persistence, platform differences. A separate document per concern would drift from the features it touches; instead every document walks the same list.
- **Concerns described once in a cross-cutting document:** Secure Input on macOS, language resolution and translation, data on disk, platform differences. These are referenced from every feature that meets them.
- **Interaction shape.** The unit of interaction is a dictation (and, for settings, a single control's edit), and its phases are Start, Ends at once, Becomes active, While active, Finish. The interrupt list and the order of cross-cutting concerns are fixed as written in the document template above.
- **Numbered rules.** These are prose documents, not numbered specifications. Stable heading anchors are enough for cross-references.
- **Where this lives.** This description lives inside the Handy repository at `product-description/` rather than in a repository of its own, so it is versioned with the code it describes. Paths in the documents are relative to the repository root unless they begin with `./`.

## Structure

```
README.md                        this file
goal.md                          the standing instructions for whoever drafts
AGENTS.md, CLAUDE.md             entry points for agents: read README.md, then goal.md
glossary.md                      shared vocabulary
bug-triage.md                    suspected defects collected from every document, with repro steps and decisions needed

verification/
  README.md                      how to run a hand-verification pass and record results
  foundations-and-dictation.md   checklists for foundations/ and dictation/
  setup-and-models.md            checklists for setup/ and models/
  settings.md                    checklists for settings/
  history-tray-integration.md    checklists for history/, tray/, integration/
  cross-cutting.md               checklists for cross-cutting/

foundations/
  triggers-and-shortcuts.md      what starts, stops, and cancels a dictation: bindings, push to talk vs toggle,
                                 debounce and release grace, the cancel shortcut, signals and command-line triggers
  audio-capture.md               the microphone: on-demand vs always-on, readiness, voice activity detection, levels
  models.md                      what a model is: catalog, downloaded, active, loaded; capabilities; idle unload
  the-settings-model.md          how settings are read, saved, reset, migrated; what "default" means
  windows-and-tray.md            the settings window, the overlay window, the menu-bar icon, start hidden, quit

dictation/
  starting-and-recording.md      from the trigger to the stop: overlay, chime, levels, what is captured
  transcribing.md                from the stop to text: batch transcription, language, text cleanup, failures
  pasting.md                     from text to the target app: paste methods, clipboard restore, auto-submit
  cancelling.md                  Escape, the overlay's ✕, the tray's Cancel, at every stage
  live-transcription.md          the Live overlay: streaming text while recording, finalizing
  post-processing.md             the second shortcut: sending the transcript to an LLM and pasting the result
  the-overlay.md                 the floating pill and panel: states, position, what it shows and when

setup/
  first-launch.md                what happens when Handy starts: window, tray, hidden start, returning users
  permissions.md                 the permissions step on macOS and the banner afterwards
  choosing-a-model.md            the onboarding model step and the first download

models/
  downloading-a-model.md         download, verify, extract, cancel, resume, failure
  switching-models.md            the active model: footer selector, tray submenu, Models page; load and unload
  the-models-page.md             search, filters, rescan, custom models, delete

settings/
  the-settings-window.md         sidebar, sections, close-to-tray, the debug-mode shortcut, toasts
  shortcut-recorder.md           changing a shortcut (the pilot)
  general.md                     General: shortcuts, push to talk, model settings, sound
  advanced.md                    Advanced: app, output, transcription, history, experimental
  post-processing-page.md        Post Process: provider, key, model, prompts
  debug.md                       Debug: log level, live logs, paste delays, diagnostics
  about.md                       About: language, theme, version, What's New, paths

history/
  the-history-page.md            entries, playback, copy, save, re-transcribe, delete, retention

tray/
  the-tray-menu.md               icon states, menu items, copy last transcript, unload model, quit

integration/
  command-line.md                --toggle-transcription, --cancel, --start-hidden, --no-tray, --debug, signals, headless
  updates.md                     update checks, install, What's New, portable installs

cross-cutting/
  secure-input.md                macOS Secure Input: detection, fallback, warning, recorder refusal
  language-and-translation.md    the language setting, how it resolves per model, translate to English, Chinese scripts
  data-on-disk.md                app data directory, settings store, recordings, logs, models, portable mode
  platform-differences.md        every place macOS, Windows, and Linux behave differently
```

## Coverage

Status is one of `not started`, `drafted`, or `verified`.

| Document | Status |
| --- | --- |
| glossary.md | drafted |
| bug-triage.md | not started |
| verification/ (5 checklists) | not started |
| foundations/triggers-and-shortcuts.md | drafted |
| foundations/audio-capture.md | drafted |
| foundations/models.md | drafted |
| foundations/the-settings-model.md | drafted |
| foundations/windows-and-tray.md | drafted |
| dictation/starting-and-recording.md | drafted |
| dictation/transcribing.md | drafted |
| dictation/pasting.md | drafted |
| dictation/cancelling.md | drafted |
| dictation/live-transcription.md | drafted |
| dictation/post-processing.md | drafted |
| dictation/the-overlay.md | drafted |
| setup/first-launch.md | not started |
| setup/permissions.md | not started |
| setup/choosing-a-model.md | not started |
| models/downloading-a-model.md | not started |
| models/switching-models.md | not started |
| models/the-models-page.md | not started |
| settings/the-settings-window.md | not started |
| settings/shortcut-recorder.md | drafted |
| settings/general.md | not started |
| settings/advanced.md | not started |
| settings/post-processing-page.md | not started |
| settings/debug.md | not started |
| settings/about.md | not started |
| history/the-history-page.md | not started |
| tray/the-tray-menu.md | not started |
| integration/command-line.md | not started |
| integration/updates.md | not started |
| cross-cutting/secure-input.md | not started |
| cross-cutting/language-and-translation.md | not started |
| cross-cutting/data-on-disk.md | not started |
| cross-cutting/platform-differences.md | not started |

## Reference

The source of truth is the Handy repository this directory lives in, at the commit named in each document's footer. The relevant locations are:

- `src-tauri/src/lib.rs`: startup, window creation, tray construction, single-instance handling, the headless path
- `src-tauri/src/transcription_coordinator.rs`: the dictation lifecycle (Idle, Recording, Processing), press debounce, push-to-talk release grace
- `src-tauri/src/actions.rs`: what starting and stopping a dictation does, the transcribe-and-paste pipeline, post-processing
- `src-tauri/src/managers/audio.rs` and `src-tauri/src/audio_toolkit/`: microphone streams, readiness, VAD, levels, mute
- `src-tauri/src/managers/transcription.rs`: model load and unload, batch and streaming transcription, text cleanup
- `src-tauri/src/managers/model.rs`, `src-tauri/src/catalog/`: the model registry, downloads, deletion, capabilities
- `src-tauri/src/managers/history.rs`: history entries and retention
- `src-tauri/src/clipboard.rs`, `src-tauri/src/input.rs`, `src-tauri/src/paste_tx/`: delivery of text
- `src-tauri/src/shortcut/`: shortcut registration, the two keyboard implementations, the shortcut recorder backend, every `change_*_setting` command
- `src-tauri/src/secure_input.rs`: macOS Secure Input monitor and fallback
- `src-tauri/src/overlay.rs`, `src/overlay/RecordingOverlay.tsx`: the overlay window and its UI
- `src-tauri/src/tray.rs`: the menu-bar icon and menu
- `src-tauri/src/settings.rs`: every setting, its default, migrations, salvage
- `src-tauri/src/cli.rs`, `src-tauri/src/signal_handle.rs`: command-line flags and Unix signals
- `src/App.tsx`, `src/components/`: the settings window, onboarding, settings pages, the footer, toasts
- `src/stores/settingsStore.ts`, `src/stores/modelStore.ts`: how the UI keeps settings and models in sync with the backend
- `src/i18n/locales/en/translation.json`: every user-facing string
