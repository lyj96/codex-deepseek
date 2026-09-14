import importlib.util
import json
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
    def test_root_package_points_to_all_supported_platform_versions(self) -> None:
        package = BUILD.root_package_json("0.154.0-deepseek.2")

        self.assertEqual(package["name"], "codex-dp")
        self.assertEqual(package["bin"], {"codex-dp": "bin/codex-dp.js"})
        self.assertEqual(
            package["optionalDependencies"],
            {
                "codex-dp-linux-x64": "npm:codex-dp@0.154.0-deepseek.2-linux-x64",
                "codex-dp-darwin-arm64": "npm:codex-dp@0.154.0-deepseek.2-darwin-arm64",
                "codex-dp-win32-x64": "npm:codex-dp@0.154.0-deepseek.2-win32-x64",
            },
        )

    def test_platform_staging_places_full_package_under_target(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            temp_path = Path(temp)
            vendor_src = temp_path / "source"
            (vendor_src / "bin").mkdir(parents=True)
            (vendor_src / "bin" / "codex").write_text("binary", encoding="utf-8")
            (vendor_src / "codex-package.json").write_text(
                '{"version":"0.154.0+deepseek.2"}\n', encoding="utf-8"
            )
            staging = temp_path / "staging"
            staging.mkdir()

            BUILD.stage_package(
                staging,
                "0.154.0-deepseek.2",
                "linux-x64",
                vendor_src,
            )

            target = "x86_64-unknown-linux-musl"
            self.assertTrue((staging / "vendor" / target / "bin" / "codex").is_file())
            package = json.loads((staging / "package.json").read_text())
            self.assertEqual(package["version"], "0.154.0-deepseek.2-linux-x64")
            self.assertEqual(package["os"], ["linux"])
            self.assertEqual(package["cpu"], ["x64"])


if __name__ == "__main__":
    unittest.main()
