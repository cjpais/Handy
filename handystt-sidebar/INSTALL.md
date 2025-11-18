# HandySTT Sidebar - Installation Guide

Complete step-by-step installation instructions for all platforms.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Linux Installation](#linux-installation)
3. [macOS Installation](#macos-installation)
4. [Windows Installation](#windows-installation)
5. [Verification](#verification)
6. [Troubleshooting](#troubleshooting)
7. [Manual Installation](#manual-installation)

## Prerequisites

Before installing the HandySTT Sidebar extension, ensure you have:

### Required Software

1. **Chromium-based browser** (Chrome, Edge, Brave, etc.)
   - Version 114 or higher
   - Download: https://www.google.com/chrome/

2. **HandySTT Desktop Application**
   - Latest version with native messaging support
   - Download: https://github.com/yourusername/HandySTT/releases
   - Or build from source (see main README)

3. **Working microphone**
   - Built-in or external
   - Verified in system settings

### Verify HandySTT Installation

Before proceeding, ensure HandySTT desktop app works:

```bash
# Launch HandySTT
# Test recording and transcription
# Note the installation path (you'll need it)
```

**Find HandySTT executable path:**

- **Linux**: `/usr/local/bin/handy` or `~/Applications/handy`
- **macOS**: `/Applications/Handy.app/Contents/MacOS/Handy`
- **Windows**: `C:\Program Files\Handy\Handy.exe` or `%LOCALAPPDATA%\Handy\Handy.exe`

---

## Linux Installation

### Step 1: Install Chrome Extension

1. Open Chrome/Chromium
2. Navigate to `chrome://extensions/`
3. Enable **Developer mode** (toggle in top-right corner)
4. Click **Load unpacked**
5. Browse to `HandySTT/handystt-sidebar` directory
6. Click **Select Folder**
7. **Copy the Extension ID** - you'll see something like:
   ```
   ID: abcdefghijklmnopqrstuvwxyz123456
   ```

### Step 2: Run Native Messaging Host Installer

```bash
cd HandySTT/handystt-sidebar/native-host
chmod +x install-linux.sh
./install-linux.sh
```

The script will prompt you for:

1. **Extension ID**: Paste the ID from Step 1
2. **Handy executable path**: Full path to HandySTT binary

Example:
```
Enter your Chrome extension ID: abcdefghijklmnopqrstuvwxyz123456
Enter full path to Handy executable: /usr/local/bin/handy
```

### Step 3: Verify Installation

The script will create:
```
~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json
```

And if Chromium is installed:
```
~/.config/chromium/NativeMessagingHosts/com.pais.handy.host.json
```

Check the file:
```bash
cat ~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json
```

Should look like:
```json
{
  "name": "com.pais.handy.host",
  "description": "HandySTT Native Messaging Host",
  "path": "/usr/local/bin/handy",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://abcdefghijklmnopqrstuvwxyz123456/"
  ]
}
```

### Step 4: Restart Browser

```bash
# Close all Chrome/Chromium windows
pkill chrome
pkill chromium

# Relaunch
google-chrome &
# or
chromium &
```

---

## macOS Installation

### Step 1: Install Chrome Extension

1. Open Chrome
2. Navigate to `chrome://extensions/`
3. Enable **Developer mode** (toggle in top-right)
4. Click **Load unpacked**
5. Select `HandySTT/handystt-sidebar` folder
6. **Copy the Extension ID**

### Step 2: Locate Handy.app

Find where Handy is installed:

```bash
# Usually in Applications
ls /Applications/Handy.app

# Or in user Applications
ls ~/Applications/Handy.app
```

If you built from source, find the executable:
```bash
find ~/HandySTT -name "Handy" -type f
```

### Step 3: Run Native Messaging Host Installer

```bash
cd HandySTT/handystt-sidebar/native-host
chmod +x install-macos.sh
./install-macos.sh
```

Provide when prompted:

1. **Extension ID**: From Step 1
2. **Handy path**: Full path to `Handy.app` or executable

Example:
```
Enter your Chrome extension ID: abcdefghijklmnopqrstuvwxyz123456
Enter full path to Handy.app or executable: /Applications/Handy.app
```

**Note**: The script automatically finds the executable inside `.app` bundles.

### Step 4: Verify Installation

Check manifest was created:
```bash
cat ~/Library/Application\ Support/Google/Chrome/NativeMessagingHosts/com.pais.handy.host.json
```

### Step 5: Grant Permissions

macOS may require additional permissions:

1. **System Preferences** → **Security & Privacy** → **Privacy**
2. Ensure **Handy** has:
   - ✅ Microphone access
   - ✅ Accessibility access (if using global shortcuts)
3. You may need to approve Handy in **Input Monitoring** for keyboard shortcuts

### Step 6: Restart Browser

Fully quit and relaunch Chrome:
```bash
# Quit Chrome (Cmd+Q)
# Or force quit
killall "Google Chrome"

# Relaunch
open -a "Google Chrome"
```

---

## Windows Installation

### Step 1: Install Chrome Extension

1. Open Chrome
2. Go to `chrome://extensions/`
3. Enable **Developer mode** (top-right toggle)
4. Click **Load unpacked**
5. Browse to `HandySTT\handystt-sidebar` folder
6. Click **Select Folder**
7. **Copy the Extension ID** (looks like `abcdefghijklmnopqrstuvwxyz123456`)

### Step 2: Locate Handy Executable

Find where Handy is installed:

```powershell
# Common locations
Get-ChildItem "C:\Program Files\Handy" -Recurse -Filter "Handy.exe"
Get-ChildItem "$env:LOCALAPPDATA\Handy" -Recurse -Filter "Handy.exe"
```

Or check the shortcut properties:
1. Right-click Handy desktop shortcut
2. Select **Properties**
3. Copy the **Target** path

### Step 3: Run Native Messaging Host Installer

**Important**: Run PowerShell as **Administrator**

1. Press `Win+X` → **Windows PowerShell (Admin)**
2. Navigate to extension directory:

```powershell
cd C:\path\to\HandySTT\handystt-sidebar\native-host
```

3. Run the installer:

```powershell
powershell -ExecutionPolicy Bypass -File install-windows.ps1
```

4. When prompted, enter:
   - **Extension ID**: From Step 1
   - **Handy.exe path**: Full path to executable

Example:
```
Enter your Chrome extension ID: abcdefghijklmnopqrstuvwxyz123456
Enter full path to Handy.exe: C:\Program Files\Handy\Handy.exe
```

### Step 4: Verify Installation

The script creates:

1. **Registry Key**:
   ```
   HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host
   ```

2. **Manifest File** (in current directory):
   ```
   com.pais.handy.host.json
   ```

Check the registry:
```powershell
Get-ItemProperty "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host"
```

### Step 5: Restart Browser

Close all Chrome windows and relaunch:

```powershell
# Close Chrome
Stop-Process -Name chrome -Force

# Wait a moment
Start-Sleep -Seconds 2

# Relaunch
Start-Process chrome
```

---

## Verification

After installation on any platform, verify everything works:

### 1. Check Extension Status

1. Go to `chrome://extensions/`
2. Find **HandySTT Sidebar**
3. Should show no errors
4. Note the ID matches what you entered

### 2. Test Native Messaging Connection

1. Click the HandySTT extension icon to open sidebar
2. Look for connection status at top:
   - **Green** "Connected to Handy" ✅ Success!
   - **Red** "Disconnected" ❌ See troubleshooting below

### 3. Test Recording

1. Ensure HandySTT desktop app is running
2. In sidebar, click **"Start Recording"**
3. Speak: "Testing one two three"
4. Click **"Stop Recording"**
5. Transcription should appear in text area

### 4. Test Pasting

1. Click into any text field on a web page
2. Click **"📋 Paste to Active Field"** in sidebar
3. Text should appear in the field

### 5. Test AI Integration

1. Select "Claude" from AI dropdown
2. Click **"🤖 Send to AI"**
3. New tab should open with Claude
4. Text should be injected into prompt field

---

## Troubleshooting

### Connection Issues

#### "Cannot connect to Handy"

**Check 1: HandySTT is running**
```bash
# Linux/macOS
ps aux | grep handy

# Windows
tasklist | findstr Handy
```

If not running, launch HandySTT desktop app.

**Check 2: Native host manifest exists**

```bash
# Linux
ls ~/.config/google-chrome/NativeMessagingHosts/

# macOS
ls ~/Library/Application\ Support/Google/Chrome/NativeMessagingHosts/

# Windows (PowerShell)
Get-ChildItem "HKCU:\Software\Google\Chrome\NativeMessagingHosts\"
```

**Check 3: Manifest has correct path**

Open the manifest file and verify `path` points to the actual executable:

```bash
# Linux/macOS
cat ~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json

# Windows (open in Notepad)
notepad com.pais.handy.host.json
```

**Check 4: Extension ID matches**

The `allowed_origins` array must contain your exact extension ID:

```json
"allowed_origins": [
  "chrome-extension://YOUR_ACTUAL_EXTENSION_ID/"
]
```

Compare with ID shown in `chrome://extensions/`.

**Fix**: Re-run installer script with correct Extension ID.

### Permission Issues

#### "Microphone permission denied"

**Linux:**
```bash
# Check microphone access
arecord -l

# Grant permissions (varies by distro)
# For PulseAudio:
pactl list sources
```

**macOS:**
```
System Preferences → Security & Privacy → Privacy → Microphone
Ensure Handy is checked
```

**Windows:**
```
Settings → Privacy → Microphone
Ensure "Allow apps to access your microphone" is ON
Ensure Handy.exe is allowed
```

### Browser Issues

#### Extension not loading

1. Check for JavaScript errors:
   - `chrome://extensions/`
   - Look for red "Errors" button under extension
   - Click to view details

2. Verify all files exist:
   ```bash
   ls handystt-sidebar/
   # Should show: manifest.json, sidepanel.html, sidepanel.js, etc.
   ```

3. Try reloading extension:
   - `chrome://extensions/`
   - Click refresh icon on HandySTT Sidebar card

#### Side panel not opening

1. Ensure you're on Chrome 114+:
   - `chrome://version/`
   - Check version number

2. Try keyboard shortcut:
   - `chrome://extensions/shortcuts`
   - Set custom shortcut for "Open HandySTT"

### Native Messaging Issues

#### Test native messaging manually

**Linux/macOS:**
```bash
echo '{"command":"ping"}' | /path/to/handy
```

Should respond with JSON output.

**Windows:**
```powershell
echo '{"command":"ping"}' | & "C:\Path\To\Handy.exe"
```

If this fails, HandySTT doesn't support native messaging mode yet.

---

## Manual Installation

If automatic scripts fail, install manually:

### Linux/macOS Manual Setup

1. Create native messaging directory:
   ```bash
   mkdir -p ~/.config/google-chrome/NativeMessagingHosts
   ```

2. Create manifest file:
   ```bash
   nano ~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json
   ```

3. Paste this content (replace placeholders):
   ```json
   {
     "name": "com.pais.handy.host",
     "description": "HandySTT Native Messaging Host",
     "path": "/FULL/PATH/TO/handy",
     "type": "stdio",
     "allowed_origins": [
       "chrome-extension://YOUR_EXTENSION_ID/"
     ]
   }
   ```

4. Save and exit (`Ctrl+O`, `Ctrl+X`)

5. Restart Chrome

### Windows Manual Setup

1. Create manifest file in extension folder:
   ```
   handystt-sidebar\native-host\com.pais.handy.host.json
   ```

2. Content (replace paths/ID):
   ```json
   {
     "name": "com.pais.handy.host",
     "description": "HandySTT Native Messaging Host",
     "path": "C:\\Full\\Path\\To\\Handy.exe",
     "type": "stdio",
     "allowed_origins": [
       "chrome-extension://YOUR_EXTENSION_ID/"
     ]
   }
   ```

   **Note**: Use double backslashes in paths!

3. Create registry key (run in PowerShell as Admin):
   ```powershell
   $regPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host"
   New-Item -Path $regPath -Force
   $manifestPath = "C:\Full\Path\To\com.pais.handy.host.json"
   Set-ItemProperty -Path $regPath -Name "(Default)" -Value $manifestPath
   ```

4. Restart Chrome

---

## Uninstallation

### Remove Extension

1. `chrome://extensions/`
2. Find HandySTT Sidebar
3. Click **Remove**
4. Confirm

### Remove Native Messaging Host

**Linux/macOS:**
```bash
rm ~/.config/google-chrome/NativeMessagingHosts/com.pais.handy.host.json
rm ~/.config/chromium/NativeMessagingHosts/com.pais.handy.host.json
```

**Windows (PowerShell as Admin):**
```powershell
Remove-Item "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host" -Recurse
```

---

## Getting Help

If you encounter issues not covered here:

1. **Check logs**:
   - Extension: Right-click sidebar → Inspect → Console
   - Service Worker: `chrome://serviceworker-internals/`
   - HandySTT: Check app logs (location varies)

2. **GitHub Issues**: https://github.com/yourusername/HandySTT/issues

3. **Discord Community**: [Join server]()

4. **Provide debug info**:
   ```
   - OS: [Linux/macOS/Windows + version]
   - Chrome version: chrome://version/
   - Extension ID: [from chrome://extensions/]
   - HandySTT version: [from app]
   - Error messages: [from console/logs]
   ```

---

**Installation complete! Enjoy local, private speech-to-text in your browser.**
