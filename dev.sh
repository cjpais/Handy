#!/bin/bash

# Dev script for Handy - sets up environment and runs tauri dev
# Usage: ./dev.sh [--clean]
#   --clean: Clear app data to test onboarding flow

# Tauri uses the identifier for app data directory, not productName
APP_IDENTIFIER="com.kbve.speechcoach"
DEV_PORT=1420

# Kill any existing instances to ensure a fresh start
echo "Cleaning up any existing dev instances..."

# Kill processes using the dev port
if command -v lsof &> /dev/null; then
    PORT_PIDS=$(lsof -ti:$DEV_PORT 2>/dev/null)
    if [ -n "$PORT_PIDS" ]; then
        echo "$PORT_PIDS" | xargs kill -9 2>/dev/null || true
    fi
fi

# Kill any existing app instances (macOS)
if [[ "$OSTYPE" == "darwin"* ]]; then
    pkill -f "kbve-app" 2>/dev/null || true
    pkill -f "KBVE" 2>/dev/null || true
fi

# Kill any lingering sidecar processes
pkill -f "llm-sidecar" 2>/dev/null || true
pkill -f "tts-sidecar" 2>/dev/null || true
pkill -f "discord-sidecar" 2>/dev/null || true

echo "Cleanup complete."
echo ""

# Fix for cmake policy warning on newer macOS
export CMAKE_POLICY_VERSION_MINIMUM=3.5

# Parse arguments
CLEAN_DATA=false
for arg in "$@"; do
    case $arg in
        --clean)
            CLEAN_DATA=true
            shift
            ;;
    esac
done

# Check if running on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    APP_DATA_DIR="$HOME/Library/Application Support/$APP_IDENTIFIER"

    # Clean app data if requested
    if [ "$CLEAN_DATA" = true ]; then
        if [ -d "$APP_DATA_DIR" ]; then
            echo "Clearing app data at: $APP_DATA_DIR"
            rm -rf "$APP_DATA_DIR"
            echo "App data cleared. Onboarding will show on next launch."
        else
            echo "No app data found at: $APP_DATA_DIR"
        fi
        echo ""
    fi

    # Check if the terminal has accessibility permissions
    echo "Note: This app requires Accessibility permissions to simulate keyboard input."
    echo "If the app crashes with 'permission to simulate input' error:"
    echo "  1. Open System Settings > Privacy & Security > Accessibility"
    echo "  2. Add and enable your terminal app (Terminal, iTerm, VS Code, etc.)"
    echo ""
    echo "Opening Accessibility settings (you can close it if already configured)..."
    open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" 2>/dev/null || true
    echo ""

elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    APP_DATA_DIR="$HOME/.local/share/$APP_IDENTIFIER"

    if [ "$CLEAN_DATA" = true ]; then
        if [ -d "$APP_DATA_DIR" ]; then
            echo "Clearing app data at: $APP_DATA_DIR"
            rm -rf "$APP_DATA_DIR"
            echo "App data cleared. Onboarding will show on next launch."
        else
            echo "No app data found at: $APP_DATA_DIR"
        fi
        echo ""
    fi

elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    APP_DATA_DIR="$APPDATA/$APP_IDENTIFIER"

    if [ "$CLEAN_DATA" = true ]; then
        if [ -d "$APP_DATA_DIR" ]; then
            echo "Clearing app data at: $APP_DATA_DIR"
            rm -rf "$APP_DATA_DIR"
            echo "App data cleared. Onboarding will show on next launch."
        else
            echo "No app data found at: $APP_DATA_DIR"
        fi
        echo ""
    fi
fi

# Determine platform target triple
if [[ "$OSTYPE" == "darwin"* ]]; then
    ARCH=$(uname -m)
    if [[ "$ARCH" == "arm64" ]]; then
        TARGET_TRIPLE="aarch64-apple-darwin"
    else
        TARGET_TRIPLE="x86_64-apple-darwin"
    fi
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    ARCH=$(uname -m)
    if [[ "$ARCH" == "aarch64" ]]; then
        TARGET_TRIPLE="aarch64-unknown-linux-gnu"
    else
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
    fi
elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    TARGET_TRIPLE="x86_64-pc-windows-msvc"
fi

# Build sidecars first
echo "Building LLM sidecar..."
(cd src-tauri/llm-sidecar && cargo build --release)
echo "Building TTS sidecar..."
(cd src-tauri/tts-sidecar && cargo build --release)
echo "Building Discord sidecar..."
(cd src-tauri/discord-sidecar && cargo build --release)

# Copy sidecars with platform-specific names for Tauri externalBin
# Tauri expects: <name>-<target_triple>[.exe]
echo "Copying sidecars with platform-specific names..."
cp src-tauri/llm-sidecar/target/release/llm-sidecar "src-tauri/llm-sidecar/llm-sidecar-${TARGET_TRIPLE}"
cp src-tauri/tts-sidecar/target/release/tts-sidecar "src-tauri/tts-sidecar/tts-sidecar-${TARGET_TRIPLE}"
cp src-tauri/discord-sidecar/target/release/discord-sidecar "src-tauri/discord-sidecar/discord-sidecar-${TARGET_TRIPLE}"
echo "Sidecars built and ready."
echo ""

# Run tauri dev
echo "Starting Tauri development server..."
bun run tauri dev
