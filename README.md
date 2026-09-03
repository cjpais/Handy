# Handy

**Handy with continuous dictation using voice activity detection (VAD).**

This fork of [Handy](https://github.com/cjpais/Handy) adds continuous dictation using Silero VAD. Instead of manually starting and stopping each recording, Handy automatically detects speech, splits it into segments, transcribes each segment, and pastes the result into the active application.

> **Status:** Experimental. This fork contains an implementation currently proposed for upstream Handy in [PR #2026](https://github.com/cjpais/Handy/pull/2026).

## Continuous Dictation

When enabled, Handy continuously monitors microphone input and automatically creates transcription segments based on speech activity.

The process is:

1. Microphone audio is continuously captured.
2. Silero VAD detects when speech begins.
3. Audio is collected while speech continues.
4. Silence marks the end of the segment.
5. The segment is sent to Handy's existing transcription pipeline.
6. The resulting text is processed and pasted into the active application.

## Enable Continuous Dictation

Continuous Dictation is currently located in Handy's **Debug Settings**.

### 1. Enable Debug Mode

Open Handy's settings and enable **Debug Mode**.

Once Debug Mode is enabled, the additional Debug settings will appear.

### 2. Enable Continuous Dictation

Go to:

**Settings → Debug → Continuous Dictation**

Turn **Continuous Dictation** on.

Handy will then continuously monitor the microphone and automatically split speech into transcription segments.

### 3. Start Speaking

Once enabled, simply speak normally.

You do not need to manually start and stop each recording. Handy will:

* Detect when you start speaking
* Continue recording while you speak
* Detect when you stop speaking
* Transcribe the completed segment
* Paste the transcription into the active application

> **Note:** Continuous Dictation is currently an experimental feature and is exposed through Debug Settings while it is being tested.

## Installation

### Download a Release

This fork does not currently provide separate release builds.

For the upstream Handy release, see the [official Handy releases](https://github.com/cjpais/Handy/releases).

To use **this fork's continuous dictation implementation**, build Handy from source using the instructions below.

### Build from Source

#### Requirements

You will need:

* [Rust](https://www.rust-lang.org/tools/install) (latest stable)
* [Bun](https://bun.sh/)
* Tauri's platform-specific prerequisites
* A supported operating system:

  * Windows
  * macOS
  * Linux

See the upstream [`BUILD.md`](https://github.com/cjpais/Handy/blob/main/BUILD.md) for platform-specific dependencies.

#### 1. Clone this repository

```bash
git clone https://github.com/siruignaw-sys/Handy-Continuous-Dictation.git
cd Handy-Continuous-Dictation
```

#### 2. Install dependencies

```bash
bun install
```

#### 3. Start the development build

```bash
bun tauri dev
```

This launches Handy with the continuous dictation implementation.

#### 4. Build a production version

```bash
bun run tauri build
```

The generated application bundles will be placed under:

```text
src-tauri/target/release/bundle/
```

The exact bundle format depends on your operating system.

## How It Works

The continuous dictation feature builds on Handy's existing audio and transcription architecture.

### Voice Activity Detection

Silero VAD is used to identify speech within the continuously captured microphone stream.

The segmenter tracks:

* Speech onset
* Speech duration
* Silence duration
* Audio buffering
* Segment boundaries

A short onset buffer is retained so the beginning of an utterance is not lost when speech is first detected.

### Transcription

Completed segments are passed to Handy's existing `TranscriptionManager`.

The currently selected transcription model is used, and the resulting text follows Handy's normal processing and paste pipeline.

## Testing

The implementation has been tested locally with:

* Normal-volume English speech
* Continuous speech across multiple segments
* Automatic VAD segmentation
* Enabling and disabling continuous dictation while Handy is running
* Whisper transcription models
* Chinese/English code-switching
* Automatic insertion of transcription into the active application

Very quiet speech and heavily code-switched audio can produce less reliable transcription depending on the underlying Whisper model.

## Upstream Contribution

This fork was created to develop and test continuous dictation for Handy.

The original feature proposal is documented in [Handy Discussion #1896](https://github.com/cjpais/Handy/discussions/1896).

The implementation has been submitted upstream as [PR #2026](https://github.com/cjpais/Handy/pull/2026).

## Development

For additional development information, platform-specific dependencies, and build details, see the upstream [`BUILD.md`](https://github.com/cjpais/Handy/blob/main/BUILD.md).

## Credits

This project is based on [Handy](https://github.com/cjpais/Handy) by cjpais and contributors.

All original Handy functionality, dependencies, and licensing remain subject to the upstream project.

## License

See [`LICENSE`](LICENSE) for the license applicable to this repository.
