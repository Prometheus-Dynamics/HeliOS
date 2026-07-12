#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNNER="$ROOT_DIR/tools/run-device-template.sh"
API_BASE="${API_BASE:-http://172.31.250.1}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT_DIR/artifacts/device-runtime/cm5}"
RESOURCE_ID=""
POLL_SECONDS="${POLL_SECONDS:-3}"
STOP_AFTER_RUN="1"

usage() {
  cat <<EOF
Run a canonical graph-template matrix against a live Helios device and capture artifacts/timings.

Usage:
  ./tools/run-device-template-matrix.sh --consume <resource_id> [options]

Options:
  --api-base <url>        Helios API base URL (default: http://172.31.250.1)
  --artifact-dir <dir>    Output artifact directory
  --consume <resource_id> Consumed resource id used by graph templates
  --poll-seconds <n>      Seconds to wait before sampling runtime
  --no-stop               Leave workloads running after capture
  -h, --help              Show this help

Examples:
  ./tools/run-device-template-matrix.sh \
    --consume virtual_cm5exp_imu-bmi088-bus4
EOF
}

die() { echo "error: $*" >&2; exit 2; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base) API_BASE="${2:-}"; shift 2 ;;
    --artifact-dir) ARTIFACT_DIR="${2:-}"; shift 2 ;;
    --consume) RESOURCE_ID="${2:-}"; shift 2 ;;
    --poll-seconds) POLL_SECONDS="${2:-}"; shift 2 ;;
    --no-stop) STOP_AFTER_RUN="0"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

[[ -n "$RESOURCE_ID" ]] || die "--consume is required"

mkdir -p "$ARTIFACT_DIR"

run_template() {
  local template_id="$1"
  local workload_local="$2"
  local label="$3"
  local -a args=(
    --api-base "$API_BASE"
    --artifact-dir "$ARTIFACT_DIR"
    --template "$template_id"
    --workload-local "$workload_local"
    --artifact-local "${workload_local}-artifact"
    --consume "$RESOURCE_ID"
    --label "$label"
    --poll-seconds "$POLL_SECONDS"
  )
  if [[ "$STOP_AFTER_RUN" == "1" ]]; then
    args+=(--stop)
  fi
  "$RUNNER" "${args[@]}"
}

run_template stream_batch_count_graph graph-template-count graph-template-count
run_template imu_pose_3axis_graph graph-template-imu3 graph-template-imu3
run_template imu_pose_6axis_graph graph-template-imu6 graph-template-imu6
run_template imu_pose_9axis_graph graph-template-imu9 graph-template-imu9
run_template localization_centroid_graph graph-template-localization graph-template-localization

echo "template matrix complete under $ARTIFACT_DIR"

