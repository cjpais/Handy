# Qwen3-ASR 0.6B Setup Guide

Handy supports [Qwen3-ASR-0.6B](https://huggingface.co/mlx-community/Qwen3-ASR-0.6B-8bit) as an alternative speech-to-text backend via [mlx-audio](https://github.com/Blaizzy/mlx-audio), running natively on Apple Silicon.

## Prerequisites

- **macOS with Apple Silicon** (M1/M2/M3/M4)
- **[uv](https://docs.astral.sh/uv/)** — fast Python package manager
  ```bash
  brew install uv
  ```

No system Python installation is required. Handy uses `uv` to create a self-contained Python 3.11 virtual environment automatically.

## How It Works

When you click **"Qwen3 ASR 0.6B → Setup"** in the model selector, Handy:

1. Locates `uv` on your system (checks `/opt/homebrew/bin/uv`, `/usr/local/bin/uv`, or resolves via login shell)
2. Creates an isolated Python 3.11 venv at `~/Library/Application Support/com.handy.app/qwen-asr-venv/`
3. Installs `mlx-audio` (from GitHub main branch) into the venv
4. Verifies the installation by importing `mlx_audio`
5. On first transcription, downloads the `mlx-community/Qwen3-ASR-0.6B-8bit` model from HuggingFace

A Python sidecar process (`qwen_asr_sidecar.py`) is spawned using the venv's Python and communicates with the Rust backend via stdin/stdout JSON protocol.

## Building from Source

```bash
# Install dependencies
bun install

# Build (use CMAKE_POLICY_VERSION_MINIMUM if you hit cmake errors on macOS)
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri build

# The app bundle is at:
# src-tauri/target/release/bundle/macos/Handy.app
```

### Installing the Build

```bash
# Remove old installation first (quit Handy if running)
rm -rf /Applications/Handy.app

# Copy the new build
cp -R src-tauri/target/release/bundle/macos/Handy.app /Applications/Handy.app
```

## Troubleshooting

### `xattr -cr` error during `tauri build`

```
failed to bundle project: failed to remove extra attributes from app bundle: `failed to run xattr`
```

**Cause:** The pip-installed `xattr` Python package (which has a different CLI interface) shadows the system `/usr/bin/xattr` via pyenv shims. Tauri's bundler calls `xattr -cr` which the pip version doesn't support.

**Fix:** Uninstall the pip `xattr` package:
```bash
pip3 uninstall xattr
pyenv rehash  # if using pyenv
```

### "mlx-audio is not installed" even after setup succeeds

**Cause:** The PyPI release of mlx-audio (v0.2.10 as of Feb 2026) does not include Qwen3-ASR support. Only the unreleased v0.3.1+ on GitHub main has the `mlx_audio.stt.load` function needed for Qwen3-ASR.

**Fix:** Handy now installs mlx-audio from the GitHub main branch:
```
git+https://github.com/Blaizzy/mlx-audio.git
```
This is handled automatically during setup. If you need to manually fix an existing venv:
```bash
uv pip install "mlx-audio @ git+https://github.com/Blaizzy/mlx-audio.git" \
  --python ~/Library/Application\ Support/com.handy.app/qwen-asr-venv/bin/python3
```

### "cannot import name 'load' from 'mlx_audio.stt'"

**Cause:** Same as above — the installed mlx-audio version is too old.

**Fix:** Same as above — install from GitHub main branch.

### `mlx_audio.__version__` AttributeError

**Cause:** The `mlx_audio` package does not expose a `__version__` attribute. Earlier code checked installation by running `import mlx_audio; print(mlx_audio.__version__)`, which threw an `AttributeError` even though the package was correctly installed.

**Fix:** The import check now uses `import mlx_audio; print('ok')`.

### App can't find `python3` or `uv` when launched from Finder

**Cause:** macOS `.app` bundles launched from Finder/Spotlight get a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), which doesn't include Homebrew paths like `/opt/homebrew/bin/`.

**Fix:** Handy resolves `uv` by checking well-known paths and falling back to a login shell lookup. The sidecar runs using the venv's absolute Python path, so no system Python dependency exists at runtime.

### Resetting the Qwen ASR Environment

To start fresh, delete the venv directory:
```bash
rm -rf ~/Library/Application\ Support/com.handy.app/qwen-asr-venv/
```
Then click "Setup" again in Handy.
