# Handy Development Notes

## Local macOS Code Signing

### Build with Developer ID Certificate

```bash
APPLE_SIGNING_IDENTITY="Developer ID Application: Rishi Patel (CY2M8GR9WC)" bun tauri build
```

### Verify Signature

```bash
codesign -dv --verbose=2 src-tauri/target/release/bundle/macos/Handy.app
```

### Notarization

After building, notarize the DMG so users don't get Gatekeeper warnings:

```bash
xcrun notarytool submit src-tauri/target/release/bundle/macos/Handy_VERSION_aarch64.dmg \
  --apple-id "your@email.com" \
  --team-id "CY2M8GR9WC" \
  --password "app-specific-password" \
  --wait
```

Create an app-specific password at https://appleid.apple.com under Security > App-Specific Passwords.

After notarization completes, staple the ticket:

```bash
xcrun stapler staple src-tauri/target/release/bundle/macos/Handy_VERSION_aarch64.dmg
```

## CI/CD Secrets (GitHub Actions)

For automated signing in `.github/workflows/build.yml`:

- `APPLE_CERTIFICATE` - .p12 certificate (base64 encoded)
- `APPLE_CERTIFICATE_PASSWORD` - Password for the .p12
- `APPLE_ID` - Apple ID email
- `APPLE_ID_PASSWORD` / `APPLE_PASSWORD` - App-specific password
- `APPLE_TEAM_ID` - Team ID (CY2M8GR9WC)
- `KEYCHAIN_PASSWORD` - Temp keychain password (any value)
