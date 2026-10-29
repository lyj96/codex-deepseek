"""Run the original Dylint package tests with its immutable Clippy history.

Dylint 6.1 resolves a legacy `master` branch while building its driver. Provide
that name in a private mirror of the revision already pinned by this package,
without changing dependencies, tests, or the user's Git configuration.
"""

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib


CLIPPY_URL = "https://github.com/rust-lang/rust-clippy"


@dataclass(frozen=True)
class Pins:
    revision: str
    nightly: str
    dylint_version: str


def read_pins(package: Path) -> Pins:
    manifest = tomllib.loads((package / "Cargo.toml").read_text(encoding="utf-8"))
    dependency = manifest["dependencies"]["clippy_utils"]
    revision = dependency.get("rev", "")
    if dependency.get("git") != CLIPPY_URL or not re.fullmatch(
        r"[0-9a-f]{40}", revision
    ):
        raise ValueError("Clippy must use the official URL and an immutable revision")
    nightly = tomllib.loads((package / "rust-toolchain").read_text(encoding="utf-8"))[
        "toolchain"
    ]["channel"]
    if not re.fullmatch(r"nightly-\d{4}-\d{2}-\d{2}", nightly):
        raise ValueError("The lint package must pin a dated nightly")
    version = manifest["dependencies"]["dylint_linting"]
    if not isinstance(version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("The lint package must pin a released Dylint version")
    if manifest["dev-dependencies"]["dylint_testing"] != version:
        raise ValueError("Dylint library and test versions differ")
    locked = tomllib.loads((package / "Cargo.lock").read_text(encoding="utf-8"))[
        "package"
    ]
    for name in ("clippy_utils", "dylint_linting", "dylint_testing"):
        matches = [item for item in locked if item["name"] == name]
        if len(matches) != 1:
            raise ValueError("Ambiguous or missing lint dependency in Cargo.lock")
        item = matches[0]
        if name == "clippy_utils":
            if item.get("source") != f"git+{CLIPPY_URL}?rev={revision}#{revision}":
                raise ValueError("Clippy manifest and lock revisions differ")
        elif item["version"] != version:
            raise ValueError("Dylint manifest and lock versions differ")
    return Pins(revision, nightly, version)


def git_environment(environment: dict[str, str], mirror: Path) -> dict[str, str]:
    child = environment.copy()
    count_text = child.get("GIT_CONFIG_COUNT", "0")
    if not re.fullmatch(r"[0-9]+", count_text) or int(count_text) > 32:
        raise ValueError("Invalid inherited Git configuration count")
    count = int(count_text)
    for index in range(count):
        if any(
            f"GIT_CONFIG_{field}_{index}" not in child for field in ("KEY", "VALUE")
        ):
            raise ValueError("Incomplete inherited Git configuration")
    for offset, (key, value) in enumerate(
        (
            (f"url.{mirror.as_uri()}.insteadOf", CLIPPY_URL),
            ("protocol.file.allow", "always"),
        )
    ):
        child[f"GIT_CONFIG_KEY_{count + offset}"] = key
        child[f"GIT_CONFIG_VALUE_{count + offset}"] = value
    child["GIT_CONFIG_COUNT"] = str(count + 2)
    child["CARGO_NET_GIT_FETCH_WITH_CLI"] = "true"
    return child


def checked(args: list[str], *, environment=None, capture=False) -> str:
    result = subprocess.run(
        args,
        env=environment,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        check=True,
    )
    return result.stdout.strip() if capture else ""


def verify_clone(clone: Path, pins: Pins, tree: str, environment) -> None:
    for ref, expected in (
        ("HEAD", pins.revision),
        ("master", pins.revision),
        ("HEAD^{tree}", tree),
    ):
        if (
            checked(
                ["git", "-C", str(clone), "rev-parse", ref],
                environment=environment,
                capture=True,
            )
            != expected
        ):
            raise ValueError("Clippy clone does not match the pinned source")
    if checked(
        ["git", "-C", str(clone), "status", "--porcelain"],
        environment=environment,
        capture=True,
    ):
        raise ValueError("Clippy clone probe is dirty")


def prepare(cache: Path, pins: Pins, environment: dict[str, str]):
    mirror = cache / "clippy-mirror.git"
    checked(["git", "init", "--quiet", "--bare", str(mirror)])
    # Full past history is required by Dylint's first-parent nightly lookup.
    checked(
        [
            "git",
            "-C",
            str(mirror),
            "fetch",
            "--quiet",
            "--no-tags",
            CLIPPY_URL,
            f"{pins.revision}:refs/heads/master",
        ]
    )
    checked(["git", "-C", str(mirror), "symbolic-ref", "HEAD", "refs/heads/master"])
    if (
        checked(["git", "-C", str(mirror), "rev-parse", "master"], capture=True)
        != pins.revision
    ):
        raise ValueError("Clippy mirror revision mismatch")
    if (
        checked(
            ["git", "-C", str(mirror), "rev-parse", "--is-shallow-repository"],
            capture=True,
        )
        != "false"
    ):
        raise ValueError("Dylint requires the pinned revision's full history")
    toolchain = checked(
        ["git", "-C", str(mirror), "show", f"{pins.revision}:rust-toolchain.toml"],
        capture=True,
    )
    if tomllib.loads(toolchain)["toolchain"]["channel"] != pins.nightly:
        raise ValueError("Pinned Clippy revision and lint nightly differ")
    tree = checked(
        ["git", "-C", str(mirror), "rev-parse", "master^{tree}"], capture=True
    )
    child = git_environment(environment, mirror)
    probe = cache / "clone-probe"
    checked(["git", "clone", "--quiet", CLIPPY_URL, str(probe)], environment=child)
    verify_clone(probe, pins, tree, child)
    child["RUSTUP_TOOLCHAIN"] = pins.nightly
    temporary = cache / "tmp"
    temporary.mkdir()
    for name in ("TMPDIR", "TMP", "TEMP"):
        child[name] = str(temporary)
    installed = checked(
        ["cargo", "dylint", "--version"], environment=child, capture=True
    )
    if installed != f"cargo-dylint {pins.dylint_version}" or not shutil.which(
        "dylint-link", path=child.get("PATH")
    ):
        raise ValueError("Installed Dylint tools do not match the package")
    for name in ("CARGO_DYLINT_VERSION", "DYLINT_LINK_VERSION"):
        if name in child and child[name] != pins.dylint_version:
            raise ValueError("CI Dylint tool version differs from the package")
    return child, tree


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[2]
    )
    parser.add_argument(
        "--cache-parent",
        type=Path,
        default=Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())),
    )
    args = parser.parse_args()
    package = args.root.resolve() / "tools/argument-comment-lint"
    pins = read_pins(package)
    parent = args.cache_parent
    if (
        not parent.is_absolute()
        or parent.resolve(strict=True) != parent
        or not parent.is_dir()
    ):
        raise ValueError(
            "Cache parent must be an existing absolute directory without symlinks"
        )
    cache = Path(tempfile.mkdtemp(prefix="codex-dylint-", dir=parent))
    record = {
        "revision": pins.revision,
        "nightly": pins.nightly,
        "dylintVersion": pins.dylint_version,
        "phase": "preparation",
        "packageTestInvoked": False,
        "exitCode": None,
    }
    print(f"Dylint package test diagnostics: {cache}", flush=True)
    try:
        child, tree = prepare(cache, pins, dict(os.environ))
        record.update(
            {
                "tree": tree,
                "phase": "original-package-tests",
                "packageTestInvoked": True,
            }
        )
        status = subprocess.run(
            ["cargo", "test", "--locked"], cwd=package, env=child, check=False
        ).returncode
        record["exitCode"] = status
        record["phase"] = "complete" if status == 0 else "original-package-tests"
        return status
    except subprocess.CalledProcessError as error:
        record["exitCode"] = error.returncode
        raise
    except (KeyError, ValueError, OSError):
        record["exitCode"] = 1
        raise
    finally:
        (cache / "result.json").write_text(
            json.dumps(record, indent=2) + "\n", encoding="utf-8"
        )


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, ValueError, OSError, subprocess.CalledProcessError) as error:
        print(
            f"Dylint package preparation failed ({type(error).__name__}); tests are not accepted",
            file=sys.stderr,
        )
        sys.exit(
            error.returncode if isinstance(error, subprocess.CalledProcessError) else 1
        )
