#!/usr/bin/env node

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

import { runSetup } from "./setup.js";

if (process.argv[2] === "setup") {
  await runSetup(process.argv.slice(3));
  process.exit(0);
}

const platformByRuntime = {
  "linux-x64": {
    defaultInstall: path.join(
      os.homedir(),
      ".local",
      "share",
      "codex-deepseek",
      "current",
      "bin",
      "codex",
    ),
  },
  "darwin-arm64": {
    defaultInstall: path.join(
      os.homedir(),
      "Library",
      "Application Support",
      "CodexDeepSeek",
      "current",
      "bin",
      "codex",
    ),
  },
  "win32-x64": {
    defaultInstall: path.join(
      process.env.LOCALAPPDATA ??
        path.join(os.homedir(), "AppData", "Local"),
      "CodexDeepSeek",
      "current",
      "bin",
      "codex.exe",
    ),
  },
};

const runtime = `${process.platform}-${process.arch}`;
const platformPackage = platformByRuntime[runtime];
if (!platformPackage) {
  throw new Error(
    `codex-dp does not provide a binary for ${process.platform} (${process.arch}). ` +
      "Supported platforms: Windows x64, Apple Silicon macOS, and Linux x64.",
  );
}

function findCodexExecutable() {
  const candidates = [process.env.CODEX_CLI_PATH];
  if (process.platform === "win32" && process.env.CODEX_DEEPSEEK_INSTALL_DIR) {
    candidates.push(
      path.join(
        process.env.CODEX_DEEPSEEK_INSTALL_DIR,
        "current",
        "bin",
        "codex.exe",
      ),
    );
  } else if (process.platform !== "win32") {
    candidates.push(path.join(os.homedir(), ".local", "bin", "codex-deepseek"));
  }
  candidates.push(platformPackage.defaultInstall);

  for (const candidate of candidates) {
    if (candidate && existsSync(candidate)) {
      return candidate;
    }
  }

  throw new Error(
    "Codex DeepSeek is not installed. Run `codex-dp setup` first.",
  );
}

const child = spawn(findCodexExecutable(), process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

child.on("error", (error) => {
  console.error(error);
  process.exit(1);
});

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    if (!child.killed) {
      try {
        child.kill(signal);
      } catch {
        // The child may already have exited.
      }
    }
  });
}

const result = await new Promise((resolve) => {
  child.on("exit", (exitCode, signal) => {
    resolve(signal ? { signal } : { exitCode: exitCode ?? 1 });
  });
});

if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.exitCode);
}
