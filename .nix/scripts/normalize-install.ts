/**
 * Make `bun install --linker=isolated` output bit-reproducible.
 *
 * Entry point for the nixpkgs FOD build — invoke this single script after
 * `bun install` to get a `node_modules/` tree that is byte-identical
 * across machines and runs with the same bun.lock.
 *
 * `bun install --linker=isolated` is not bit-reproducible out of the box,
 * even with a frozen lockfile, a pinned bun version, `--ignore-scripts`,
 * and a clean `BUN_INSTALL_CACHE_DIR`. Running it across a local NixOS
 * sandbox, a GitHub Actions ubuntu-latest runner, and a GitHub Actions
 * macos-latest runner produces subtly different `node_modules/` trees
 * from identical inputs. Two independent sources of drift show up:
 *
 *   - Missing `.bin/<peer>` entries around circular peer dependencies,
 *     caused by a timing race inside bun's isolated installer thread
 *     pool. See `heal-peer-dep-bins.ts` for the full explanation and fix.
 *
 *   - Non-deterministic symlink creation order in `.bun/node_modules/`
 *     and in each per-package `.bin/` directory. NAR hashing sorts
 *     entries during serialization, so this is usually harmless, but we
 *     defensively rebuild both trees in canonical sorted order.
 *     See `canonicalize-node-modules.ts` and `normalize-bun-binaries.ts`.
 *
 * Each sub-script is also runnable standalone for debugging:
 *
 *     bun --bun .nix/scripts/<name>.ts
 */

import { canonicalizeNodeModules } from "./canonicalize-node-modules.ts";
import { healPeerDepBins } from "./heal-peer-dep-bins.ts";
import { normalizeBunBinaries } from "./normalize-bun-binaries.ts";

await canonicalizeNodeModules();
await healPeerDepBins();
await normalizeBunBinaries();
