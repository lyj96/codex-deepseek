"""Reject paid/custom runner labels when upstream workflows are synchronized."""

from pathlib import Path
import re
import sys


STANDARD_RUNNERS = {
    "ubuntu-latest",
    "ubuntu-22.04",
    "ubuntu-24.04",
    "ubuntu-24.04-arm",
    "windows-latest",
    "windows-2022",
    "windows-2025",
    "windows-11-arm",
    "macos-latest",
    "macos-14",
    "macos-15",
    "macos-15-intel",
}
RUNNER_FIELD = re.compile(
    r"^\s*(?:-\s*)?(runs-on|runs_on|runner|archive_runner|os):\s*(.*?)\s*$"
)
RUNNER_EXPRESSIONS = {
    "${{ matrix.os }}",
    "${{ matrix.runner }}",
    "${{ matrix.runs_on }}",
    "${{ matrix.runs_on || matrix.os }}",
    "${{ matrix.runs_on || matrix.runner }}",
    "${{ inputs.runner }}",
    "${{ inputs.archive_runner || inputs.runner }}",
}


def violations(text):
    for number, line in enumerate(text.splitlines(), 1):
        match = RUNNER_FIELD.match(line)
        if not match:
            continue
        value = match[2].split(" #", 1)[0].strip("\"'")
        if line in {"      runner:", "      archive_runner:"}:
            continue  # workflow_call input declarations, not assignments
        # Matrix/input expressions are resolved from the literal fields checked
        # elsewhere in these workflows. Runner mappings are intentionally banned.
        if value in RUNNER_EXPRESSIONS:
            continue
        if value not in STANDARD_RUNNERS:
            yield number, value


def main():
    workflows = Path(__file__).resolve().parents[1] / "workflows"
    errors = []
    for path in sorted(workflows.glob("*.y*ml")):
        for number, value in violations(path.read_text(encoding="utf-8")):
            errors.append(f"{path.name}:{number}: non-standard runner: {value!r}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Workflow runner labels use the public-repository standard runner allowlist.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
