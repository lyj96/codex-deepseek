import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  createReadStream,
  createWriteStream,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import https from "node:https";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const repo = "lyj96/codex-deepseek";
const supportedRedirectHost = (hostname) =>
  hostname === "github.com" || hostname.endsWith(".githubusercontent.com");

export function releaseTagForVersion(version) {
  const match = /^(\d+\.\d+\.\d+)-deepseek\.([1-9]\d*)(?:-npm\.[1-9]\d*)?$/.exec(
    version,
  );
  if (!match) {
    throw new Error(`Unsupported codex-dp package version: ${version}`);
  }
  return `codex-v${match[1]}-deepseek.${match[2]}`;
}

function printHelp() {
  console.log(`Usage: codex-dp setup [options]

Options:
  --install-dir PATH    Install Codex DeepSeek for Desktop in PATH
  --deepseek-key KEY    Configure a DeepSeek API key (hidden prompt is safer)
  --ssh-host HOST       Install/update a registered Linux x64 SSH host
  --discover-ssh        List SSH hosts found in the local SSH config
  --update-remotes      Update all previously registered SSH hosts
  --yes                 Accept installer confirmation prompts
  --help                Show this help

The setup command downloads the installer from the matching GitHub Release,
verifies it against SHA256SUMS, and then runs it.`);
}

function download(url, destination, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    if (
      parsed.protocol !== "https:" ||
      !supportedRedirectHost(parsed.hostname)
    ) {
      reject(new Error(`Refusing download from unexpected URL: ${url}`));
      return;
    }

    const request = https.get(parsed, (response) => {
      if (
        [301, 302, 303, 307, 308].includes(response.statusCode) &&
        response.headers.location
      ) {
        response.resume();
        if (redirectsLeft === 0) {
          reject(new Error(`Too many redirects while downloading ${url}`));
          return;
        }
        const redirectUrl = new URL(
          response.headers.location,
          parsed,
        ).toString();
        download(redirectUrl, destination, redirectsLeft - 1).then(
          resolve,
          reject,
        );
        return;
      }

      if (response.statusCode !== 200) {
        response.resume();
        reject(
          new Error(`Download failed with HTTP ${response.statusCode}: ${url}`),
        );
        return;
      }

      const output = createWriteStream(destination, { mode: 0o600 });
      response.pipe(output);
      output.on("finish", () => output.close(resolve));
      output.on("error", reject);
    });
    request.on("error", reject);
  });
}

function sha256(filePath) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const stream = createReadStream(filePath);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("end", () => resolve(hash.digest("hex")));
    stream.on("error", reject);
  });
}

function windowsArguments(args, releaseTag) {
  const valueOptions = new Map([
    ["--install-dir", "-InstallDir"],
    ["--deepseek-key", "-DeepSeekKey"],
    ["--ssh-host", "-SshHost"],
  ]);
  const switchOptions = new Map([
    ["--discover-ssh", "-DiscoverSsh"],
    ["--update-remotes", "-UpdateRemotes"],
    ["--yes", "-Yes"],
  ]);
  const translated = [];
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--release" || argument === "-ReleaseTag") {
      throw new Error(
        "codex-dp setup always uses its matching release version.",
      );
    }
    if (valueOptions.has(argument)) {
      if (index + 1 >= args.length) {
        throw new Error(`${argument} requires a value.`);
      }
      translated.push(valueOptions.get(argument), args[index + 1]);
      index += 1;
    } else if (switchOptions.has(argument)) {
      translated.push(switchOptions.get(argument));
    } else {
      translated.push(argument);
    }
  }
  translated.push("-ReleaseTag", releaseTag);
  return translated;
}

function unixArguments(args, releaseTag) {
  if (args.includes("--release")) {
    throw new Error("codex-dp setup always uses its matching release version.");
  }
  return [...args, "--release", releaseTag];
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: "inherit", env: process.env });
    child.on("error", reject);
    child.on("exit", (exitCode, signal) => {
      if (signal) {
        reject(new Error(`Installer exited after receiving ${signal}.`));
      } else if (exitCode === 0) {
        resolve();
      } else {
        reject(new Error(`Installer exited with status ${exitCode ?? 1}.`));
      }
    });
  });
}

export async function runSetup(args) {
  if (args.includes("--help") || args.includes("-h")) {
    printHelp();
    return;
  }

  const installerByPlatform = {
    win32: "install-windows.ps1",
    darwin: "install-macos.sh",
    linux: "install-linux.sh",
  };
  const installerName = installerByPlatform[process.platform];
  if (!installerName) {
    throw new Error(`Unsupported setup platform: ${process.platform}`);
  }

  const packageJson = JSON.parse(
    readFileSync(path.join(__dirname, "..", "package.json"), "utf8"),
  );
  const releaseTag = releaseTagForVersion(packageJson.version);
  const releaseBase = `https://github.com/${repo}/releases/download/${releaseTag}`;
  const tempDirectory = mkdtempSync(path.join(os.tmpdir(), "codex-dp-setup-"));
  const installerPath = path.join(tempDirectory, installerName);
  const checksumsPath = path.join(tempDirectory, "SHA256SUMS");

  try {
    console.log(`Downloading verified installer for ${releaseTag}...`);
    await Promise.all([
      download(`${releaseBase}/${installerName}`, installerPath),
      download(`${releaseBase}/SHA256SUMS`, checksumsPath),
    ]);

    const checksumLine = readFileSync(checksumsPath, "utf8")
      .split(/\r?\n/)
      .find((line) => line.trim().endsWith(` ${installerName}`));
    const expected = checksumLine?.trim().split(/\s+/)[0]?.toLowerCase();
    const actual = await sha256(installerPath);
    if (!expected || !/^[0-9a-f]{64}$/.test(expected) || actual !== expected) {
      throw new Error(`SHA-256 verification failed for ${installerName}.`);
    }

    chmodSync(installerPath, 0o700);
    if (process.platform === "win32") {
      await run("powershell.exe", [
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        installerPath,
        ...windowsArguments(args, releaseTag),
      ]);
    } else {
      await run("bash", [installerPath, ...unixArguments(args, releaseTag)]);
    }
  } finally {
    rmSync(tempDirectory, { recursive: true, force: true });
  }
}
