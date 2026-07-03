# Poptart

**A free, open source AI voice keyboard that works completely offline.**

Poptart is a fork of [Handy](https://github.com/cjpais/Handy) by [CJ Pais](https://github.com/cjpais), extended with premium dictation features inspired by [Wispr Flow](https://wisprflow.ai) — while staying 100% local. Hold a hotkey, speak, and clean, formatted text appears at your cursor in whatever app you're using. No cloud, no subscription, no audio leaving your machine.

## What Poptart adds on top of Handy

Handy provides the excellent core: local speech-to-text (Whisper / Parakeet / more), push-to-talk, VAD, cross-platform text injection, history, and LLM post-processing. Poptart builds Wispr Flow–style features on that foundation:

- **Local AI cleanup by default** — post-processing ships enabled and pointed at a local [Ollama](https://ollama.com) instance (`qwen3:8b`). Filler words, punctuation, and self-corrections are cleaned up on-device out of the box. Any OpenAI-compatible provider still works.
- **Command Mode** — select text in any app, hold the Command Mode hotkey (default `ctrl+option+space`), and speak an instruction like *"make this a bulleted list"* or *"make this more formal"*. The selection is replaced with the edited result. Your clipboard is preserved, and a failed request never destroys your selection.
- **App-context awareness** — the `${app}` prompt variable resolves to the app you're dictating into, so the default prompt matches tone to the target: casual in Slack, formal in Mail. (macOS)
- **Snippets** — say a trigger phrase and it expands to saved text before the AI pass. Say *"my email"*, get your address. Configured in Advanced settings alongside Handy's custom words.

All of Handy's own features (custom dictionary, translation, streaming overlay, multi-model support, etc.) are unchanged.

## Getting started

1. Build from source (see [BUILD.md](BUILD.md)) — requires [Bun](https://bun.sh) and Rust: `bun install && bun tauri build`
2. For local AI cleanup: `brew install ollama && ollama pull qwen3:8b` (or configure any OpenAI-compatible provider in Post Process settings)
3. Launch, grant microphone + accessibility permissions, pick a transcription model
4. Hold `option+space` and talk

## Credits & license

Poptart is built on [Handy](https://github.com/cjpais/Handy) — the vast majority of this codebase is the work of CJ Pais and the Handy contributors, and the full upstream commit history is preserved in this repository. If you want the original, actively-maintained upstream app, get it at [handy.computer](https://handy.computer).

The Handy name, logo, and brand assets are not open source and are not used here; Poptart uses its own name and artwork. This is an unofficial fork and is not endorsed by or affiliated with the Handy project.

MIT License — see [LICENSE](LICENSE). Additional thanks to OpenAI (Whisper), NVIDIA (Parakeet), Silero (VAD), ggml/transcribe.cpp, and the Tauri team.
