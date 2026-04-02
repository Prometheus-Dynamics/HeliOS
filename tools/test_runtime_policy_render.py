#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT_PATH = REPO_ROOT / "gaia/tools/runtime-policy/render_runtime_policy.py"
SOURCE_PATH = REPO_ROOT / "gaia/configs/env/runtime/runtime-policy-source.toml"
ENV_OUTPUT_PATH = REPO_ROOT / "gaia/configs/env/runtime/generated-runtime-policy.toml"
RUST_OUTPUT_PATH = REPO_ROOT / "backend/src/libs/lib-runtime-policy/src/generated.rs"


def load_renderer_module():
    spec = importlib.util.spec_from_file_location("runtime_policy_renderer", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


class RuntimePolicyRenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.renderer = load_renderer_module()
        cls.source = load_toml(SOURCE_PATH)

    def test_generated_env_output_matches_snapshot(self) -> None:
        expected = ENV_OUTPUT_PATH.read_text(encoding="utf-8")
        self.assertEqual(self.renderer.render_env_toml(self.source), expected)

    def test_generated_rust_output_matches_snapshot(self) -> None:
        expected = RUST_OUTPUT_PATH.read_text(encoding="utf-8")
        self.assertEqual(self.renderer.render_rust(self.source), expected)

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

    def test_rendered_rust_output_preserves_optional_tokio_fields(self) -> None:
        rendered = self.renderer.render_rust(self.source)
        self.assertIn("pub const HELIOS_API_TOKIO_POLICY", rendered)
        self.assertIn('env_var: "HELIOS_ENGINE_BLOCKING_KEEP_ALIVE_MS", default: 3000', rendered)
        self.assertIn("pub const HELIOS_PERIPHERALS_TOKIO_POLICY", rendered)
        self.assertIn("thread_stack_bytes: None,", rendered)
        self.assertIn("blocking_keep_alive_ms: None,", rendered)


if __name__ == "__main__":
    unittest.main()
