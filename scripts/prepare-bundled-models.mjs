#!/usr/bin/env node
// Copies models listed in src-tauri/bundled-models.json from sourceDir into
// src-tauri/resources/models/ so tauri build includes them in the installer.
// Skips files that are already up-to-date (same size). Logs everything.

import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, statSync } from "node:fs";
import path from "node:path";

const projectRoot = process.cwd();
const configPath = path.join(projectRoot, "src-tauri", "bundled-models.json");
const resourcesDir = path.join(projectRoot, "src-tauri", "resources", "models");

function log(msg) {
  console.log(`[prepare-bundled-models] ${msg}`);
}

function dirSize(p) {
  let total = 0;
  const stack = [p];
  while (stack.length) {
    const cur = stack.pop();
    const st = statSync(cur);
    if (st.isDirectory()) {
      for (const entry of readdirSync(cur)) {
        stack.push(path.join(cur, entry));
      }
    } else {
      total += st.size;
    }
  }
  return total;
}

import { readdirSync } from "node:fs";

function sameSize(src, dst) {
  try {
    const srcStat = statSync(src);
    const dstStat = statSync(dst);
    if (srcStat.isDirectory() !== dstStat.isDirectory()) return false;
    if (srcStat.isDirectory()) {
      return dirSize(src) === dirSize(dst);
    }
    return srcStat.size === dstStat.size;
  } catch {
    return false;
  }
}

function humanSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function main() {
  if (!existsSync(configPath)) {
    log(`No bundled-models.json at ${configPath}, skipping.`);
    return;
  }

  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const models = Array.isArray(config.models) ? config.models : [];

  if (models.length === 0) {
    log("No models listed in bundled-models.json. Cleaning resources/models entries.");
    // Note: we don't wipe the directory because silero_vad_v4.onnx & gigaam_vocab.txt live here too.
    return;
  }

  const sourceDir = config.sourceDir;
  if (!sourceDir) {
    console.error("[prepare-bundled-models] ERROR: 'sourceDir' missing in bundled-models.json");
    process.exit(1);
  }

  if (!existsSync(sourceDir)) {
    console.error(`[prepare-bundled-models] ERROR: sourceDir does not exist: ${sourceDir}`);
    process.exit(1);
  }

  mkdirSync(resourcesDir, { recursive: true });

  // Files we must always keep (not bundled models, but other runtime resources).
  const KEEP = new Set([".gitkeep", "silero_vad_v4.onnx", "gigaam_vocab.txt"]);
  const wanted = new Set(models.map((m) => m.filename));
  for (const existing of readdirSync(resourcesDir)) {
    if (KEEP.has(existing)) continue;
    if (wanted.has(existing)) continue;
    const stale = path.join(resourcesDir, existing);
    log(`PRUNE ${existing} (not in bundled-models.json)`);
    rmSync(stale, { recursive: true, force: true });
  }

  for (const entry of models) {
    const { filename, type } = entry;
    if (!filename || !type) {
      console.error(`[prepare-bundled-models] ERROR: model entry missing filename/type: ${JSON.stringify(entry)}`);
      process.exit(1);
    }

    const src = path.join(sourceDir, filename);
    const dst = path.join(resourcesDir, filename);

    if (!existsSync(src)) {
      console.error(`[prepare-bundled-models] ERROR: source not found: ${src}`);
      process.exit(1);
    }

    const srcStat = statSync(src);
    const expectDir = type === "directory";
    if (srcStat.isDirectory() !== expectDir) {
      console.error(
        `[prepare-bundled-models] ERROR: ${filename} type mismatch (config=${type}, actual=${srcStat.isDirectory() ? "directory" : "file"})`,
      );
      process.exit(1);
    }

    if (sameSize(src, dst)) {
      log(`SKIP ${filename} (already up-to-date)`);
      continue;
    }

    if (existsSync(dst)) {
      log(`REPLACE ${filename}`);
      rmSync(dst, { recursive: true, force: true });
    } else {
      log(`COPY ${filename}`);
    }

    try {
      // For directories, filter out macOS AppleDouble metadata files (._*)
      // that get created when the source disk was used on macOS. They are
      // not real ONNX/model files and can cause cpSync to fail on Windows.
      const filter = expectDir
        ? (s) => {
            const base = path.basename(s);
            return !base.startsWith("._") && base !== ".DS_Store";
          }
        : undefined;
      cpSync(src, dst, { recursive: expectDir, filter, errorOnExist: false, force: true });
    } catch (err) {
      console.error(`[prepare-bundled-models] ERROR copying ${filename}: ${err.message}`);
      console.error(`  src: ${src}`);
      console.error(`  dst: ${dst}`);
      if (err.code) console.error(`  code: ${err.code}`);
      if (err.path) console.error(`  path: ${err.path}`);
      process.exit(1);
    }

    const size = expectDir ? dirSize(dst) : statSync(dst).size;
    log(`  -> ${dst} (${humanSize(size)})`);
  }

  log(`Done. ${models.length} bundled model(s) ready in ${resourcesDir}`);
}

main();
