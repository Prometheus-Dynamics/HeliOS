#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any


DEFAULT_CONFIG_PATH = Path(__file__).with_name("shim_guardrails.json")
DEFAULT_SCAN_ROOTS = ("backend", "frontend", "tools", ".github")
SKIP_DIR_NAMES = {
    ".git",
    ".svelte-kit",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
}
MARKER_TOKEN = "TEMP_SHIM:"


class ShimGuardrailConfigError(RuntimeError):
    pass


@dataclass(frozen=True)
class RequiredShim:
    id: str
    subsystem: str
    path: str
    owner: str
    delete_by: date
    legacy_path: str
    canonical_path: str
    removal_trigger: str
    reason: str
    replace_with: str


@dataclass(frozen=True)
class ShimGuardrailConfig:
    required_shims: tuple[RequiredShim, ...]


@dataclass(frozen=True)
class DiscoveredShim:
    id: str
    path: str
    line: int


@dataclass(frozen=True)
class Violation:
    code: str
    path: str
    message: str


def require_non_empty_text(value: Any, *, field_name: str, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ShimGuardrailConfigError(f"{context} is missing required text field {field_name}")
    return value.strip()


def require_iso_date(value: Any, *, field_name: str, context: str) -> date:
    raw = require_non_empty_text(value, field_name=field_name, context=context)
    try:
        return date.fromisoformat(raw)
    except ValueError as err:
        raise ShimGuardrailConfigError(f"{context} has invalid ISO date field {field_name}: {raw}") from err


def parse_required_shim(raw: Any, index: int) -> RequiredShim:
    context = f"required_shims[{index}]"
    if not isinstance(raw, dict):
        raise ShimGuardrailConfigError(f"{context} must be an object")
    return RequiredShim(
        id=require_non_empty_text(raw.get("id"), field_name="id", context=context),
        subsystem=require_non_empty_text(raw.get("subsystem"), field_name="subsystem", context=context),
        path=require_non_empty_text(raw.get("path"), field_name="path", context=context),
        owner=require_non_empty_text(raw.get("owner"), field_name="owner", context=context),
        delete_by=require_iso_date(raw.get("delete_by"), field_name="delete_by", context=context),
        legacy_path=require_non_empty_text(raw.get("legacy_path"), field_name="legacy_path", context=context),
        canonical_path=require_non_empty_text(raw.get("canonical_path"), field_name="canonical_path", context=context),
        removal_trigger=require_non_empty_text(raw.get("removal_trigger"), field_name="removal_trigger", context=context),
        reason=require_non_empty_text(raw.get("reason"), field_name="reason", context=context),
        replace_with=require_non_empty_text(raw.get("replace_with"), field_name="replace_with", context=context),
    )


def validate_required_shims(required: tuple[RequiredShim, ...]) -> None:
    duplicate_ids = find_duplicates(rule.id for rule in required)
    if duplicate_ids:
        raise ShimGuardrailConfigError(f"shim guardrail config defines duplicate ids: {', '.join(duplicate_ids)}")

    canonical_by_subsystem: dict[str, str] = {}
    for rule in required:
        if rule.legacy_path == rule.canonical_path:
            raise ShimGuardrailConfigError(
                f"shim {rule.id!r} declares the same legacy_path and canonical_path: {rule.legacy_path}"
            )
        prior = canonical_by_subsystem.get(rule.subsystem)
        if prior is None:
            canonical_by_subsystem[rule.subsystem] = rule.canonical_path
            continue
        if prior != rule.canonical_path:
            raise ShimGuardrailConfigError(
                "shim guardrail config defines multiple canonical paths for subsystem "
                f"{rule.subsystem!r}: {prior!r} vs {rule.canonical_path!r}"
            )


def load_config(config_path: Path) -> ShimGuardrailConfig:
    try:
        raw = json.loads(config_path.read_text(encoding="utf-8"))
    except FileNotFoundError as err:
        raise ShimGuardrailConfigError(f"shim guardrail config not found: {config_path}") from err
    except json.JSONDecodeError as err:
        raise ShimGuardrailConfigError(f"shim guardrail config is not valid json: {config_path}: {err}") from err

    if not isinstance(raw, dict):
        raise ShimGuardrailConfigError("shim guardrail config root must be an object")

    raw_required = raw.get("required_shims", [])
    if not isinstance(raw_required, list):
        raise ShimGuardrailConfigError("required_shims must be an array")

    required = tuple(parse_required_shim(item, index) for index, item in enumerate(raw_required))
    validate_required_shims(required)

    return ShimGuardrailConfig(required_shims=required)


def find_duplicates(values: Any) -> list[str]:
    seen: set[str] = set()
    duplicates: list[str] = []
    for value in values:
        if value in seen and value not in duplicates:
            duplicates.append(value)
        seen.add(value)
    return duplicates


def iter_candidate_files(repo_root: Path, scan_roots: tuple[str, ...]) -> list[Path]:
    candidates: list[Path] = []
    for root_name in scan_roots:
        root = repo_root / root_name
        if not root.exists():
            continue
        if root.is_file():
            candidates.append(root)
            continue
        for path in root.rglob("*"):
            if any(part in SKIP_DIR_NAMES for part in path.parts):
                continue
            if path.is_file():
                candidates.append(path)
    return candidates


def discover_shims(repo_root: Path, scan_roots: tuple[str, ...]) -> list[DiscoveredShim]:
    git_grep = discover_shims_with_git_grep(repo_root, scan_roots)
    if git_grep is not None:
        return git_grep

    shims: list[DiscoveredShim] = []
    for path in iter_candidate_files(repo_root, scan_roots):
        try:
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            continue
        for line_no, line in enumerate(lines, start=1):
            marker_value = extract_marker_id(line)
            if marker_value is None:
                continue
            if not marker_value:
                shims.append(
                    DiscoveredShim(
                        id="",
                        path=path.relative_to(repo_root).as_posix(),
                        line=line_no,
                    )
                )
                continue
            shims.append(
                DiscoveredShim(
                    id=marker_value,
                    path=path.relative_to(repo_root).as_posix(),
                    line=line_no,
                )
                )
    return shims


def discover_shims_with_git_grep(repo_root: Path, scan_roots: tuple[str, ...]) -> list[DiscoveredShim] | None:
    command = ["git", "grep", "-n", MARKER_TOKEN, "--", *scan_roots]
    try:
        result = subprocess.run(
            command,
            cwd=repo_root,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        return None

    if result.returncode not in (0, 1):
        return None

    shims: list[DiscoveredShim] = []
    for raw_line in result.stdout.splitlines():
        path_text, line_text, content = raw_line.split(":", 2)
        marker_value = extract_marker_id(content)
        if marker_value is None:
            continue
        shims.append(
            DiscoveredShim(
                id=marker_value,
                path=path_text,
                line=int(line_text),
            )
        )
    return shims


def extract_marker_id(line: str) -> str | None:
    stripped = line.lstrip()
    for prefix in ("// ", "//", "# ", "#"):
        marker_prefix = f"{prefix}{MARKER_TOKEN}"
        if stripped.startswith(marker_prefix):
            return stripped[len(marker_prefix) :].strip()
    return None


def evaluate_guardrails(
    repo_root: Path,
    config: ShimGuardrailConfig,
    *,
    today: date,
    scan_roots: tuple[str, ...],
) -> list[Violation]:
    violations: list[Violation] = []
    discovered = discover_shims(repo_root, scan_roots)
    discovered_by_id: dict[str, list[DiscoveredShim]] = {}
    for shim in discovered:
        if not shim.id:
            violations.append(Violation(code="SHIM_MARKER_MISSING_ID", path=f"{shim.path}:{shim.line}", message="TEMP_SHIM marker is missing a registered shim id"))
            continue
        discovered_by_id.setdefault(shim.id, []).append(shim)

    required_by_id = {rule.id: rule for rule in config.required_shims}

    for shim_id, entries in discovered_by_id.items():
        if shim_id not in required_by_id:
            for entry in entries:
                violations.append(
                    Violation(
                        code="UNREGISTERED_SHIM",
                        path=f"{entry.path}:{entry.line}",
                        message=f"shim id {shim_id!r} is not registered in {DEFAULT_CONFIG_PATH.name}",
                    )
                )

    for rule in config.required_shims:
        candidate = repo_root / rule.path
        if not candidate.is_file():
            violations.append(
                Violation(
                    code="SHIM_FILE_MISSING",
                    path=rule.path,
                    message=(
                        "shim source file is missing "
                        f"(subsystem: {rule.subsystem}; owner: {rule.owner}; canonical path: {rule.canonical_path}; "
                        f"removal trigger: {rule.removal_trigger})"
                    ),
                )
            )
            continue

        if rule.delete_by < today:
            violations.append(
                Violation(
                    code="SHIM_EXPIRED",
                    path=rule.path,
                    message=(
                        f"shim {rule.id!r} expired on {rule.delete_by.isoformat()} "
                        f"(subsystem: {rule.subsystem}; legacy path: {rule.legacy_path}; canonical path: {rule.canonical_path}; "
                        f"removal trigger: {rule.removal_trigger}; replace with: {rule.replace_with})"
                    ),
                )
            )

        matches = discovered_by_id.get(rule.id, [])
        if not matches:
            violations.append(
                Violation(
                    code="SHIM_MARKER_MISSING",
                    path=rule.path,
                    message=(
                        f"required shim marker {rule.id!r} is missing "
                        f"(subsystem: {rule.subsystem}; legacy path: {rule.legacy_path}; canonical path: {rule.canonical_path}; "
                        f"delete by: {rule.delete_by.isoformat()}; removal trigger: {rule.removal_trigger})"
                    ),
                )
            )
            continue

        if len(matches) > 1:
            locations = ", ".join(f"{entry.path}:{entry.line}" for entry in matches)
            violations.append(
                Violation(
                    code="SHIM_MARKER_DUPLICATED",
                    path=rule.path,
                    message=f"shim marker {rule.id!r} appears multiple times: {locations}",
                )
            )
            continue

        match = matches[0]
        if match.path != rule.path:
            violations.append(
                Violation(
                    code="SHIM_MARKER_WRONG_PATH",
                    path=f"{match.path}:{match.line}",
                    message=f"shim marker {rule.id!r} must live in {rule.path}, not {match.path}",
                )
            )

    return violations


def format_violations(violations: list[Violation]) -> str:
    lines = ["Shim guardrails failed:"]
    for violation in violations:
        lines.append(f"- [{violation.code}] {violation.path}: {violation.message}")
    return "\n".join(lines)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Fail when temporary compatibility shims are unregistered, missing canonical-path migration metadata, "
            "or past their removal deadline."
        )
    )
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
        help="Path to the shim guardrail config json file.",
    )
    parser.add_argument(
        "--today",
        type=date.fromisoformat,
        default=date.today(),
        help="Override the current date for testing, in YYYY-MM-DD form.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        config = load_config(args.config)
    except ShimGuardrailConfigError as err:
        print(f"Shim guardrails config error: {err}", file=sys.stderr)
        return 2

    violations = evaluate_guardrails(
        args.repo_root.resolve(),
        config,
        today=args.today,
        scan_roots=DEFAULT_SCAN_ROOTS,
    )
    if violations:
        print(format_violations(violations), file=sys.stderr)
        return 1

    subsystem_count = len({rule.subsystem for rule in config.required_shims})
    print(
        "Shim guardrails passed: "
        f"{len(config.required_shims)} required shims checked across {subsystem_count} canonical-path subsystems"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
