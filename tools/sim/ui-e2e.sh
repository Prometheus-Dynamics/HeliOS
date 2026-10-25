#!/usr/bin/env bash
# The UI's end-to-end suite against the HeliOS sim: starts tools/sim/up.sh (the real services,
# the simulated camera and board, and helios-api serving the UI), waits for the API, builds the UI
# when it is not built, runs the `sim` project of ui/playwright.config.ts, and always stops the sim.
# Exits non-zero when the suite fails. See ui/e2e/README.md.
#
# Variables (all optional; up.sh documents the rest):
#   SIM_BIN_DIR        the HeliOS binaries (default: <repo>/backend/target/debug)
#   ORION_NODE_BIN     orion-node (default: <repo>/backend/target/sim/orion/bin/orion-node)
#   LEMNOSD_BIN_DIR    lemnosd and lemnos-orion (default: <repo>/backend/target/sim/lemnos/bin)
#   SIM_UI_DIR         the UI's static build (default: <repo>/ui/build)
#   SIM_BIND           helios-api's address (default: 127.0.0.1:5811; 5812 is another helper's)
#   SIM_RUN_DIR        runtime dir (default: a fresh dir under $XDG_RUNTIME_DIR or $TMPDIR, kept
#                      for its logs; keep it short: the services' Unix sockets must fit SUN_LEN)
#   SIM_PLAYWRIGHT_ARGS  extra arguments for `playwright test`, e.g. "-g tuning"
#   SIM_SKIP_BUILD     1 (default): run the prebuilt binaries, never cargo or bun for the backend
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
UI="$ROOT/ui"
export SIM_SKIP_BUILD="${SIM_SKIP_BUILD:-1}"
export SIM_BIND="${SIM_BIND:-127.0.0.1:5811}"
export SIM_UI_DIR="${SIM_UI_DIR:-$UI/build}"
SIM_BIN_DIR="${SIM_BIN_DIR:-$ROOT/backend/target/debug}"
ORION_NODE_BIN="${ORION_NODE_BIN:-$ROOT/backend/target/sim/orion/bin/orion-node}"
LEMNOSD_BIN_DIR="${LEMNOSD_BIN_DIR:-$ROOT/backend/target/sim/lemnos/bin}"
RUN="${SIM_RUN_DIR:-$(mktemp -d "${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/helios-e2e.XXXXXX")}"
mkdir -p "$RUN"

log() { printf '[ui-e2e] %s\n' "$*"; }
die() { printf '[ui-e2e] error: %s\n' "$*" >&2; exit 1; }

command -v bun >/dev/null || die "bun not found"
for bin in "$SIM_BIN_DIR/helios-peripherals" "$SIM_BIN_DIR/helios-engine" "$SIM_BIN_DIR/helios-api" "$SIM_BIN_DIR/helios-sim-scene" "$SIM_BIN_DIR/libhelios_eidos_plugin.so" "$ORION_NODE_BIN" "$LEMNOSD_BIN_DIR/lemnosd" "$LEMNOSD_BIN_DIR/lemnos-orion"; do
  [[ -e "$bin" ]] || die "missing $bin (set SIM_BIN_DIR, ORION_NODE_BIN or LEMNOSD_BIN_DIR)"
done

# The UI the sim serves, and the test suite's dependencies.
if [[ ! -f "$SIM_UI_DIR/index.html" ]]; then
  log "building the UI"
  ( cd "$UI" && { [[ -d node_modules ]] || bun install --frozen-lockfile; } && bun run build )
fi
[[ -f "$SIM_UI_DIR/index.html" ]] || die "no UI build at $SIM_UI_DIR"
( cd "$UI" && { [[ -d node_modules/@playwright/test ]] || bun install --frozen-lockfile; } )

# The browser: Chromium for Playwright (a no-op when installed).
( cd "$UI" && bunx playwright install chromium >/dev/null )

# --- The sim -----------------------------------------------------------------------------------

SIM_PID=""
stop_sim() {
  trap - EXIT INT TERM
  if [[ -n "$SIM_PID" ]] && kill -0 "$SIM_PID" 2>/dev/null; then
    log "stopping the sim (pid $SIM_PID)"
    # up.sh stops the services it started, by PID, when it gets TERM.
    kill -TERM "$SIM_PID" 2>/dev/null || true
    local deadline=$((SECONDS + 30))
    while kill -0 "$SIM_PID" 2>/dev/null && ((SECONDS < deadline)); do sleep 0.5; done
    if kill -0 "$SIM_PID" 2>/dev/null; then
      log "the sim did not stop in time: killing pid $SIM_PID"
      kill -KILL "$SIM_PID" 2>/dev/null || true
    fi
    wait "$SIM_PID" 2>/dev/null || true
  fi
  SIM_PID=""
}
trap stop_sim EXIT INT TERM

log "starting the sim at http://$SIM_BIND (runtime dir $RUN)"
SIM_RUN_DIR="$RUN" taskset -c 0-9,12-21 "$ROOT/tools/sim/up.sh" >"$RUN/ui-e2e-sim.log" 2>&1 &
SIM_PID=$!

deadline=$((SECONDS + 180))
until curl -fsS "http://$SIM_BIND/v1/health" >/dev/null 2>&1; do
  kill -0 "$SIM_PID" 2>/dev/null || die "the sim exited before its API answered (see $RUN/ui-e2e-sim.log)"
  ((SECONDS < deadline)) || die "helios-api did not answer at $SIM_BIND (see $RUN/logs)"
  sleep 1
done
log "the sim's API answers; running the sim project"

# --- The suite ---------------------------------------------------------------------------------

rc=0
( cd "$UI" && HELIOS_E2E_URL="http://$SIM_BIND" bunx playwright test --project=sim ${SIM_PLAYWRIGHT_ARGS:-} ) || rc=$?
stop_sim
if [[ "$rc" -eq 0 ]]; then
  log "passed"
else
  log "failed (exit $rc): the report is in $UI/playwright-report, the sim's logs in $RUN/logs"
fi
exit "$rc"
