/**
 * Heal missing `.bin/<peer>` symlinks produced by bun's isolated installer.
 *
 * ## The bug
 *
 * Bun's `--linker=isolated` installer creates `.bin/<name>` symlinks inside
 * each package's private node_modules/.bin/ for every dependency that has a
 * `bin` field in its manifest — regular, optional, AND peer dependencies all
 * go through the same code path (`Installer.zig::linkDependencyBins`), and
 * the decision to link is made purely on whether the source file exists on
 * disk at the moment the linker looks (`bin.zig`:
 *
 *     if (!bun.sys.exists(abs_target)) {
 *         this.skipped_due_to_missing_bin = true;
 *         return;
 *     }
 *
 * ). For most dependencies the installer blocks the consuming package on the
 * provider via `isTaskBlocked`, so by the time `linkDependencyBins` runs for
 * package A the provider's file is guaranteed to be in place. But for
 * circular peer dependency pairs — A declares B as a peer, B (transitively)
 * depends on A — that blocking would deadlock, so bun's `Store.isCycle`
 * detector explicitly bypasses it and lets both sides run in parallel.
 *
 * The consequence is a plain timing race between two worker threads. Which
 * side wins depends on anything that shifts the relative scheduling of the
 * two workers — CPU load, thread-pool size, filesystem write latency and
 * caching, the kernel scheduler, NICE / cgroup limits — so the same bun
 * version with the same bun.lock and the same install flags can produce
 * different `.bin/` sets not just between different hosts but in principle
 * between two consecutive runs on the same host. In practice we have
 * observed divergence between a local NixOS sandbox, a GitHub Actions
 * ubuntu-latest runner, and a GitHub Actions macos-latest runner, which is
 * enough to break any single-hash FOD.
 *
 * Concretely, the Handy install hits this with
 *   - update-browserslist-db/.bin/browserslist (update-browserslist-db
 *     declares browserslist as a peer, browserslist has
 *     update-browserslist-db in its regular dependencies → cycle), and
 *   - @eslint-community/eslint-utils/.bin/eslint (eslint has eslint-utils
 *     as a regular dep, eslint-utils declares eslint as a peer → cycle).
 *
 * There is no bun configuration flag or env var that makes the outcome
 * deterministic, and no upstream issue yet tracks this specific symptom
 * (oven-sh/bun#28147 is the closest family match, different project). See
 * the header of `canonicalize-node-modules.ts` for our sibling normalization
 * pass that rebuilds `.bun/node_modules/` in sorted order.
 *
 * ## The fix this script applies
 *
 * For every package under `node_modules/.bun/` we walk its declared
 * `peerDependencies`, find each resolved peer inside the package's private
 * `node_modules/`, and create any `.bin/<name> → ../<peerName>/<path>`
 * symlinks that bun's installer "intended" to create but may have skipped.
 * Entries that already exist are left alone (the script is idempotent).
 *
 * This is the "fix by adding" approach — we produce the complete `.bin/`
 * set that bun would have produced without the race, rather than stripping
 * the inconsistent subset. Advantages:
 *
 *   - Matches bun's intended behavior; if bun ever fixes the race upstream,
 *     this script becomes a no-op (every entry it would add already exists)
 *     and the FOD hash is unchanged.
 *   - Preserves `.bin/` entries that real code might depend on. We don't
 *     rely on the (true but brittle) argument that peer-dep `.bin/` entries
 *     are dead code in Tauri apps.
 *   - Easy to explain in review: we're patching a known upstream race bug
 *     with the exact output the upstream code is trying to produce.
 */

import { lstat, mkdir, readdir, readlink, symlink } from "fs/promises";
import { join } from "path";

type Manifest = {
  name?: string;
  bin?: string | Record<string, string>;
  peerDependencies?: Record<string, string>;
};

async function isDirectory(path: string) {
  try {
    const info = await lstat(path);
    return info.isDirectory();
  } catch {
    return false;
  }
}

async function readManifest(path: string): Promise<Manifest | null> {
  const file = Bun.file(path);
  if (!(await file.exists())) return null;
  return (await file.json()) as Manifest;
}

// Parse a .bun entry directory name (e.g. "@babel+core@7.28.5+a1c3dd1b9adf390b")
// back into an npm package name ("@babel/core"). Returns null for entries that
// do not look like <name>@<version>[+<peer-hash>].
function parsePkgName(bunEntry: string): string | null {
  const at = bunEntry.startsWith("@")
    ? bunEntry.indexOf("@", 1)
    : bunEntry.indexOf("@");
  if (at <= 0) return null;
  return bunEntry.slice(0, at).replace(/\+/g, "/");
}

// Unscoped name used as the default bin name when `bin` is a bare string.
// For "@scope/pkg" returns "pkg"; for "pkg" returns "pkg".
function defaultBinName(pkgName: string): string {
  const slash = pkgName.lastIndexOf("/");
  return slash >= 0 ? pkgName.slice(slash + 1) : pkgName;
}

type BinSpec = { name: string; path: string };

function parseBinField(pkgName: string, binField: Manifest["bin"]): BinSpec[] {
  if (!binField) return [];
  if (typeof binField === "string") {
    return [{ name: defaultBinName(pkgName), path: binField }];
  }
  return Object.entries(binField).map(([name, path]) => ({
    name: defaultBinName(name),
    path,
  }));
}

// Produce the relative symlink target that bun itself uses for a .bin entry
// sitting inside `.bun/<containing>/node_modules/.bin/`, pointing to a file
// under `.bun/<containing>/node_modules/<peerName>/...`.
function binTarget(peerName: string, binPath: string): string {
  const clean = binPath.replace(/^\.\//, "");
  return `../${peerName}/${clean}`;
}

type HealedEntry = {
  containingEntry: string;
  containingPkg: string;
  peerName: string;
  binName: string;
  target: string;
};

export async function healPeerDepBins(): Promise<void> {
  const root = process.cwd();
  const bunRoot = join(root, "node_modules/.bun");

  if (!(await isDirectory(bunRoot))) {
    console.log("[heal-peer-dep-bins] no .bun directory, skipping");
    return;
  }

  const bunEntries = (await readdir(bunRoot)).sort();
  const healed: HealedEntry[] = [];

  for (const entry of bunEntries) {
    const pkgName = parsePkgName(entry);
    if (!pkgName) continue;
    const containingNodeModules = join(bunRoot, entry, "node_modules");
    if (!(await isDirectory(containingNodeModules))) continue;

    const manifest = await readManifest(
      join(containingNodeModules, pkgName, "package.json"),
    );
    if (!manifest) continue;

    const peers = Object.keys(manifest.peerDependencies ?? {});
    if (peers.length === 0) continue;

    const binRoot = join(containingNodeModules, ".bin");

    for (const peerName of peers) {
      // Peer may be optional and unresolved, or may not even be a real package
      // directory in this install layout (e.g. bundled peer). Skip anything we
      // cannot verify as "the peer's package.json is reachable from here".
      const peerManifest = await readManifest(
        join(containingNodeModules, peerName, "package.json"),
      );
      if (!peerManifest) continue;

      const bins = parseBinField(peerName, peerManifest.bin);
      if (bins.length === 0) continue;

      // Ensure the bin directory exists (it may be absent entirely if bun's
      // race skipped every link it would have created for this package).
      await mkdir(binRoot, { recursive: true });

      for (const bin of bins) {
        const linkPath = join(binRoot, bin.name);
        const target = binTarget(peerName, bin.path);

        // Idempotent: skip if anything already occupies this path. We do not
        // overwrite entries bun created; if bun already wrote a .bin/<name>,
        // that is either the correct target (race won) or a close variant we
        // should not second-guess.
        try {
          await lstat(linkPath);
          continue;
        } catch {
          // Does not exist — fall through to create.
        }

        await symlink(target, linkPath);
        healed.push({
          containingEntry: entry,
          containingPkg: pkgName,
          peerName,
          binName: bin.name,
          target,
        });
      }
    }
  }

  if (healed.length > 0) {
    console.log(
      `[heal-peer-dep-bins] healed ${healed.length} missing peer .bin entries:`,
    );
    for (const h of healed) {
      console.log(
        `  ${h.containingEntry}/node_modules/.bin/${h.binName} → ${h.target} (peer ${h.peerName} of ${h.containingPkg})`,
      );
    }
  } else {
    console.log("[heal-peer-dep-bins] nothing to heal");
  }
}

if (import.meta.main) {
  await healPeerDepBins();
}
