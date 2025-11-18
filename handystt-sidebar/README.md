# HandySTT Chromium Sidebar Extension

Local, private speech-to-text in your browser's right sidebar - powered by HandySTT.

![HandySTT Sidebar](https://img.shields.io/badge/status-beta-yellow)
![Manifest V3](https://img.shields.io/badge/manifest-v3-blue)
![Chrome](https://img.shields.io/badge/chrome-compatible-green)

## Overview

HandySTT Sidebar integrates the HandySTT desktop application into Chromium's side panel, providing persistent, cross-tab voice-to-text input that works with any web page or AI chat interface.

### Features

✅ **Right-side sidebar** - Always accessible, never intrusive
✅ **Local processing** - CPU-only STT models (Parakeet V3, Whisper)
✅ **Privacy first** - No cloud services, all processing on device
✅ **AI integration** - Direct routing to Claude, ChatGPT, Gemini
✅ **Paste anywhere** - Send transcription to any active text field
✅ **History tracking** - Keep last 50 transcriptions
✅ **Cross-tab persistent** - Works across all browser tabs
✅ **Native messaging** - Communicates with HandySTT desktop app

## Architecture

```
┌─────────────────────────────────────────────┐
│  Chromium Browser                           │
│  ┌──────────────────┬───────────────────┐  │
│  │                  │  HandySTT Sidebar │  │
│  │  Web Content     │  ┌──────────────┐ │  │
│  │                  │  │  🎤 Record   │ │  │
│  │                  │  │  📝 Text     │ │  │
│  │                  │  │  📋 Paste    │ │  │
│  │                  │  │  🤖 Send AI  │ │  │
│  │                  │  └──────────────┘ │  │
│  └──────────────────┴───────────────────┘  │
│              ▲                              │
│              │ Native Messaging             │
└──────────────┼──────────────────────────────┘
               │
               ▼
┌──────────────────────────────────────────────┐
│  HandySTT Desktop App                        │
│  • Audio capture                             │
│  • STT processing (Parakeet/Whisper)         │
│  • Native messaging host                     │
└──────────────────────────────────────────────┘
```

## Requirements

### Software
- **Chromium-based browser** (Chrome, Edge, Brave, etc.) - Version 114+
- **HandySTT desktop app** - Latest version with native messaging support
- **Operating System**:
  - Linux (x64)
  - macOS (10.13+)
  - Windows (10/11)

### Hardware
- Microphone (built-in or external)
- 4GB RAM minimum (8GB recommended for larger models)
- CPU with AVX2 support (for optimal Parakeet performance)

## Installation

### 1. Install HandySTT Desktop App

First, ensure you have the HandySTT desktop application installed and working:

```bash
# Download from GitHub releases
https://github.com/yourusername/HandySTT/releases

# Or build from source
git clone https://github.com/yourusername/HandySTT
cd HandySTT
cargo build --release
```

### 2. Install Chrome Extension

#### Option A: Load Unpacked (Development)

1. Open Chrome and navigate to `chrome://extensions/`
2. Enable **Developer mode** (toggle in top-right)
3. Click **Load unpacked**
4. Select the `handystt-sidebar` directory
5. Note the **Extension ID** (e.g., `abcdefghijklmnopqrstuvwxyz123456`)

#### Option B: Install from Chrome Web Store (Coming Soon)

Extension will be published to the Chrome Web Store after beta testing.

### 3. Set Up Native Messaging Host

The extension needs to communicate with the HandySTT desktop app via native messaging.

See **[INSTALL.md](INSTALL.md)** for detailed platform-specific instructions.

**Quick Start:**

**Linux:**
```bash
cd handystt-sidebar/native-host
chmod +x install-linux.sh
./install-linux.sh
```

**macOS:**
```bash
cd handystt-sidebar/native-host
chmod +x install-macos.sh
./install-macos.sh
```

**Windows (PowerShell as Administrator):**
```powershell
cd handystt-sidebar\native-host
powershell -ExecutionPolicy Bypass -File install-windows.ps1
```

### 4. Restart Browser

Close and reopen Chrome/Chromium for the native messaging host to be recognized.

## Usage

### Opening the Sidebar

- Click the HandySTT extension icon in the toolbar
- Or right-click anywhere → "Open HandySTT Sidebar"
- Or use keyboard shortcut (customize in `chrome://extensions/shortcuts`)

### Recording Transcription

1. Click **"Start Recording"** in the sidebar
2. Speak clearly into your microphone
3. Click **"Stop Recording"** when finished
4. Transcription appears in the text area

### Pasting to Active Field

1. Click into any text input field on the page
2. Click **"📋 Paste to Active Field"** in sidebar
3. Text is inserted at cursor position

### Sending to AI

1. Select AI target from dropdown (Claude, ChatGPT, Gemini, Custom)
2. Click **"🤖 Send to AI"**
3. Extension opens/focuses the AI tab and injects your text

### Using History

- Previous transcriptions are saved automatically
- Click any history item to reload it
- Clear history with **"Clear"** button

### Configuring Settings

1. Click ⚙️ settings icon
2. Configure:
   - Auto-paste after transcription
   - Auto-send to AI
   - STT model selection
   - Custom AI URL

## Configuration

### STT Models

The extension supports multiple STT models via HandySTT:

| Model | Size | Speed | Accuracy | CPU Only |
|-------|------|-------|----------|----------|
| Parakeet V3 | Small | Fast | Good | ✅ |
| Whisper Small | Medium | Medium | Better | ✅ |
| Whisper Medium | Large | Slower | Best | ✅ |
| Whisper Turbo | Medium | Fast | Best | ⚠️ |

### Custom AI URLs

To add custom AI endpoints:

1. Open Settings
2. Select "Custom..." from AI target dropdown
3. Enter full URL (e.g., `https://your-ai.com/chat`)
4. Extension will attempt to inject text into the page

## Troubleshooting

### "Cannot connect to Handy" Error

**Symptoms:** Red "Disconnected" status, recording button disabled

**Solutions:**
1. Verify HandySTT desktop app is running
2. Check native messaging host is installed:
   - Linux: `~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json`
   - macOS: `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.pais.handy.host.json`
   - Windows: Check registry key `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host`
3. Verify extension ID in manifest matches your extension
4. Restart browser
5. Check HandySTT logs for errors

### "No microphone detected" Error

**Solutions:**
1. Grant microphone permission to HandySTT desktop app
2. Check system audio settings
3. Verify microphone is not in use by another app
4. Restart HandySTT desktop app

### Paste Not Working

**Solutions:**
1. Ensure a text field is focused (click into it first)
2. Some sites block programmatic text insertion - try clipboard instead
3. Check browser console for errors (`Ctrl+Shift+I` → Console)

### AI Injection Not Working

**Solutions:**
1. AI sites change their HTML frequently - selectors may need updates
2. Fallback: Extension copies to clipboard - paste manually
3. Try "Custom" AI URL with your specific endpoint

### Extension Not Loading

**Solutions:**
1. Check `chrome://extensions/` for errors
2. Verify all files are present in extension directory
3. Disable conflicting extensions
4. Clear extension cache and reload

## Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/yourusername/HandySTT
cd HandySTT/handystt-sidebar

# No build step needed - pure HTML/CSS/JS
# Just load unpacked in Chrome
```

### File Structure

```
handystt-sidebar/
├── manifest.json          # Extension manifest (Manifest V3)
├── sidepanel.html         # Sidebar UI
├── sidepanel.js           # Main logic
├── sidepanel.css          # Styling
├── background.js          # Service worker
├── icons/                 # Extension icons
│   ├── icon16.png
│   ├── icon48.png
│   └── icon128.png
├── native-host/           # Native messaging setup
│   ├── com.pais.handy.host.json
│   ├── install-linux.sh
│   ├── install-macos.sh
│   └── install-windows.ps1
├── README.md              # This file
└── INSTALL.md             # Installation guide
```

### Testing

1. Load unpacked extension in Chrome
2. Open DevTools for sidebar: Right-click sidebar → "Inspect"
3. View background service worker logs: `chrome://serviceworker-internals/`
4. Test native messaging: Check HandySTT app logs

### Contributing

Contributions welcome! Please see [CONTRIBUTING.md](../CONTRIBUTING.md) in the main repository.

## Privacy & Security

- **Local processing**: All STT happens on your device
- **No telemetry**: Extension does not send any data externally
- **Native messaging**: Secure stdio-based communication with desktop app
- **Minimal permissions**: Only requests necessary Chrome APIs
- **Open source**: Audit the code yourself

## Permissions Explained

| Permission | Why Needed |
|------------|------------|
| `sidePanel` | Display UI in browser sidebar |
| `storage` | Save settings and history locally |
| `activeTab` | Paste text into current tab |
| `scripting` | Inject text into AI chat pages |
| `nativeMessaging` | Communicate with HandySTT app |
| `<all_urls>` | Support pasting/AI injection on any site |

## Roadmap

### Phase 1 (Current)
- [x] Basic sidebar UI
- [x] Native messaging to HandySTT
- [x] Recording and transcription
- [x] Paste to active field
- [x] Send to AI (Claude, ChatGPT, Gemini)
- [x] History management

### Phase 2 (Planned)
- [ ] Push-to-talk mode (hold key to record)
- [ ] Voice commands ("send to Claude", "paste")
- [ ] Keyboard shortcuts customization
- [ ] Export history (JSON/CSV)
- [ ] Streaming transcription (real-time display)

### Phase 3 (Future)
- [ ] MultAi integration (route to specific models)
- [ ] Multi-language UI support
- [ ] Custom AI endpoint templates
- [ ] Sync history across devices (optional)
- [ ] Browser action toolbar controls

## License

MIT License - see [LICENSE](../LICENSE) in main repository

## Credits

- **HandySTT** by [cjpais](https://github.com/cjpais)
- **Chrome Side Panel API** by Google
- **STT Models**: NVIDIA Parakeet, OpenAI Whisper

## Links

- **GitHub**: https://github.com/yourusername/HandySTT
- **Issues**: https://github.com/yourusername/HandySTT/issues
- **Discord**: [Join community]()
- **Documentation**: [Full docs]()

---

**Made with ❤️ for privacy-conscious power users.**
