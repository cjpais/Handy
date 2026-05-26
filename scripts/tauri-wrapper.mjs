#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import http from "node:http";
import path from "node:path";

function isWindows() {
  return process.platform === "win32";
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
  });
  return result.status === 0;
}

function resolveLocalCommand(command) {
  if (command === "tauri") {
    const tauriJs = path.join(process.cwd(), "node_modules", "@tauri-apps", "cli", "tauri.js");
    if (existsSync(tauriJs)) {
      return { command: process.execPath, argsPrefix: [tauriJs] };
    }
  }

  const names = isWindows()
    ? [`${command}.exe`, `${command}.bunx`, command]
    : [command];

  for (const name of names) {
    const candidate = path.join(process.cwd(), "node_modules", ".bin", name);
    if (existsSync(candidate)) {
      return { command: candidate, argsPrefix: [] };
    }
  }

  return { command, argsPrefix: [] };
}

function shouldApplyLocalBuildConfig(args) {
  return args.length > 0 && args[0] === "build";
}

function isBuildOrDev(args) {
  return args.length > 0 && (args[0] === "build" || args[0] === "dev");
}

// NSIS has a ~2GB hard limit on total installer size. If bundled models push
// resources above this threshold, NSIS makensis crashes with "error mmapping
// file ... is out of range". WiX/MSI has no such limit and is the right choice
// for fat installers.
const NSIS_RESOURCE_BUDGET_BYTES = 1.6 * 1024 * 1024 * 1024; // 1.6 GB safety margin

function dirSizeBytes(p) {
  let total = 0;
  const stack = [p];
  while (stack.length) {
    const cur = stack.pop();
    let st;
    try {
      st = statSync(cur);
    } catch {
      continue;
    }
    if (st.isDirectory()) {
      let entries;
      try {
        entries = readdirSync(cur);
      } catch {
        continue;
      }
      for (const entry of entries) stack.push(path.join(cur, entry));
    } else {
      total += st.size;
    }
  }
  return total;
}

function detectPreferredBundler() {
  const resourcesDir = path.join(process.cwd(), "src-tauri", "resources");
  if (!existsSync(resourcesDir)) return "nsis";
  const size = dirSizeBytes(resourcesDir);
  return size > NSIS_RESOURCE_BUDGET_BYTES ? "msi" : "nsis";
}

// WiX light.exe writes CAB temp files (often 2-3x the source size) to %TEMP%.
// On Windows the default %TEMP% lives on C:\, which frequently runs out of
// space for fat installers. Redirect TEMP/TMP into the project drive so light
// has room to breathe. Caller is responsible for cleaning the dir afterwards.
function ensureSpaciousTempDir(forBundler) {
  if (!isWindows()) return;
  if (forBundler !== "msi") return;

  const projectRoot = process.cwd();
  const customTemp = path.join(projectRoot, "src-tauri", "target", "wix-temp");
  try {
    if (!existsSync(customTemp)) {
      mkdirSync(customTemp, { recursive: true });
    }
  } catch (e) {
    console.warn(`[tauri-wrapper] could not create ${customTemp}: ${e.message}`);
    return;
  }

  process.env.TEMP = customTemp;
  process.env.TMP = customTemp;
  console.log(`[tauri-wrapper] redirected TEMP/TMP -> ${customTemp} (for WiX light.exe CAB temp)`);
}

function runPrepareBundledModels() {
  const script = path.join(process.cwd(), "scripts", "prepare-bundled-models.mjs");
  if (!existsSync(script)) return;
  const result = spawnSync(process.execPath, [script], {
    stdio: "inherit",
    env: process.env,
  });
  if (result.status !== 0) {
    console.error("[tauri-wrapper] prepare-bundled-models.mjs failed");
    process.exit(result.status ?? 1);
  }
}

// Stage Runtime/VC_redist.x64.exe into src-tauri/resources/Runtime/ so the
// Tauri bundler picks it up via resources/**/* and ships it inside the NSIS
// installer. Only relevant for Windows builds — calling on macOS/Linux would
// just bloat their bundles for no reason. The NSIS template's `Section
// VCRedist` then runs it silently on first install when the registry shows
// VC++ 2015-2022 x64 runtime is missing.
function stageVcRedistForWindows(args) {
  if (!isWindows()) return;
  if (!isBuildOrDev(args)) return;
  // No need to ship the redist into the dev tree.
  if (args[0] === "dev") return;

  const projectRoot = process.cwd();
  const src = path.join(projectRoot, "Runtime", "VC_redist.x64.exe");
  if (!existsSync(src)) {
    console.warn(
      `[tauri-wrapper] Runtime/VC_redist.x64.exe not found at ${src}; ` +
        "NSIS will skip the VC redist bootstrap section. " +
        "Drop the binary in place if you want offline VC++ install support.",
    );
    return;
  }

  const dstDir = path.join(projectRoot, "src-tauri", "resources", "Runtime");
  const dst = path.join(dstDir, "VC_redist.x64.exe");

  let needsCopy = true;
  if (existsSync(dst)) {
    try {
      const srcStat = statSync(src);
      const dstStat = statSync(dst);
      if (srcStat.size === dstStat.size && srcStat.mtimeMs <= dstStat.mtimeMs) {
        needsCopy = false;
      }
    } catch {
      // fall through to copy
    }
  }

  if (!needsCopy) {
    console.log(`[tauri-wrapper] VC_redist.x64.exe already staged at ${dst}`);
    return;
  }

  mkdirSync(dstDir, { recursive: true });
  copyFileSync(src, dst);
  const sizeMb = (statSync(dst).size / 1024 / 1024).toFixed(1);
  console.log(`[tauri-wrapper] staged VC_redist.x64.exe (${sizeMb} MB) -> ${dst}`);
}

function shouldApplyLocalDevConfig(args) {
  return args.length > 0 && args[0] === "dev";
}

function hasConfigOverride(args) {
  return args.some((arg, i) => arg === "--config" || (i > 0 && args[i - 1] === "--config") || arg.startsWith("--config="));
}

function hasBundlesOverride(args) {
  return args.some((arg, i) => arg === "--bundles" || (i > 0 && args[i - 1] === "--bundles") || arg.startsWith("--bundles="));
}

function createTempConfig(config) {
  const projectRoot = process.cwd();
  const tmpDir = mkdtempSync(path.join(projectRoot, "src-tauri", "tauri-wrapper-"));
  const tmpConfigPath = path.join(tmpDir, "tauri.local.conf.json");
  writeFileSync(tmpConfigPath, JSON.stringify(config, null, 2), "utf8");
  process.on("exit", () => {
    rmSync(tmpDir, { recursive: true, force: true });
  });
  return tmpConfigPath;
}

function waitForHttp(url, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;

  return new Promise((resolve, reject) => {
    const poll = () => {
      const req = http.get(url, (res) => {
        res.resume();
        resolve();
      });

      req.on("error", (error) => {
        if (Date.now() >= deadline) {
          reject(error);
          return;
        }
        setTimeout(poll, 250);
      });

      req.setTimeout(1000, () => {
        req.destroy();
      });
    };

    poll();
  });
}

function startFrontendDevServerIfNeeded(args) {
  if (!isWindows()) return { args, child: null };
  if (!shouldApplyLocalDevConfig(args)) return { args, child: null };
  if (hasConfigOverride(args)) return { args, child: null };

  const projectRoot = process.cwd();
  const child = spawn("bun", ["run", "dev"], {
    cwd: projectRoot,
    env: process.env,
    stdio: "inherit",
    windowsHide: true,
  });

  const cleanup = () => {
    if (!child.killed) {
      child.kill();
    }
  };

  child.on("exit", (code, signal) => {
    if (code !== 0 && signal !== "SIGTERM") {
      console.error(`[tauri-wrapper] frontend dev server exited with code ${code ?? signal}`);
    }
  });

  process.on("exit", cleanup);
  process.on("SIGINT", () => {
    cleanup();
    process.exit(130);
  });
  process.on("SIGTERM", () => {
    cleanup();
    process.exit(143);
  });

  const configPath = createTempConfig({
    build: {
      beforeDevCommand: "",
      devUrl: "http://localhost:1420",
      frontendDist: "../dist",
    },
  });

  console.log("[tauri-wrapper] started frontend dev server directly; disabled Tauri beforeDevCommand.");
  return { args: [...args, "--config", configPath], child };
}

function maybeAddUnsignedBuildConfig(args) {
  if (!isWindows()) return args;
  if (!shouldApplyLocalBuildConfig(args)) return args;
  if (hasConfigOverride(args)) return args;
  if (commandExists("trusted-signing-cli")) return args;

  const projectRoot = process.cwd();
  const baseConfigPath = path.join(projectRoot, "src-tauri", "tauri.conf.json");
  if (!existsSync(baseConfigPath)) return args;

  const baseConfig = JSON.parse(readFileSync(baseConfigPath, "utf8"));
  if (!baseConfig?.bundle?.windows?.signCommand) return args;

  const config = {
    build: {
      // Tauri treats quoted --cwd values literally on Windows, so keep the
      // path unquoted here and use Bun's supported flag order.
      beforeBuildCommand: `bun run --cwd ${projectRoot} build`,
      frontendDist: "../dist",
    },
    bundle: {
      createUpdaterArtifacts: false,
      windows: {
        signCommand: "where.exe /q cmd",
      },
    },
  };

  if (!hasBundlesOverride(args)) {
    const preferred = detectPreferredBundler();
    config.bundle.targets = preferred;
    if (preferred === "msi") {
      console.log(
        "[tauri-wrapper] resources/ exceeds NSIS ~2GB limit, defaulting to MSI (WiX) bundler.",
      );
    }
    ensureSpaciousTempDir(preferred);
  } else {
    const explicit = args
      .map((a, i) => (a === "--bundles" ? args[i + 1] : a.startsWith("--bundles=") ? a.slice(10) : null))
      .find(Boolean);
    if (explicit && explicit.split(",").map((s) => s.trim()).includes("msi")) {
      ensureSpaciousTempDir("msi");
    }
  }

  const tmpConfigPath = createTempConfig(config);

  console.log("[tauri-wrapper] trusted-signing-cli not found, using local unsigned NSIS build config.");
  return [...args, "--config", tmpConfigPath];
}

if (isWindows()) {
  ensureMsvcUtf8Flag();
  pinGgmlCpuBaseline();
}

// Force whisper-rs-sys / ggml to compile against a portable x86_64 baseline so
// the resulting binary doesn't crash with STATUS_ILLEGAL_INSTRUCTION (0xc000001d)
// on user machines that lack the build host's instruction set extensions.
//
// Implementation note: whisper-rs-sys's build.rs forwards env vars whose name
// starts with `CMAKE_` straight to `cmake -D`. So setting CMAKE_TOOLCHAIN_FILE
// makes cmake load our file before any `option(GGML_...)` runs, and the cached
// FORCE assignments stick.
function pinGgmlCpuBaseline() {
  if (process.env.HANDY_DISABLE_GGML_CPU_BASELINE === "1") {
    console.log("[tauri-wrapper] ggml CPU baseline disabled via HANDY_DISABLE_GGML_CPU_BASELINE=1");
    return;
  }
  if (process.env.CMAKE_TOOLCHAIN_FILE) {
    console.log(
      `[tauri-wrapper] CMAKE_TOOLCHAIN_FILE already set (${process.env.CMAKE_TOOLCHAIN_FILE}); not overriding ggml baseline`,
    );
    return;
  }
  const toolchain = path.join(process.cwd(), "scripts", "ggml-cpu-baseline.cmake");
  if (!existsSync(toolchain)) {
    console.warn(`[tauri-wrapper] ggml-cpu-baseline.cmake not found at ${toolchain}, skipping`);
    return;
  }
  process.env.CMAKE_TOOLCHAIN_FILE = toolchain;
  console.log(`[tauri-wrapper] CMAKE_TOOLCHAIN_FILE -> ${toolchain} (ggml CPU baseline: AVX+FMA+F16C, no AVX2/AVX512)`);
}

async function main() {
  const rawArgs = process.argv.slice(2);
  if (isBuildOrDev(rawArgs)) {
    runPrepareBundledModels();
    stageVcRedistForWindows(rawArgs);
  }

  const dev = startFrontendDevServerIfNeeded(rawArgs);
  if (dev.child) {
    try {
      await waitForHttp("http://localhost:1420");
    } catch (error) {
      console.error(`[tauri-wrapper] frontend dev server did not become ready: ${error.message}`);
      process.exit(1);
    }
  }

  const args = maybeAddUnsignedBuildConfig(dev.args);
  const tauriCommand = resolveLocalCommand("tauri");
  const result = spawnSync(tauriCommand.command, [...tauriCommand.argsPrefix, ...args], {
    stdio: "inherit",
    env: process.env,
  });

  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }

  process.exit(result.status ?? 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
