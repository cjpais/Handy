# Embedded Python Setup for Handy

This document describes how the embedded Python environment is set up for running Qwen3 ASR without requiring users to install Python dependencies manually.

## Overview

Handy bundles a minimal Python environment with `mlx-audio` and its dependencies, so users don't need to:
- Install Python
- Install `mlx-audio` manually
- Deal with Python environment issues

## Architecture

```
Handy.app/
├── Contents/
│   ├── MacOS/
│   │   └── handy (Rust main binary)
│   └── Resources/
│       ├── models/              (Voice models)
│       ├── scripts/             (qwen3_asr_server.py)
│       └── python/              (Embedded Python)
│           ├── bin/
│           │   ├── python3      (Python executable)
│           │   └── pip
│           └── lib/
│               └── python3.11/
│                   └── site-packages/  (mlx-audio, numpy, etc.)
```

## Build Process

### Prerequisites

- macOS with Apple Silicon (for MLX support)
- Xcode Command Line Tools
- Rust toolchain
- Bun

### Building with Embedded Python

```bash
# Run the complete build script
./scripts/build-with-python.sh
```

This script will:
1. Download Python 3.11 framework
2. Create a minimal Python installation
3. Install `mlx-audio` and dependencies
4. Build the Tauri app with embedded Python

### Manual Steps

If you need more control, you can run each step manually:

```bash
# Step 1: Setup embedded Python
./scripts/setup-embedded-python.sh

# Step 2: Install dependencies
./scripts/install-python-deps.sh

# Step 3: Build Tauri app
bun run tauri build
```

## How It Works

### 1. Python Detection (Rust side)

The `qwen3_engine.rs` module tries to find Python in this order:

1. **App Resources**: `Handy.app/Contents/Resources/python/bin/python3`
2. **Environment variable**: `HANDY_EMBEDDED_PYTHON`
3. **Development path**: `src-tauri/resources/python/bin/python3`
4. **Fallback**: System `python3`

### 2. Runtime Behavior

When Qwen3 engine starts:

1. `get_embedded_python_path()` searches for embedded Python
2. If found, uses it to run `qwen3_asr_server.py`
3. If not found, falls back to system Python
4. Server process communicates via stdin/stdout

### 3. Bundling

Tauri's `tauri.conf.json` includes:

```json
{
  "bundle": {
    "resources": ["resources/**/*"]
  }
}
```

This ensures `src-tauri/resources/python/` is copied into the app bundle.

## Size Considerations

| Component | Approximate Size |
|-----------|-----------------|
| Python Framework (minimal) | ~30 MB |
| mlx-audio + dependencies | ~200 MB |
| **Total embedded Python** | **~230 MB** |

The app bundle size will increase by approximately 230 MB, but users don't need to install anything separately.

## Troubleshooting

### Embedded Python not found

Check if Python exists in the app bundle:

```bash
ls -la Handy.app/Contents/Resources/python/bin/
```

### Test embedded Python manually

```bash
# Run embedded Python
Handy.app/Contents/Resources/python/bin/python3 -c "import mlx_audio; print('OK')"
```

### Fallback to system Python

If embedded Python fails, the app will automatically fall back to system Python. Check logs for:

```
[INFO] Using embedded Python: ...
# or
[INFO] Embedded Python not found, falling back to system Python
```

## Development

During development, you can use system Python for faster iteration:

```bash
# Don't run setup scripts, just use system Python
# The code will automatically fall back to system Python
bun run tauri dev
```

To test with embedded Python during development:

```bash
# Setup embedded Python once
./scripts/setup-embedded-python.sh
./scripts/install-python-deps.sh

# Run in dev mode (will use embedded Python if found)
bun run tauri dev
```

## Future Improvements

1. **Lazy loading**: Only extract Python on first use
2. **Delta updates**: Update only changed dependencies
3. **Compression**: Compress Python libraries to reduce bundle size
4. **Optional bundling**: Make embedded Python optional for users who prefer system Python
