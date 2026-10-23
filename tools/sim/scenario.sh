#!/usr/bin/env bash
# Runs a pose scenario (tools/sim/scenarios/<name>: layout.json and camera.json) end to end: the
# sim (up.sh) with Styx rendering that layout along that camera path, as a glTF scene, then scenario.ts (the
# live-map template with the layout, each pose against Styx's truth), then stops the sim.
#   tools/sim/scenario.sh far-lone-tag
#   SIM_SCENARIO_S=64   how long poses are recorded
#   SIM_BIND, SIM_RUN_DIR, SIM_SKIP_BUILD, ...: as for up.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NAME="${1:?usage: scenario.sh <name> (a directory under tools/sim/scenarios)}"
DIR="$ROOT/tools/sim/scenarios/$NAME"
[[ -f "$DIR/layout.json" && -f "$DIR/camera.json" ]] || { echo "no $DIR/layout.json and camera.json" >&2; exit 2; }
command -v bun >/dev/null || { echo "scenario.sh needs bun" >&2; exit 2; }

export SIM_RUN_DIR="${SIM_RUN_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/helios-scenario.XXXXXX")}"
export SIM_BIND="${SIM_BIND:-127.0.0.1:5800}"
CALIBRATION="$ROOT/tools/sim/ov9782-calibration.json"
# up.sh builds the scene from these (helios-sim-scene) and sets the camera spec, as for the default.
unset STYX_SIM_CAMERAS
export SIM_SCENE_LAYOUT="$DIR/layout.json" SIM_SCENE_CAMERA="$DIR/camera.json" SIM_SCENE_CALIBRATION="$CALIBRATION"

"$ROOT/tools/sim/up.sh" > "$SIM_RUN_DIR/up.log" 2>&1 &
UP=$!
trap 'kill -TERM "$UP" 2>/dev/null || true; wait "$UP" 2>/dev/null || true' EXIT
deadline=$((SECONDS + ${SIM_UP_TIMEOUT_S:-900}))
until curl -fsS "http://$SIM_BIND/v1/health" >/dev/null 2>&1; do
  kill -0 "$UP" 2>/dev/null || { echo "the sim exited (see $SIM_RUN_DIR/up.log)" >&2; exit 1; }
  ((SECONDS < deadline)) || { echo "the sim did not come up (see $SIM_RUN_DIR/up.log)" >&2; exit 1; }
  sleep 1
done
SIM_URL="http://$SIM_BIND" SIM_TRUTH="$SIM_RUN_DIR/truth.jsonl" SIM_SCENARIO="$NAME" SIM_CALIBRATION="$CALIBRATION" \
  bun run "$ROOT/tools/sim/scenario.ts"
