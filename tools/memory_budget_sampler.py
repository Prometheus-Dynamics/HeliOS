#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import json
import math
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_PROCESSES = [
    "helios-engine",
    "helios-api",
    "helios-peripherals",
]


@dataclass
class ProcessSample:
    name: str
    pss_kib: int
    rss_kib: int


def run(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def ssh_base(target: str, password: str | None) -> list[str]:
    base: list[str] = []
    if password:
        base.extend(["sshpass", "-p", password])
    base.extend(
        [
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            target,
        ]
    )
    return base


def ssh_run(target: str, password: str | None, shell_script: str) -> str:
    cmd = ssh_base(target, password) + ["bash", "-s"]
    proc = subprocess.run(cmd, input=shell_script, check=True, text=True, capture_output=True)
    return proc.stdout


def snapshot_remote_processes(target: str, password: str | None, processes: list[str]) -> list[ProcessSample]:
    process_list = " ".join(processes)
    script = f"""\
set -euo pipefail
for p in {process_list}; do
  pid=$(pidof "$p" || true)
  if [ -n "$pid" ]; then
    awk -v name="$p" 'BEGIN{{pss=0;rss=0}} /^Pss:/{{pss+=$2}} /^Rss:/{{rss+=$2}} END{{printf "%s %d %d\\n", name, pss, rss}}' "/proc/$pid/smaps_rollup"
  fi
done
"""
    out = ssh_run(target, password, script)
    samples: list[ProcessSample] = []
    for line in out.splitlines():
        parts = line.strip().split()
        if len(parts) != 3:
            continue
        samples.append(ProcessSample(name=parts[0], pss_kib=int(parts[1]), rss_kib=int(parts[2])))
    return samples


def restart_services(target: str, password: str | None, services: list[str]) -> None:
    service_list = " ".join(services)
    ssh_run(target, password, f"set -euo pipefail\nsystemctl restart {service_list}\n")


def http_json(url: str, method: str = "GET", body: bytes | None = None) -> Any:
    req = urllib.request.Request(url, data=body, method=method)
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def http_status(url: str, method: str = "GET", body: bytes | None = None) -> int:
    req = urllib.request.Request(url, data=body, method=method)
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            resp.read()
            return resp.status
    except urllib.error.HTTPError as err:
        return err.code


def wait_for_http_ok(url: str, timeout_seconds: float) -> None:
    deadline = time.monotonic() + timeout_seconds
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            if http_status(url) == 200:
                return
        except Exception as err:  # pragma: no cover - best-effort operational retry
            last_error = err
        time.sleep(0.5)
    if last_error is not None:
        raise RuntimeError(f"timed out waiting for {url}: {last_error}")
    raise RuntimeError(f"timed out waiting for {url}")


class PreviewConsumer:
    def __init__(self, url: str) -> None:
        self.url = url
        self.proc: subprocess.Popen[str] | None = None

    def start(self) -> None:
        self.proc = subprocess.Popen(
            ["curl", "-sf", self.url, "-o", "/dev/null"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )

    def stop(self) -> None:
        if self.proc is None:
            return
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)
        self.proc = None


def percentile(sorted_values: list[int], pct: float) -> int:
    if not sorted_values:
        raise ValueError("no values")
    idx = int((len(sorted_values) - 1) * pct)
    idx = max(0, min(len(sorted_values) - 1, idx))
    return sorted_values[idx]


def mib_from_kib(value_kib: int) -> float:
    return value_kib / 1024.0


DEVICE_HELP = (
    "Each board has its own USB address (Atlas serial-hash-v1, e.g. 172.31.209.217): the board identity's gadget.address (curl http://<board>.local:5899/.well-known/pd-device), Atlas, or `ip -4 addr show usbbr0` on the board."
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Sample live Helios process memory against a threshold.",
        epilog="The device has no default address. " + DEVICE_HELP,
    )
    parser.add_argument("--device", default=os.environ.get("HELIOS_DEVICE"), help="Board address or host name (default: $HELIOS_DEVICE)")
    parser.add_argument("--ssh", help="SSH target (default: root@<device>)")
    parser.add_argument("--pass", dest="password", default="root", help="SSH password for sshpass")
    parser.add_argument("--api-url", help="Base API URL (default: http://<device>:5800)")
    parser.add_argument("--manifest-json", type=Path, help="Optional stream manifest JSON to POST before sampling")
    parser.add_argument("--preview", action="store_true", help="Attach a /preview consumer during sampling")
    parser.add_argument("--restart-services", action="store_true", help="Restart core services before sampling")
    parser.add_argument("--api-ready-timeout-seconds", type=float, default=60.0, help="Seconds to wait for helios-api after restarts")
    parser.add_argument("--settle-seconds", type=float, default=5.0, help="Seconds to settle before sampling")
    parser.add_argument("--sample-seconds", type=float, default=60.0, help="Sampling duration")
    parser.add_argument("--interval-seconds", type=float, default=1.0, help="Sampling interval")
    parser.add_argument("--threshold-mib", type=float, default=100.0, help="Threshold for Helios subtotal PSS")
    parser.add_argument("--required-percent", type=float, default=90.0, help="Required percent of samples under threshold")
    parser.add_argument("--processes", nargs="*", default=DEFAULT_PROCESSES, help="Processes to include in subtotal")
    parser.add_argument("--output-json", type=Path, help="Optional path to write full sample data as JSON")
    args = parser.parse_args()
    if not args.device and not (args.ssh and args.api_url):
        parser.error("no device: pass --device <addr> or set HELIOS_DEVICE (or both --ssh and --api-url). " + DEVICE_HELP)
    args.ssh = args.ssh or f"root@{args.device}"
    args.api_url = args.api_url or f"http://{args.device}:5800"
    return args


def main() -> int:
    args = parse_args()
    api_url = args.api_url.rstrip("/")
    stream_id: str | None = None
    preview: PreviewConsumer | None = None

    try:
        if args.restart_services:
            restart_services(
                args.ssh,
                args.password,
                ["helios-engine", "helios-api", "helios-peripherals"],
            )
            wait_for_http_ok(f"{api_url}/v1/health", args.api_ready_timeout_seconds)

        if args.manifest_json:
            manifest = args.manifest_json.read_bytes()
            response = http_json(f"{api_url}/v1/streams", method="POST", body=manifest)
            stream_id = response["stream_id"]

        if args.preview and stream_id:
            preview = PreviewConsumer(f"{api_url}/v1/streams/{stream_id}/preview")
            preview.start()

        if args.settle_seconds > 0:
            time.sleep(args.settle_seconds)

        deadline = time.monotonic() + args.sample_seconds
        samples: list[dict[str, Any]] = []

        while True:
            now = time.monotonic()
            if now > deadline and samples:
                break
            snapshot = snapshot_remote_processes(args.ssh, args.password, args.processes)
            subtotal_kib = sum(sample.pss_kib for sample in snapshot)
            samples.append(
                {
                    "ts_monotonic": now,
                    "subtotal_pss_kib": subtotal_kib,
                    "processes": [
                        {"name": sample.name, "pss_kib": sample.pss_kib, "rss_kib": sample.rss_kib}
                        for sample in snapshot
                    ],
                }
            )
            sleep_for = args.interval_seconds - (time.monotonic() - now)
            if sleep_for > 0 and time.monotonic() + sleep_for < deadline + args.interval_seconds:
                time.sleep(sleep_for)
            if time.monotonic() > deadline and samples:
                break

        subtotals = sorted(sample["subtotal_pss_kib"] for sample in samples)
        threshold_kib = int(args.threshold_mib * 1024)
        under = sum(value <= threshold_kib for value in subtotals)
        under_pct = (under / len(subtotals)) * 100.0
        summary = {
            "samples": len(subtotals),
            "threshold_mib": args.threshold_mib,
            "required_percent": args.required_percent,
            "under_threshold_samples": under,
            "under_threshold_percent": under_pct,
            "min_mib": mib_from_kib(subtotals[0]),
            "p50_mib": mib_from_kib(percentile(subtotals, 0.50)),
            "p90_mib": mib_from_kib(percentile(subtotals, 0.90)),
            "max_mib": mib_from_kib(subtotals[-1]),
            "avg_mib": mib_from_kib(int(sum(subtotals) / len(subtotals))),
            "passes_gate": under_pct >= args.required_percent,
            "stream_id": stream_id,
            "preview": bool(args.preview and stream_id),
        }

        print(json.dumps(summary, indent=2, sort_keys=True))

        if args.output_json:
            args.output_json.write_text(json.dumps({"summary": summary, "samples": samples}, indent=2))

        return 0 if summary["passes_gate"] else 1
    finally:
        if preview is not None:
            preview.stop()
        if stream_id is not None:
            http_status(f"{api_url}/v1/streams/{stream_id}", method="DELETE")


if __name__ == "__main__":
    raise SystemExit(main())
