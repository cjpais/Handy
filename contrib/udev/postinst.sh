#!/bin/sh
# Post-install step for packages that ship 70-handy-keys.rules: reload udev so
# the rule takes effect without a reboot. Guarded so installs inside chroots
# and containers (no udev daemon) don't fail the package transaction.
set -e

if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload || true
    udevadm trigger || true
fi

exit 0
