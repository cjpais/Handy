# Hand verification

The feature documents were written from the code and the tests. This directory is the protocol for checking them against the running product, one observable claim at a time.

## What is here

| File | Covers |
| --- | --- |
| [foundations-and-dictation.md](foundations-and-dictation.md) | `foundations/*` and `dictation/*` |
| [setup-and-models.md](setup-and-models.md) | `setup/*` and `models/*` |
| [settings.md](settings.md) | `settings/*` |
| [history-tray-integration.md](history-tray-integration.md) | `history/*`, `tray/*`, `integration/*` |
| [cross-cutting.md](cross-cutting.md) | `cross-cutting/*` |

Each file has one table per document. Each row is an item with a stable ID (`TRIG-07`, `PASTE-12`), a priority, what it needs (a device or condition), the claim with a link to the document section, the setup, numbered steps, the expected result, and a Result column for the tester. Items that cannot be checked by hand (design questions, things that need a product decision) are listed under each document as "Not checkable by hand".

Priorities: **P1** is an established fact, a claim many documents depend on, or a suspected bug; **P2** is an ordinary claim; **P3** is a number, a color, or a timing.

## How to run a pass

1. Bring up the surface. Either run the installed app (`/Applications/Handy.app`) or, from the repository root, `CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev` (the VAD model must be present: `curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx`). For a clean state quit Handy, move `~/Library/Application Support/com.pais.handy` aside (it holds settings, history, recordings, legacy models, and logs; catalog models live in `~/.cache/huggingface/hub` and can stay), then launch. Restore the directory afterwards. A text editor (TextEdit) with a new document is the standard paste target.
2. Confirm the commit. Every document says `Verified against Handy commit af48dd6`. Run `git rev-parse --short HEAD` in the repository; if it differs, the documents describe a different build and some failures will be drift, not defects. The dev build reports "Handy v0.9.6 (Dev)" in the tray tooltip.
3. Keep the documents open beside Handy. Read the linked section before each item; the item is a summary, the section is the claim.
4. Work through P1 first across all files, then P2, then P3.
5. Record `pass`, `fail`, or `blocked` in the Result column, with a note for anything other than a clean pass. A fail is something the document says that the product does not do; a blocked item could not be run (no device, no network, a prior failure in the way).
6. File every fail in [`bug-triage.md`](../bug-triage.md): if the entry exists, add a Status line quoting the item ID; if not, add an entry with the item ID under "Raised by". A fail is not automatically a product bug; sometimes the document is wrong, and the fix is to the document. Say which in the Status line.
7. When every P1 and P2 item for a document has passed or been filed, change its row in the [coverage table](../README.md#coverage) from `drafted` to `verified`.

## Devices and conditions

- **mac** — any Mac running the build; the default. Items that say nothing else need only this.
- **mic** — a working microphone. The built-in MacBook microphone is the reference; a Bluetooth headset is a separate condition (**bt-mic**) for latency items.
- **usb-mic** — a microphone that can be unplugged mid-recording.
- **streaming-model** — a model tagged "Streaming" downloaded and active (Parakeet Unified EN 0.6B is the default recommendation). **batch-model** — a non-streaming model active (Whisper Medium).
- **no-model** — no model selected (delete the active model, or a fresh state before onboarding completes).
- **llm** — a working post-processing provider with a valid API key; **bad-llm** — the Custom provider pointed at an unreachable port.
- **network-off** — Wi-Fi turned off before the step.
- **secure-input** — Terminal.app with Secure Keyboard Entry enabled (Terminal › Secure Keyboard Entry) left running; disable it to clear.
- **two-monitors** — a second display attached.
- **debug** — debug mode on (Cmd+Shift+D in the settings window).
- **log** — read `~/Library/Logs/com.pais.handy/handy.log` (or the Debug page's Live Logs) to observe a claim that has no on-screen evidence.
- **shell** — a terminal to run `/Applications/Handy.app/Contents/MacOS/Handy --flag` or `kill -USR2 <pid>`.

Traps: the settings window must be visible to see toasts, so hide it only when an item says so; the overlay appears on the monitor under the mouse pointer, not the one with the text editor; push to talk releases are honored 50 ms late, so very short taps test the "ends at once" path rather than a normal dictation; the 30 ms debounce makes double-taps look like single presses.

## Driving the product from a script

Most of Handy has no scriptable surface. What can be driven: the remote-control flags (`--toggle-transcription`, `--toggle-post-process`, `--cancel`) and the Unix signals, which are exact equivalents of a toggle-mode press; the headless `--transcribe-file` path for checking model output on a known WAV; and the log file for timing and decision evidence (every stage of a dictation logs at debug level, which is the default file level). Real shortcut presses, the overlay, the tray menu, and pasting must be watched by hand. Synthetic key events from a script may not reach the shortcut layer and must not be used to test shortcut items.

## Results so far

No pass has been run. Every document is `drafted`. The checklists were written from the documents without watching the product, so their expected results restate the documents' claims and have not themselves been confirmed.
