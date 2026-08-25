#!/usr/bin/env bash
#
# Stage transcribe-cpp's dynamic backend libraries into a destination directory.
#
# On Linux/Windows, Handy builds transcribe-cpp in its `dynamic-backends`
# posture: a shared `libtranscribe` plus loadable per-ISA ggml backend modules
# (`libggml-cpu-<isa>.so`, `libggml-vulkan.so`, ...). These are produced by the
# transcribe-cpp-sys CMake install during the cargo build, but nothing ships
# them. This script copies them, as a co-located set, next to where the `handy`
# binary will find them at runtime:
#   - the loader resolves `libtranscribe` via the $ORIGIN-relative rpath baked
#     into `handy` (see src-tauri/build.rs);
#   - transcribe's init_backends_default() then loads the ggml modules from
#     libtranscribe's own directory (a strictly package-local scan), so the
#     modules MUST sit beside it.
#
# Usage: stage-transcribe-libs.sh <src-lib-dir> <dest-dir>
#   <src-lib-dir>  the transcribe-cpp-sys install lib dir (contains
#                  libtranscribe.so* + libggml*.so* + transcribe-link.json)
#   <dest-dir>     where to place them (e.g. an AppImage's usr/lib)
set -euo pipefail

SRC="${1:?usage: stage-transcribe-libs.sh <src-lib-dir> <dest-dir>}"
DEST="${2:?usage: stage-transcribe-libs.sh <src-lib-dir> <dest-dir>}"

if [ ! -d "$SRC" ]; then
  echo "ERROR: source lib dir does not exist: $SRC" >&2
  exit 1
fi

mkdir -p "$DEST"

# Copy the shared lib + every ggml lib/module. `-L` dereferences any SONAME
# symlinks so the package gets real files (mirrors the onnxruntime deb step).
# `|| true` guards an unmatched glob; the verification below is the real gate.
cp -vL "$SRC"/libtranscribe.so* "$DEST"/ 2>/dev/null || true
cp -vL "$SRC"/libggml*.so*      "$DEST"/ 2>/dev/null || true

# Fail loudly if the core lib or the CPU backend modules are absent — a missing
# CPU module means no usable compute device on machines without a GPU backend,
# which is exactly the SIGILL-safe baseline this whole posture exists to deliver.
if ! ls "$DEST"/libtranscribe.so* >/dev/null 2>&1; then
  echo "ERROR: libtranscribe.so missing from $DEST (nothing copied from $SRC)" >&2
  ls -la "$SRC" >&2 || true
  exit 1
fi
if ! ls "$DEST"/libggml-cpu*.so* >/dev/null 2>&1; then
  echo "ERROR: no ggml CPU backend modules (libggml-cpu*.so) staged into $DEST" >&2
  ls -la "$SRC" >&2 || true
  exit 1
fi

echo "Staged transcribe-cpp dynamic backends into $DEST:"
ls -la "$DEST" | grep -E "libtranscribe|libggml" || true
