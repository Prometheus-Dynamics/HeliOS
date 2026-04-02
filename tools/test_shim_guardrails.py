#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from datetime import date
from pathlib import Path

import shim_guardrails as guardrails


class ShimGuardrailsTests(unittest.TestCase):
    def test_load_config_requires_delete_by(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            config_path = root / "shim_guardrails.json"
            config_path.write_text(
                json.dumps(
                    {
                        "required_shims": [
                            {
                                "id": "demo-shim",
                                "path": "backend/src/example.rs",
                                "owner": "HeliOS",
                                "reason": "demo shim",
                                "replace_with": "delete it",
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(guardrails.ShimGuardrailConfigError, "delete_by"):
                guardrails.load_config(config_path)

    def test_evaluate_reports_missing_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            guarded_file = root / "backend/src/example.rs"
            guarded_file.parent.mkdir(parents=True)
            guarded_file.write_text("// no shim marker here\n", encoding="utf-8")

            config = guardrails.ShimGuardrailConfig(
                required_shims=(
                    guardrails.RequiredShim(
                        id="demo-shim",
                        path="backend/src/example.rs",
                        owner="HeliOS",
                        delete_by=date(2026, 9, 30),
                        reason="demo reason",
                        replace_with="delete the fallback",
                    ),
                )
            )

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("backend",),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "SHIM_MARKER_MISSING")

    def test_evaluate_reports_unregistered_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            shim_file = root / "backend/src/example.rs"
            shim_file.parent.mkdir(parents=True)
            shim_file.write_text("// TEMP_SHIM: stray-shim\n", encoding="utf-8")

            config = guardrails.ShimGuardrailConfig(required_shims=())

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("backend",),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "UNREGISTERED_SHIM")

    def test_string_literals_do_not_count_as_markers(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            shim_file = root / "tools/example.py"
            shim_file.parent.mkdir(parents=True)
            shim_file.write_text('MARKER_TOKEN = "TEMP_SHIM: stray-shim"\n', encoding="utf-8")

            config = guardrails.ShimGuardrailConfig(required_shims=())

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("tools",),
            )

            self.assertEqual(violations, [])

    def test_evaluate_reports_duplicate_markers(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            shim_file = root / "backend/src/example.rs"
            shim_file.parent.mkdir(parents=True)
            shim_file.write_text("// TEMP_SHIM: demo-shim\n// TEMP_SHIM: demo-shim\n", encoding="utf-8")

            config = guardrails.ShimGuardrailConfig(
                required_shims=(
                    guardrails.RequiredShim(
                        id="demo-shim",
                        path="backend/src/example.rs",
                        owner="HeliOS",
                        delete_by=date(2026, 9, 30),
                        reason="demo reason",
                        replace_with="delete the fallback",
                    ),
                )
            )

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("backend",),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "SHIM_MARKER_DUPLICATED")

    def test_evaluate_reports_expired_shim(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            shim_file = root / "backend/src/example.rs"
            shim_file.parent.mkdir(parents=True)
            shim_file.write_text("// TEMP_SHIM: demo-shim\n", encoding="utf-8")

            config = guardrails.ShimGuardrailConfig(
                required_shims=(
                    guardrails.RequiredShim(
                        id="demo-shim",
                        path="backend/src/example.rs",
                        owner="HeliOS",
                        delete_by=date(2026, 1, 1),
                        reason="demo reason",
                        replace_with="delete the fallback",
                    ),
                )
            )

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("backend",),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "SHIM_EXPIRED")

    def test_evaluate_passes_with_registered_marker(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            shim_file = root / "backend/src/example.rs"
            shim_file.parent.mkdir(parents=True)
            shim_file.write_text(
                "\n".join(
                    [
                        "// TEMP_SHIM: demo-shim",
                        "// delete after migration",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            config = guardrails.ShimGuardrailConfig(
                required_shims=(
                    guardrails.RequiredShim(
                        id="demo-shim",
                        path="backend/src/example.rs",
                        owner="HeliOS",
                        delete_by=date(2026, 9, 30),
                        reason="demo reason",
                        replace_with="delete the fallback",
                    ),
                )
            )

            violations = guardrails.evaluate_guardrails(
                root,
                config,
                today=date(2026, 3, 31),
                scan_roots=("backend",),
            )

            self.assertEqual(violations, [])

    def test_format_violations_lists_each_entry(self) -> None:
        violations = [
            guardrails.Violation(code="SHIM_EXPIRED", path="a.rs", message="deadline passed"),
            guardrails.Violation(code="UNREGISTERED_SHIM", path="b.rs:2", message="missing config"),
        ]

        text = guardrails.format_violations(violations)

        self.assertIn("[SHIM_EXPIRED] a.rs: deadline passed", text)
        self.assertIn("[UNREGISTERED_SHIM] b.rs:2: missing config", text)


if __name__ == "__main__":
    unittest.main()
