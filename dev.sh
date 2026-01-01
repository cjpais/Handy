#!/bin/bash

# Dev script for Handy - sets up environment and runs tauri dev
# Usage: ./dev.sh [--clean]
#   --clean: Clear app data to test onboarding flow

# Tauri uses the identifier for app data directory, not productName
APP_IDENTIFIER="com.kbve.speechcoach"

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

# Run tauri dev
echo "Starting Tauri development server..."
bun run tauri dev
