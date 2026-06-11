#!/usr/bin/env bash
set -euo pipefail

UUID="handy-status@pais.com"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ZIP_PATH="$(mktemp -d)/${UUID}.shell-extension.zip"

if ! command -v gnome-extensions >/dev/null 2>&1; then
  echo "error: gnome-extensions CLI not found. Install GNOME Shell first." >&2
  exit 1
fi

echo "Packing extension from ${SRC_DIR}..."
gnome-extensions pack --force \
  --extra-source=icons \
  --extra-source=README.md \
  "${SRC_DIR}" \
  -o "$(dirname "${ZIP_PATH}")"

echo "Installing ${UUID}..."
gnome-extensions install --force "${ZIP_PATH}"

echo "Enabling ${UUID}..."
gnome-extensions enable "${UUID}"

echo
echo "Done. State:"
gnome-extensions info "${UUID}" | grep -E "State|Enabled"

if [[ "${XDG_SESSION_TYPE:-}" == "wayland" ]]; then
  echo
  echo "Note: on Wayland you must log out and back in for GNOME Shell to load the extension."
fi
