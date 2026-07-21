// Portable installs can't self-update in place (no installer, and Windows won't
// let a running exe replace itself). Instead of dumping the user on the releases
// page to hand-pick one of ~27 assets, deep-link the exact NSIS setup.exe for
// their architecture.
//
// ponytail: asset name follows the tauri bundler convention
// `Handy_<version>_<arch>-setup.exe`. If the bundler naming or repo slug changes,
// update RELEASES_BASE / the template here.

const RELEASES_BASE = "https://github.com/cjpais/Handy/releases/latest";

/**
 * Build the direct download URL for the NSIS installer matching the running arch.
 * Falls back to the generic releases page when the version is unknown.
 *
 * @param version app version to update to (e.g. "0.9.5"), or undefined
 * @param archName value from `@tauri-apps/plugin-os` `arch()` (e.g. "x86_64", "aarch64")
 */
export function buildPortableInstallerUrl(
  version: string | undefined,
  archName: string,
): string {
  if (!version) return RELEASES_BASE;
  // Handy only ships x64 and arm64 Windows installers; everything non-arm64 -> x64.
  const archStr = archName === "aarch64" ? "arm64" : "x64";
  return `${RELEASES_BASE}/download/Handy_${version}_${archStr}-setup.exe`;
}

export const PORTABLE_RELEASES_URL = RELEASES_BASE;
