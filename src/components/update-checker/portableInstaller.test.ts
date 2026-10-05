// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/update-checker/portableInstaller.test.ts
import assert from "node:assert";
import {
  resolvePortableInstallerUrl,
  PORTABLE_RELEASES_URL,
} from "./portableInstaller";

const X64_SETUP =
  "https://github.com/cjpais/Handy/releases/download/v0.9.5/Handy_0.9.5_x64-setup.exe";
const ARM64_SETUP =
  "https://github.com/cjpais/Handy/releases/download/v0.9.5/Handy_0.9.5_arm64-setup.exe";

// Trimmed copy of the real latest.json served from the updater endpoint.
const manifest = {
  version: "0.9.5",
  platforms: {
    "windows-x86_64-nsis": { url: X64_SETUP, signature: "…" },
    "windows-x86_64-msi": {
      url: "https://github.com/cjpais/Handy/releases/download/v0.9.5/Handy_0.9.5_x64_en-US.msi",
    },
    "windows-aarch64-nsis": { url: ARM64_SETUP, signature: "…" },
    "darwin-aarch64": {
      url: "https://github.com/cjpais/Handy/releases/download/v0.9.5/Handy_aarch64.app.tar.gz",
    },
  },
};

// x64 Windows -> the x64 NSIS asset, pinned to the release tag
assert.equal(
  resolvePortableInstallerUrl(manifest, "windows", "x86_64"),
  X64_SETUP,
);

// arm64 Windows -> the arm64 NSIS asset
assert.equal(
  resolvePortableInstallerUrl(manifest, "windows", "aarch64"),
  ARM64_SETUP,
);

// no manifest (check() failed or returned no update) -> releases page fallback
assert.equal(
  resolvePortableInstallerUrl(undefined, "windows", "x86_64"),
  PORTABLE_RELEASES_URL,
);

// no NSIS bundle for this arch -> releases page fallback
assert.equal(
  resolvePortableInstallerUrl(manifest, "windows", "x86"),
  PORTABLE_RELEASES_URL,
);

// non-Windows portable install -> releases page, never a Windows .exe
assert.equal(
  resolvePortableInstallerUrl(manifest, "macos", "aarch64"),
  PORTABLE_RELEASES_URL,
);

// malformed manifest -> releases page fallback
assert.equal(
  resolvePortableInstallerUrl({ platforms: "nope" }, "windows", "x86_64"),
  PORTABLE_RELEASES_URL,
);

console.log("portableInstaller: all assertions passed");
