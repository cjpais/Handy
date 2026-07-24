// Standalone assert check (no test framework in this repo). Run with:
//   bun src/components/update-checker/portableInstaller.test.ts
import assert from "node:assert";
import {
  buildPortableInstallerUrl,
  PORTABLE_RELEASES_URL,
} from "./portableInstaller";

// x64 arch -> x64 setup.exe
assert.equal(
  buildPortableInstallerUrl("0.9.5", "x86_64"),
  "https://github.com/cjpais/Handy/releases/latest/download/Handy_0.9.5_x64-setup.exe",
);

// arm64 arch -> arm64 setup.exe
assert.equal(
  buildPortableInstallerUrl("0.9.5", "aarch64"),
  "https://github.com/cjpais/Handy/releases/latest/download/Handy_0.9.5_arm64-setup.exe",
);

// unknown/missing version -> generic releases page fallback
assert.equal(
  buildPortableInstallerUrl(undefined, "x86_64"),
  PORTABLE_RELEASES_URL,
);

// any non-arm64 arch collapses to x64 (Handy ships no 32-bit Windows build)
assert.equal(
  buildPortableInstallerUrl("1.0.0", "x86"),
  "https://github.com/cjpais/Handy/releases/latest/download/Handy_1.0.0_x64-setup.exe",
);

console.log("portableInstaller: all assertions passed");
