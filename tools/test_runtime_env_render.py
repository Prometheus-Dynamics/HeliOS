#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT_PATH = REPO_ROOT / "gaia/tools/runtime-env/render_runtime_env.py"
SOURCE_PATH = REPO_ROOT / "gaia/configs/env/runtime/runtime-env-source.toml"
ENV_OUTPUT_PATH = REPO_ROOT / "gaia/configs/env/runtime/generated-runtime-env.toml"


def load_renderer_module():
    spec = importlib.util.spec_from_file_location("runtime_env_renderer", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


class RuntimeEnvRenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.renderer = load_renderer_module()
        cls.source = load_toml(SOURCE_PATH)

    def test_generated_env_output_matches_snapshot(self) -> None:
        expected = ENV_OUTPUT_PATH.read_text(encoding="utf-8")
        self.assertEqual(self.renderer.render_env_toml(self.source), expected)

    def test_rendered_env_output_applies_profiles_and_tokio_defaults(self) -> None:
        rendered = tomllib.loads(self.renderer.render_env_toml(self.source))
        api_env = rendered["sets"]["helios-api"]
        engine_env = rendered["sets"]["helios-engine"]
        peripherals_env = rendered["sets"]["helios-peripherals"]

        self.assertEqual(api_env["MALLOC_ARENA_MAX"], "1")
        self.assertEqual(api_env["HELIOS_API_THREAD_STACK_BYTES"], "1048576")
        self.assertEqual(api_env["HELIOS_API_BLOCKING_KEEP_ALIVE_MS"], "500")

        self.assertEqual(engine_env["RAYON_NUM_THREADS"], "4")
        self.assertEqual(engine_env["HELIOS_ENGINE_WORKER_THREADS"], "4")
        self.assertEqual(engine_env["HELIOS_ENGINE_BLOCKING_KEEP_ALIVE_MS"], "3000")

        self.assertEqual(peripherals_env["MALLOC_ARENA_MAX"], "2")
        self.assertEqual(peripherals_env["MALLOC_TRIM_THRESHOLD_"], "131072")
        self.assertEqual(peripherals_env["HELIOS_PERIPHERALS_WORKER_THREADS"], "1")
        self.assertNotIn("RUST_MIN_STACK", peripherals_env)
        self.assertNotIn("HELIOS_PERIPHERALS_THREAD_STACK_BYTES", peripherals_env)

if __name__ == "__main__":
    unittest.main()
