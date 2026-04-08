/**
 * Normalize .bin symlinks inside bun's internal module directories.
 *
 * Bun may create .bin/ symlinks in non-deterministic order within each
 * node_modules/.bun/<pkg>@<ver>/node_modules/.bin/ directory. This script
 * rebuilds them deterministically by reading package.json "bin" fields and
 * recreating symlinks in sorted order.
 *
 * Adapted from opencode (https://github.com/anomalyco/opencode), MIT license.
 */

import { lstat, mkdir, readdir, rm, symlink } from "fs/promises";
import { join, relative } from "path";

type PackageManifest = {
  name?: string;
  bin?: string | Record<string, string>;
};

async function exists(path: string) {
  try {
    await lstat(path);
    return true;
  } catch {
    return false;
  }
}

async function isDirectory(path: string) {
  try {
    const info = await lstat(path);
    return info.isDirectory();
  } catch {
    return false;
  }
}

async function readManifest(dir: string) {
  const file = Bun.file(join(dir, "package.json"));
  if (!(await file.exists())) return null;
  return (await file.json()) as PackageManifest;
}

async function collectPackages(modulesRoot: string) {
  const found: string[] = [];
  const topLevel = (await readdir(modulesRoot)).sort();
  for (const name of topLevel) {
    if (name === ".bin" || name === ".bun") continue;
    const full = join(modulesRoot, name);
    if (!(await isDirectory(full))) continue;
    if (name.startsWith("@")) {
      const scoped = (await readdir(full)).sort();
      for (const child of scoped) {
        const scopedDir = join(full, child);
        if (await isDirectory(scopedDir)) found.push(scopedDir);
      }
      continue;
    }
    found.push(full);
  }
  return found.sort();
}

function normalizeBinName(name: string) {
  const slash = name.lastIndexOf("/");
  return slash >= 0 ? name.slice(slash + 1) : name;
}

const root = process.cwd();
const bunRoot = join(root, "node_modules/.bun");

if (!(await isDirectory(bunRoot))) {
  console.log("[normalize-bun-binaries] no .bun directory, skipping");
  process.exit(0);
}

const bunEntries = (await readdir(bunRoot)).sort();
let rewritten = 0;

for (const entry of bunEntries) {
  const modulesRoot = join(bunRoot, entry, "node_modules");
  if (!(await exists(modulesRoot))) continue;

  const binRoot = join(modulesRoot, ".bin");
  await rm(binRoot, { recursive: true, force: true });
  await mkdir(binRoot, { recursive: true });

  const packageDirs = await collectPackages(modulesRoot);
  for (const packageDir of packageDirs) {
    const manifest = await readManifest(packageDir);
    if (!manifest?.bin) continue;

    const seen = new Set<string>();
    const binField = manifest.bin;

    if (typeof binField === "string") {
      const fallback = manifest.name ?? packageDir.split("/").pop();
      if (fallback) {
        const normalizedName = normalizeBinName(fallback);
        if (!seen.has(normalizedName)) {
          const resolved = join(packageDir, binField);
          if (await exists(resolved)) {
            const destination = join(binRoot, normalizedName);
            const relativeTarget = relative(binRoot, resolved) || ".";
            await rm(destination, { force: true });
            await symlink(relativeTarget, destination);
            seen.add(normalizedName);
            rewritten++;
          }
        }
      }
    } else {
      const entries = Object.entries(binField).sort((a, b) =>
        a[0].localeCompare(b[0]),
      );
      for (const [name, target] of entries) {
        if (!name || !target) continue;
        const normalizedName = normalizeBinName(name);
        if (seen.has(normalizedName)) continue;
        const resolved = join(packageDir, target);
        if (!(await exists(resolved))) continue;
        const destination = join(binRoot, normalizedName);
        const relativeTarget = relative(binRoot, resolved) || ".";
        await rm(destination, { force: true });
        await symlink(relativeTarget, destination);
        seen.add(normalizedName);
        rewritten++;
      }
    }
  }
}

console.log(`[normalize-bun-binaries] rebuilt ${rewritten} links`);
