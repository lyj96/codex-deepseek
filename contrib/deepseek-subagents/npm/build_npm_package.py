#!/usr/bin/env python3
"""Build the thin codex-dp npm installer tarball."""

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
VERSION_PATTERN = re.compile(r"^\d+\.\d+\.\d+-deepseek\.[1-9]\d*(?:-npm\.[1-9]\d*)?$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--staging-dir", type=Path)
    parser.add_argument("--pack-output", type=Path, required=True)
    return parser.parse_args()


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
        "files": ["bin/codex-dp.js", "bin/setup.js", "README.md", "LICENSE"],
        "repository": {
            "type": "git",
            "url": "git+https://github.com/lyj96/codex-deepseek.git",
            "directory": "contrib/deepseek-subagents/npm",
        },
        "homepage": "https://github.com/lyj96/codex-deepseek",
        "bugs": "https://github.com/lyj96/codex-deepseek/issues",
        "keywords": ["codex", "deepseek", "agent", "cli"],
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
) -> None:
    copy_common_files(staging_dir)
    bin_dir = staging_dir / "bin"
    bin_dir.mkdir()
    for filename in ("codex-dp.js", "setup.js"):
        shutil.copy2(SCRIPT_DIR / "bin" / filename, bin_dir / filename)
    package_json = root_package_json(version)

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
            "--version must use X.Y.Z-deepseek.N or "
            "X.Y.Z-deepseek.N-npm.M, with N and M starting at 1."
        )
    staging_dir = prepare_staging_dir(args.staging_dir)
    stage_package(staging_dir, args.version)
    npm_pack(staging_dir, args.pack_output)
    print(f"Built {NPM_NAME}@{args.version} at {args.pack_output.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
