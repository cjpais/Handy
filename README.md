
# Handy

**Handy with continuous dictation using voice activity detection (VAD).**

This fork of [Handy](https://github.com/cjpais/Handy) adds continuous dictation through automatic speech segmentation. Instead of requiring the user to start and stop recording manually for every utterance, Handy continuously listens for speech, detects when a segment begins and ends, and sends completed segments through the existing transcription pipeline.

> **Status:** Experimental feature / upstream pull request in progress

## What's Added

### Continuous Dictation

When enabled, Handy:

1. Continuously captures microphone audio.
2. Uses Silero VAD to detect speech.
3. Waits for speech to begin before creating a segment.
4. Keeps collecting audio while speech continues.
5. Ends the segment after a period of silence.
6. Sends the completed segment to Handy's existing transcription system.
7. Processes and pastes the resulting transcription normally.

This allows dictation to feel more like continuous speech input rather than repeated push-to-talk interactions.

## Why?

Handy is already very good at turning speech into text, but traditional push-to-talk dictation requires the user to manually control each recording.

Continuous dictation makes it possible to simply speak naturally while Handy handles the recording boundaries automatically.

## Implementation

The feature builds on Handy's existing audio and transcription infrastructure rather than introducing a separate transcription system.

### Voice Activity Detection

Continuous segmentation uses **Silero VAD** to determine whether incoming audio contains speech.

The segmenter maintains:

* Speech onset detection
* Silence tracking
* Audio buffering
* Segment boundaries
* Automatic segment callbacks

A small amount of audio from the beginning of speech is retained so the beginning of an utterance is not lost when speech is first detected.

### Transcription

Completed segments are passed through Handy's existing `TranscriptionManager`.

The selected Whisper model is loaded when continuous dictation is enabled, including when the feature is enabled while Handy is already running.

## Configuration

Continuous dictation can be enabled from:

**Settings → Debug → Continuous Dictation**

The setting is persisted with Handy's existing settings system.

## Testing

The feature has been tested locally with:

* Normal-volume English speech
* Continuous speech across multiple segments
* Automatic VAD segmentation
* Runtime enabling/disabling of continuous dictation
* Existing Whisper transcription models
* Chinese/English code-switching
* Automatic insertion of transcribed text into the active application

Transcription quality can vary with very quiet speech and multilingual/code-switched audio because those behaviors depend on the underlying Whisper model.

## Upstream

This work is intended as a contribution to the original Handy project:

**https://github.com/cjpais/Handy**

Related discussion:

**Handy Discussion #1896**

Upstream pull request:

**PR #2026**

## Development

This repository follows the development setup and requirements of the upstream Handy project. See the upstream repository and `CONTRIBUTING.md` for build instructions and contribution guidelines.

## License

See the upstream Handy repository for licensing information.
