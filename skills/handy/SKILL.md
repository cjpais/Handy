---
name: handy
description: Transcribe WAV files and list models with the Handy speech-to-text CLI, headlessly from the terminal.
---

# Handy CLI (headless transcription)

Handy is a speech-to-text app. Its binary also works as a headless CLI: it can
transcribe a WAV file using an already-installed model, without any GUI.

## 1. Locating the binary

Check if `handy` is on PATH:

```bash
handy --help
```

If that works, use `handy` below. If not, invoke the binary at its install
location:

- macOS: `/Applications/Handy.app/Contents/MacOS/Handy`
- Linux (deb/RPM): `/usr/bin/handy`
- Linux (AUR): `/usr/bin/handy`

A symlink at `~/.local/bin/handy` can be created from the app's settings; older
installs may not have it, hence the full-path fallback above.

## 2. Check installed models

Models must already be installed through the app GUI; the CLI never downloads.

```bash
handy --list-models            # human-readable
handy --list-models --json     # machine-readable; ids usable with --model
```

## 3. Transcribe a WAV file

```bash
handy --transcribe-file meeting.wav             # transcription to stdout
handy --transcribe-file meeting.wav -o out.txt  # write to a file
handy --transcribe-file meeting.wav --json      # JSON output
handy --transcribe-file m.wav --model <id>      # pick a specific model
```

Optional: `--device-index <N>` hard-selects a compute device (see
`--list-devices`); transcribe-cpp (whisper-family) models only. `--repeat <N>`
runs the transcription N times and reports `best_ms` (the fastest run) for
benchmarking.

## Constraints

- Input must be a WAV file (16 kHz mono recommended). No other formats are
  decoded yet; convert with `ffmpeg -i in.mp3 -ar 16000 -ac 1 out.wav` if needed.
- The headless run starts its own instance; it does not forward to a running
  app, and both can coexist.
- Nonzero exit code on failure (missing model, unreadable file, bad flags).
  Check stderr for the message.
