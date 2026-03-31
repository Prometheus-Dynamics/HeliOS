#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_CONFIG_PATH = Path(__file__).with_name("architecture_guardrails.json")


class GuardrailConfigError(RuntimeError):
    pass


@dataclass(frozen=True)
class LineLimitRule:
    path: str
    max_lines: int
    owner: str
    review_note: str


@dataclass(frozen=True)
class ForbiddenPathRule:
    path: str
    owner: str
    reason: str


@dataclass(frozen=True)
class GuardrailConfig:
    line_limits: tuple[LineLimitRule, ...]
    forbidden_paths: tuple[ForbiddenPathRule, ...]


@dataclass(frozen=True)
class Violation:
    code: str
    path: str
    message: str


def require_non_empty_text(value: Any, *, field_name: str, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise GuardrailConfigError(f"{context} is missing required text field {field_name}")
    return value.strip()


def require_positive_int(value: Any, *, field_name: str, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise GuardrailConfigError(f"{context} has invalid positive integer field {field_name}")
    return value


def parse_line_limit_rule(raw: Any, index: int) -> LineLimitRule:
    context = f"line_limits[{index}]"
    if not isinstance(raw, dict):
        raise GuardrailConfigError(f"{context} must be an object")
    return LineLimitRule(
        path=require_non_empty_text(raw.get("path"), field_name="path", context=context),
        max_lines=require_positive_int(raw.get("max_lines"), field_name="max_lines", context=context),
        owner=require_non_empty_text(raw.get("owner"), field_name="owner", context=context),
        review_note=require_non_empty_text(raw.get("review_note"), field_name="review_note", context=context),
    )


def parse_forbidden_path_rule(raw: Any, index: int) -> ForbiddenPathRule:
    context = f"forbidden_paths[{index}]"
    if not isinstance(raw, dict):
        raise GuardrailConfigError(f"{context} must be an object")
    return ForbiddenPathRule(
        path=require_non_empty_text(raw.get("path"), field_name="path", context=context),
        owner=require_non_empty_text(raw.get("owner"), field_name="owner", context=context),
        reason=require_non_empty_text(raw.get("reason"), field_name="reason", context=context),
    )


def load_config(config_path: Path) -> GuardrailConfig:
    try:
        raw = json.loads(config_path.read_text(encoding="utf-8"))
    except FileNotFoundError as err:
        raise GuardrailConfigError(f"guardrail config not found: {config_path}") from err
    except json.JSONDecodeError as err:
        raise GuardrailConfigError(f"guardrail config is not valid json: {config_path}: {err}") from err

    if not isinstance(raw, dict):
        raise GuardrailConfigError("guardrail config root must be an object")

    raw_line_limits = raw.get("line_limits", [])
    raw_forbidden_paths = raw.get("forbidden_paths", [])
    if not isinstance(raw_line_limits, list):
        raise GuardrailConfigError("line_limits must be an array")
    if not isinstance(raw_forbidden_paths, list):
        raise GuardrailConfigError("forbidden_paths must be an array")

    return GuardrailConfig(
        line_limits=tuple(parse_line_limit_rule(item, index) for index, item in enumerate(raw_line_limits)),
        forbidden_paths=tuple(parse_forbidden_path_rule(item, index) for index, item in enumerate(raw_forbidden_paths)),
    )


def count_lines(path: Path) -> int:
    return len(path.read_text(encoding="utf-8", errors="replace").splitlines())


def evaluate_guardrails(repo_root: Path, config: GuardrailConfig) -> list[Violation]:
    violations: list[Violation] = []

    for rule in config.line_limits:
        candidate = repo_root / rule.path
        if not candidate.is_file():
            violations.append(
                Violation(
                    code="GUARDED_FILE_MISSING",
                    path=rule.path,
                    message=(
                        f"guarded file is missing; either update the architecture guardrail or restore the file "
                        f"(owner: {rule.owner}; review note: {rule.review_note})"
                    ),
                )
            )
            continue

        line_count = count_lines(candidate)
        if line_count > rule.max_lines:
            violations.append(
                Violation(
                    code="MAX_LINES_EXCEEDED",
                    path=rule.path,
                    message=(
                        f"{line_count} lines exceeds max {rule.max_lines} "
                        f"(owner: {rule.owner}; review note: {rule.review_note})"
                    ),
                )
            )

    for rule in config.forbidden_paths:
        candidate = repo_root / rule.path
        if candidate.exists():
            violations.append(
                Violation(
                    code="FORBIDDEN_PATH_PRESENT",
                    path=rule.path,
                    message=f"forbidden path is present (owner: {rule.owner}; reason: {rule.reason})",
                )
            )

    return violations


def format_violations(violations: list[Violation]) -> str:
    lines = ["Architecture guardrails failed:"]
    for violation in violations:
        lines.append(f"- [{violation.code}] {violation.path}: {violation.message}")
    return "\n".join(lines)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Fail when protected files regrow or forbidden paths reappear.")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="Path to the repository root. Defaults to the parent of tools/.",
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG_PATH,
        help="Path to the guardrail config json file.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        config = load_config(args.config)
    except GuardrailConfigError as err:
        print(f"Architecture guardrails config error: {err}", file=sys.stderr)
        return 2

    violations = evaluate_guardrails(args.repo_root.resolve(), config)
    if violations:
        print(format_violations(violations), file=sys.stderr)
        return 1

    print(
        "Architecture guardrails passed: "
        f"{len(config.line_limits)} line-limit rules, {len(config.forbidden_paths)} forbidden-path rules"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
