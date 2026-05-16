#!/usr/bin/env bash
# Wrapper for @tauri-apps/cli on macOS + Bun (Apple Silicon).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_JS="${ROOT}/node_modules/@tauri-apps/cli/tauri.js"

if [[ "$(uname -s)" == "Darwin" ]]; then
  export SDKROOT="${SDKROOT:-$(xcrun --show-sdk-path)}"
  export CC="${CC:-$(xcrun -find cc)}"
  export CXX="${CXX:-$(xcrun -find c++)}"
  export CMAKE_POLICY_VERSION_MINIMUM="${CMAKE_POLICY_VERSION_MINIMUM:-3.5}"
  export CXXFLAGS="-stdlib=libc++ -I${SDKROOT}/usr/include/c++/v1 ${CXXFLAGS:-}"
  if [[ -d /opt/homebrew/bin ]]; then
    export PATH="/opt/homebrew/bin:${PATH}"
  fi
fi

resolve_node() {
  local candidate clean_path

  # Bun prepends ~/node_modules/.bin/node (x64 compat) for package scripts.
  clean_path="$(
    echo "$PATH" | tr ':' '\n' | grep -vE '(^|/)\.bun/bin$|node_modules/\.bin$' | paste -sd ':' -
  )"
  candidate="$(PATH="$clean_path" command -v node 2>/dev/null || true)"
  if [[ -n "$candidate" ]] && [[ "$("$candidate" -p "process.arch" 2>/dev/null || echo "")" == "arm64" ]]; then
    echo "$candidate"
    return 0
  fi
  if [[ -d "${HOME}/.nvm/versions/node" ]]; then
    candidate="$(find "${HOME}/.nvm/versions/node" -path '*/bin/node' -type f 2>/dev/null | sort -V -r | head -1)"
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  fi
  echo "${candidate:-node}"
}

exec "$(resolve_node)" "$TAURI_JS" "$@"
