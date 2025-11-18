#!/bin/bash
# Install native messaging host for HandySTT on Linux

set -e

MANIFEST_NAME="com.pais.handy.host.json"
NATIVE_MESSAGING_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
CHROMIUM_DIR="$HOME/.config/chromium/NativeMessagingHosts"

echo "HandySTT Native Messaging Host Installer (Linux)"
echo "=================================================="

# Get extension ID
read -p "Enter your Chrome extension ID: " EXTENSION_ID

# Get Handy executable path
read -p "Enter full path to Handy executable: " HANDY_PATH

if [ ! -f "$HANDY_PATH" ]; then
    echo "Error: Handy executable not found at $HANDY_PATH"
    exit 1
fi

# Create manifest
cat > "$MANIFEST_NAME" <<EOF
{
  "name": "com.pais.handy.host",
  "description": "HandySTT Native Messaging Host",
  "path": "$HANDY_PATH",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$EXTENSION_ID/"
  ]
}
EOF

echo "Created manifest file: $MANIFEST_NAME"

# Install for Chrome
mkdir -p "$NATIVE_MESSAGING_DIR"
cp "$MANIFEST_NAME" "$NATIVE_MESSAGING_DIR/"
echo "Installed for Google Chrome: $NATIVE_MESSAGING_DIR/$MANIFEST_NAME"

# Install for Chromium
if [ -d "$HOME/.config/chromium" ]; then
    mkdir -p "$CHROMIUM_DIR"
    cp "$MANIFEST_NAME" "$CHROMIUM_DIR/"
    echo "Installed for Chromium: $CHROMIUM_DIR/$MANIFEST_NAME"
fi

echo ""
echo "Installation complete!"
echo "Please restart Chrome/Chromium for changes to take effect."
echo ""
echo "Extension ID: $EXTENSION_ID"
echo "Handy path: $HANDY_PATH"
