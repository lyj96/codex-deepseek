import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "dylint_package", Path(__file__).with_name("run_dylint_package_tests.py")
)
runner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runner
SPEC.loader.exec_module(runner)


class PinTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.package = self.root / "tools/argument-comment-lint"
        self.package.mkdir(parents=True)
        self.revision = "a" * 40
        self.manifest = f'''[dependencies]
clippy_utils = {{ git = "{runner.CLIPPY_URL}", rev = "{self.revision}" }}
dylint_linting = "6.1.0"
[dev-dependencies]
dylint_testing = "6.1.0"
'''
        self.lock = f"""[[package]]
name = "clippy_utils"
version = "0.1.100"
source = "git+{runner.CLIPPY_URL}?rev={self.revision}#{self.revision}"
[[package]]
name = "dylint_linting"
version = "6.1.0"
[[package]]
name = "dylint_testing"
version = "6.1.0"
"""
        self.write_inputs()

    def write_inputs(self, nightly="nightly-2026-08-20"):
        (self.package / "Cargo.toml").write_text(self.manifest, encoding="utf-8")
        (self.package / "Cargo.lock").write_text(self.lock, encoding="utf-8")
        (self.package / "rust-toolchain").write_text(
            f'[toolchain]\nchannel = "{nightly}"\n', encoding="utf-8"
        )

    def test_reads_original_released_stack(self):
        self.assertEqual(
            runner.read_pins(self.package),
            runner.Pins(self.revision, "nightly-2026-08-20", "6.1.0"),
        )

    def test_rejects_mutable_revision_and_unapproved_origin(self):
        original = self.manifest
        for manifest in (
            original.replace(self.revision, "master"),
            original.replace(runner.CLIPPY_URL, "https://example.invalid/clippy"),
        ):
            with self.subTest(manifest=manifest):
                self.manifest = manifest
                self.write_inputs()
                with self.assertRaises(ValueError):
                    runner.read_pins(self.package)

    def test_rejects_lock_drift_and_mismatched_test_version(self):
        self.lock = self.lock.replace("#" + self.revision, "#" + "b" * 40)
        self.write_inputs()
        with self.assertRaises(ValueError):
            runner.read_pins(self.package)
        self.lock = self.lock.replace("#" + "b" * 40, "#" + self.revision)
        self.manifest = self.manifest.replace(
            'dylint_testing = "6.1.0"', 'dylint_testing = "6.3.0"'
        )
        self.write_inputs()
        with self.assertRaises(ValueError):
            runner.read_pins(self.package)

    def test_rejects_floating_nightly(self):
        self.write_inputs(nightly="nightly")
        with self.assertRaises(ValueError):
            runner.read_pins(self.package)

    def test_failed_package_exit_is_retained(self):
        completed = subprocess.CompletedProcess(["cargo", "test", "--locked"], 101)
        with (
            patch.object(
                sys,
                "argv",
                ["runner", "--root", str(self.root), "--cache-parent", str(self.root)],
            ),
            patch.object(
                runner, "prepare", return_value=({"SYNTHETIC_TEST": "1"}, "b" * 40)
            ),
            patch.object(runner.subprocess, "run", return_value=completed) as run,
        ):
            self.assertEqual(runner.main(), 101)
        self.assertEqual(run.call_args.args[0], ["cargo", "test", "--locked"])
        self.assertEqual(run.call_args.kwargs["cwd"], self.package)
        result = json.loads(
            next(self.root.glob("codex-dylint-*/result.json")).read_text()
        )
        self.assertTrue(result["packageTestInvoked"])
        self.assertEqual(result["exitCode"], 101)
        self.assertEqual(result["phase"], "original-package-tests")


class GitEnvironmentTests(unittest.TestCase):
    def test_preserves_existing_configuration_without_mutating_parent(self):
        original = {
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.autocrlf",
            "GIT_CONFIG_VALUE_0": "false",
        }
        child = runner.git_environment(original, Path(__file__).resolve().parent)
        self.assertEqual(original["GIT_CONFIG_COUNT"], "1")
        self.assertNotIn("GIT_CONFIG_KEY_1", original)
        self.assertEqual(child["GIT_CONFIG_COUNT"], "3")
        self.assertEqual(child["GIT_CONFIG_VALUE_0"], "false")
        self.assertEqual(child["GIT_CONFIG_VALUE_1"], runner.CLIPPY_URL)

    def test_rejects_malformed_or_incomplete_inherited_configuration(self):
        for environment in (
            {"GIT_CONFIG_COUNT": "-1"},
            {"GIT_CONFIG_COUNT": "33"},
            {"GIT_CONFIG_COUNT": "one"},
            {"GIT_CONFIG_COUNT": "1"},
        ):
            with self.subTest(environment=environment), self.assertRaises(ValueError):
                runner.git_environment(environment, Path(__file__).resolve().parent)

    def test_real_legacy_clone_uses_only_the_private_pinned_history(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            original = root / "original"
            mirror = root / "mirror.git"
            probe = root / "probe"
            environment = dict(os.environ)
            environment.update(
                {
                    "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_CONFIG_GLOBAL": os.devnull,
                    "GIT_CONFIG_COUNT": "0",
                }
            )

            def git(*args):
                return subprocess.check_output(
                    ["git", *args], env=environment, text=True
                ).strip()

            git("init", "--quiet", "--initial-branch=master", str(original))
            (original / "rust-toolchain.toml").write_text(
                '[toolchain]\nchannel = "nightly-2026-08-20"\n'
            )
            git("-C", str(original), "add", "rust-toolchain.toml")
            git(
                "-C",
                str(original),
                "-c",
                "user.name=Synthetic Test",
                "-c",
                "user.email=synthetic@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "pinned fixture",
            )
            revision = git("-C", str(original), "rev-parse", "HEAD")
            tree = git("-C", str(original), "rev-parse", "HEAD^{tree}")
            git("clone", "--quiet", "--bare", str(original), str(mirror))
            child = runner.git_environment(environment, mirror)
            runner.checked(
                ["git", "clone", "--quiet", runner.CLIPPY_URL, str(probe)],
                environment=child,
            )
            pins = runner.Pins(revision, "nightly-2026-08-20", "6.1.0")
            runner.verify_clone(probe, pins, tree, child)
            with self.assertRaises(ValueError):
                runner.verify_clone(
                    probe,
                    runner.Pins("b" * 40, pins.nightly, pins.dylint_version),
                    tree,
                    child,
                )
            self.assertEqual(environment["GIT_CONFIG_COUNT"], "0")


if __name__ == "__main__":
    unittest.main()
