#!/bin/bash
#
# Install Python dependencies into embedded Python environment
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="$PROJECT_ROOT/src-tauri/resources"
PYTHON_DIR="$RESOURCES_DIR/python"
PYTHON_SHORT_VERSION="3.11"

echo "=== Installing Python dependencies ==="

if [ ! -d "$PYTHON_DIR" ]; then
    echo "Error: Python directory not found at $PYTHON_DIR"
    echo "Please run setup-embedded-python.sh first"
    exit 1
fi

cd "$PYTHON_DIR"

# Get the Python executable
PYTHON_BIN="$PYTHON_DIR/bin/python3"

if [ ! -f "$PYTHON_BIN" ]; then
    echo "Error: Python binary not found at $PYTHON_BIN"
    exit 1
fi

echo "Using Python: $PYTHON_BIN"
$PYTHON_BIN --version

# Install pip if not present
if [ ! -f "$PYTHON_DIR/bin/pip" ]; then
    echo "Installing pip..."
    curl https://bootstrap.pypa.io/get-pip.py -o get-pip.py
    "$PYTHON_BIN" get-pip.py --prefix="$PYTHON_DIR"
    rm -f get-pip.py
fi

PIP_BIN="$PYTHON_DIR/bin/pip"

# Install dependencies
echo "Installing mlx-audio and dependencies..."
"$PIP_BIN" install \
    --target="$PYTHON_DIR/lib/python$PYTHON_SHORT_VERSION/site-packages" \
    --upgrade \
    mlx-audio \
    numpy

# Verify installation
echo "Verifying installation..."
"$PYTHON_BIN" -c "from mlx_audio.stt import load; print('mlx-audio installed successfully')"
"$PYTHON_BIN" -c "import numpy; print(f'numpy {numpy.__version__} installed successfully')"

echo "=== Dependencies installed ==="
echo "Location: $PYTHON_DIR/lib/python$PYTHON_SHORT_VERSION/site-packages"
echo "Total size: $(du -sh "$PYTHON_DIR" | cut -f1)"
