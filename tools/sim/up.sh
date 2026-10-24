#!/usr/bin/env bash
# Runs the HeliOS services on this x86 machine against Styx's simulated cameras and Lemnos's
# simulated board: orion-node, lemnosd --sim (the Raze board definition over simulated buses) and
# its Orion bridge lemnos-orion, helios-peripherals (sim cameras from STYX_SIM_CAMERAS),
# helios-engine and helios-api, on a temp runtime directory. The UI is served by helios-api on the same port, as on the board.
#
# Tier 1 of docs/docs/fast-test-pipeline-plan.md: the first step of the Docker sim (`sim-x86_64`),
# so every path and variable here is one a container can set too:
#   SIM_SKIP_BUILD=1          run prebuilt binaries (no cargo, no bun)
#   SIM_PROFILE=NAME          the cargo profile (default dev; dev-release for timings and long runs)
#   SIM_BIN_DIR=DIR           the HeliOS binaries and libhelios_eidos_plugin.so (default: that
#                             profile's backend/target directory)
#   ORION_NODE_BIN=PATH       an orion-node binary (default: built from Orion at the rev in backend/Cargo.toml)
#   SIM_UI_DIR=DIR            the UI's static build (default: ui/build, built with bun when missing)
#   SIM_RUN_DIR=DIR           runtime dir (default: a fresh mktemp dir; kept after exit for its logs)
#   SIM_BIND=ADDR:PORT        helios-api's address (default 127.0.0.1:5800)
#   STYX_SIM_CAMERAS=SPEC     simulated cameras (default: an OV9782 rendering a glTF scene built by
#                             helios-sim-scene from SIM_SCENE_LAYOUT and SIM_SCENE_CAMERA, through a
#                             real Raze lens (SIM_SCENE_CALIBRATION, tools/sim/ov9782-calibration.json),
#                             with ground truth in <run dir>/truth.jsonl; see docs/simulation.md in Styx)
#   SIM_SCENE_LAYOUT=PATH     the default scene's tags (default tools/sim/scenarios/default/layout.json)
#   SIM_SCENE_CAMERA=PATH     the default scene's camera (default tools/sim/scenarios/default/camera.json)
#   SIM_SCENE_CALIBRATION=PATH  the default scene's lens (default tools/sim/ov9782-calibration.json)
#   SIM_SCENE_SCENERY=PATH    textured scenery besides the tags, a scenery.json (default: none)
#   SIM_CLOCK=lockstep        Styx's sim clock (default real): lockstep advances each frame's time
#                             by exactly one frame period however slow the render, so the camera
#                             moves per frame as at the mode's frame rate (slower than wall time)
#   SIM_SCENE_TIME_SCALE=N    play the camera path N times slower (default 1): with the sim
#                             rendering at R fps, N = 120 / R gives each frame the motion of a
#                             120 fps camera
#   LEMNOSD_BIN_DIR=DIR       lemnosd and lemnos-orion built with `sim,orion` (default: built from
#                             Lemnos at the commit backend/Cargo.lock pins)
#   SIM_LEMNOS=0              run without lemnosd (the services' no-hardware paths)
#   SIM_LEMNOS_BOARD=PATH     lemnosd's board (default tools/sim/raze-board.toml, Atlas's Raze file)
#   SIM_LEMNOS_SCRIPT=PATH    the simulated board's script (default tools/sim/lemnos-sim.toml)
#   SIM_BOARD_CAMERA_FILE=PATH  the board's camera description helios-api reads for an
#                             uncalibrated camera's lens hint (default: none, so the lens is
#                             "unknown"; tools/sim/raze-camera.toml is the Raze's: fisheye)
#   SIM_JOBS=N                cargo jobs (default 8)
#
# Ctrl-C (or a service exiting) stops the services this script started, by PID, and nothing else.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BACKEND="$ROOT/backend"
SIM_JOBS="${SIM_JOBS:-8}"
SIM_BIND="${SIM_BIND:-127.0.0.1:5800}"
STYX_SIM_CAMERAS="${STYX_SIM_CAMERAS:-}"
SIM_RUST_LOG="${SIM_RUST_LOG:-info}"

log() { printf '[sim] %s\n' "$*"; }
die() { printf '[sim] error: %s\n' "$*" >&2; exit 1; }

# --- Build -------------------------------------------------------------------------------------

SIM_PROFILE="${SIM_PROFILE:-dev}"
if [[ "$SIM_PROFILE" == dev ]]; then PROFILE_DIR=debug; else PROFILE_DIR="$SIM_PROFILE"; fi
SIM_BIN_DIR="${SIM_BIN_DIR:-$BACKEND/target/$PROFILE_DIR}"
if [[ -z "${SIM_SKIP_BUILD:-}" ]]; then
  command -v cargo >/dev/null || die "cargo not found"
  log "building HeliOS services ($SIM_PROFILE profile, Styx sim feature)"
  # Board builds never set `sim`: only this script's build enables helios-peripherals/sim.
  nice -n 10 cargo build --manifest-path "$BACKEND/Cargo.toml" -j "$SIM_JOBS" --profile "$SIM_PROFILE" \
    -p helios-peripherals -p helios-engine -p helios-api -p helios-eidos-plugin -p helios-sim-scene \
    --features helios-peripherals/sim
fi

# orion-node for x86: the appliance feature set of Gaia's packaging/gaia/orion-node.toml, built
# from the Orion commit backend/Cargo.toml pins (the same control protocol as the HeliOS services).
if [[ -z "${ORION_NODE_BIN:-}" ]]; then
  ORION_REV="$(sed -n 's/^orion = .*rev = "\([0-9a-f]*\)".*/\1/p' "$BACKEND/Cargo.toml" | head -n1)"
  [[ -n "$ORION_REV" ]] || die "no orion rev in backend/Cargo.toml"
  ORION_NODE_BIN="$ROOT/backend/target/sim/orion/bin/orion-node"
  if [[ -z "${SIM_SKIP_BUILD:-}" ]]; then
    command -v cargo >/dev/null || die "cargo not found"
    log "installing orion-node $ORION_REV (dev profile, appliance features) to backend/target/sim/orion"
    nice -n 10 cargo install --locked --debug --git https://github.com/Prometheus-Dynamics/Orion.git \
      --rev "$ORION_REV" --no-default-features --features peer-tcp,discovery-mdns,systemd-notify \
      --root "$ROOT/backend/target/sim/orion" --target-dir "$ROOT/backend/target/sim/orion-build" \
      -j "$SIM_JOBS" orion-node
  fi
fi
[[ -x "$ORION_NODE_BIN" ]] || die "orion-node not executable: $ORION_NODE_BIN"

# lemnosd with its simulated board and the Orion bridge, from the Lemnos commit backend/Cargo.lock
# pins for lemnos-ipc (the same IPC the HeliOS services speak). `sim` is never in a board build.
SIM_LEMNOS="${SIM_LEMNOS:-1}"
if [[ "$SIM_LEMNOS" != 0 && -z "${LEMNOSD_BIN_DIR:-}" ]]; then
  LEMNOS_REV="$(grep -A2 '^name = "lemnos-ipc"' "$BACKEND/Cargo.lock" | sed -n 's/.*#\([0-9a-f]*\)"$/\1/p' | head -n1)"
  [[ -n "$LEMNOS_REV" ]] || die "no lemnos-ipc commit in backend/Cargo.lock"
  LEMNOSD_BIN_DIR="$ROOT/backend/target/sim/lemnos/bin"
  if [[ -z "${SIM_SKIP_BUILD:-}" ]]; then
    command -v cargo >/dev/null || die "cargo not found"
    log "installing lemnosd and lemnos-orion $LEMNOS_REV (dev profile, sim and orion) to backend/target/sim/lemnos"
    nice -n 10 cargo install --locked --debug --git https://github.com/Prometheus-Dynamics/Lemnos.git \
      --rev "$LEMNOS_REV" --features sim,orion --root "$ROOT/backend/target/sim/lemnos" \
      --target-dir "$ROOT/backend/target/sim/lemnos-build" -j "$SIM_JOBS" lemnosd
  fi
fi
if [[ "$SIM_LEMNOS" != 0 ]]; then
  for bin in lemnosd lemnos-orion; do
    [[ -x "$LEMNOSD_BIN_DIR/$bin" ]] || die "missing $LEMNOSD_BIN_DIR/$bin (build without SIM_SKIP_BUILD, set LEMNOSD_BIN_DIR, or SIM_LEMNOS=0)"
  done
fi
SIM_LEMNOS_BOARD="${SIM_LEMNOS_BOARD:-$ROOT/tools/sim/raze-board.toml}"
SIM_LEMNOS_SCRIPT="${SIM_LEMNOS_SCRIPT:-$ROOT/tools/sim/lemnos-sim.toml}"

for bin in helios-peripherals helios-engine helios-api helios-sim-scene libhelios_eidos_plugin.so; do
  [[ -e "$SIM_BIN_DIR/$bin" ]] || die "missing $SIM_BIN_DIR/$bin (build without SIM_SKIP_BUILD, or set SIM_BIN_DIR)"
done

# The UI: helios-api serves ui/build when it exists. Built with bun when it does not.
SIM_UI_DIR="${SIM_UI_DIR:-$ROOT/ui/build}"
if [[ ! -f "$SIM_UI_DIR/index.html" && -z "${SIM_SKIP_BUILD:-}" ]]; then
  if command -v bun >/dev/null; then
    log "building the UI (bun)"
    ( cd "$ROOT/ui" && { [[ -d node_modules ]] || bun install --frozen-lockfile; } && bun run build )
  else
    log "bun not found: the UI is not built, helios-api serves the API only"
  fi
fi
if [[ ! -f "$SIM_UI_DIR/index.html" ]]; then
  log "no UI at $SIM_UI_DIR: serving the API only"
  SIM_UI_DIR=off
fi

# --- Runtime directory -------------------------------------------------------------------------

RUN="${SIM_RUN_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/helios-sim.XXXXXX")}"
mkdir -p "$RUN"/{orion,orion-state,ipc/streams,engine,plugins,state,api/state,api/uploads,streams,logs,lemnos,board}
# Only the Eidos plugin library: the engine loads every .so in its plugin directories.
ln -sf "$SIM_BIN_DIR/libhelios_eidos_plugin.so" "$RUN/plugins/libhelios_eidos_plugin.so"
# The default camera: a glTF scene of AprilTags (helios-sim-scene) through a real Raze OV9782 lens.
# Each frame's true camera pose and truth points go to truth.jsonl, which the smoke test scores
# detections against. Styx's spec keys for the lens come from helios-sim-scene's stdout.
if [[ -z "$STYX_SIM_CAMERAS" ]]; then
  SIM_SCENE_LAYOUT="${SIM_SCENE_LAYOUT:-$ROOT/tools/sim/scenarios/default/layout.json}"
  SIM_SCENE_CAMERA="${SIM_SCENE_CAMERA:-$ROOT/tools/sim/scenarios/default/camera.json}"
  SIM_SCENE_CALIBRATION="${SIM_SCENE_CALIBRATION:-$ROOT/tools/sim/ov9782-calibration.json}"
  SIM_SCENE_SCENERY="${SIM_SCENE_SCENERY:-}"
  SCENERY_ARGS=()
  if [[ -n "$SIM_SCENE_SCENERY" ]]; then
    [[ -f "$SIM_SCENE_SCENERY" ]] || die "no scenery file $SIM_SCENE_SCENERY"
    SCENERY_ARGS=(--scenery "$SIM_SCENE_SCENERY")
  fi
  log "building the scene into $RUN/scene.glb"
  LENS_SPEC="$("$SIM_BIN_DIR/helios-sim-scene" --layout "$SIM_SCENE_LAYOUT" --camera "$SIM_SCENE_CAMERA" \
    --calibration "$SIM_SCENE_CALIBRATION" "${SCENERY_ARGS[@]}" --time-scale "${SIM_SCENE_TIME_SCALE:-1}" \
    --out "$RUN/scene.glb")" || die "helios-sim-scene failed"
  STYX_SIM_CAMERAS="ov9782,scene=gltf,path=$RUN/scene.glb,$LENS_SPEC,clock=${SIM_CLOCK:-real},truth=$RUN/truth.jsonl"
fi
export STYX_SIM_CAMERAS
# An empty startup file: no default pipeline (smoke.sh creates its own).
: > "$RUN/startup.toml"

NODE_ID="node-local"
ORION_SOCKET="$RUN/orion/control.sock"
ORION_STREAM_SOCKET="$RUN/orion/control-stream.sock"
ENGINE_SOCKET="$RUN/engine/engine.sock"
ENGINE_LIVE_SOCKET="$RUN/engine/engine-live.sock"
LEMNOSD_SOCKET="$RUN/lemnos/lemnosd.sock"   # absent with SIM_LEMNOS=0: the services run without it
PIDS=()
NAMES=()

# --- Start and stop ----------------------------------------------------------------------------

start() {
  local name="$1"; shift
  local log_file="$RUN/logs/$name.log"
  # Each service gets only what it reads: the environment is set per command, not exported.
  env "$@" >"$log_file" 2>&1 &
  PIDS+=("$!")
  NAMES+=("$name")
  log "started $name (pid $!, log $log_file)"
}

stop_all() {
  local i
  trap - EXIT INT TERM
  if ((${#PIDS[@]})); then
    log "stopping ${NAMES[*]}"
    for i in "${!PIDS[@]}"; do kill -TERM "${PIDS[$i]}" 2>/dev/null || true; done
    local deadline=$((SECONDS + 15))
    for i in "${!PIDS[@]}"; do
      while kill -0 "${PIDS[$i]}" 2>/dev/null && ((SECONDS < deadline)); do sleep 0.2; done
      if kill -0 "${PIDS[$i]}" 2>/dev/null; then
        log "${NAMES[$i]} did not stop in time: killing pid ${PIDS[$i]}"
        kill -KILL "${PIDS[$i]}" 2>/dev/null || true
      fi
      wait "${PIDS[$i]}" 2>/dev/null || true
    done
  fi
  log "stopped. runtime dir and logs: $RUN"
}

wait_for_file() {
  local path="$1" what="$2" deadline=$((SECONDS + 60))
  while [[ ! -S "$path" ]]; do
    ((SECONDS < deadline)) || die "$what did not create $path (see $RUN/logs)"
    sleep 0.2
  done
}

trap stop_all EXIT
trap 'stop_all; exit 130' INT TERM

start orion-node \
  ORION_NODE_ID="$NODE_ID" \
  ORION_NODE_STATE_DIR="$RUN/orion-state" \
  ORION_NODE_IPC_SOCKET="$ORION_SOCKET" \
  ORION_NODE_IPC_STREAM_SOCKET="$ORION_STREAM_SOCKET" \
  ORION_NODE_HTTP_ADDR=off \
  ORION_NODE_LOCAL_AUTH=same-user-or-group-or-root \
  ORION_NODE_HOST_FACTS_REFRESH_MS=2000 \
  RUST_LOG="$SIM_RUST_LOG" \
  "$ORION_NODE_BIN"
wait_for_file "$ORION_SOCKET" orion-node

if [[ "$SIM_LEMNOS" != 0 ]]; then
  start lemnosd \
    RUST_LOG="$SIM_RUST_LOG" \
    "$LEMNOSD_BIN_DIR/lemnosd" --sim --board "$SIM_LEMNOS_BOARD" --sim-script "$SIM_LEMNOS_SCRIPT" \
    --sim-root "$RUN/lemnos/sim" --socket "$LEMNOSD_SOCKET" --update-status off
  wait_for_file "$LEMNOSD_SOCKET" lemnosd
  # lemnosd's devices as Orion resources (resource/lemnos.*), as on the board.
  start lemnos-orion \
    LEMNOSD_SOCKET="$LEMNOSD_SOCKET" \
    ORION_NODE_ID="$NODE_ID" \
    ORION_NODE_IPC_SOCKET="$ORION_SOCKET" \
    ORION_NODE_IPC_STREAM_SOCKET="$ORION_STREAM_SOCKET" \
    RUST_LOG="$SIM_RUST_LOG" \
    "$LEMNOSD_BIN_DIR/lemnos-orion"
fi

start helios-peripherals \
  HELIOS_NODE_ID="$NODE_ID" \
  HELIOS_IPC_DIR="$RUN/ipc" \
  HELIOS_PERIPHERAL_STREAM_DIR="$RUN/ipc/streams" \
  HELIOS_PERIPHERALS_LEMNOSD_SOCKET="$LEMNOSD_SOCKET" \
  HELIOS_PERIPHERALS_LEMNOSD_WAIT_MS=500 \
  ORION_NODE_IPC_SOCKET="$ORION_SOCKET" \
  ORION_NODE_IPC_STREAM_SOCKET="$ORION_STREAM_SOCKET" \
  STYX_SIM_CAMERAS="$STYX_SIM_CAMERAS" \
  RUST_LOG="$SIM_RUST_LOG" \
  "$SIM_BIN_DIR/helios-peripherals"

start helios-engine \
  HELIOS_NODE_ID="$NODE_ID" \
  ENGINE_SOCKET="$ENGINE_SOCKET" \
  ENGINE_JOURNAL_PATH="$RUN/logs/engine.journal.log" \
  HELIOS_ENGINE_LIVE_SOCKET="$ENGINE_LIVE_SOCKET" \
  HELIOS_ENGINE_CATALOG_PATH="$RUN/state/catalog.json" \
  HELIOS_NT4_SETTINGS="$RUN/api/nt4.json" \
  HELIOS_NODE_REGISTRY_SNAPSHOT_PATH="$RUN/state/node-registry.snapshot.json" \
  HELIOS_ENGINE_CACHE_NODE_REGISTRY=0 \
  HELIOS_DAEDALUS_PLUGIN_DIRS="$RUN/plugins" \
  HELIOS_DAEDALUS_DEMAND_DRIVEN=1 \
  HELIOS_ENGINE_METRICS_LEVEL=timing \
  HELIOS_STYX_CAPTURE_QUEUE_DEPTH=1 \
  HELIOS_STREAM_PREVIEW_ENCODE_INTERVAL_MS=4 \
  ORION_NODE_IPC_SOCKET="$ORION_SOCKET" \
  ORION_NODE_IPC_STREAM_SOCKET="$ORION_STREAM_SOCKET" \
  RUST_LOG="$SIM_RUST_LOG" \
  "$SIM_BIN_DIR/helios-engine"
# The engine advertises ENGINE_SOCKET but does not bind it (the board's helios-engine.socket does).
wait_for_file "$ENGINE_LIVE_SOCKET" helios-engine

start helios-api \
  HELIOS_API_BIND="$SIM_BIND" \
  HELIOS_API_PLAYBACK_STREAM_DIR="$RUN/streams" \
  HELIOS_NODE_ID="$NODE_ID" \
  HELIOS_ORION_IPC_SOCKET="$ORION_SOCKET" \
  HELIOS_ORION_IPC_STREAM_SOCKET="$ORION_STREAM_SOCKET" \
  HELIOS_API_STATE_DIR="$RUN/api/state" \
  HELIOS_API_UPLOAD_DIR="$RUN/api/uploads" \
  HELIOS_API_AUTH_FILE="$RUN/api/auth.json" \
  HELIOS_API_UI_DIR="$SIM_UI_DIR" \
  HELIOS_API_STARTUP_FILE="$RUN/startup.toml" \
  HELIOS_BOARD_IDENTITY_PATH="$RUN/board/identity.json" \
  HELIOS_LEMNOSD_SOCKET="$LEMNOSD_SOCKET" \
  HELIOS_ENGINE_LIVE_SOCKET="$ENGINE_LIVE_SOCKET" \
  HELIOS_BOARD_CAMERA_FILE="${SIM_BOARD_CAMERA_FILE:-$RUN/board/no-camera.toml}" \
  RUST_LOG="$SIM_RUST_LOG" \
  "$SIM_BIN_DIR/helios-api"

# --- Ready -------------------------------------------------------------------------------------

URL="http://$SIM_BIND"
deadline=$((SECONDS + 60))
until curl -fsS "$URL/v1/health" >/dev/null 2>&1; do
  ((SECONDS < deadline)) || die "helios-api did not answer on $URL (see $RUN/logs/helios-api.log)"
  sleep 0.3
done

printf '\n'
printf '  HeliOS sim is up.\n'
printf '  UI and API:  %s/\n' "$URL"
printf '  Cameras:     %s\n' "$STYX_SIM_CAMERAS"
if [[ "$SIM_LEMNOS" != 0 ]]; then printf '  Board:       lemnosd --sim %s (%s)\n' "$SIM_LEMNOS_BOARD" "$LEMNOSD_SOCKET"; fi
printf '  Smoke test:  SIM_TRUTH=%s tools/sim/smoke.sh (SIM_URL=%s)\n' "$RUN/truth.jsonl" "$URL"
printf '  Logs:        %s/logs\n' "$RUN"
printf '  Ctrl-C to stop.\n\n'

# Stop as soon as any service exits: a half-running stack is not a sim.
while true; do
  for i in "${!PIDS[@]}"; do
    if ! kill -0 "${PIDS[$i]}" 2>/dev/null; then
      wait "${PIDS[$i]}" 2>/dev/null || true
      log "${NAMES[$i]} exited (see $RUN/logs/${NAMES[$i]}.log)"
      exit 1
    fi
  done
  sleep 1
done
