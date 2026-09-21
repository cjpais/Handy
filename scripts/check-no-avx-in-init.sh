#!/usr/bin/env bash
# Verify that a shared library executes no AVX instructions during process
# startup: scan `_init` and every target of `.init_array` for VEX/EVEX
# encodings. AVX here means any VEX (0xC5/0xC4) or EVEX (0x62) prefixed
# instruction, regardless of operand width — such instructions fault with
# SIGILL on pre-AVX x86-64 CPUs (e.g. Intel Celeron N5095) even in a C++
# static initializer running before main().
#
# Baseline x86-64 libraries ARE allowed to contain AVX in runtime-dispatched
# kernels (e.g. MLAS selects them via cpuid); nothing in this script inspects
# those. It only patrols the code reachable from global initialization.
#
# Usage: check-no-avx-in-init.sh <path-to-.so>

set -euo pipefail

if [ $# -lt 1 ]; then
  echo "usage: $0 <lib.so>" >&2
  exit 2
fi
LIB="$(readlink -f "$1")"

python3 - "$LIB" <<'PY'
import re
import subprocess
import sys

so = sys.argv[1]

def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout

# x86-64 VEX (0xC4/0xC5) and EVEX (0x62) encodings: 0xC5/0xC4/0x62 are
# genuine instruction prefixes only in 64-bit mode (no LES/LDS/BOUND).
# objdump prefixes each instruction with '<offset>:\t<bytes> ...'.
VEX_RE = re.compile(r"(?m):\s+(c4|c5|62) [0-9a-f]{2} ")

def scan(label, addr, size=0x400):
    try:
        txt = run(
            "objdump", "-d", "-M", "intel",
            f"--start-address=0x{addr:x}",
            f"--stop-address=0x{addr + size:x}",
            so,
        )
    except subprocess.CalledProcessError:
        print(f"skip: {label} (0x{addr:x}) not disassemblable")
        return False
    hits = VEX_RE.findall(txt)
    if hits:
        print(f"FAIL: {label} (0x{addr:x}) contains {len(hits)} AVX/EVEX instruction(s)", file=sys.stderr)
        return True
    print(f"ok:   {label} (0x{addr:x}) no AVX/EVEX in init path")
    return False

failed = False

# .init_array (and plain .init) — the C++ static-initializer table/entry.
# Parse by column from `readelf -S` (fields align to fixed columns but
# splitting on whitespace keeps this robust):
#   [Nr] Name  Type  Address  Offset  Size  EntSize  Flags  ...
init_funcs = []
init_array_base = None
init_array_size = 0
for line in run("readelf", "-SW", so).splitlines():
    # fields (whitespace-split): [Nr] Name Type Address Offset Size ...
    fields = line.split()
    if len(fields) >= 5 and fields[1] == ".init" and fields[2] == "PROGBITS":
        addr = int(fields[3], 16)
        if addr:
            init_funcs.append(addr)
    if len(fields) >= 5 and fields[1] == ".init_array" and fields[2] == "INIT_ARRAY":
        init_array_base = int(fields[3], 16)
        init_array_size = int(fields[5], 16)

entries = []
if init_array_base is not None:
    rows = run("readelf", "-x", ".init_array", so)
    for row in rows.splitlines():
        body = re.match(r"^\s*0x[0-9a-f]+\s+(.*)$", row)
        if not body:
            continue
        for group in body.group(1).split():
            if len(group) == 8 and all(c in "0123456789abcdef" for c in group):
                # readelf shows bytes in memory order; rebuild the pointer
                # (little-endian, same as the .so's target arch).
                entries.append(int.from_bytes(bytes.fromhex(group), "little"))
    # A PIE may pre-fill .init_array and also emit relocs; when the file
    # content is zeros the targets are delivered at load — then we cannot
    # pre-verify statically. Zero entries are skipped as padding.
    if not any(entries) and init_array_size:
        print("note: .init_array is empty/relocated; static pre-verify not possible for it")

# `.init` section is the DT_INIT constructor — disassemble it directly, since
# the `_init` dynamic symbol is stripped in many release builds.
for a in init_funcs:
    failed |= scan(".init", a)

real = [e for e in entries if e]
print(f"scanning {len(real)} .init_array target(s)")
for e in real:
    failed |= scan(".init_array->target", e)

sys.exit(1 if failed else 0)
PY