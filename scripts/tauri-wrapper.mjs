#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

function isWindows() {
  return process.platform === "win32";
}

function hasValidVulkanSdk(root) {
  if (!root) return false;
  return (
    existsSync(path.join(root, "Include", "vulkan", "vulkan.h")) &&
    existsSync(path.join(root, "Lib", "vulkan-1.lib")) &&
    existsSync(path.join(root, "Bin", "glslc.exe"))
  );
}

function resolveVulkanSdkRoot(current) {
  if (hasValidVulkanSdk(current)) {
    return current;
  }

  const roots = ["C:\\VulkanSDK", "D:\\VulkanSDK", "E:\\VulkanSDK", "F:\\VulkanSDK"];
  const candidates = [];

  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const entry of readdirSync(root, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        candidates.push(path.join(root, entry.name));
      }
    }
  }

  candidates.sort((a, b) =>
    b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" }),
  );

  for (const candidate of candidates) {
    if (hasValidVulkanSdk(candidate)) {
      return candidate;
    }
  }

  return null;
}

function prependPath(dir) {
  if (!dir) return;

  const sep = isWindows() ? ";" : ":";
  const current = process.env.PATH || "";
  const parts = current.split(sep).filter(Boolean);
  const normalizedDir = isWindows() ? dir.toLowerCase() : dir;
  const deduped = parts.filter((p) => (isWindows() ? p.toLowerCase() : p) !== normalizedDir);
  process.env.PATH = [dir, ...deduped].join(sep);
}

function prependFlag(envKey, flag) {
  const current = (process.env[envKey] || "").trim();
  if (!current) {
    process.env[envKey] = flag;
    return;
  }

  const escaped = flag.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (!new RegExp(`(^|\\s)${escaped}(\\s|$)`, "i").test(current)) {
    process.env[envKey] = `${flag} ${current}`;
  }
}

function ensureMsvcUtf8Flag() {
  prependFlag("CL", "/utf-8");
  prependFlag("CMAKE_CXX_FLAGS", "/utf-8");
  prependFlag("CMAKE_CXX_FLAGS_RELWITHDEBINFO", "/utf-8");
}

function commandExists(command) {
  const checkCmd = isWindows() ? "where.exe" : "which";
  const result = spawnSync(checkCmd, [command], {
    stdio: "ignore",
    shell: isWindows(),
  });
  return result.status === 0;
}

function shouldApplyLocalBuildConfig(args) {
  return args.length > 0 && args[0] === "build";
}

function hasConfigOverride(args) {
  return args.some((arg, i) => arg === "--config" || (i > 0 && args[i - 1] === "--config") || arg.startsWith("--config="));
}

function hasBundlesOverride(args) {
  return args.some((arg, i) => arg === "--bundles" || (i > 0 && args[i - 1] === "--bundles") || arg.startsWith("--bundles="));
}

function maybeAddUnsignedBuildConfig(args) {
  if (!isWindows()) return args;
  if (!shouldApplyLocalBuildConfig(args)) return args;
  if (hasConfigOverride(args)) return args;
  if (commandExists("trusted-signing-cli")) return args;

  const baseConfigPath = path.join(process.cwd(), "src-tauri", "tauri.conf.json");
  if (!existsSync(baseConfigPath)) return args;

  const config = JSON.parse(readFileSync(baseConfigPath, "utf8"));
  if (!config?.bundle?.windows?.signCommand) return args;
  config.bundle.windows.signCommand = "where.exe /q cmd";
  config.bundle.createUpdaterArtifacts = false;
  if (!hasBundlesOverride(args)) {
    config.bundle.targets = "nsis";
  }

  const tmpDir = mkdtempSync(path.join(os.tmpdir(), "handy-tauri-"));
  const tmpConfigPath = path.join(tmpDir, "tauri.local.conf.json");
  writeFileSync(tmpConfigPath, JSON.stringify(config, null, 2), "utf8");

  console.log("[tauri-wrapper] trusted-signing-cli not found, using local unsigned NSIS build config.");
  return [...args, "--config", tmpConfigPath];
}

if (isWindows()) {
  const previous = process.env.VULKAN_SDK;
  const resolved = resolveVulkanSdkRoot(previous);
  if (resolved) {
    process.env.VULKAN_SDK = resolved;
    process.env.VK_SDK_PATH = resolved;
    prependPath(path.join(resolved, "Bin"));
    if (previous !== resolved) {
      console.log(`[tauri-wrapper] VULKAN_SDK -> ${resolved}`);
    }
  }

  ensureMsvcUtf8Flag();
}

const args = maybeAddUnsignedBuildConfig(process.argv.slice(2));
const tauriCommand = "tauri";
const result = spawnSync(tauriCommand, args, {
  stdio: "inherit",
  env: process.env,
  shell: isWindows(),
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
