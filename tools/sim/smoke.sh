#!/usr/bin/env bash
# Smoke test of a running HeliOS sim (start one with tools/sim/up.sh): the camera appears, an
# AprilTag pipeline ticks with no failed ticks, live outputs carry frame_ts_ns, and preview frames
# match those outputs by capture time. Prints a pass/fail line per check and the timings.
#   SIM_URL=http://127.0.0.1:5800   helios-api (default)
#   SIM_SMOKE_TIMEOUT_S=60          per wait (camera, ticks)
#   SIM_SMOKE_WINDOW_MS=4000        how long live and preview are recorded for the match
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
command -v bun >/dev/null || { echo "smoke.sh needs bun (https://bun.sh): its WebSocket client runs the checks" >&2; exit 2; }
exec bun run "$ROOT/tools/sim/smoke.ts" "$@"
