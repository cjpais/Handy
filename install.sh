#!/usr/bin/env bash
set -euo pipefail

APP_NAME="madera-tools"
APP_DISPLAY="Madera Tools"
INSTALL_DIR="$HOME/.local/bin"

echo "=== $APP_DISPLAY installer ==="
echo ""

# Detect package manager
detect_pm() {
    if command -v apt &>/dev/null; then echo "apt"
    elif command -v dnf &>/dev/null; then echo "dnf"
    elif command -v pacman &>/dev/null; then echo "pacman"
    elif command -v zypper &>/dev/null; then echo "zypper"
    elif command -v nix-env &>/dev/null; then echo "nix"
    else echo "unknown"
    fi
}

PM=$(detect_pm)

# Detect display server
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    DISPLAY_SERVER="wayland"
elif [ -n "${DISPLAY:-}" ]; then
    DISPLAY_SERVER="x11"
else
    DISPLAY_SERVER="unknown"
fi

echo "Package manager: $PM"
echo "Display server:  $DISPLAY_SERVER"
echo ""

# Install packages with detected package manager
install_packages() {
    local packages="$1"
    [ -z "$packages" ] && return
    echo "Installing: $packages"
    case "$PM" in
        apt)    sudo apt install -y $packages ;;
        dnf)    sudo dnf install -y $packages ;;
        pacman) sudo pacman -S --needed --noconfirm $packages ;;
        zypper) sudo zypper install -y $packages ;;
        *)
            echo "  Please install manually: $packages"
            return 0
            ;;
    esac
}

# Check and install runtime dependencies
MISSING=""

# WebKitGTK (Tauri needs it even for the hidden WebView)
if ! ldconfig -p 2>/dev/null | grep -q "libwebkit2gtk-4.1"; then
    case "$PM" in
        apt)    MISSING="$MISSING libwebkit2gtk-4.1-0 libayatana-appindicator3-1" ;;
        dnf)    MISSING="$MISSING webkit2gtk4.1 libayatana-appindicator-gtk3" ;;
        pacman) MISSING="$MISSING webkit2gtk-4.1 libayatana-appindicator" ;;
        zypper) MISSING="$MISSING libwebkit2gtk-4_1-0 libayatana-appindicator3-1" ;;
    esac
fi

# Display server tools
if [ "$DISPLAY_SERVER" = "wayland" ]; then
    command -v wl-copy &>/dev/null || case "$PM" in
        apt|dnf|pacman|zypper) MISSING="$MISSING wl-clipboard" ;;
    esac
    command -v wtype &>/dev/null || case "$PM" in
        apt|dnf|pacman|zypper) MISSING="$MISSING wtype" ;;
    esac
elif [ "$DISPLAY_SERVER" = "x11" ]; then
    command -v xdotool &>/dev/null || case "$PM" in
        apt|dnf|pacman|zypper) MISSING="$MISSING xdotool" ;;
    esac
    command -v xclip &>/dev/null || case "$PM" in
        apt|dnf|pacman|zypper) MISSING="$MISSING xclip" ;;
    esac
fi

if [ -n "$MISSING" ]; then
    echo "Missing dependencies:$MISSING"
    if [ "$PM" != "unknown" ] && [ "$PM" != "nix" ]; then
        read -rp "Install them now? [Y/n] " answer
        answer="${answer:-Y}"
        if [[ "$answer" =~ ^[Yy]$ ]]; then
            install_packages "$MISSING"
        else
            echo "Skipping. You may need to install manually."
        fi
    elif [ "$PM" = "nix" ]; then
        echo "NixOS detected — add these to your system config or use nix-shell."
    fi
else
    echo "All runtime dependencies found."
fi

echo ""

# Build
if ! command -v bun &>/dev/null; then
    echo "ERROR: bun is required to build. Install from https://bun.sh"
    exit 1
fi

echo "Installing frontend dependencies..."
bun install

echo "Building Tauri app..."
bun run tauri build 2>&1

# Find the built binary
BINARY=$(find src-tauri/target/release -maxdepth 1 -name "$APP_NAME" -type f -executable 2>/dev/null | head -1)
if [ -z "$BINARY" ]; then
    BINARY=$(find src-tauri/target/release/bundle -name "$APP_NAME" -type f -executable 2>/dev/null | head -1)
fi
if [ -z "$BINARY" ]; then
    echo "ERROR: Could not find built binary"
    exit 1
fi

# Install binary
mkdir -p "$INSTALL_DIR"
cp "$BINARY" "$INSTALL_DIR/$APP_NAME"
echo "Installed binary to $INSTALL_DIR/$APP_NAME"

# Install trigger script
CTL_PATH="$INSTALL_DIR/${APP_NAME}-ctl"
cat > "$CTL_PATH" << 'CTLEOF'
#!/usr/bin/env bash
CMD="${1:-transcribe}"
CTLEOF
echo "PID=\$(pgrep -f '$APP_NAME' | head -1)" >> "$CTL_PATH"
cat >> "$CTL_PATH" << 'CTLEOF'

if [ -z "$PID" ]; then
    echo "App is not running"
    exit 1
fi

case "$CMD" in
    transcribe|t)     kill -USR2 "$PID" ;;
    post-process|pp)  kill -USR2 "$PID" ;;
    cancel|c)         kill -USR1 "$PID" ;;
    *) echo "Usage: $(basename "$0") [transcribe|post-process|cancel]"; exit 1 ;;
esac
CTLEOF
chmod +x "$CTL_PATH"
echo "Installed trigger to $CTL_PATH"

# Desktop shortcut hints per DE
echo ""
echo "=== Keyboard shortcuts ==="

DE="unknown"
if [ -d "$HOME/.config/cosmic" ]; then DE="cosmic"
elif [ "${XDG_CURRENT_DESKTOP:-}" = "GNOME" ]; then DE="gnome"
elif [ "${XDG_CURRENT_DESKTOP:-}" = "KDE" ]; then DE="kde"
elif [ "${XDG_CURRENT_DESKTOP:-}" = "Hyprland" ]; then DE="hyprland"
elif [ "${XDG_CURRENT_DESKTOP:-}" = "sway" ]; then DE="sway"
fi

case "$DE" in
    cosmic)
        SHORTCUT_DIR="$HOME/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1"
        mkdir -p "$SHORTCUT_DIR"
        SHORTCUT_FILE="$SHORTCUT_DIR/custom"
        if [ -f "$SHORTCUT_FILE" ] && grep -q "$APP_NAME" "$SHORTCUT_FILE"; then
            echo "COSMIC shortcuts already registered."
        else
            [ -f "$SHORTCUT_FILE" ] && cp "$SHORTCUT_FILE" "${SHORTCUT_FILE}.bak"
            cat > "$SHORTCUT_FILE" << SHORTCUTS
{
    (modifiers: [Ctrl], key: "space"): Spawn("${APP_NAME}-ctl transcribe"),
    (modifiers: [Ctrl, Shift], key: "space"): Spawn("${APP_NAME}-ctl post-process"),
}
SHORTCUTS
            echo "Registered COSMIC shortcuts (logout/login to activate):"
        fi
        echo "  Ctrl+Space       -> transcribe"
        echo "  Ctrl+Shift+Space -> post-process"
        ;;
    gnome)
        echo "GNOME: Settings > Keyboard > Custom Shortcuts:"
        echo "  Ctrl+Space       -> ${APP_NAME}-ctl transcribe"
        echo "  Ctrl+Shift+Space -> ${APP_NAME}-ctl post-process"
        ;;
    kde)
        echo "KDE: System Settings > Shortcuts > Custom Shortcuts:"
        echo "  Ctrl+Space       -> ${APP_NAME}-ctl transcribe"
        echo "  Ctrl+Shift+Space -> ${APP_NAME}-ctl post-process"
        ;;
    hyprland)
        echo "Hyprland: add to ~/.config/hypr/hyprland.conf:"
        echo "  bind = CTRL, space, exec, ${APP_NAME}-ctl transcribe"
        echo "  bind = CTRL SHIFT, space, exec, ${APP_NAME}-ctl post-process"
        ;;
    sway)
        echo "Sway: add to ~/.config/sway/config:"
        echo "  bindsym Ctrl+space exec ${APP_NAME}-ctl transcribe"
        echo "  bindsym Ctrl+Shift+space exec ${APP_NAME}-ctl post-process"
        ;;
    *)
        echo "Configure shortcuts in your DE/WM:"
        echo "  Ctrl+Space       -> ${APP_NAME}-ctl transcribe"
        echo "  Ctrl+Shift+Space -> ${APP_NAME}-ctl post-process"
        ;;
esac

# NVIDIA warning
if lspci 2>/dev/null | grep -qi nvidia; then
    echo ""
    echo "=== NVIDIA GPU detected ==="
    echo "If the app crashes at startup, launch with:"
    echo "  WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \\"
    echo "  JavaScriptCoreUseJIT=0 WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER=1 \\"
    echo "  $APP_NAME"
fi

echo ""
echo "=== Installation complete ==="
echo "Start: $APP_NAME"
echo "Trigger: ${APP_NAME}-ctl transcribe"
