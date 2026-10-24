"""Fail closed if the seven user-approved exceptions outlive their one release."""

import json
import os
from pathlib import Path
import sys
import tomllib


APPROVED = {
    "RUSTSEC-2026-0370": ("zvariant", "4.2.0"),
    "RUSTSEC-2026-0371": ("zbus_names", "3.0.0"),
    "RUSTSEC-2026-0372": ("zvariant_derive", "4.2.0"),
    "RUSTSEC-2026-0374": ("basic-toml", "0.1.10"),
    "RUSTSEC-2026-0385": ("ctutils", "0.4.2"),
    "RUSTSEC-2026-0386": ("tokio", "1.52.3"),
    "RUSTSEC-2026-0387": ("crypto-bigint", "0.5.5"),
}
RELEASE = "codex-v0.164.0-deepseek.1"
BASELINE_IGNORES = {
    "RUSTSEC-2024-0388",
    "RUSTSEC-2025-0057",
    "RUSTSEC-2024-0436",
    "RUSTSEC-2023-0089",
    "RUSTSEC-2024-0320",
    "RUSTSEC-2025-0141",
    "RUSTSEC-2026-0118",
    "RUSTSEC-2026-0119",
    "RUSTSEC-2026-0194",
    "RUSTSEC-2026-0195",
}


def advisory_ids(entries):
    ids = [entry if isinstance(entry, str) else entry["id"] for entry in entries]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate advisory exception")
    return set(ids)


def validate_policy(policy, workspace_version, packages, audit_ids, deny_ids, ref):
    active = (audit_ids | deny_ids) & APPROVED.keys()
    if policy is None:
        if active:
            raise ValueError("one-release exceptions lack their approval record")
        return []
    if workspace_version != "0.164.0" or policy["baseVersion"] != workspace_version:
        raise ValueError("one-release advisory approval expired for this Cargo version")
    if policy["releaseTag"] != RELEASE or (
        ref.startswith("refs/tags/") and ref != f"refs/tags/{RELEASE}"
    ):
        raise ValueError("one-release advisory approval does not cover this tag")
    entries = policy["entries"]
    ids = advisory_ids(entries)
    if (
        ids != APPROVED.keys()
        or audit_ids != deny_ids
        or audit_ids != BASELINE_IGNORES | APPROVED.keys()
    ):
        raise ValueError("approval record and audit/deny exceptions do not match")
    for entry in entries:
        package, version = APPROVED[entry["id"]]
        if (entry["package"], entry["version"]) != (package, version):
            raise ValueError("approval record changed an approved package/version")
        versions = {p["version"] for p in packages if p["name"] == package}
        if versions != {version}:
            raise ValueError(
                f"review/remove expired exception for {package}: {versions}"
            )
    return entries


def main():
    root = Path(__file__).resolve().parents[2]
    policy_path = root / ".github/advisory-exceptions-0.164.0.json"
    policy = (
        json.loads(policy_path.read_text(encoding="utf-8"))
        if policy_path.exists()
        else None
    )
    cargo = root / "codex-rs"
    manifest = tomllib.loads((cargo / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((cargo / "Cargo.lock").read_text(encoding="utf-8"))
    audit = tomllib.loads((cargo / ".cargo/audit.toml").read_text(encoding="utf-8"))
    deny = tomllib.loads((cargo / "deny.toml").read_text(encoding="utf-8"))
    entries = validate_policy(
        policy,
        manifest["workspace"]["package"]["version"],
        lock["package"],
        advisory_ids(audit["advisories"]["ignore"]),
        advisory_ids(deny["advisories"]["ignore"]),
        os.environ.get("GITHUB_REF", ""),
    )
    for entry in entries:
        print(
            f"TEMPORARY UNRESOLVED {entry['id']} "
            f"{entry['package']}@{entry['version']}; approval only for {RELEASE}"
        )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, ValueError, OSError) as error:
        print(f"Advisory exception policy rejected: {error}", file=sys.stderr)
        sys.exit(1)
