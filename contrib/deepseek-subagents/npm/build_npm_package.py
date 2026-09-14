#!/usr/bin/env python3
"""Build the codex-dp root or platform-specific npm tarball."""

import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
NPM_NAME = "codex-dp"
VERSION_PATTERN = re.compile(r"^\d+\.\d+\.\d+-deepseek\.[1-9]\d*$")

PLATFORMS = {
    "linux-x64": {
        "alias": "codex-dp-linux-x64",
        "target": "x86_64-unknown-linux-musl",
        "os": "linux",
        "cpu": "x64",
    },
    "darwin-arm64": {
        "alias": "codex-dp-darwin-arm64",
        "target": "aarch64-apple-darwin",
        "os": "darwin",
        "cpu": "arm64",
    },
    "win32-x64": {
        "alias": "codex-dp-win32-x64",
        "target": "x86_64-pc-windows-msvc",
        "os": "win32",
        "cpu": "x64",
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--package",
        choices=("root", *PLATFORMS),
        default="root",
    )
    parser.add_argument("--version", required=True)
    parser.add_argument(
        "--vendor-src",
        type=Path,
        help="Extracted codex package directory for a platform package.",
    )
    parser.add_argument("--staging-dir", type=Path)
    parser.add_argument("--pack-output", type=Path, required=True)
    return parser.parse_args()


def package_version(version: str, package: str) -> str:
    return version if package == "root" else f"{version}-{package}"


def root_package_json(version: str) -> dict:
    return {
        "name": NPM_NAME,
        "version": version,
        "description": (
            "Community Codex fork with DeepSeek external sub-agent support."
        ),
        "license": "Apache-2.0",
        "type": "module",
        "bin": {"codex-dp": "bin/codex-dp.js"},
        "engines": {"node": ">=18"},
        "files": ["bin", "README.md", "LICENSE"],
        "repository": {
            "type": "git",
            "url": "git+https://github.com/lyj96/codex-deepseek.git",
            "directory": "contrib/deepseek-subagents/npm",
        },
        "homepage": "https://github.com/lyj96/codex-deepseek",
        "bugs": "https://github.com/lyj96/codex-deepseek/issues",
        "keywords": ["codex", "deepseek", "agent", "cli"],
        "optionalDependencies": {
            config["alias"]: f"npm:{NPM_NAME}@{version}-{platform}"
            for platform, config in PLATFORMS.items()
        },
    }


def platform_package_json(version: str, platform: str) -> dict:
    config = PLATFORMS[platform]
    return {
        "name": NPM_NAME,
        "version": package_version(version, platform),
        "description": f"Native payload for codex-dp ({platform}).",
        "license": "Apache-2.0",
        "os": [config["os"]],
        "cpu": [config["cpu"]],
        "engines": {"node": ">=18"},
        "files": ["vendor", "README.md", "LICENSE"],
        "repository": {
            "type": "git",
            "url": "git+https://github.com/lyj96/codex-deepseek.git",
            "directory": "contrib/deepseek-subagents/npm",
        },
    }


def prepare_staging_dir(requested: Path | None) -> Path:
    if requested is None:
        return Path(tempfile.mkdtemp(prefix="codex-dp-npm-stage-"))
    staging_dir = requested.resolve()
    staging_dir.mkdir(parents=True, exist_ok=True)
    if any(staging_dir.iterdir()):
        raise RuntimeError(f"Staging directory is not empty: {staging_dir}")
    return staging_dir


def copy_common_files(staging_dir: Path) -> None:
    shutil.copy2(SCRIPT_DIR / "README.md", staging_dir / "README.md")
    shutil.copy2(REPO_ROOT / "LICENSE", staging_dir / "LICENSE")


def stage_package(
    staging_dir: Path,
    version: str,
    package: str,
    vendor_src: Path | None,
) -> None:
    copy_common_files(staging_dir)
    if package == "root":
        shutil.copytree(SCRIPT_DIR / "bin", staging_dir / "bin")
        package_json = root_package_json(version)
    else:
        if vendor_src is None:
            raise RuntimeError(f"--vendor-src is required for {package}")
        vendor_src = vendor_src.resolve()
        target = PLATFORMS[package]["target"]
        binary_name = "codex.exe" if package == "win32-x64" else "codex"
        binary_path = vendor_src / "bin" / binary_name
        if not binary_path.is_file():
            raise RuntimeError(f"Expected native binary not found: {binary_path}")
        shutil.copytree(vendor_src, staging_dir / "vendor" / target)
        package_json = platform_package_json(version, package)

    with (staging_dir / "package.json").open("w", encoding="utf-8") as output:
        json.dump(package_json, output, indent=2)
        output.write("\n")


def npm_pack(staging_dir: Path, output_path: Path) -> None:
    output_path = output_path.resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="codex-dp-npm-pack-") as temp:
        temp_dir = Path(temp)
        env = os.environ.copy()
        env["NPM_CONFIG_CACHE"] = str(temp_dir / "cache")
        npm_command = shutil.which("npm.cmd" if os.name == "nt" else "npm")
        if npm_command is None:
            raise RuntimeError("npm was not found on PATH.")
        result = subprocess.run(
            [npm_command, "pack", "--json", "--pack-destination", str(temp_dir)],
            cwd=staging_dir,
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )
        data = json.loads(result.stdout)
        if not data or not data[0].get("filename"):
            raise RuntimeError("npm pack did not report a tarball.")
        generated = temp_dir / data[0]["filename"]
        if not generated.is_file():
            raise RuntimeError(f"npm pack output is missing: {generated}")
        shutil.move(generated, output_path)


def main() -> int:
    args = parse_args()
    if not VERSION_PATTERN.fullmatch(args.version):
        raise RuntimeError(
            "--version must use the form X.Y.Z-deepseek.N, with N starting at 1."
        )
    staging_dir = prepare_staging_dir(args.staging_dir)
    stage_package(staging_dir, args.version, args.package, args.vendor_src)
    npm_pack(staging_dir, args.pack_output)
    print(
        f"Built {NPM_NAME}@{package_version(args.version, args.package)} "
        f"at {args.pack_output.resolve()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
