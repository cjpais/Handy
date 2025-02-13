# Handy App

A user-friendly desktop application that makes the [Handy voice assistant](https://github.com/cjpais/handy) more accessible and easier to distribute. This app provides a native interface for voice-controlled AI interactions, making it simple for users to get started without dealing with Python setup.

Everything is currently a work in progress. A lot of stuff probably does not work.

## Core Purpose

- 📦 Make Handy easily distributable and installable for everyone
- 🎯 Provide the same powerful voice-controlled AI capabilities as the CLI version
- 🧠 Support flexible LLM configuration (local or remote)
- ⚙️ Offer simple configuration for keyboard shortcuts
- 🪟 Enable floating windows for AI chat interactions

## Key Features

### V1 (Primary Focus)
- 🎙️ Voice-to-text transcription using MLX Whisper
- 🤖 Flexible AI model support:
  - Remote APIs (OpenRouter, OpenAI, Anthropic, etc.)
  - Local models (llama.cpp, MLX models, etc.)
  - Custom API endpoints
- 💬 Floating chat windows for AI interactions
- ⚡ Native desktop app experience
- ⚙️ User-friendly configuration UI

### Future Enhancements
- 📋 Smart clipboard management for context
- 📸 Screenshot capabilities
- 🔄 MCP provider functionality
- 📝 Extended context window management

## Requirements

- macOS (currently macOS-only)
- Internet connection for remote AI services (optional)
- API keys for remote services (optional)

## Quick Start

1. Download the latest release from the releases page
2. Install the application
3. Launch and grant necessary permissions
4. Configure your AI preferences:
   - Use existing environment variables if set
   - Configure through UI:
     - Select local or remote models
     - Add API keys if needed
     - Customize model parameters
5. Start using voice commands with the default shortcuts!

## Environment Variables

The app automatically detects and uses common environment variables:
```bash
# Remote API Keys
OPENAI_API_KEY=your_openai_key
OPENROUTER_API_KEY=your_openrouter_key
ANTHROPIC_API_KEY=your_anthropic_key

# Model Configuration
DEFAULT_MODEL=gpt-4  # Example model choice
MODEL_BASE_URL=http://localhost:8080  # For custom API endpoints
```

## Configuration

The app allows you to configure:
- AI Model Settings:
  - Model provider (local or remote)
  - API endpoints and keys
  - Model parameters (temperature, context length, etc.)
- Keyboard shortcuts
- Window preferences for AI chat
- Transcription settings

## Development Setup

1. Clone the repository:
```bash
git clone [repository-url]
```

2. Install dependencies:
```bash
# Using npm
npm install

# Or using Bun
bun install
```

3. Run the development version:
```bash
npm run tauri dev
# or
bun run tauri dev
```

## Architecture

- **Frontend**: React + TypeScript for the user interface
- **Backend**: Rust-based Tauri for system integration
- **Key Plugins**:
  - `@tauri-apps/plugin-global-shortcut`: Keyboard shortcut management
  - `tauri-plugin-macos-permissions-api`: System permissions
  - `@tauri-apps/plugin-window`: Multi-window management

## Comparison with CLI Version

This desktop app provides the same core functionality as the [original Handy CLI](https://github.com/cjpais/handy) but with these improvements:
- No Python setup required
- Easy installation process
- Native desktop experience
- Configurable through UI
- Floating chat windows
- More flexible LLM configuration
- Future support for visual features

## Contributing

Contributions are welcome! The priority is currently on:
1. Core functionality parity with CLI version
2. Installation and configuration experience
3. Chat window implementation
4. Model configuration interface

## Roadmap

### V1
- [ ] Core voice-to-text functionality
- [ ] Flexible AI model integration
- [ ] Settings configuration
- [ ] Easy installation process
- [ ] Environment variable support

### Future
- [ ] Basic chat windows
- [ ] Clipboard management integration
- [ ] Screenshot capabilities
- [ ] MCP provider functionality
- [ ] Extended context management
- [ ] Multiple chat window layouts

## Related Projects

- [Handy CLI](https://github.com/cjpais/handy) - The original command-line interface version
