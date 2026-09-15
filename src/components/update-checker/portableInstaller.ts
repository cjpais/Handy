// Portable installs can't self-update in place (no installer, and Windows won't
// let a running exe replace itself). Instead of dumping the user on the releases
// page to hand-pick one of ~27 assets, deep-link the NSIS setup.exe for their
// platform and architecture.
//
// The URL comes straight out of the updater manifest (`Update.rawJson`) rather
// than being rebuilt from the bundler's file-naming convention: it stays correct
// if asset names or the repo slug change, and it points at the immutable
// `releases/download/v<version>/…` tag URL instead of a moving `latest` link.

export const PORTABLE_RELEASES_URL =
  "https://github.com/cjpais/Handy/releases/latest";

/**
 * Pick the NSIS installer URL for the running target out of the update manifest.
 * Falls back to the generic releases page whenever there is no matching entry —
 * e.g. a portable install on a platform Handy ships no NSIS bundle for.
 *
 * @param rawJson `Update.rawJson`, the deserialized `latest.json` manifest
 * @param platformName value from `@tauri-apps/plugin-os` `platform()`
 * @param archName value from `@tauri-apps/plugin-os` `arch()` ("x86_64", "aarch64")
 */
export function resolvePortableInstallerUrl(
  rawJson: Record<string, unknown> | undefined,
  platformName: string,
  archName: string,
): string {
  // NSIS is a Windows-only bundle; nothing else has an installer to link to.
  if (platformName !== "windows") return PORTABLE_RELEASES_URL;

  const platforms = rawJson?.platforms;
  if (!platforms || typeof platforms !== "object") return PORTABLE_RELEASES_URL;

  const entry = (platforms as Record<string, unknown>)[
    `windows-${archName}-nsis`
  ];
  if (!entry || typeof entry !== "object") return PORTABLE_RELEASES_URL;

  const url = (entry as Record<string, unknown>).url;
  return typeof url === "string" ? url : PORTABLE_RELEASES_URL;
}
