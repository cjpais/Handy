/**
 * Canonicalize bun's internal node_modules symlinks for reproducible FOD hashes.
 *
 * Isolated-install layout produced by `bun install --linker=isolated`:
 *
 *   node_modules/
 *   ├── react          → .bun/react@18.3.1/node_modules/react        (symlink)
 *   ├── minimatch      → .bun/minimatch@3.1.2/node_modules/minimatch (symlink)
 *   └── .bun/
 *       ├── react@18.3.1/
 *       │   └── node_modules/
 *       │       └── react/                      ← real package content
 *       ├── minimatch@3.1.2/
 *       │   └── node_modules/
 *       │       └── minimatch/                  ← real package content
 *       └── node_modules/                       ← target of this script
 *           ├── react      → ../react@18.3.1/node_modules/react
 *           ├── minimatch  → ../minimatch@3.1.2/node_modules/minimatch
 *           └── @babel/
 *               └── core   → ../../@babel+core@7.28.5+…/node_modules/@babel/core
 *
 * Real package content lives in .bun/<pkg>@<ver>/node_modules/<pkg>/.
 * The .bun/node_modules/ directory (linkRoot) holds only symlinks — it acts
 * as a fallback upward-resolution path for packages inside .bun/.
 *
 * Bun's creation order for those symlinks is not guaranteed to be stable
 * across hosts or filesystems, which can break fixed-output derivation hashes.
 * This script reads the existing symlinks, removes them, and recreates them
 * in lexicographic order while preserving the exact targets bun picked.
 */

import { lstat, mkdir, readdir, readlink, rm, symlink } from "fs/promises";
import { join } from "path";

type LinkEntry = {
  slug: string;
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

async function collectLinks(dir: string, prefix: string): Promise<LinkEntry[]> {
  const result: LinkEntry[] = [];
  const names = await readdir(dir);
  for (const name of names) {
    const full = join(dir, name);
    const info = await lstat(full);
    if (info.isSymbolicLink()) {
      const target = await readlink(full);
      const slug = prefix ? `${prefix}/${name}` : name;
      result.push({ slug, target });
    } else if (info.isDirectory() && !prefix && name.startsWith("@")) {
      result.push(...(await collectLinks(full, name)));
    }
  }
  return result;
}

export async function canonicalizeNodeModules(): Promise<void> {
  const root = process.cwd();
  const linkRoot = join(root, "node_modules/.bun/node_modules");

  if (!(await isDirectory(linkRoot))) {
    console.log(
      "[canonicalize-node-modules] no .bun/node_modules directory, skipping",
    );
    return;
  }

  const entries = await collectLinks(linkRoot, "");
  entries.sort((a, b) => a.slug.localeCompare(b.slug));

  await rm(linkRoot, { recursive: true, force: true });
  await mkdir(linkRoot, { recursive: true });

  for (const { slug, target } of entries) {
    const parts = slug.split("/");
    const leaf = parts.pop();
    if (!leaf) continue;
    const parent = join(linkRoot, ...parts);
    await mkdir(parent, { recursive: true });
    await symlink(target, join(parent, leaf));
  }

  console.log(`[canonicalize-node-modules] rebuilt ${entries.length} links`);
}

if (import.meta.main) {
  await canonicalizeNodeModules();
}
