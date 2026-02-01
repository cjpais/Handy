# Handy

[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge&logo=discord&logoColor=white)]([https://discord.gg/your-invite-link](https://discord.gg/WVBeWsNXK4))

**A free, open source, and extensible speech-to-text application that works completely offline.**

Handy is a cross-platform desktop application built with Tauri (Rust + React/TypeScript) that provides simple, privacy-focused speech transcription. Press a shortcut, speak, and have your words appear in any text field—all without sending your voice to the cloud.

## Why Handy?

Handy was created to fill the gap for a truly open source, extensible speech-to-text tool. As stated on [handy.computer](https://handy.computer):

- **Free**: Accessibility tooling belongs in everyone's hands, not behind a paywall
- **Open Source**: Together we can build further. Extend Handy for yourself and contribute to something bigger
- **Private**: Your voice stays on your computer. Get transcriptions without sending audio to the cloud
- **Simple**: One tool, one job. Transcribe what you say and put it into a text box

Handy isn't trying to be the best speech-to-text app—it's trying to be the most forkable one.

## How It Works

1. **Press** a configurable keyboard shortcut to start/stop recording (or use push-to-talk mode)
2. **Speak** your words while the shortcut is active
3. **Release** and Handy processes your speech using Whisper
4. **Get** your transcribed text pasted directly into whatever app you're using

The process is entirely local:
- Silence is filtered using VAD (Voice Activity Detection) with Silero
- Transcription uses Whisper Small model with GPU acceleration when available
- Works on Windows, macOS, and Linux

## Quick Start

### Installation

1. Download the latest release from the [releases page](https://github.com/cjpais/Handy/releases) or the [website](https://handy.computer)
2. Install the application following platform-specific instructions
3. Launch Handy and grant necessary system permissions (microphone, accessibility)
4. Configure your preferred keyboard shortcuts in Settings
5. Start transcribing!

### Development Setup

**Prerequisites:**
- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/) package manager
- Platform-specific requirements:
  - **macOS**: Xcode Command Line Tools
  - **Windows**: Microsoft C++ Build Tools
  - **Linux**: Build essentials, ALSA development libraries

**Getting Started:**

```bash
# Clone the repository
git clone git@github.com:cjpais/Handy.git
cd Handy

# Install dependencies
bun install

# Run in development mode
bun run tauri dev
# if it fails with cmake error on MacOS, try
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev

# Build for production
bun run tauri build
```

**Model Files Setup:**

For development, you need to download the required model files:

1. Create the models directory inside the resources folder:
   ```bash
   mkdir -p src-tauri/resources/models
   ```

2. Download the required VAD model for development:
   ```bash
   # Download Silero VAD model (required for voice activity detection)
   curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
   ```

   **Note:** Whisper models are no longer bundled with the app. Users will download their preferred model (Small, Medium, Turbo, or Large) from within the app on first run.

**Whisper Models:**

The app now supports dynamic model downloading and switching:
- **Small**: Fast, good for most use cases
- **Medium**: Better accuracy, balanced performance
- **Turbo**: Optimized large model with improved speed
- **Large**: Highest accuracy, slower processing

Users can download and switch between models directly from the app's settings interface. No models are bundled with the app, reducing the initial download size.

**Qwen3-ASR (Apple Silicon only):**

Handy also supports [Qwen3-ASR-0.6B](https://huggingface.co/mlx-community/Qwen3-ASR-0.6B-8bit) via mlx-audio for native Apple Silicon inference. This requires `uv` (`brew install uv`) and is set up automatically from the model selector. See the [Qwen3-ASR Setup Guide](docs/qwen3-asr-setup.md) for details and troubleshooting.

**Language Support:**

Handy supports multilingual transcription with special handling for Chinese users:
- **Auto**: Automatically detects the spoken language
- **Auto (Prefer Trad. Chinese)**: Auto-detects English/Chinese, outputs Chinese as Traditional characters
- **Chinese Traditional**: Forces Traditional Chinese output (繁體中文)
- **Chinese Simplified**: Forces Simplified Chinese output (简体中文)
- **50+ other languages**: Including Spanish, French, Japanese, Korean, and more

The "Auto (Prefer Trad. Chinese)" option is ideal for bilingual English/Chinese speakers who want automatic language detection while ensuring Chinese text always appears in Traditional characters.

## Architecture

Handy is built as a Tauri application combining:

- **Frontend**: React + TypeScript with Tailwind CSS for the settings UI
- **Backend**: Rust for system integration, audio processing, and ML inference
- **Core Libraries**:
  - `whisper-rs`: Local speech recognition with Whisper models
  - `cpal`: Cross-platform audio I/O
  - `vad-rs`: Voice Activity Detection
  - `rdev`: Global keyboard shortcuts and system events
  - `rubato`: Audio resampling

## Known Issues & Current Limitations

This project is actively being developed and has some [known issues](https://github.com/cjpais/Handy/issues). We believe in transparency about the current state:

### Platform Support
- **Apple Silicon Macs**
- **x64 Windows**
- **x64 Linux**

### Active Issues
- Paste functionality occasionally produces just 'v' instead of full text on macOS
- VAD filter sometimes includes trailing "thank you" in transcriptions
- Transcription end-cutting due to potential threading issues
- Microphone remains active for optimal latency (design choice under discussion)

## Contributing

We're actively seeking contributors! Priority areas include:

### High Priority
1. **Cross-platform support** - Windows and Linux compatibility
2. **Code quality improvements** - Better error handling, architecture refinements
3. **Bug fixes** - Address the known issues listed above
4. **Performance optimization** - Reduce latency, improve resource usage

### Feature Requests
- Configurable microphone selection
- Multiple STT model options (beyond Whisper Small)
- Modifier-only key bindings
- Enhanced VAD configuration

### How to Contribute

1. **Check existing issues** at [github.com/cjpais/Handy/issues](https://github.com/cjpais/Handy/issues)
2. **Fork the repository** and create a feature branch
3. **Test thoroughly** on your target platform
4. **Submit a pull request** with clear description of changes
5. **Join the discussion** - reach out at [contact@handy.computer](mailto:contact@handy.computer)

The goal is to create both a useful tool and a foundation for others to build upon—a well-patterned, simple codebase that serves the community.

## Related Projects

- **[Handy CLI](https://github.com/cjpais/handy-cli)** - The original Python command-line version
- **[handy.computer](https://handy.computer)** - Project website with demos and documentation

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Acknowledgments

- **Whisper** by OpenAI for the speech recognition model
- **whisper.cpp and ggml** for amazing cross-platform whisper inference/acceleration
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework
- **Community contributors** helping make Handy better

---

*"Your search for the right speech-to-text tool can end here—not because Handy is perfect, but because you can make it perfect for you."*
