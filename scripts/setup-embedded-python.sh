#!/bin/bash
#
# Setup embedded Python environment for Handy app
# This script downloads and configures a minimal Python environment
# with mlx-audio and other dependencies bundled.
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="$PROJECT_ROOT/src-tauri/resources"
PYTHON_DIR="$RESOURCES_DIR/python"
PYTHON_VERSION="3.11.10"
PYTHON_SHORT_VERSION="3.11"

echo "=== Setting up embedded Python for Handy ==="
echo "Project root: $PROJECT_ROOT"
echo "Python version: $PYTHON_VERSION"

# Clean up existing Python directory
if [ -d "$PYTHON_DIR" ]; then
    echo "Removing existing Python directory..."
    rm -rf "$PYTHON_DIR"
fi

mkdir -p "$PYTHON_DIR"
cd "$PYTHON_DIR"

# Download Python framework for macOS
echo "Downloading Python $PYTHON_VERSION..."
PYTHON_PKG="python-$PYTHON_VERSION-macos11.pkg"
PYTHON_URL="https://www.python.org/ftp/python/$PYTHON_VERSION/$PYTHON_PKG"

if [ ! -f "$PYTHON_PKG" ]; then
    curl -L -o "$PYTHON_PKG" "$PYTHON_URL"
fi

# Extract the package
echo "Extracting Python package..."
pkgutil --expand-full "$PYTHON_PKG" python_pkg_extracted

# Find the Python framework
FRAMEWORK_PATH=$(find python_pkg_extracted -name "Python.framework" -type d | head -1)

if [ -z "$FRAMEWORK_PATH" ]; then
    echo "Error: Python.framework not found in package"
    exit 1
fi

echo "Found Python framework at: $FRAMEWORK_PATH"

# Create minimal Python installation
echo "Creating minimal Python installation..."
mkdir -p python_framework

# Copy only necessary parts of the framework
cp -R "$FRAMEWORK_PATH" python_framework/

# Create bin directory and symlinks
mkdir -p bin
cd bin
ln -sf ../python_framework/Python.framework/Versions/$PYTHON_SHORT_VERSION/bin/python$PYTHON_SHORT_VERSION python3
ln -sf python3 python
cd ..

# Create lib directory structure
mkdir -p lib/python$PYTHON_SHORT_VERSION/site-packages

# Copy standard library (minimal)
echo "Copying standard library..."
cp -R python_framework/Python.framework/Versions/$PYTHON_SHORT_VERSION/lib/python$PYTHON_SHORT_VERSION/* \
    lib/python$PYTHON_SHORT_VERSION/ 2>/dev/null || true

# Remove unnecessary files to reduce size
echo "Removing unnecessary files..."
rm -rf lib/python$PYTHON_SHORT_VERSION/test
rm -rf lib/python$PYTHON_SHORT_VERSION/unittest/test
rm -rf lib/python$PYTHON_SHORT_VERSION/lib2to3/tests
rm -rf lib/python$PYTHON_SHORT_VERSION/idlelib
rm -rf lib/python$PYTHON_SHORT_VERSION/tkinter
rm -rf lib/python$PYTHON_SHORT_VERSION/turtledemo
rm -rf lib/python$PYTHON_SHORT_VERSION/ensurepip
rm -rf lib/python$PYTHON_SHORT_VERSION/pydoc_data
rm -rf python_pkg_extracted
rm -f "$PYTHON_PKG"

# Create pip configuration to use local packages
cat > lib/python$PYTHON_SHORT_VERSION/site-packages/sitecustomize.py << 'EOF'
import sys
import os

# Add the embedded site-packages to path
script_dir = os.path.dirname(os.path.abspath(__file__))
site_packages = os.path.join(script_dir)
if site_packages not in sys.path:
    sys.path.insert(0, site_packages)
EOF

echo "=== Python framework setup complete ==="
echo "Location: $PYTHON_DIR"
echo "Size: $(du -sh . | cut -f1)"
