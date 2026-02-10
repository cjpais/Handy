#!/bin/bash
#
# Build Handy app with embedded Python environment
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Building Handy with embedded Python ==="
echo "Project root: $PROJECT_ROOT"

# Step 1: Setup embedded Python
echo ""
echo "Step 1: Setting up embedded Python..."
"$SCRIPT_DIR/setup-embedded-python.sh"

# Step 2: Install Python dependencies
echo ""
echo "Step 2: Installing Python dependencies..."
"$SCRIPT_DIR/install-python-deps.sh"

# Step 3: Build Tauri app
echo ""
echo "Step 3: Building Tauri app..."
cd "$PROJECT_ROOT"
bun run tauri build

echo ""
echo "=== Build complete ==="
echo "The app bundle includes embedded Python with mlx-audio"
echo ""
echo "To verify the embedded Python:"
echo "  1. Open the built app: open src-tauri/target/release/bundle/macos/Handy.app"
echo "  2. Check Resources/python directory"
