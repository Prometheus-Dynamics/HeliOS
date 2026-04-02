#!/usr/bin/env python3
import os
import pathlib
import subprocess


def main() -> int:
    repo_root = pathlib.Path(__file__).resolve().parents[1]
    env = os.environ.copy()
    env["UPDATE_STREAM_CAPABILITY_GOLDENS"] = "1"
    tmpdir = repo_root / ".tmp" / "stream-capability-goldens"
    tmpdir.mkdir(parents=True, exist_ok=True)
    env["TMPDIR"] = str(tmpdir)
    cmd = [
        "cargo",
        "test",
        "-p",
        "helios-api",
        "stream_capability_compatibility_goldens_are_in_sync",
        "--",
        "--nocapture",
    ]
    subprocess.run(cmd, cwd=repo_root / "backend", env=env, check=True)
    print("Updated testdata/stream_capability_compatibility_goldens.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
