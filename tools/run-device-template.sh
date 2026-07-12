#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_DIR_DEFAULT="$ROOT_DIR/artifacts/device-runtime/cm5"
API_BASE_DEFAULT="http://172.31.250.1"

API_BASE="${API_BASE:-$API_BASE_DEFAULT}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ARTIFACT_DIR_DEFAULT}"
TEMPLATE_ID=""
WORKLOAD_LOCAL=""
ARTIFACT_LOCAL=""
RESOURCE_ID=""
CONSUME_IDS=()
LABEL=""
POLL_SECONDS="${POLL_SECONDS:-3}"
STOP_AFTER_RUN="0"
MEASURE_TIMINGS="1"

usage() {
  cat <<EOF
Run a workload template against a live Helios device and capture canonical API artifacts.

Usage:
  ./tools/run-device-template.sh --template <template_id> --workload-local <name> [options]

Options:
  --api-base <url>          Helios API base URL (default: $API_BASE_DEFAULT)
  --artifact-dir <dir>      Output artifact directory (default: $ARTIFACT_DIR_DEFAULT)
  --template <id>           Workload template id to run
  --workload-local <name>   Local workload name used for instantiated workload id
  --artifact-local <name>   Local artifact name override
  --consume <resource_id>   Consumed resource id; repeat for multi-stream workloads
  --label <name>            Artifact label suffix (default: template id)
  --poll-seconds <n>        Seconds to wait before sampling runtime (default: 3)
  --stop                    Stop the workload after sampling
  --no-timings              Skip curl timing capture
  -h, --help                Show this help

Examples:
  ./tools/run-device-template.sh \\
    --template stream_batch_count_graph \\
    --workload-local graph-refresh \\
    --consume virtual_cm5exp_imu-bmi088-bus4

  ./tools/run-device-template.sh \\
    --template imu_pose_9axis_graph \\
    --workload-local imu-pose-9 \\
    --consume virtual_cm5exp_imu-bmi088-bus4 \\
    --label imu9
EOF
}

die() { echo "error: $*" >&2; exit 2; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base) API_BASE="${2:-}"; shift 2 ;;
    --artifact-dir) ARTIFACT_DIR="${2:-}"; shift 2 ;;
    --template) TEMPLATE_ID="${2:-}"; shift 2 ;;
    --workload-local) WORKLOAD_LOCAL="${2:-}"; shift 2 ;;
    --artifact-local) ARTIFACT_LOCAL="${2:-}"; shift 2 ;;
    --consume) RESOURCE_ID="${2:-}"; CONSUME_IDS+=("${2:-}"); shift 2 ;;
    --label) LABEL="${2:-}"; shift 2 ;;
    --poll-seconds) POLL_SECONDS="${2:-}"; shift 2 ;;
    --stop) STOP_AFTER_RUN="1"; shift ;;
    --no-timings) MEASURE_TIMINGS="0"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

[[ -n "${TEMPLATE_ID}" ]] || die "--template is required"
[[ -n "${WORKLOAD_LOCAL}" ]] || die "--workload-local is required"

if [[ -z "${LABEL}" ]]; then
  LABEL="${TEMPLATE_ID}"
fi

mkdir -p "$ARTIFACT_DIR"

WORKLOAD_KIND="graph"
case "$TEMPLATE_ID" in
  synthetic_counter_ingress|resource_stream_ingress)
    WORKLOAD_KIND="ingress"
    ;;
esac

REQUEST_FILE="$ARTIFACT_DIR/${LABEL}-template-run-request.json"
RUN_FILE="$ARTIFACT_DIR/${LABEL}-template-run-response.json"
WORKLOAD_FILE="$ARTIFACT_DIR/${LABEL}-workload.json"
RUNTIME_FILE="$ARTIFACT_DIR/${LABEL}-workload-runtime.json"
SYSTEM_FILE="$ARTIFACT_DIR/${LABEL}-system-state.json"
STREAMS_FILE="$ARTIFACT_DIR/${LABEL}-streams.json"
STOP_FILE="$ARTIFACT_DIR/${LABEL}-stop-response.txt"
TIMINGS_FILE="$ARTIFACT_DIR/${LABEL}-timings.json"

TIME_METRIC_FMT='{"url":"%{url_effective}","http_code":%{http_code},"time_namelookup":%{time_namelookup},"time_connect":%{time_connect},"time_starttransfer":%{time_starttransfer},"time_total":%{time_total}}'

curl_json() {
  local method="$1"
  local url="$2"
  local output_file="$3"
  local metric_name="$4"
  shift 4
  local tmp_metrics
  tmp_metrics="$(mktemp)"
  curl -fsSL \
    -X "$method" \
    "$@" \
    -w "$TIME_METRIC_FMT" \
    "$url" \
    -o "$output_file" \
    > "$tmp_metrics"

  if [[ "$MEASURE_TIMINGS" == "1" ]]; then
    python3 - "$TIMINGS_FILE" "$metric_name" "$tmp_metrics" <<'PY'
import json, pathlib, sys
target_path = pathlib.Path(sys.argv[1])
metric_name = sys.argv[2]
metric_path = pathlib.Path(sys.argv[3])
metric = json.loads(metric_path.read_text(encoding="utf-8"))
if target_path.exists():
    doc = json.loads(target_path.read_text(encoding="utf-8"))
else:
    doc = {}
doc[metric_name] = metric
target_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  fi
  rm -f "$tmp_metrics"
}

python3 - "$REQUEST_FILE" "$WORKLOAD_LOCAL" "$ARTIFACT_LOCAL" "${CONSUME_IDS[@]}" <<'PY'
import json, sys
request_path, workload_local, artifact_local, *resource_ids = sys.argv[1:]
body = {"workload_local": workload_local, "exposes": []}
if artifact_local:
    body["artifact_local"] = artifact_local
resource_ids = [resource_id for resource_id in resource_ids if resource_id]
if resource_ids:
    body["consumes"] = resource_ids
with open(request_path, "w", encoding="utf-8") as fh:
    json.dump(body, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY

if [[ "$MEASURE_TIMINGS" == "1" ]]; then
  cat > "$TIMINGS_FILE" <<EOF
{
  "template_id": "${TEMPLATE_ID}",
  "label": "${LABEL}",
  "api_base": "${API_BASE}"
}
EOF
fi

curl_json POST "${API_BASE}/v1/templates/workloads/${TEMPLATE_ID}/run" "$RUN_FILE" "template_run" \
  -H 'content-type: application/json' \
  --data-binary "@${REQUEST_FILE}"

WORKLOAD_ID="$(python3 - "$RUN_FILE" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as fh:
    data = json.load(fh)
print(data["workload"]["id"])
PY
)"

sleep "$POLL_SECONDS"

curl_json GET "${API_BASE}/v1/workloads/${WORKLOAD_ID}" "$WORKLOAD_FILE" "workload_detail_cold"
curl_json GET "${API_BASE}/v1/workloads/${WORKLOAD_ID}" "$WORKLOAD_FILE" "workload_detail_warm"
curl_json GET "${API_BASE}/v1/workloads/${WORKLOAD_ID}/runtime" "$RUNTIME_FILE" "workload_runtime_cold"
curl_json GET "${API_BASE}/v1/workloads/${WORKLOAD_ID}/runtime" "$RUNTIME_FILE" "workload_runtime_warm"
curl_json GET "${API_BASE}/v1/system/state" "$SYSTEM_FILE" "system_state_cold"
curl_json GET "${API_BASE}/v1/system/state" "$SYSTEM_FILE" "system_state_warm"
curl_json GET "${API_BASE}/v1/streams" "$STREAMS_FILE" "streams_cold"
curl_json GET "${API_BASE}/v1/streams" "$STREAMS_FILE" "streams_warm"

if [[ "$STOP_AFTER_RUN" == "1" ]]; then
  curl_json POST "${API_BASE}/v1/workloads/${WORKLOAD_ID}/stop" "$STOP_FILE" "workload_stop"
fi

cat <<EOF
captured:
  request: $REQUEST_FILE
  run: $RUN_FILE
  workload: $WORKLOAD_FILE
  runtime: $RUNTIME_FILE
  system: $SYSTEM_FILE
  streams: $STREAMS_FILE
  timings: $TIMINGS_FILE
EOF
