/**
 * Normalize .bin symlinks inside bun's internal module directories.
 *
 * In an isolated install, every package under .bun/ gets its own private
 * node_modules/ with a .bin/ directory holding symlinks to its dependencies'
 * executables:
 *
 *   node_modules/.bun/
 *   ├── @vitejs+plugin-react@4.7.0+…/
 *   │   └── node_modules/
 *   │       ├── vite  → ../../vite@6.4.1+…/node_modules/vite    (peer symlink)
 *   │       └── .bin/                            ← target of this script
 *   │           └── vite  → ../vite/bin/vite.js
 *   ├── eslint@9.39.1+…/
 *   │   └── node_modules/
 *   │       └── .bin/
 *   │           └── eslint → ../eslint/bin/eslint.js
 *   └── vite@6.4.1+…/
 *       └── node_modules/
 *           └── vite/                            ← real package content
 *               └── bin/vite.js
 *
 * Real executables live in .bun/<pkg>@<ver>/node_modules/<pkg>/…; every .bin/
 * entry is just a relative symlink reached through the peer symlinks in the
 * same node_modules/.
 *
 * Bun's creation order for those .bin/ symlinks is not guaranteed to be stable
 * across hosts or filesystems, which can break fixed-output derivation hashes.
 * This script reads the .bin/ symlinks bun produced, removes them, and
 * recreates them in lexicographic order while preserving the exact targets
 * bun picked.
 */

import { lstat, mkdir, readdir, readlink, rm, symlink } from "fs/promises";
import { join } from "path";

type BinEntry = {
  name: string;
  target: string;
};

async function isDirectory(path: string) {
  try {
    const info = await lstat(path);
    return info.isDirectory();
  } catch {
    return false;
  }
}

async function collectBinLinks(binRoot: string): Promise<BinEntry[]> {
  const entries: BinEntry[] = [];
  let names: string[];
  try {
    names = await readdir(binRoot);
  } catch {
    return entries;
  }
  for (const name of names) {
    const full = join(binRoot, name);
    const info = await lstat(full);
    if (!info.isSymbolicLink()) continue;
    entries.push({ name, target: await readlink(full) });
  }
  return entries;
}

export async function normalizeBunBinaries(): Promise<void> {
  const root = process.cwd();
  const bunRoot = join(root, "node_modules/.bun");

  if (!(await isDirectory(bunRoot))) {
    console.log("[normalize-bun-binaries] no .bun directory, skipping");
    return;
  }

  const bunEntries = (await readdir(bunRoot)).sort();
  let rewritten = 0;

  for (const entry of bunEntries) {
    const binRoot = join(bunRoot, entry, "node_modules", ".bin");
    if (!(await isDirectory(binRoot))) continue;

    const bins = await collectBinLinks(binRoot);
    if (bins.length === 0) continue;
    bins.sort((a, b) => a.name.localeCompare(b.name));

    await rm(binRoot, { recursive: true, force: true });
    await mkdir(binRoot, { recursive: true });

    for (const { name, target } of bins) {
      await symlink(target, join(binRoot, name));
      rewritten++;
    }
  }

  console.log(`[normalize-bun-binaries] rebuilt ${rewritten} links`);
}

if (import.meta.main) {
  await normalizeBunBinaries();
}
