# Install native messaging host for HandySTT on Windows
# Run as: powershell -ExecutionPolicy Bypass -File install-windows.ps1

$ErrorActionPreference = "Stop"

Write-Host "HandySTT Native Messaging Host Installer (Windows)" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
Write-Host ""

# Get extension ID
$ExtensionID = Read-Host "Enter your Chrome extension ID"

# Get Handy executable path
$HandyPath = Read-Host "Enter full path to Handy.exe"

if (-not (Test-Path $HandyPath)) {
    Write-Host "Error: Handy executable not found at $HandyPath" -ForegroundColor Red
    exit 1
}

# Convert to absolute path with escaped backslashes
$HandyPath = (Resolve-Path $HandyPath).Path -replace '\\', '\\'

# Create manifest content
$ManifestContent = @"
{
  "name": "com.pais.handy.host",
  "description": "HandySTT Native Messaging Host",
  "path": "$HandyPath",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$ExtensionID/"
  ]
}
"@

# Save manifest to file
$ManifestFile = "com.pais.handy.host.json"
$ManifestContent | Out-File -FilePath $ManifestFile -Encoding UTF8
Write-Host "Created manifest file: $ManifestFile" -ForegroundColor Green

# Registry path for Chrome
$RegistryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.pais.handy.host"

# Create registry key
if (-not (Test-Path $RegistryPath)) {
    New-Item -Path $RegistryPath -Force | Out-Null
}

# Set registry value to manifest path
$ManifestFullPath = (Resolve-Path $ManifestFile).Path
Set-ItemProperty -Path $RegistryPath -Name "(Default)" -Value $ManifestFullPath

Write-Host ""
Write-Host "Installation complete!" -ForegroundColor Green
Write-Host "Registry key created: $RegistryPath" -ForegroundColor Yellow
Write-Host "Manifest location: $ManifestFullPath" -ForegroundColor Yellow
Write-Host ""
Write-Host "Please restart Chrome for changes to take effect." -ForegroundColor Cyan
Write-Host ""
Write-Host "Extension ID: $ExtensionID"
Write-Host "Handy path: $HandyPath"
