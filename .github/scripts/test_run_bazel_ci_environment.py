"""Exercise the shell wrapper without downloading or building Bazel targets."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class WindowsEnvironmentTest(unittest.TestCase):
    def invoke(self, *, remote=False, system_root="C:/Windows"):
        env = os.environ.copy()
        env.update(
            RUNNER_OS="Windows",
            CODEX_BAZEL_BIN="echo",
            CODEX_BAZEL_WINDOWS_PATH="C:/Windows;C:/Tools",
            PROCESSOR_ARCHITECTURE="AMD64",
            SystemRoot=system_root,
            VOICE_WINDOWS_BAZEL_REPOSITORY="D:/temp/voice-tools",
        )
        env.pop("BUILDBUDDY_API_KEY", None)
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
                    "build",
                    "--",
                    "//example:target",
                ],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )

    def test_local_windows_has_fixed_host_and_target_identity(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("--host_platform=//:local_windows_msvc", result.stdout)
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

    def test_local_windows_requires_identity(self):
        result = self.invoke(system_root="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SystemRoot must be set", result.stderr)

    def test_remote_linux_does_not_receive_windows_identity(self):
        result = self.invoke(remote=True, system_root="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("--action_env=SystemRoot", result.stdout)
        self.assertNotIn("--host_action_env=PROCESSOR_ARCHITECTURE", result.stdout)
        self.assertNotIn("--inject_repository=voice_windows_tools", result.stdout)


if __name__ == "__main__":
    unittest.main()
