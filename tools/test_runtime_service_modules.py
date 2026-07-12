#!/usr/bin/env python3
from __future__ import annotations

import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
BASE_DISTRO = REPO_ROOT / "gaia/configs/distros/helios/base.toml"
SQUASHFS_DISTRO = REPO_ROOT / "gaia/configs/distros/helios/squashfs.toml"
RUNTIME_MODULE_DIR = REPO_ROOT / "gaia/configs/modules/stage/services/runtime"
LEGACY_RUNTIME_CORE = REPO_ROOT / "gaia/configs/modules/stage/services/runtime_core.toml"
EXPECTED_RUNTIME_MODULES = (
    "helios-usb-shell.toml",
    "orion-node.toml",
    "helios-engine.toml",
    "helios-updater.toml",
    "helios-peripherals.toml",
    "helios-fan-overlays.toml",
    "helios-os-self-check.toml",
    "helios-api.toml",
    "helios-ide.toml",
    "helios-diagnostics.toml",
    "helios-frontend.toml",
    "helios-set-governor.toml",
    "helios-set-build-time.toml",
    "helios-journal-vacuum.toml",
    "helios-ota-confirm.toml",
)
EXPECTED_RUNTIME_IMPORTS = [
    f"../../modules/stage/services/runtime/{module_name}" for module_name in EXPECTED_RUNTIME_MODULES
]


def load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


class RuntimeServiceModuleTests(unittest.TestCase):
    def assert_runtime_imports(self, distro_path: Path) -> None:
        imports = load_toml(distro_path)["stage"]["services"]["imports"]
        self.assertNotIn("../../modules/stage/services/runtime_core.toml", imports)
        actual_runtime_imports = [path for path in imports if path.startswith("../../modules/stage/services/runtime/")]
        self.assertEqual(actual_runtime_imports, EXPECTED_RUNTIME_IMPORTS)

    def test_runtime_core_module_stays_deleted(self) -> None:
        self.assertFalse(LEGACY_RUNTIME_CORE.exists())

    def test_runtime_module_directory_matches_expected_service_set(self) -> None:
        actual = sorted(path.name for path in RUNTIME_MODULE_DIR.glob("*.toml"))
        self.assertEqual(actual, sorted(EXPECTED_RUNTIME_MODULES))

    def test_base_distro_uses_service_owned_runtime_modules(self) -> None:
        self.assert_runtime_imports(BASE_DISTRO)

    def test_squashfs_distro_uses_service_owned_runtime_modules(self) -> None:
        self.assert_runtime_imports(SQUASHFS_DISTRO)


if __name__ == "__main__":
    unittest.main()
