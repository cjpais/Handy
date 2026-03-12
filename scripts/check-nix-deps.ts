// scripts/check-nix-deps.ts — Keep .nix/bun.nix in sync with bun.lock
//
// Handy uses bun2nix to generate per-package Nix fetchurl expressions from
// bun.lock. This replaces the old FOD (Fixed-Output Derivation) approach
// where a single hash covered the entire node_modules — that hash would
// break whenever the bun version in nixpkgs changed, even without any
// dependency updates.
//
// How it works:
//   1. Computes sha256 of bun.lock
//   2. Compares with stored hash in .nix/bun-lock-hash
//   3. If they match — nothing to do (~2ms)
//   4. If they differ — runs `bunx bun2nix` to regenerate .nix/bun.nix
//
// When it runs:
//   - Automatically via "postinstall" in package.json — triggers after every
//     bun install / bun add / bun remove / bun update
//   - Can also be run manually: bun scripts/check-nix-deps.ts
//
// What to commit:
//   If the script regenerated .nix/bun.nix, commit it together with bun.lock:
//     git add bun.lock .nix/bun.nix .nix/bun-lock-hash

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { join, resolve } from "path";

const root = resolve(import.meta.dirname, "..");
const nixDir = join(root, ".nix");
const lockFile = join(root, "bun.lock");
const hashFile = join(nixDir, "bun-lock-hash");
const nixFile = join(nixDir, "bun.nix");

// No bun.lock — nothing to do
if (!existsSync(lockFile)) process.exit(0);

// Ensure .nix directory exists
mkdirSync(nixDir, { recursive: true });

// Compute sha256 of the current bun.lock
const currentHash = new Bun.CryptoHasher("sha256")
  .update(readFileSync(lockFile))
  .digest("hex");

// Read the previously stored hash (empty if first run)
const storedHash = existsSync(hashFile)
  ? readFileSync(hashFile, "utf-8").trim()
  : "";

// If hashes match, bun.nix is up to date — nothing to do
if (currentHash === storedHash) process.exit(0);

// bun.lock has changed — regenerate the Nix dependency file
console.log(`[check-nix-deps] bun.lock has changed, regenerating ${nixFile}...`);

const result = Bun.spawnSync(["bunx", "bun2nix", "-o", nixFile], {
  cwd: root,
  stdio: ["inherit", "inherit", "inherit"],
});

if (result.exitCode !== 0) {
  console.error("[check-nix-deps] Error: bunx bun2nix failed. bun.nix may be outdated.");
  console.error(`[check-nix-deps] Try running manually: bunx bun2nix -o ${nixFile}`);
  process.exit(1);
}

writeFileSync(hashFile, currentHash + "\n");
console.log(`[check-nix-deps] Updated ${nixFile}`);
console.log("[check-nix-deps] Don't forget to commit: .nix/bun.nix .nix/bun-lock-hash");
