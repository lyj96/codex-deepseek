import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("build_npm_package.py")
SPEC = importlib.util.spec_from_file_location("build_npm_package", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Cannot load {MODULE_PATH}")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class BuildNpmPackageTest(unittest.TestCase):
    def test_root_package_is_a_thin_installer(self) -> None:
        package = BUILD.root_package_json("0.154.0-deepseek.2")

        self.assertEqual(package["name"], "codex-dp")
        self.assertEqual(package["bin"], {"codex-dp": "bin/codex-dp.js"})
        self.assertNotIn("optionalDependencies", package)

    def test_root_package_accepts_an_npm_only_hotfix_version(self) -> None:
        package = BUILD.root_package_json("0.154.0-deepseek.2-npm.1")

        self.assertEqual(package["version"], "0.154.0-deepseek.2-npm.1")

    def test_thin_package_staging_contains_only_javascript_launcher(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_path = Path(temp)
            staging = temp_path / "staging"
            staging.mkdir()

            BUILD.stage_package(staging, "0.154.0-deepseek.2")

            self.assertTrue((staging / "bin" / "codex-dp.js").is_file())
            self.assertFalse((staging / "vendor").exists())


if __name__ == "__main__":
    unittest.main()
