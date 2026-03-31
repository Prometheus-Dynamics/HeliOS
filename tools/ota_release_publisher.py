#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Callable, Iterable


ARTIFACT_KINDS = {"frontend_bundle", "service_bundle"}


class PublishError(RuntimeError):
    pass


@dataclass(frozen=True)
class PublishSpec:
    artifact_kind: str
    base_url: str
    source_dir: Path
    entries: tuple[str, ...]
    requested_by: str
    post_url: str
    timeout_seconds: float
    state_poll_seconds: float
    delete_image_after_apply: bool


@dataclass(frozen=True)
class UploadResponse:
    image_url: str
    size_bytes: int | None
    checksum: str | None
    raw: dict[str, Any]


@dataclass(frozen=True)
class ApplyResponse:
    update_id: str
    raw: dict[str, Any]


@dataclass(frozen=True)
class StateSnapshot:
    update_id: str | None
    stage: str | None
    last_error: str | None
    raw: dict[str, Any]


@dataclass(frozen=True)
class PublishResult:
    artifact_kind: str
    update_id: str
    bundle_path: str
    uploaded_image_url: str
    size_bytes: int | None
    checksum: str | None
    post_url: str


def shell_join(parts: Iterable[str]) -> str:
    return " ".join(subprocess.list2cmdline([part]) for part in parts)


def run(cmd: list[str], *, dry_run: bool = False, capture_output: bool = False) -> subprocess.CompletedProcess[str]:
    if dry_run:
        print(f"+ {shell_join(cmd)}")
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
    return subprocess.run(cmd, check=True, text=True, capture_output=capture_output)


def ensure_artifact_kind(value: str) -> str:
    if value not in ARTIFACT_KINDS:
        raise PublishError(f"unsupported artifact kind: {value}")
    return value


def normalize_base_url(value: str) -> str:
    normalized = value.strip().rstrip("/")
    if not normalized:
        raise PublishError("base url must not be empty")
    return normalized


def default_post_url(base_url: str, artifact_kind: str) -> str:
    if artifact_kind == "frontend_bundle":
        if base_url.endswith("/v1"):
            root = base_url[:-3]
            return f"{root}/"
        return f"{base_url}/"
    return base_url


def json_request(url: str, *, method: str = "GET", body: dict[str, Any] | None = None) -> dict[str, Any]:
    data = None
    headers: dict[str, str] = {}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = response.read().decode("utf-8")
            if not payload:
                return {}
            return json.loads(payload)
    except urllib.error.HTTPError as err:  # pragma: no cover - operational path
        body_text = err.read().decode("utf-8", errors="replace")
        raise PublishError(f"request failed ({method} {url} -> HTTP {err.code}): {body_text}") from err
    except urllib.error.URLError as err:  # pragma: no cover - operational path
        raise PublishError(f"request failed ({method} {url}): {err}") from err


def try_json_request(url: str, *, method: str = "GET", body: dict[str, Any] | None = None) -> dict[str, Any] | None:
    try:
        return json_request(url, method=method, body=body)
    except PublishError:
        return None


def http_status(url: str) -> int | None:
    request = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            response.read()
            return response.status
    except urllib.error.HTTPError as err:  # pragma: no cover - operational path
        return err.code
    except urllib.error.URLError:  # pragma: no cover - operational path
        return None


def build_bundle_archive(source_dir: Path, bundle_path: Path, entries: Iterable[str]) -> None:
    source_dir = source_dir.resolve()
    if not source_dir.is_dir():
        raise PublishError(f"source dir not found: {source_dir}")
    chosen_entries = tuple(entries)
    if not chosen_entries:
        raise PublishError("refusing to build an empty OTA bundle")
    with tarfile.open(bundle_path, "w") as archive:
        for entry in chosen_entries:
            if entry in {"", "."}:
                archive.add(source_dir, arcname=".")
                continue
            full_path = source_dir / entry
            if not full_path.exists():
                raise PublishError(f"bundle entry not found: {full_path}")
            archive.add(full_path, arcname=entry)


def parse_upload_response(payload: dict[str, Any]) -> UploadResponse:
    image_url = payload.get("image_url")
    if not isinstance(image_url, str) or not image_url.strip():
        raise PublishError("OTA upload response did not include image_url")
    size_value = payload.get("size_bytes")
    if isinstance(size_value, bool):
        size_bytes = None
    elif isinstance(size_value, (int, float)):
        size_bytes = int(size_value)
    else:
        size_bytes = None
    checksum_value = payload.get("sha256")
    checksum = checksum_value if isinstance(checksum_value, str) and checksum_value.strip() else None
    return UploadResponse(image_url=image_url, size_bytes=size_bytes, checksum=checksum, raw=payload)


def parse_apply_response(payload: dict[str, Any]) -> ApplyResponse:
    update_id = payload.get("update_id")
    if not isinstance(update_id, str) or not update_id.strip():
        raise PublishError("OTA apply response did not include update_id")
    return ApplyResponse(update_id=update_id, raw=payload)


def parse_state_snapshot(payload: dict[str, Any]) -> StateSnapshot:
    state = payload.get("state")
    if not isinstance(state, dict):
        return StateSnapshot(update_id=None, stage=None, last_error=None, raw=payload)
    update_id = state.get("update_id")
    stage = state.get("stage")
    last_error = state.get("last_error")
    return StateSnapshot(
        update_id=update_id if isinstance(update_id, str) and update_id.strip() else None,
        stage=stage if isinstance(stage, str) and stage.strip() else None,
        last_error=last_error if isinstance(last_error, str) and last_error.strip() else None,
        raw=payload,
    )


def wait_for_apply_completion(
    update_id: str,
    label: str,
    timeout_seconds: float,
    poll_state: Callable[[], StateSnapshot | None],
    *,
    now: Callable[[], float] = time.monotonic,
    sleep_fn: Callable[[float], None] = time.sleep,
) -> None:
    deadline = now() + timeout_seconds
    seen_update = False
    last_stage: str | None = None
    while now() < deadline:
        snapshot = poll_state()
        if snapshot is None:
            sleep_fn(1.0)
            continue

        if snapshot.update_id == update_id and snapshot.stage != last_stage:
            print(f"{label} OTA stage: {snapshot.stage or 'unknown'}")
            last_stage = snapshot.stage

        if snapshot.update_id == update_id:
            seen_update = True
            if snapshot.stage == "complete":
                return
            if snapshot.stage == "rolled_back":
                raise PublishError(f"{label} OTA rolled back: {snapshot.last_error or 'unknown error'}")
        elif seen_update and snapshot.update_id is None:
            print(f"{label} OTA stage: complete")
            return

        sleep_fn(1.0)

    raise PublishError(f"timed out waiting for {label} OTA apply {update_id}")


def wait_for_http_ok(url: str, label: str, timeout_seconds: float, *, poll_seconds: float) -> None:
    deadline = time.monotonic() + timeout_seconds
    last_status: int | None = None
    while time.monotonic() < deadline:
        status = http_status(url)
        if status == 200:
            return
        last_status = status
        time.sleep(poll_seconds)
    if last_status is None:
        raise PublishError(f"{label} did not become reachable at {url} after OTA apply")
    raise PublishError(f"{label} did not become reachable at {url} after OTA apply (last status: {last_status})")


class OtaClient:
    def __init__(self, base_url: str, *, dry_run: bool = False) -> None:
        self.base_url = normalize_base_url(base_url)
        self.dry_run = dry_run

    def upload_bundle(self, bundle_path: Path) -> UploadResponse:
        if self.dry_run:
            print(f"+ curl -F file=@{bundle_path} {self.base_url}/ota/upload")
            return UploadResponse(
                image_url="file:///dry-run/upload.tar",
                size_bytes=bundle_path.stat().st_size if bundle_path.exists() else None,
                checksum=None,
                raw={},
            )
        proc = run(
            ["curl", "-sS", "-F", f"file=@{bundle_path}", f"{self.base_url}/ota/upload"],
            capture_output=True,
        )
        return parse_upload_response(json.loads(proc.stdout))

    def apply_bundle(self, spec: PublishSpec, upload: UploadResponse) -> ApplyResponse:
        payload: dict[str, Any] = {
            "requested_by": spec.requested_by,
            "artifact_kind": spec.artifact_kind,
            "image_url": upload.image_url,
            "delete_image_after_apply": spec.delete_image_after_apply,
        }
        if upload.size_bytes is not None:
            payload["size_bytes"] = upload.size_bytes
        if upload.checksum is not None:
            payload["checksum"] = upload.checksum

        if self.dry_run:
            print(
                "+ curl -X POST -H 'Content-Type: application/json' "
                f"--data {json.dumps(payload)!r} {self.base_url}/ota/apply"
            )
            return ApplyResponse(update_id="dry-run-update-id", raw=payload)

        response = json_request(f"{self.base_url}/ota/apply", method="POST", body=payload)
        return parse_apply_response(response)

    def current_state(self) -> StateSnapshot | None:
        if self.dry_run:
            return StateSnapshot(update_id=None, stage=None, last_error=None, raw={})
        payload = try_json_request(f"{self.base_url}/ota/state")
        if payload is None:
            return None
        return parse_state_snapshot(payload)

    def publish(self, spec: PublishSpec, bundle_path: Path) -> PublishResult:
        upload = self.upload_bundle(bundle_path)
        apply = self.apply_bundle(spec, upload)
        if not self.dry_run:
            wait_for_apply_completion(
                apply.update_id,
                human_label(spec.artifact_kind),
                spec.timeout_seconds,
                self.current_state,
            )
            wait_for_http_ok(
                spec.post_url,
                human_label(spec.artifact_kind),
                spec.timeout_seconds,
                poll_seconds=spec.state_poll_seconds,
            )
        return PublishResult(
            artifact_kind=spec.artifact_kind,
            update_id=apply.update_id,
            bundle_path=str(bundle_path),
            uploaded_image_url=upload.image_url,
            size_bytes=upload.size_bytes,
            checksum=upload.checksum,
            post_url=spec.post_url,
        )


def human_label(artifact_kind: str) -> str:
    if artifact_kind == "frontend_bundle":
        return "Frontend"
    if artifact_kind == "service_bundle":
        return "Service bundle"
    return artifact_kind


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Publish frontend/service bundle artifacts through HeliOS OTA.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    publish = subparsers.add_parser("publish", help="Build a release bundle and publish it through OTA.")
    publish.add_argument("--artifact-kind", required=True, choices=sorted(ARTIFACT_KINDS))
    publish.add_argument("--base-url", required=True, help="OTA API base URL, for example http://172.31.250.1/v1")
    publish.add_argument("--requested-by", default="deploy-live")
    publish.add_argument("--source-dir", required=True, type=Path)
    publish.add_argument("--entry", action="append", default=[], help="Relative path inside source-dir to include. Use '.' for the full tree.")
    publish.add_argument("--post-url", help="URL that must return HTTP 200 after activation. Defaults based on artifact kind.")
    publish.add_argument("--timeout-seconds", type=float, default=120.0)
    publish.add_argument("--state-poll-seconds", type=float, default=1.0)
    publish.add_argument("--keep-image", action="store_true", help="Do not delete the staged OTA upload after apply.")
    publish.add_argument("--bundle-path", type=Path, help="Optional tar path to reuse instead of a temp file.")
    publish.add_argument("--dry-run", action="store_true")
    publish.add_argument("--json", action="store_true", help="Emit the final publish result as JSON.")
    return parser.parse_args(argv)


def build_publish_spec(args: argparse.Namespace) -> PublishSpec:
    artifact_kind = ensure_artifact_kind(args.artifact_kind)
    base_url = normalize_base_url(args.base_url)
    entries = tuple(args.entry or ["."])
    post_url = args.post_url or default_post_url(base_url, artifact_kind)
    return PublishSpec(
        artifact_kind=artifact_kind,
        base_url=base_url,
        source_dir=args.source_dir,
        entries=entries,
        requested_by=args.requested_by,
        post_url=post_url,
        timeout_seconds=args.timeout_seconds,
        state_poll_seconds=args.state_poll_seconds,
        delete_image_after_apply=not args.keep_image,
    )


def publish_command(args: argparse.Namespace) -> int:
    spec = build_publish_spec(args)
    client = OtaClient(spec.base_url, dry_run=args.dry_run)

    bundle_path = args.bundle_path
    temp_dir: tempfile.TemporaryDirectory[str] | None = None
    try:
        if bundle_path is None:
            temp_dir = tempfile.TemporaryDirectory(prefix="helios-ota-release-")
            bundle_path = Path(temp_dir.name) / f"{spec.artifact_kind}.tar"
        else:
            bundle_path = bundle_path.resolve()
            bundle_path.parent.mkdir(parents=True, exist_ok=True)

        if args.dry_run:
            print(f"+ build bundle {bundle_path} from {spec.source_dir} entries={list(spec.entries)!r}")
        else:
            build_bundle_archive(spec.source_dir, bundle_path, spec.entries)

        result = client.publish(spec, bundle_path)
        if args.json:
            print(json.dumps(asdict(result), sort_keys=True))
        else:
            print(f"{human_label(spec.artifact_kind)} publish complete: update_id={result.update_id}")
        return 0
    finally:
        if temp_dir is not None:
            temp_dir.cleanup()


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    try:
        if args.command == "publish":
            return publish_command(args)
        raise PublishError(f"unsupported command: {args.command}")
    except PublishError as err:
        print(f"error: {err}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
