# Curano AI Dictate

[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge&logo=discord&logoColor=white)](https://discord.com/invite/WVBeWsNXK4)

**A Curano AI fork of Handy, preserving local desktop dictation and adding Curano LiveSTT server-based transcription.**

Curano AI Dictate, also referred to as Curano Dictate, is a Curano AI fork of Handy, a cross-platform desktop speech-to-text application. The fork preserves Handy's local transcription backend and includes Curano LiveSTT support for server-based live transcription.

Press a shortcut, speak, and have your words appear in any text field. Privacy depends on the selected transcription backend: Local mode processes audio on device, while Curano LiveSTT mode sends microphone audio to the configured Curano LiveSTT server for transcription.

## Backend Modes

- **Local model**: Offline, on-device transcription using local STT models such as Whisper or Parakeet. Local transcription remains available in Settings.
- **Curano LiveSTT**: Server-based live transcription using the configured Curano LiveSTT endpoint. In this mode, microphone audio is sent to the configured LiveSTT server.

For LiveSTT architecture and protocol notes, see [docs/livestt-integration-plan.md](docs/livestt-integration-plan.md).
For local LiveSTT protocol testing, see [docs/livestt-mock-server.md](docs/livestt-mock-server.md).

## Attribution

Curano AI Dictate is forked from [Handy](https://github.com/cjpais/Handy) by cjpais. The original project provides the local-first desktop dictation foundation.

## Why Handy?

Handy was created to fill the gap for a truly open source, extensible speech-to-text tool. As stated on [handy.computer](https://handy.computer):

- **Free**: Accessibility tooling belongs in everyone's hands, not behind a paywall
- **Open Source**: Together we can build further. Extend Handy for yourself and contribute to something bigger
- **Private local mode**: With the Local backend, your voice stays on your computer and transcription does not send audio to the cloud
- **Simple**: One tool, one job. Transcribe what you say and put it into a text box

Handy isn't trying to be the best speech-to-text app - it's trying to be the most forkable one.

## How It Works

1. **Press** a configurable keyboard shortcut to start/stop recording (or use push-to-talk mode)
2. **Speak** your words while the shortcut is active
3. **Release** and the selected backend processes your speech
4. **Get** your transcribed text pasted directly into whatever app you're using

With the Local backend, the process is entirely local:

- Silence is filtered using VAD (Voice Activity Detection) with Silero
- Transcription uses your choice of models:
  - **Whisper models** (Small/Medium/Turbo/Large) with GPU acceleration when available
  - **Parakeet V3** - CPU-optimized model with excellent performance and automatic language detection
- Works on Windows, macOS, and Linux

With the Curano LiveSTT backend, recording streams microphone audio to a configured Curano LiveSTT server and pastes final text returned by that server. Partial transcripts are used for overlay/debug display only and are not pasted.

## Curano LiveSTT Usage

LiveSTT is optional. Local model transcription remains available in Settings and keeps Whisper/Parakeet transcription available.

1. Open Settings -> General -> Transcription Backend.
2. Select **Local model** for on-device transcription or **Curano LiveSTT** for server-based transcription.
3. When using LiveSTT, set **Server URL** to the HTTP or HTTPS base URL of the Curano LiveSTT server. The setting may be left empty until you are ready to configure LiveSTT. Production servers should use HTTPS. Plain HTTP is accepted only for localhost testing such as `http://127.0.0.1:8787`. The desktop app derives the WebSocket endpoint from this base URL.
4. Use **Login** with the configured server. The returned access and refresh tokens are kept in memory only and are cleared by **Logout** or app restart.
5. Optionally set a numeric **Consultation ID**. When present, it is sent as `consultation_id` on the LiveSTT WebSocket URL and stored with the history entry metadata.

Normal LiveSTT stop behavior is protocol-based: Curano Dictate stops microphone capture, stops sending audio chunks, sends `stop_record`, waits for `session_ended`, pastes the accumulated final transcript once, then closes the WebSocket transport. Closing the WebSocket is cleanup or cancellation, not the normal stop-recording signal.

## Privacy

Privacy depends on the selected backend:

- **Local model** processes microphone audio locally on this device using local models.
- **Curano LiveSTT** sends microphone audio to the configured Curano LiveSTT server for transcription. Authentication requests are also sent to the configured server.

Curano Dictate does not store LiveSTT JWT access or refresh tokens in settings, local storage, files, logs, or history. LiveSTT username/password inputs are used for login requests and the password field is cleared after login attempts.

## LiveSTT Protocol Notes

The desktop client accepts an HTTP or HTTPS server base URL and connects to:

```text
ws(s)://SERVER/api/ws/live-transcription?token=ACCESS_TOKEN&audio_format=pcm
```

If a consultation ID is configured, the client also includes:

```text
consultation_id=123
```

Desktop LiveSTT audio uses raw PCM binary WebSocket frames: mono, 16 kHz, signed 16-bit little-endian samples, no WAV header, with chunks around 250 ms.

LiveSTT login returns both `access_token` and `refresh_token`. Before a LiveSTT session starts, the app uses the current access token when it is still fresh; if it is near expiry, the app calls `POST /auth/refresh` with `{ "refresh_token": "..." }` and replaces both tokens from the response. If the WebSocket rejects auth during handshake, recording, or finalization, the app attempts up to two bounded refresh/reconnect attempts per LiveSTT session and replays captured PCM for the current recording. If replay itself fails after reconnect, the app aborts the session safely and surfaces the error. If refresh returns unauthorized or forbidden, the tokens are cleared and the user must log in again.

The normal completion command is:

```json
{ "type": "stop_record" }
```

The client keeps the WebSocket open after `stop_record` and waits for:

```json
{ "type": "session_ended", "session_id": 123 }
```

Curano LiveSTT desktop accumulates `final.text` chunks in order and pastes the accumulated final transcript. `partial.text` is kept separately for overlay/debug preview and does not overwrite accumulated final text. If finalization times out, paste uses accumulated final text when available; partial text is never used as pasted fallback.

## LiveSTT Developer Notes

Use the local mock server for protocol testing:

```bash
bun run livestt:mock
```

Then set the LiveSTT server URL in the app to:

```text
http://127.0.0.1:8787
```

The mock login endpoint returns mock access and refresh tokens for any submitted credentials and does not contact Curano services. Do not use real credentials with the mock server.

Completed LiveSTT implementation areas include backend selection, in-memory authentication, WebSocket client, PCM chunk streaming, session manager integration, settings UI, overlay transcript events, mock server support, and history metadata.

Known limitations:

- LiveSTT access and refresh tokens are memory-only and are not persisted across app restarts.
- Desktop LiveSTT currently uses PCM, not WebM/Opus.
- LiveSTT requires a reachable configured server and successful login before recording.
- Partial transcript paste is disabled by default and should remain off unless timeout fallback is explicitly desired.

## Quick Start

### Installation

1. Build Curano AI Dictate from this repository: [elenik72/Curano-AI-Dictate](https://github.com/elenik72/Curano-AI-Dictate)
2. If you specifically want the upstream app instead of this fork, use upstream Handy releases from the [releases page](https://github.com/cjpais/Handy/releases) / [website](https://handy.computer)
   - **macOS**: [Homebrew cask](https://formulae.brew.sh/cask/handy): `brew install --cask handy`
   - **Windows**: [winget](https://github.com/microsoft/winget-pkgs): `winget install cjpais.Handy` \
     **Note:** These Homebrew and winget packages install upstream Handy, not Curano AI Dictate.
3. Install the application
4. Launch Curano Dictate and grant necessary system permissions (microphone, accessibility)
5. Configure your preferred keyboard shortcuts in Settings
6. Start transcribing!

### Development Setup

For detailed build instructions including platform-specific requirements, see [BUILD.md](BUILD.md).

## Integrations

<a href="https://www.raycast.com/mattiacolombomc/handy" title="Install Handy Raycast Extension"><img src="https://www.raycast.com/mattiacolombomc/handy/install_button@2x.png?v=1.1" height="64" style="height: 64px;" alt="Install handy Raycast Extension" /></a>

The upstream Handy Raycast extension can control Handy-compatible builds from [Raycast](https://www.raycast.com) - start/stop recording, browse transcript history, manage dictionary, switch models and languages.

[Source](https://github.com/mattiacolombomc/raycast-handy) · by [@mattiacolombomc](https://github.com/mattiacolombomc)

## Architecture

Curano AI Dictate is built as a Tauri application combining:

- **Frontend**: React + TypeScript with Tailwind CSS for the settings UI
- **Backend**: Rust for system integration, audio processing, and ML inference
- **Core Libraries**:
  - `whisper-rs`: Local speech recognition with Whisper models
  - `transcribe-rs`: CPU-optimized speech recognition with Parakeet models
  - `cpal`: Cross-platform audio I/O
  - `vad-rs`: Voice Activity Detection
  - `rdev`: Global keyboard shortcuts and system events
  - `rubato`: Audio resampling

### Debug Mode

Curano Dictate includes an advanced debug mode for development and troubleshooting. Access it by pressing:

- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

### CLI Parameters

Curano Dictate supports command-line flags for controlling a running instance and customizing startup behavior. These work on all platforms (macOS, Windows, Linux). The packaged app display name is Curano AI Dictate, but the inherited command-line binary name in current examples remains `handy`.

**Remote control flags** (sent to an already-running instance via the single-instance plugin):

```bash
handy --toggle-transcription    # Toggle recording on/off
handy --toggle-post-process     # Toggle recording with post-processing on/off
handy --cancel                  # Cancel the current operation
```

**Startup flags:**

```bash
handy --start-hidden            # Start without showing the main window
handy --no-tray                 # Start without the system tray icon
handy --debug                   # Enable debug mode with verbose logging
handy --help                    # Show all available flags
```

Flags can be combined for autostart scenarios:

```bash
handy --start-hidden --no-tray
```

> **macOS tip:** If you need CLI flags from an installed app bundle, invoke the bundled executable directly. The app bundle display name follows Curano AI Dictate packaging, while the inherited binary/CLI name remains `handy` for now.

## Known Issues & Current Limitations

This project is actively being developed and inherits some [known Handy issues](https://github.com/cjpais/Handy/issues). We believe in transparency about the current state:

### Major Issues (Help Wanted)

**Whisper Model Crashes:**

- Whisper models crash on certain system configurations (Windows and Linux)
- Does not affect all systems - issue is configuration-dependent
  - If you experience crashes and are a developer, please help to fix and provide debug logs!

**Wayland Support (Linux):**

- Limited support for Wayland display server
- Requires [`wtype`](https://github.com/atx/wtype) or [`dotool`](https://sr.ht/~geb/dotool/) for text input to work correctly (see [Linux Notes](#linux-notes) below for installation)

### Linux Notes

**Text Input Tools:**

For reliable text input on Linux, install the appropriate tool for your display server:

| Display Server | Recommended Tool | Install Command                                    |
| -------------- | ---------------- | -------------------------------------------------- |
| X11            | `xdotool`        | `sudo apt install xdotool`                         |
| Wayland        | `wtype`          | `sudo apt install wtype`                           |
| Both           | `dotool`         | `sudo apt install dotool` (requires `input` group) |

- **X11**: Install `xdotool` for both direct typing and clipboard paste shortcuts
- **Wayland**: Install `wtype` (preferred) or `dotool` for text input to work correctly
- **dotool setup**: Requires adding your user to the `input` group: `sudo usermod -aG input $USER` (then log out and back in)

Without these tools, Curano Dictate falls back to enigo which may have limited compatibility, especially on Wayland.

**Other Notes:**

- **Runtime library dependency (`libgtk-layer-shell.so.0`)**:
  - Curano Dictate links `gtk-layer-shell` on Linux. If startup fails with `error while loading shared libraries: libgtk-layer-shell.so.0`, install the runtime package for your distro:

    | Distro        | Package to install    | Example command                        |
    | ------------- | --------------------- | -------------------------------------- |
    | Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
    | Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
    | Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

  - For building from source on Ubuntu/Debian, you may also need `libgtk-layer-shell-dev`.

- The recording overlay is disabled by default on Linux (`Overlay Position: None`) because certain compositors treat it as the active window. When the overlay is visible it can steal focus, which prevents Curano Dictate from pasting back into the application that triggered transcription. If you enable the overlay anyway, be aware that clipboard-based pasting might fail or end up in the wrong window.
- If you are having trouble with the app, running with the environment variable `WEBKIT_DISABLE_DMABUF_RENDERER=1` may help
- If Curano Dictate fails to start reliably on Linux, see [Troubleshooting -> Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- **Global keyboard shortcuts (Wayland):** On Wayland, system-level shortcuts must be configured through your desktop environment or window manager. Use the [CLI flags](#cli-parameters) as the command for your custom shortcut.

  **GNOME:**
  1. Open **Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts**
  2. Click the **+** button to add a new shortcut
  3. Set the **Name** to `Toggle Curano Dictate Transcription`
  4. Set the **Command** to `handy --toggle-transcription`
  5. Click **Set Shortcut** and press your desired key combination (e.g., `Super+O`)

  **KDE Plasma:**
  1. Open **System Settings > Shortcuts > Custom Shortcuts**
  2. Click **Edit > New > Global Shortcut > Command/URL**
  3. Name it `Toggle Curano Dictate Transcription`
  4. In the **Trigger** tab, set your desired key combination
  5. In the **Action** tab, set the command to `handy --toggle-transcription`

  **Sway / i3:**

  Add to your config file (`~/.config/sway/config` or `~/.config/i3/config`):

  ```ini
  bindsym $mod+o exec handy --toggle-transcription
  ```

  **Hyprland:**

  Add to your config file (`~/.config/hypr/hyprland.conf`):

  ```ini
  bind = $mainMod, O, exec, handy --toggle-transcription
  ```

- You can also manage global shortcuts outside of Curano Dictate via Unix signals, which lets Wayland window managers or other hotkey daemons keep ownership of keybindings:

  | Signal    | Action                                    | Example                |
  | --------- | ----------------------------------------- | ---------------------- |
  | `SIGUSR2` | Toggle transcription                      | `pkill -USR2 -n handy` |
  | `SIGUSR1` | Toggle transcription with post-processing | `pkill -USR1 -n handy` |

  Example Sway config:

  ```ini
  bindsym $mod+o exec pkill -USR2 -n handy
  bindsym $mod+p exec pkill -USR1 -n handy
  ```

  `pkill` here simply delivers the signal - it does not terminate the process.

### Platform Support

- **macOS (both Intel and Apple Silicon)**
- **x64 Windows**
- **x64 Linux**

### System Requirements/Recommendations

The following are recommendations for running Curano Dictate on your own machine. If you don't meet the system requirements, the performance of the application may be degraded. We are working on improving the performance across all kinds of computers and hardware.

**For Whisper Models:**

- **macOS**: M series Mac, Intel Mac
- **Windows**: Intel, AMD, or NVIDIA GPU
- **Linux**: Intel, AMD, or NVIDIA GPU
  - Ubuntu 22.04, 24.04

**For Parakeet V3 Model:**

- **CPU-only operation** - runs on a wide variety of hardware
- **Minimum**: Intel Skylake (6th gen) or equivalent AMD processors
- **Performance**: ~5x real-time speed on mid-range hardware (tested on i5)
- **Automatic language detection** - no manual language selection required

## Roadmap & Active Development

We're actively working on several features and improvements. Contributions and feedback are welcome!

### In Progress

**Debug Logging:**

- Adding debug logging to a file to help diagnose issues

**macOS Keyboard Improvements:**

- Support for Globe key as transcription trigger
- A rewrite of global shortcut handling for MacOS, and potentially other OS's too.

**Opt-in Analytics:**

- Collect anonymous usage data to help improve the app
- Privacy-first approach with clear opt-in

**Settings Refactoring:**

- Cleanup and refactor settings system which is becoming bloated and messy
- Implement better abstractions for settings management

**Tauri Commands Cleanup:**

- Abstract and organize Tauri command patterns
- Investigate tauri-specta for improved type safety and organization

## Verify Release Signatures

This fork does not yet define its own Curano release-signing or updater channel.

`src-tauri/tauri.conf.json` still contains inherited upstream Handy updater metadata (`plugins.updater.pubkey` and `plugins.updater.endpoints`). Treat that metadata as upstream Handy configuration, not as Curano AI Dictate release verification material.

To avoid shared-updater confusion, update checks are disabled by default in this fork until Curano-specific release infrastructure is configured.

Do not use `gpg` for these `.sig` files.

## Troubleshooting

### Manual Model Installation (For Proxy Users or Network Restrictions)

If you're behind a proxy, firewall, or in a restricted network environment where Curano Dictate cannot download local models automatically, you can manually download and install them. The URLs are publicly accessible from any browser.

#### Step 1: Find Your App Data Directory

1. Open Curano Dictate settings
2. Navigate to the **About** section
3. Copy the "App Data Directory" path shown there, or use the shortcuts:
   - **macOS**: `Cmd+Shift+D` to open debug menu
   - **Windows/Linux**: `Ctrl+Shift+D` to open debug menu

The typical paths are currently:

- **macOS**: `~/Library/Application Support/com.pais.handy/`
- **Windows**: `C:\Users\{username}\AppData\Roaming\com.pais.handy\`
- **Linux**: `~/.config/com.pais.handy/`

These paths still use the inherited upstream Handy identifier namespace. A separate Curano production data directory needs a dedicated migration plan before it can be changed safely.

#### Step 2: Create Models Directory

Inside your app data directory, create a `models` folder if it doesn't already exist:

```bash
# macOS/Linux
mkdir -p ~/Library/Application\ Support/com.pais.handy/models

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path "$env:APPDATA\com.pais.handy\models"
```

#### Step 3: Download Model Files

Download the models you want from below

**Whisper Models (single .bin files):**

- Small (487 MB): `https://blob.handy.computer/ggml-small.bin`
- Medium (492 MB): `https://blob.handy.computer/whisper-medium-q4_1.bin`
- Turbo (1600 MB): `https://blob.handy.computer/ggml-large-v3-turbo.bin`
- Large (1100 MB): `https://blob.handy.computer/ggml-large-v3-q5_0.bin`

**Parakeet Models (compressed archives):**

- V2 (473 MB): `https://blob.handy.computer/parakeet-v2-int8.tar.gz`
- V3 (478 MB): `https://blob.handy.computer/parakeet-v3-int8.tar.gz`

#### Step 4: Install Models

**For Whisper Models (.bin files):**

Simply place the `.bin` file directly into the `models` directory:

```
{app_data_dir}/models/
├── ggml-small.bin
├── whisper-medium-q4_1.bin
├── ggml-large-v3-turbo.bin
└── ggml-large-v3-q5_0.bin
```

**For Parakeet Models (.tar.gz archives):**

1. Extract the `.tar.gz` file
2. Place the **extracted directory** into the `models` folder
3. The directory must be named exactly as follows:
   - **Parakeet V2**: `parakeet-tdt-0.6b-v2-int8`
   - **Parakeet V3**: `parakeet-tdt-0.6b-v3-int8`

Final structure should look like:

```
{app_data_dir}/models/
├── parakeet-tdt-0.6b-v2-int8/     (directory with model files inside)
│   ├── (model files)
│   └── (config files)
└── parakeet-tdt-0.6b-v3-int8/     (directory with model files inside)
    ├── (model files)
    └── (config files)
```

**Important Notes:**

- For Parakeet models, the extracted directory name **must** match exactly as shown above
- Do not rename the `.bin` files for Whisper models - use the exact filenames from the download URLs
- After placing the files, restart Curano Dictate to detect the new models

#### Step 5: Verify Installation

1. Restart Curano Dictate
2. Open Settings → Models
3. Your manually installed models should now appear as "Downloaded"
4. Select the model you want to use and test transcription

### Custom Whisper Models

Curano Dictate can auto-discover custom Whisper GGML models placed in the `models` directory. This is useful for users who want to use fine-tuned or community models not included in the default model list.

**How to use:**

1. Obtain a Whisper model in GGML `.bin` format (e.g., from [Hugging Face](https://huggingface.co/models?search=whisper%20ggml))
2. Place the `.bin` file in your `models` directory (see paths above)
3. Restart Curano Dictate to discover the new model
4. The model will appear in the "Custom Models" section of the Models settings page

**Important:**

- Community models are user-provided and may not receive troubleshooting assistance
- The model must be a valid Whisper GGML format (`.bin` file)
- Model name is derived from the filename (e.g., `my-custom-model.bin` → "My Custom Model")

### Linux Startup Crashes or Instability

If Curano Dictate fails to start reliably on Linux - for example, it crashes shortly after launch, never shows its window, or reports a Wayland protocol error - try the steps below in order.

**1. Install (or reinstall) `gtk-layer-shell`**

Curano Dictate uses `gtk-layer-shell` for its recording overlay and links against it at runtime. A missing or broken installation is the most common cause of startup failures and can manifest as a crash or a hang well before any window is shown. Make sure the runtime package is installed for your distro:

| Distro        | Package to install    | Example command                        |
| ------------- | --------------------- | -------------------------------------- |
| Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
| Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
| Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

If it is already installed and you still see startup problems, try reinstalling it (e.g. `sudo pacman -S gtk-layer-shell` again) in case the library files were corrupted by a partial upgrade.

**2. Disable the GTK layer shell overlay (`HANDY_NO_GTK_LAYER_SHELL`)**

If installing the library does not help, you can skip `gtk-layer-shell` initialization entirely as a workaround. On some compositors (notably KDE Plasma under Wayland) it has been reported to interact poorly with the recording overlay. With this variable set, the overlay falls back to a regular always-on-top window:

```bash
HANDY_NO_GTK_LAYER_SHELL=1 handy
```

**3. Disable WebKit DMA-BUF renderer (`WEBKIT_DISABLE_DMABUF_RENDERER`)**

On some GPU/driver combinations the WebKitGTK DMA-BUF renderer can cause the window to fail to render or to crash. Try:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 handy
```

**Making a workaround permanent**

Once you've found a flag that helps, export it from your shell profile (`~/.bashrc`, `~/.zshenv`, …) or from the desktop autostart entry that launches Curano Dictate. If you launch Curano Dictate from a `.desktop` file, you can prefix the `Exec=` line, e.g.:

```ini
Exec=env HANDY_NO_GTK_LAYER_SHELL=1 handy
```

If a workaround helps you, please [open an issue](https://github.com/cjpais/Handy/issues) describing your distro, desktop environment, and session type - that information helps us narrow down the underlying bug.

### How to Contribute

1. **Check existing issues** at [github.com/cjpais/Handy/issues](https://github.com/cjpais/Handy/issues)
2. **Fork the repository** and create a feature branch
3. **Test thoroughly** on your target platform
4. **Submit a pull request** with clear description of changes
5. **Join the discussion** - reach out at [contact@handy.computer](mailto:contact@handy.computer)

The goal is to create both a useful tool and a foundation for others to build upon - a well-patterned, simple codebase that serves the community.

## Sponsors

<div align="center">
  We're grateful for the support of our sponsors who help make Handy possible:
  <br><br>
  <a href="https://wordcab.com">
    <img src="sponsor-images/wordcab.png" alt="Wordcab" width="120" height="120">
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://github.com/epicenter-so/epicenter">
    <img src="sponsor-images/epicenter.png" alt="Epicenter" width="120" height="120">
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://boltai.com?utm_source=handy">
    <img src="sponsor-images/boltai.jpg" alt="Bolt AI" width="120" height="120">
  </a>
</div>

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
