import runpy
import unittest
from pathlib import Path


violations = runpy.run_path(str(Path(__file__).with_name("check-free-runners.py")))[
    "violations"
]


class FreeRunnerTests(unittest.TestCase):
    def test_standard_and_matrix_runners(self):
        text = "    runs-on: ${{ matrix.runner }}\n          - runner: macos-15\n    runs-on: windows-2025"
        self.assertEqual(list(violations(text)), [])

    def test_paid_custom_and_mapping_runners(self):
        for value in (
            "macos-15-xlarge",
            "custom-linux-xl",
            "",
            "[self-hosted, linux]",
            "${{ vars.RUNNER }}",
        ):
            with self.subTest(value=value):
                self.assertEqual(len(list(violations(f"    runs-on: {value}"))), 1)

    def test_input_declarations(self):
        self.assertEqual(list(violations("      runner:\n      archive_runner:")), [])


if __name__ == "__main__":
    unittest.main()
