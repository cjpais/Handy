#!/usr/bin/env bash
# Updates outputHashes and bunDeps hash in flake.nix
# when Cargo or JS dependencies change.
#
# Usage: ./scripts/update-nix-hashes.sh
#
# Handles:
#   - Version changes in git dependencies (Cargo.lock → outputHashes)
#   - bun.lock changes (→ bunDeps outputHash)
#
# Requires: nix, awk, sed
# Works on: NixOS, Ubuntu/Debian, macOS

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

FLAKE_NIX="$PROJECT_DIR/flake.nix"
CARGO_LOCK="$PROJECT_DIR/src-tauri/Cargo.lock"

FAKE_HASH="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

# Portable sed -i (macOS requires -i '', GNU sed requires just -i)
sedi() {
  if sed --version >/dev/null 2>&1; then
    sed -i "$@"
  else
    sed -i '' "$@"
  fi
}

if ! command -v nix >/dev/null 2>&1; then
  echo "error: nix is not installed. Install it from https://nixos.org/download/" >&2
  exit 1
fi
if [ ! -f "$FLAKE_NIX" ]; then
  echo "error: flake.nix not found at $FLAKE_NIX" >&2
  exit 1
fi
if [ ! -f "$CARGO_LOCK" ]; then
  echo "error: Cargo.lock not found at $CARGO_LOCK" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Step 1: Extract git dependency representative keys from Cargo.lock
#
# Cargo.lock format (consecutive lines per package):
#   [[package]]
#   name = "foo"
#   version = "1.2.3"
#   source = "git+https://...#commit"
#
# Multiple packages from the same git URL share one outputHash entry keyed
# by the alphabetically first "name-version" from that URL.
# ---------------------------------------------------------------------------

extract_cargo_git_keys() {
  awk '
    /^name = /    { name    = substr($3, 2, length($3) - 2) }
    /^version = / { version = substr($3, 2, length($3) - 2) }
    /^source = "git\+/ {
      src = $3
      gsub(/^"git\+/, "", src)
      sub(/#.*/, "", src)
      key = name "-" version
      if (!(src in best) || key < best[src])
        best[src] = key
    }
    END { for (s in best) print best[s] }
  ' "$CARGO_LOCK" | sort
}

# ---------------------------------------------------------------------------
# Step 2: Extract current outputHashes keys from flake.nix
# ---------------------------------------------------------------------------

extract_flake_keys() {
  # Portable: no grep -P, use awk instead
  sed -n '/outputHashes/,/};/p' "$FLAKE_NIX" \
    | awk -F'"' '/sha256-/ { print $2 }' \
    | sort
}

# ---------------------------------------------------------------------------
# Step 3: Compare keys and update flake.nix where needed
# ---------------------------------------------------------------------------

update_output_hash_keys() {
  local cargo_keys flake_keys
  cargo_keys=$(extract_cargo_git_keys)
  flake_keys=$(extract_flake_keys)

  local changed=0

  # For each flake key, check if it still matches a Cargo.lock git dep.
  # If the package name matches but version differs -> update.
  echo "$flake_keys" | while IFS= read -r fk; do
    [ -z "$fk" ] && continue

    # Extract the package name prefix (everything before the version)
    fname=$(echo "$fk" | sed 's/-[0-9][0-9.]*[-0-9]*$//')

    if echo "$cargo_keys" | grep -qxF "$fk"; then
      continue
    fi

    # Key not found in Cargo.lock — look for a replacement with the same name
    replacement=$(echo "$cargo_keys" | while IFS= read -r ck; do
      cname=$(echo "$ck" | sed 's/-[0-9][0-9.]*[-0-9]*$//')
      if [ "$cname" = "$fname" ]; then
        echo "$ck"
        break
      fi
    done)

    if [ -n "$replacement" ]; then
      echo "outputHashes: $fk -> $replacement"
      sedi "s|\"$fk\" = \"sha256-[^\"]*\"|\"$replacement\" = \"$FAKE_HASH\"|" "$FLAKE_NIX"
      changed=1
    else
      echo "warning: $fk not found in Cargo.lock git deps and no replacement detected" >&2
      echo "         This entry may need to be removed or added manually." >&2
    fi
  done

  # Check for new git deps not yet in flake.nix
  echo "$cargo_keys" | while IFS= read -r ck; do
    [ -z "$ck" ] && continue
    if ! echo "$flake_keys" | grep -qxF "$ck" && ! grep -q "\"$ck\"" "$FLAKE_NIX"; then
      echo "warning: git dep $ck exists in Cargo.lock but not in flake.nix outputHashes" >&2
      echo "         You may need to add it manually." >&2
    fi
  done

  return $changed
}

# ---------------------------------------------------------------------------
# Step 4: Iteratively fix hashes by running nix build and parsing errors
# ---------------------------------------------------------------------------

fix_hashes() {
  local max_attempts=10
  local attempt=0

  while [ "$attempt" -lt "$max_attempts" ]; do
    attempt=$((attempt + 1))
    echo ""
    echo "=== nix build attempt $attempt/$max_attempts ==="

    local output
    if output=$(nix build .#handy 2>&1); then
      echo "Build successful!"
      return 0
    fi

    # Check for hash mismatch
    if echo "$output" | grep -q "hash mismatch in fixed-output derivation"; then
      local specified got
      specified=$(echo "$output" | grep "specified:" | awk '{print $2}')
      got=$(echo "$output" | grep "got:" | awk '{print $2}')

      if [ -n "$specified" ] && [ -n "$got" ]; then
        echo "Hash mismatch: $specified -> $got"
        sedi "s|$specified|$got|" "$FLAKE_NIX"
        continue
      fi
    fi

    # If we can't parse the error, show it and bail out
    echo ""
    echo "Build failed with an error that cannot be fixed automatically:" >&2
    echo "$output" | tail -20 >&2
    return 1
  done

  echo "error: exceeded max attempts ($max_attempts)" >&2
  return 1
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

cd "$PROJECT_DIR"

echo "=== Nix flake hash updater ==="
echo ""
echo "Checking outputHashes keys against Cargo.lock..."

if update_output_hash_keys; then
  echo "All outputHashes keys are up to date."
fi

echo ""
echo "Running nix build to verify/fix hashes..."
fix_hashes

echo ""
echo "Done. Changes in flake.nix:"
git diff --stat -- flake.nix 2>/dev/null || true
