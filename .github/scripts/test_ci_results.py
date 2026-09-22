"""Exercise the fail-closed dependency gate used by PR and release validation."""

import json
import os
from pathlib import Path
import subprocess
import sys
import unittest


class DependencyGateTest(unittest.TestCase):
    def invoke(self, result):
        env = os.environ.copy()
        env["NEEDS"] = json.dumps(
            {"checks": {"result": "success"}, "regressions": {"result": result}}
        )
        return subprocess.run(
            [sys.executable, str(Path(__file__).with_name("check_ci_results.py"))],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_accepts_success(self):
        self.assertEqual(self.invoke("success").returncode, 0)

    def test_rejects_incomplete_or_unsuccessful_dependency(self):
        for result in ("failure", "cancelled", "skipped", "pending", ""):
            with self.subTest(result=result):
                outcome = self.invoke(result)
                self.assertNotEqual(outcome.returncode, 0)
                self.assertIn("regressions:", outcome.stdout)


if __name__ == "__main__":
    unittest.main()
