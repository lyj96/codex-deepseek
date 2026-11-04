"""Exercise the shell wrapper without downloading or building Bazel targets."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class WindowsEnvironmentTest(unittest.TestCase):
    def invoke(
        self,
        *,
        remote=False,
        system_root="C:/Windows",
        uppercase=False,
        args=(),
        hosted=False,
        runner_os="Windows",
        command="build",
        targets=("//example:target",),
        target_triple=None,
    ):
        env = os.environ.copy()
        env.update(
            RUNNER_OS=runner_os,
            CODEX_BAZEL_BIN="echo",
            CODEX_BAZEL_WINDOWS_PATH="C:/Windows;C:/Tools",
            PROCESSOR_ARCHITECTURE="AMD64",
            SystemRoot=system_root,
            VOICE_WINDOWS_BAZEL_REPOSITORY="D:/temp/voice-tools",
        )
        env.pop("BUILDBUDDY_API_KEY", None)
        env.pop("SYSTEMROOT", None)
        env.pop("GITHUB_ACTIONS", None)
        env.pop("RUNNER_ENVIRONMENT", None)
        env.pop("CODEX_BAZEL_TARGET_TRIPLE", None)
        if target_triple is not None:
            env["CODEX_BAZEL_TARGET_TRIPLE"] = target_triple
        if hosted:
            env.update(GITHUB_ACTIONS="true", RUNNER_ENVIRONMENT="github-hosted")
        if uppercase:
            env["SYSTEMROOT"] = env.pop("SystemRoot")
        if remote:
            env["BUILDBUDDY_API_KEY"] = "test-only-not-a-secret"
        with tempfile.TemporaryDirectory() as directory:
            # Windows checkouts may have CRLF; test the same LF scripts as Linux CI.
            for name in ("run-bazel-ci.sh", "run_bazel_with_buildbuddy.py"):
                destination = Path(directory) / name
                destination.write_text(
                    Path(__file__).with_name(name).read_text(), newline="\n"
                )
                destination.chmod(0o755)
            return subprocess.run(
                [
                    "bash",
                    str(Path(directory) / "run-bazel-ci.sh"),
                    "--windows-cross-compile",
                    "--",
                    command,
                    *args,
                    "--",
                    *targets,
                ],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )

    def test_local_windows_has_fixed_host_and_target_identity(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--host_platform=//:local_windows_msvc", result.stdout)
        self.assertIn("--platforms=//:windows_x86_64_gnullvm", result.stdout)
        self.assertIn(
            "--extra_toolchains=//:windows_x86_64_msvc_cc_toolchain", result.stdout
        )
        self.assertIn(
            "--inject_repository=voice_windows_tools=D:/temp/voice-tools", result.stdout
        )
        self.assertIn(
            "--//third_party/voice:windows_installed_tools=@voice_windows_tools//:tools",
            result.stdout,
        )
        for prefix in ("--action_env=", "--host_action_env="):
            self.assertIn(prefix + "SystemRoot=C:/Windows", result.stdout)
            self.assertIn(prefix + "PROCESSOR_ARCHITECTURE=AMD64", result.stdout)

    def test_explicit_platform_overrides_are_preserved(self):
        for args in (
            ("--host_platform=//:custom_host", "--platforms=//:custom_target"),
            ("--host_platform", "//:custom_host", "--platforms", "//:custom_target"),
        ):
            with self.subTest(args=args):
                result = self.invoke(args=args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(" ".join(args), result.stdout)
                self.assertNotIn("--host_platform=//:local_windows_msvc", result.stdout)
                self.assertNotIn("--platforms=//:windows_x86_64_gnullvm", result.stdout)

    def test_disposable_disk_cache_default_respects_explicit_overrides(self):
        result = self.invoke(hosted=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--disk_cache= ", result.stdout)
        for args in (("--disk_cache=D:/cache",), ("--disk_cache", "D:/cache")):
            with self.subTest(args=args):
                result = self.invoke(hosted=True, args=args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(" ".join(args), result.stdout)
                self.assertNotIn("--disk_cache= ", result.stdout)
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("--disk_cache=", result.stdout)

    def test_declared_matrix_target_survives_local_and_remote_defaults(self):
        for runner_os, triple in (
            ("Linux", "x86_64-unknown-linux-gnu"),
            ("Linux", "x86_64-unknown-linux-musl"),
            ("macOS", "x86_64-apple-darwin"),
            ("macOS", "aarch64-apple-darwin"),
            ("Windows", "x86_64-pc-windows-gnullvm"),
        ):
            for remote in (False, True):
                with self.subTest(triple=triple, remote=remote):
                    result = self.invoke(
                        runner_os=runner_os, remote=remote, target_triple=triple
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    platform = f"--platforms=@rules_rs//rs/platforms:{triple}"
                    self.assertEqual(result.stdout.count(platform), 1)
                    self.assertNotIn(
                        "--platforms=//:windows_x86_64_gnullvm", result.stdout
                    )
                    if remote:
                        self.assertGreater(
                            result.stdout.index(platform),
                            result.stdout.index("--config=ci-"),
                        )

    def test_declared_target_preserves_explicit_platform_and_rejects_unknown(self):
        for args in (("--platforms=//:custom",), ("--platforms", "//:custom")):
            result = self.invoke(
                runner_os="Linux",
                args=args,
                target_triple="x86_64-unknown-linux-musl",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(" ".join(args), result.stdout)
            self.assertNotIn("--platforms=@rules_rs", result.stdout)
        result = self.invoke(target_triple="unknown-target")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unsupported Bazel CI target", result.stderr)

    def test_local_windows_requires_identity(self):
        result = self.invoke(system_root="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SystemRoot must be set", result.stderr)

    def test_rollout_strategy_is_limited_to_hosted_linux_tests(self):
        strategy = "--strategy=TestRunner=processwrapper-sandbox"
        rollout = "//codex-rs/rollout:rollout-unit-tests"
        result = self.invoke(
            hosted=True, runner_os="Linux", command="test", targets=(rollout,)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(strategy, result.stdout)
        self.assertNotIn("--strategy_regexp=", result.stdout)
        for options in (
            dict(hosted=False, runner_os="Linux", command="test"),
            dict(hosted=True, runner_os="Linux", command="build"),
            dict(hosted=True, runner_os="Windows", command="test"),
            dict(hosted=True, runner_os="macOS", command="test"),
            dict(hosted=True, runner_os="Linux", command="test", remote=True),
        ):
            with self.subTest(options=options):
                result = self.invoke(targets=(rollout,), **options)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn(strategy, result.stdout)
        for targets in (("//...",), (rollout, "//example:target")):
            with self.subTest(targets=targets):
                result = self.invoke(
                    hosted=True, runner_os="Linux", command="test", targets=targets
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn(strategy, result.stdout)

    def test_git_bash_uppercase_system_root(self):
        result = self.invoke(uppercase=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--action_env=SystemRoot=C:/Windows", result.stdout)

    def test_remote_linux_does_not_receive_windows_identity(self):
        result = self.invoke(remote=True, system_root="", hosted=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("--action_env=SystemRoot", result.stdout)
        self.assertNotIn("--host_action_env=PROCESSOR_ARCHITECTURE", result.stdout)
        self.assertNotIn("--inject_repository=voice_windows_tools", result.stdout)
        self.assertNotIn(
            "--extra_toolchains=//:windows_x86_64_msvc_cc_toolchain", result.stdout
        )
        self.assertNotIn("--platforms=//:windows_x86_64_gnullvm", result.stdout)
        self.assertNotIn("--disk_cache= ", result.stdout)


if __name__ == "__main__":
    unittest.main()
