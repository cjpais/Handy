#!/bin/bash
# Install native messaging host for HandySTT on macOS

set -e

MANIFEST_NAME="com.pais.handy.host.json"
CHROME_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
CHROMIUM_DIR="$HOME/Library/Application Support/Chromium/NativeMessagingHosts"

echo "HandySTT Native Messaging Host Installer (macOS)"
echo "================================================="

# Get extension ID
read -p "Enter your Chrome extension ID: " EXTENSION_ID

# Get Handy executable path
read -p "Enter full path to Handy.app or executable: " HANDY_PATH

if [ ! -e "$HANDY_PATH" ]; then
    echo "Error: Handy not found at $HANDY_PATH"
    exit 1
fi

# If it's a .app bundle, find the executable inside
if [[ "$HANDY_PATH" == *.app ]]; then
    HANDY_EXEC="$HANDY_PATH/Contents/MacOS/Handy"
    if [ ! -f "$HANDY_EXEC" ]; then
        echo "Error: Cannot find executable in $HANDY_PATH"
        exit 1
    fi
    HANDY_PATH="$HANDY_EXEC"
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
mkdir -p "$CHROME_DIR"
cp "$MANIFEST_NAME" "$CHROME_DIR/"
echo "Installed for Google Chrome: $CHROME_DIR/$MANIFEST_NAME"

# Install for Chromium
if [ -d "$HOME/Library/Application Support/Chromium" ]; then
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
