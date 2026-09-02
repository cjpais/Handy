#!/usr/bin/env bash
# Refresh src-tauri/vendor/utype from upstream. Pass a commit or tag to pin a
# specific revision; defaults to the tip of the default branch.
set -euo pipefail

REPO=https://github.com/vanviegen/utype
REV=${1:-HEAD}
DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/vendor/utype"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

git clone --quiet "$REPO" "$TMP/utype"
git -C "$TMP/utype" checkout --quiet "$REV"
COMMIT=$(git -C "$TMP/utype" rev-parse HEAD)

cp "$TMP/utype"/{utype.c,utype.h,cli.c,LICENSE} "$DEST/"
sed -i "s/^Pinned commit: .*/Pinned commit: \`$COMMIT\`/" "$DEST/README.md"

echo "Vendored utype at $COMMIT"
