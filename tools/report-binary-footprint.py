#!/usr/bin/env python3

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


def run(cmd, cwd=None, check=True):
    return subprocess.run(cmd, cwd=cwd, check=check, capture_output=True, text=True)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_size_summary(path: Path):
    output = run(["size", "-d", str(path)]).stdout.splitlines()
    if len(output) < 2:
        return None
    parts = output[1].split()
    if len(parts) < 5:
        return None
    return {
        "text_bytes": int(parts[0]),
        "data_bytes": int(parts[1]),
        "bss_bytes": int(parts[2]),
        "dec_bytes": int(parts[3]),
        "hex": parts[4],
    }


def parse_sections(path: Path):
    output = run(["size", "-A", "-d", str(path)]).stdout.splitlines()
    sections = []
    for line in output[2:]:
        parts = line.split()
        if len(parts) < 2:
            continue
        try:
            size_bytes = int(parts[1])
        except ValueError:
            continue
        sections.append({"name": parts[0], "size_bytes": size_bytes})
    sections.sort(key=lambda section: section["size_bytes"], reverse=True)
    return sections


def parse_needed_libs(path: Path):
    output = run(["readelf", "-d", "-W", str(path)]).stdout.splitlines()
    libs = []
    for line in output:
        if "(NEEDED)" not in line:
            continue
        start = line.find("[")
        end = line.find("]", start + 1)
        if start == -1 or end == -1:
            continue
        libs.append(line[start + 1 : end])
    return libs


def parse_top_symbols(path: Path, limit: int):
    output = run(["nm", "-S", "--size-sort", "--print-size", "--radix=d", "--demangle", str(path)], check=False)
    if output.returncode != 0:
        return []
    symbols = []
    for line in output.stdout.splitlines():
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        try:
            size_bytes = int(parts[1])
        except ValueError:
            continue
        if size_bytes <= 0:
            continue
        symbols.append({"size_bytes": size_bytes, "symbol": parts[3]})
    symbols.sort(key=lambda symbol: symbol["size_bytes"], reverse=True)
    return symbols[:limit]


def parse_cargo_bloat(stdout: str):
    crates = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(payload, dict):
            continue
        name = payload.get("name")
        size = payload.get("size")
        if not isinstance(name, str) or not isinstance(size, int):
            continue
        crates.append({"crate": name, "size_bytes": size})
    crates.sort(key=lambda crate: crate["size_bytes"], reverse=True)
    return crates


def cargo_bloat_report(manifest_path: Path, package: str, bin_name: str, target_dir: str | None):
    if shutil.which("cargo-bloat") is None:
        return {"available": False, "reason": "cargo-bloat not installed"}
    cmd = [
        "cargo",
        "bloat",
        "--manifest-path",
        str(manifest_path),
        "--release",
        "--crates",
        "--message-format",
        "json",
        "-n",
        "0",
        "-p",
        package,
        "--bin",
        bin_name,
    ]
    if target_dir:
        cmd.extend(["--target-dir", target_dir])
    result = run(cmd, cwd=str(manifest_path.parent), check=False)
    if result.returncode != 0:
        return {"available": True, "error": result.stderr.strip() or result.stdout.strip()}
    return {"available": True, "crates": parse_cargo_bloat(result.stdout)}


def parse_spec(raw: str):
    parts = raw.split(":", 2)
    if len(parts) != 3:
        raise ValueError(f"invalid spec {raw!r}; expected package:bin:path")
    return {"package": parts[0], "bin": parts[1], "path": parts[2]}


def build_report(spec, manifest_path: Path | None, include_bloat: bool, top_symbols: int, target_dir: str | None):
    path = Path(spec["path"]).resolve()
    report = {
        "package": spec["package"],
        "bin": spec["bin"],
        "path": str(path),
        "exists": path.exists(),
    }
    if not path.exists():
        return report

    report.update(
        {
            "file_size_bytes": path.stat().st_size,
            "sha256": sha256_file(path),
            "size": parse_size_summary(path),
            "sections": parse_sections(path),
            "needed_libs": parse_needed_libs(path),
            "top_symbols": parse_top_symbols(path, top_symbols),
        }
    )

    if include_bloat and manifest_path is not None:
        report["cargo_bloat"] = cargo_bloat_report(manifest_path, spec["package"], spec["bin"], target_dir)

    return report


def main():
    parser = argparse.ArgumentParser(description="Report binary footprint and linked/runtime-facing size signals.")
    parser.add_argument("--manifest-path", help="Cargo manifest to use for optional cargo-bloat runs")
    parser.add_argument("--spec", action="append", default=[], help="Binary spec in package:bin:path form")
    parser.add_argument("--output", help="Write JSON report to this path instead of stdout")
    parser.add_argument("--include-bloat", action="store_true", help="Include cargo-bloat per-crate breakdowns")
    parser.add_argument("--top-symbols", type=int, default=20, help="Number of largest symbols to include")
    parser.add_argument("--target-dir", help="Optional cargo target-dir for cargo-bloat")
    args = parser.parse_args()

    if not args.spec:
        parser.error("at least one --spec is required")

    manifest_path = Path(args.manifest_path).resolve() if args.manifest_path else None
    specs = [parse_spec(raw) for raw in args.spec]
    report = {
        "cwd": os.getcwd(),
        "manifest_path": str(manifest_path) if manifest_path else None,
        "reports": [build_report(spec, manifest_path, args.include_bloat, args.top_symbols, args.target_dir) for spec in specs],
    }
    encoded = json.dumps(report, indent=2, sort_keys=True)
    if args.output:
        Path(args.output).write_text(encoded + "\n", encoding="utf-8")
    else:
        sys.stdout.write(encoded + "\n")


if __name__ == "__main__":
    main()
