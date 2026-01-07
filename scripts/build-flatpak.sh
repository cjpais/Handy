#!/usr/bin/env bash
set -euo pipefail

# Build Flatpak for Handy
# Usage: ./scripts/build-flatpak.sh [path-to-deb]
#
# If no .deb path is provided, it will look for one in the default Tauri output location.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
FLATPAK_DIR="$PROJECT_ROOT/src-tauri/flatpak"
BUILD_DIR="$PROJECT_ROOT/flatpak-build"
REPO_DIR="$PROJECT_ROOT/flatpak-repo"

APP_ID="com.pais.handy"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check for required tools
check_dependencies() {
    local missing=()

    if ! command -v flatpak &> /dev/null; then
        missing+=("flatpak")
    fi

    if ! command -v flatpak-builder &> /dev/null; then
        missing+=("flatpak-builder")
    fi

    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing required tools: ${missing[*]}"
        echo ""
        echo "Install them with:"
        echo "  Debian/Ubuntu: sudo apt install flatpak flatpak-builder"
        echo "  Arch:          sudo pacman -S flatpak flatpak-builder"
        echo "  Fedora:        sudo dnf install flatpak flatpak-builder"
        exit 1
    fi
}

# Check for required Flatpak runtime
check_runtime() {
    if ! flatpak info org.gnome.Platform//48 &> /dev/null; then
        log_warn "GNOME Platform 48 runtime not installed"
        log_info "Installing required runtimes..."
        flatpak install -y --user flathub org.gnome.Platform//48 org.gnome.Sdk//48
    fi
}

# Find .deb file
find_deb() {
    local deb_path="$1"

    if [ -n "$deb_path" ] && [ -f "$deb_path" ]; then
        echo "$deb_path"
        return 0
    fi

    # Look in default Tauri build locations
    local search_paths=(
        "$PROJECT_ROOT/src-tauri/target/release/bundle/deb"
        "$PROJECT_ROOT/src-tauri/target/debug/bundle/deb"
    )

    for search_path in "${search_paths[@]}"; do
        if [ -d "$search_path" ]; then
            local found_deb
            found_deb=$(find "$search_path" -name "*.deb" -type f | head -1)
            if [ -n "$found_deb" ]; then
                echo "$found_deb"
                return 0
            fi
        fi
    done

    return 1
}

main() {
    local deb_input="${1:-}"

    log_info "Handy Flatpak Builder"
    echo ""

    # Check dependencies
    check_dependencies

    # Check runtime
    check_runtime

    # Find .deb file
    log_info "Looking for .deb file..."
    local deb_path
    if ! deb_path=$(find_deb "$deb_input"); then
        log_error "No .deb file found!"
        echo ""
        echo "Either:"
        echo "  1. Build the app first: bun run tauri build --bundles deb"
        echo "  2. Provide the path: $0 /path/to/handy.deb"
        exit 1
    fi

    log_info "Using .deb: $deb_path"

    # Clean previous builds
    rm -rf "$BUILD_DIR" "$REPO_DIR"
    mkdir -p "$BUILD_DIR"

    # Copy .deb to flatpak directory with expected name
    cp "$deb_path" "$FLATPAK_DIR/handy.deb"
    log_info "Copied .deb to flatpak build directory"

    # Build the Flatpak
    log_info "Building Flatpak..."
    cd "$FLATPAK_DIR"

    flatpak-builder \
        --force-clean \
        --user \
        --disable-cache \
        --repo="$REPO_DIR" \
        "$BUILD_DIR" \
        "$APP_ID.yaml"

    # Create single-file bundle
    local version
    version=$(grep -o '"version": "[^"]*"' "$PROJECT_ROOT/src-tauri/tauri.conf.json" | cut -d'"' -f4)
    local bundle_name="handy_${version}_x86_64.flatpak"

    log_info "Creating Flatpak bundle..."
    flatpak build-bundle \
        "$REPO_DIR" \
        "$PROJECT_ROOT/$bundle_name" \
        "$APP_ID"

    # Cleanup temporary .deb copy
    rm -f "$FLATPAK_DIR/handy.deb"

    echo ""
    log_info "Flatpak bundle created: $PROJECT_ROOT/$bundle_name"
    echo ""
    echo "To install locally:"
    echo "  flatpak install --user $bundle_name"
    echo ""
    echo "To run:"
    echo "  flatpak run $APP_ID"
    echo ""
    echo "To toggle transcription via signal (useful for Wayland):"
    echo "  pkill -SIGUSR2 -f 'flatpak.*handy' || flatpak kill --signal=USR2 $APP_ID"
}

main "$@"
