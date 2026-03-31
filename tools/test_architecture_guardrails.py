#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import architecture_guardrails as guardrails


class ArchitectureGuardrailsTests(unittest.TestCase):
    def test_load_config_requires_review_note(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            config_path = root / "guardrails.json"
            config_path.write_text(
                json.dumps(
                    {
                        "line_limits": [
                            {
                                "path": "backend/src/example.rs",
                                "max_lines": 10,
                                "owner": "HeliOS",
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(guardrails.GuardrailConfigError, "review_note"):
                guardrails.load_config(config_path)

    def test_evaluate_reports_oversized_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            guarded_file = root / "backend/src/example.rs"
            guarded_file.parent.mkdir(parents=True)
            guarded_file.write_text("line1\nline2\nline3\n", encoding="utf-8")

            config = guardrails.GuardrailConfig(
                line_limits=(
                    guardrails.LineLimitRule(
                        path="backend/src/example.rs",
                        max_lines=2,
                        owner="HeliOS",
                        review_note="example limit",
                    ),
                ),
                forbidden_paths=(),
            )

            violations = guardrails.evaluate_guardrails(root, config)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "MAX_LINES_EXCEEDED")
            self.assertIn("exceeds max 2", violations[0].message)

    def test_evaluate_reports_missing_guarded_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            config = guardrails.GuardrailConfig(
                line_limits=(
                    guardrails.LineLimitRule(
                        path="backend/src/example.rs",
                        max_lines=10,
                        owner="HeliOS",
                        review_note="missing file should fail",
                    ),
                ),
                forbidden_paths=(),
            )

            violations = guardrails.evaluate_guardrails(root, config)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "GUARDED_FILE_MISSING")

    def test_evaluate_reports_forbidden_path(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            forbidden_file = root / "backend/src/legacy.rs"
            forbidden_file.parent.mkdir(parents=True)
            forbidden_file.write_text("// legacy\n", encoding="utf-8")

            config = guardrails.GuardrailConfig(
                line_limits=(),
                forbidden_paths=(
                    guardrails.ForbiddenPathRule(
                        path="backend/src/legacy.rs",
                        owner="HeliOS",
                        reason="legacy split path",
                    ),
                ),
            )

            violations = guardrails.evaluate_guardrails(root, config)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].code, "FORBIDDEN_PATH_PRESENT")

    def test_evaluate_passes_when_repo_matches_rules(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            guarded_file = root / "frontend/src/example.ts"
            guarded_file.parent.mkdir(parents=True)
            guarded_file.write_text("const x = 1;\n", encoding="utf-8")

            config = guardrails.GuardrailConfig(
                line_limits=(
                    guardrails.LineLimitRule(
                        path="frontend/src/example.ts",
                        max_lines=5,
                        owner="HeliOS",
                        review_note="small demo file",
                    ),
                ),
                forbidden_paths=(
                    guardrails.ForbiddenPathRule(
                        path="frontend/src/legacy.ts",
                        owner="HeliOS",
                        reason="legacy file must stay deleted",
                    ),
                ),
            )

            violations = guardrails.evaluate_guardrails(root, config)

            self.assertEqual(violations, [])

    def test_format_violations_lists_each_entry(self) -> None:
        violations = [
            guardrails.Violation(code="MAX_LINES_EXCEEDED", path="a.rs", message="too big"),
            guardrails.Violation(code="FORBIDDEN_PATH_PRESENT", path="b.rs", message="must stay deleted"),
        ]

        text = guardrails.format_violations(violations)

        self.assertIn("[MAX_LINES_EXCEEDED] a.rs: too big", text)
        self.assertIn("[FORBIDDEN_PATH_PRESENT] b.rs: must stay deleted", text)


if __name__ == "__main__":
    unittest.main()
