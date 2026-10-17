#!/usr/bin/env bash
# One command to build Voxa on an Apple Silicon Mac: bundle, ad-hoc sign, zip.
#
#   bash scripts/voxa/build-mac.sh
#
# Needs Xcode command line tools, Rust (https://rustup.rs), Bun (https://bun.sh)
# and CMake. Output: dist-artifact/Voxa-macos-arm64.zip (same as CI).
set -euo pipefail

cd "$(dirname "$0")/../.."

export CMAKE_POLICY_VERSION_MINIMUM=3.5   # ggml's CMake files on newer CMake
TARGET=aarch64-apple-darwin

rustup target add "$TARGET"
bun install --frozen-lockfile
bunx tauri build --target "$TARGET" --bundles app

APP="src-tauri/target/$TARGET/release/bundle/macos/Voxa.app"
[ -d "$APP" ] || { echo "Voxa.app was not produced" >&2; exit 1; }

codesign --force --deep --options runtime --sign - \
  --entitlements src-tauri/Entitlements.plist "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

mkdir -p dist-artifact
rm -f dist-artifact/Voxa-macos-arm64.zip
ditto -c -k --sequesterRsrc --keepParent "$APP" dist-artifact/Voxa-macos-arm64.zip

echo
echo "Built: $APP"
echo "Zip:   dist-artifact/Voxa-macos-arm64.zip"
echo "Install: copy Voxa.app to /Applications, then run: xattr -cr /Applications/Voxa.app"
