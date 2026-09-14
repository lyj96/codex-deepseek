#!/usr/bin/env node

import { spawn } from "node:child_process";
import { existsSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { runSetup } from "./setup.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);
const packageRoot = realpathSync(path.join(__dirname, ".."));

if (process.argv[2] === "setup") {
  await runSetup(process.argv.slice(3));
  process.exit(0);
}

const platformByRuntime = {
  "linux-x64": {
    packageName: "codex-dp-linux-x64",
    target: "x86_64-unknown-linux-musl",
  },
  "darwin-arm64": {
    packageName: "codex-dp-darwin-arm64",
    target: "aarch64-apple-darwin",
  },
  "win32-x64": {
    packageName: "codex-dp-win32-x64",
    target: "x86_64-pc-windows-msvc",
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
  let vendorRoot;
  try {
    const packageJsonPath = require.resolve(
      `${platformPackage.packageName}/package.json`,
    );
    vendorRoot = path.join(path.dirname(packageJsonPath), "vendor");
  } catch {
    vendorRoot = path.join(packageRoot, "vendor");
  }

  const binaryPath = path.join(
    vendorRoot,
    platformPackage.target,
    "bin",
    process.platform === "win32" ? "codex.exe" : "codex",
  );
  if (existsSync(binaryPath)) {
    return binaryPath;
  }

  throw new Error(
    `Missing optional dependency ${platformPackage.packageName}. ` +
      "Reinstall with: npm install -g codex-dp@latest",
  );
}

const env = {
  ...process.env,
  CODEX_MANAGED_BY_NPM: "1",
  CODEX_MANAGED_PACKAGE_ROOT: packageRoot,
};

const child = spawn(findCodexExecutable(), process.argv.slice(2), {
  stdio: "inherit",
  env,
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
