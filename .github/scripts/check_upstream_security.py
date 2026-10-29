"""Record every advisory, accepting only exact official-stable package identities.

The 2026-10-29 standing user decision accepts inherited risks, not a clean scan.
Operational failures and Fork-only affected dependencies remain blocking.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.request import urlopen


def download(url):
    with urlopen(url, timeout=60) as response:
        return response.read()


def official_lock(policy, fetch=download):
    origin = "https://api.github.com/repos/openai/codex"
    tag = "rust-v" + policy["baseVersion"]
    obj = json.loads(fetch(f"{origin}/git/ref/tags/{tag}"))["object"]
    if obj["type"] == "tag":
        obj = json.loads(fetch(f"{origin}/git/tags/{obj['sha']}"))["object"]
    if obj["type"] != "commit" or obj["sha"] != policy["officialCommit"]:
        raise ValueError("official stable tag does not match the pinned commit")
    return fetch("https://raw.githubusercontent.com/openai/codex/"
                 + policy["officialCommit"] + "/codex-rs/Cargo.lock")


def identity(package):
    return tuple(package.get(key) for key in ("name", "version", "source", "checksum"))


def validate_policy(policy, version, lock_bytes, ref):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version) or policy["baseVersion"] != version:
        raise ValueError("official provenance is for another Cargo base")
    if not re.fullmatch(r"[0-9a-f]{40}", policy["officialCommit"]):
        raise ValueError("official commit must be immutable")
    if ref.startswith("refs/tags/") and not re.fullmatch(
        rf"refs/tags/codex-v{re.escape(version)}-deepseek\.[1-9]\d*", ref
    ):
        raise ValueError("native tag does not match official base")
    if hashlib.sha256(lock_bytes).hexdigest() != policy["officialLockSha256"]:
        raise ValueError("official Cargo.lock checksum mismatch")
    return tomllib.loads(lock_bytes.decode("utf-8"))["package"]


def classify(diagnostics, packages, upstream, scanner_exit):
    official = {identity(package) for package in upstream}
    accepted, rejected = [], []
    summary, errors = None, 0
    for record in diagnostics:
        if record.get("type") == "summary":
            summary = record["fields"].get("advisories")
            continue
        if record.get("type") != "diagnostic":
            continue
        fields = record["fields"]
        errors += fields.get("severity") == "error"
        advisory = fields.get("advisory")
        if advisory:
            affected = [graph["Krate"] for graph in fields.get("graphs", [])]
            matched = bool(affected) and re.fullmatch(r"RUSTSEC-\d{4}-\d{4}", advisory["id"])
            for crate in affected:
                locked = [p for p in packages if (p["name"], p["version"]) == (crate["name"], crate["version"])]
                # Name/version-only diagnostics cannot disambiguate two different sources.
                matched = matched and len(locked) == 1 and identity(locked[0]) in official
                matched = matched and advisory["package"] == crate["name"]
            item = {"id": advisory["id"], "severity": fields.get("severity"), "packages": affected,
                    "title": advisory.get("title"), "riskFixed": False}
            (accepted if matched else rejected).append(item)
        elif fields.get("severity") == "error" or fields.get("code") == "index-failure":
            rejected.append({"operationalFailure": fields.get("code"), "message": fields.get("message")})
    if summary is None or scanner_exit not in (0, 1) or (scanner_exit == 1 and not errors):
        rejected.append({"operationalFailure": "incomplete or unexpected scanner execution"})
    if summary and summary.get("errors", 0) != errors:
        rejected.append({"operationalFailure": "diagnostics do not cover all scanner errors"})
    return {"scannerExit": scanner_exit, "scannerSummary": summary, "acceptedUpstreamRisks": accepted,
            "rejected": rejected, "status": "rejected" if rejected else "accepted-with-inherited-risks",
            "vulnerabilitiesFixed": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--official-lock", type=Path, help="Offline copy, still verified against pinned SHA256")
    parser.add_argument("--cargo-deny", default="cargo-deny")
    parser.add_argument("--report-dir", type=Path, required=True)
    args = parser.parse_args()
    root, reports = args.root.resolve(), args.report_dir.resolve()
    reports.mkdir(parents=True, exist_ok=True)
    cargo = root / "codex-rs"
    policy = json.loads((root / ".github/upstream-stable-security.json").read_text(encoding="utf-8"))
    version = tomllib.loads((cargo / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    lock = tomllib.loads((cargo / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    official_bytes = args.official_lock.read_bytes() if args.official_lock else official_lock(policy)
    upstream = validate_policy(policy, version, official_bytes, os.environ.get("GITHUB_REF", ""))
    (reports / "official-Cargo.lock").write_bytes(official_bytes)
    for path in (cargo / "deny.toml", cargo / ".cargo/audit.toml"):
        if tomllib.loads(path.read_text(encoding="utf-8"))["advisories"].get("ignore", []):
            raise ValueError("advisory ignores hide findings; use the exact upstream-parity report")
    metadata = reports / "metadata.json"
    with metadata.open("wb") as output, (reports / "metadata.stderr.log").open("wb") as error:
        result = subprocess.run(["cargo", "metadata", "--all-features", "--locked", "--format-version", "1"],
                                cwd=cargo, stdout=output, stderr=error, check=False)
    if result.returncode:
        raise ValueError(f"fresh all-features metadata failed: exit {result.returncode}")
    config = reports / "advisories.toml"
    config.write_text("[advisories]\nignore = []\n", encoding="utf-8")
    stderr = reports / "scanner.stderr.ndjson"
    with (reports / "scanner.stdout.ndjson").open("wb") as output, stderr.open("wb") as error:
        scan = subprocess.run([args.cargo_deny, "--all-features", "--locked", "--format", "json", "check",
                               "advisories", "--metadata-path", str(metadata), "--config", str(config), "--show-stats"],
                              cwd=cargo, stdout=output, stderr=error, check=False)
    with stderr.open(encoding="utf-8") as stream:
        report = classify((json.loads(line) for line in stream if line.strip()), lock, upstream, scan.returncode)
    report.update({"baseVersion": version, "officialCommit": policy["officialCommit"], "metadataExit": result.returncode})
    (reports / "upstream-risk-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    for item in report["acceptedUpstreamRisks"]:
        print(f"UNRESOLVED UPSTREAM RISK {item['id']}: {item['packages']}; not fixed")
    if report["rejected"]:
        raise ValueError(f"{len(report['rejected'])} non-inherited or operational failures; inspect saved report")
    print("Security parity accepted; raw scanner exit and unresolved findings retained.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, ValueError, OSError) as error:
        print(f"Upstream security parity rejected: {error}", file=sys.stderr)
        sys.exit(1)
