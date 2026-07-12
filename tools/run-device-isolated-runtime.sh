#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SSH_TARGET="${SSH_TARGET:-root@172.31.250.1}"
SSH_PASS="${SSH_PASS:-root}"
BINS_DIR="${BINS_DIR:-$ROOT_DIR/output/cm5/binaries}"
REMOTE_ROOT="${REMOTE_ROOT:-/tmp/helios-isolated}"
API_PORT="${API_PORT:-5802}"
NODE_ID="${NODE_ID:-cm5exp}"
NODE_DISPLAY_NAME="${NODE_DISPLAY_NAME:-CM5exp isolated}"
CLUSTER_ID="${CLUSTER_ID:-helios-isolated}"
PROFILE_LABEL="${PROFILE_LABEL:-isolated}"
NODE_TICK_MS="${NODE_TICK_MS:-250}"
NODE_API_TIMEOUT_MS="${NODE_API_TIMEOUT_MS:-250}"
SENSOR_CONFIG_PATHS="${SENSOR_CONFIG_PATHS:-/etc/helios/sensors.toml}"

usage() {
  cat <<EOF
Stage current Helios binaries onto the CM5 and run an isolated API/engine/peripherals stack under /tmp.

Usage:
  ./tools/run-device-isolated-runtime.sh <start|stop|status> [options]

Options:
  --ssh <user@host>      SSH target (default: $SSH_TARGET)
  --pass <password>      SSH password (default: $SSH_PASS)
  --bins-dir <dir>       Local directory containing helios-node/engine/peripherals
  --remote-root <dir>    Remote runtime root (default: $REMOTE_ROOT)
  --api-port <port>      Remote API bind port (default: $API_PORT)
  --node-id <id>         Node id for isolated runtime (default: $NODE_ID)
  --display-name <name>  Node display name
  --cluster-id <id>      Cluster id
  --profile-label <id>   Label written into process names/log messages
  --node-tick-ms <ms>    API node tick interval
  --node-api-timeout-ms <ms> Node API timeout
  --sensor-config-paths <csv> Sensor config paths passed to peripherals

Examples:
  ./tools/run-device-isolated-runtime.sh start --bins-dir output/cm5/binaries
  ./tools/run-device-isolated-runtime.sh status
  ./tools/run-device-isolated-runtime.sh stop
EOF
}

die() { echo "error: $*" >&2; exit 2; }

ssh_cmd() {
  sshpass -p "$SSH_PASS" ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "$SSH_TARGET" "$@"
}

upload_bins() {
  local remote_dir="$1"
  tar -C "$BINS_DIR" -cf - helios-node helios-engine helios-peripherals | sshpass -p "$SSH_PASS" ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "$SSH_TARGET" "tar -C '$remote_dir' -xf -"
}

ACTION="${1:-}"
[[ -n "$ACTION" ]] || { usage; exit 2; }
shift || true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ssh) SSH_TARGET="${2:-}"; shift 2 ;;
    --pass) SSH_PASS="${2:-}"; shift 2 ;;
    --bins-dir) BINS_DIR="${2:-}"; shift 2 ;;
    --remote-root) REMOTE_ROOT="${2:-}"; shift 2 ;;
    --api-port) API_PORT="${2:-}"; shift 2 ;;
    --node-id) NODE_ID="${2:-}"; shift 2 ;;
    --display-name) NODE_DISPLAY_NAME="${2:-}"; shift 2 ;;
    --cluster-id) CLUSTER_ID="${2:-}"; shift 2 ;;
    --profile-label) PROFILE_LABEL="${2:-}"; shift 2 ;;
    --node-tick-ms) NODE_TICK_MS="${2:-}"; shift 2 ;;
    --node-api-timeout-ms) NODE_API_TIMEOUT_MS="${2:-}"; shift 2 ;;
    --sensor-config-paths) SENSOR_CONFIG_PATHS="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

case "$ACTION" in
  start|stop|status) ;;
  *) die "action must be one of: start, stop, status" ;;
esac

REMOTE_BIN_DIR="$REMOTE_ROOT/bin"
REMOTE_IPC_DIR="$REMOTE_ROOT/ipc"
REMOTE_STREAM_DIR="$REMOTE_ROOT/streams"
REMOTE_LOG_DIR="$REMOTE_ROOT/logs"
REMOTE_RUN_DIR="$REMOTE_ROOT/run"
SSH_HOST="${SSH_TARGET##*@}"

remote_start_script() {
  cat <<EOF
set -euo pipefail
kill_matching() {
  pattern="\$1"
  pids=\$(ps -ef | grep "\$pattern" | grep -v grep | awk '{print \$1}' || true)
  for pid in \$pids; do
    kill -9 "\$pid" 2>/dev/null || true
  done
}
mkdir -p '$REMOTE_BIN_DIR' '$REMOTE_IPC_DIR' '$REMOTE_STREAM_DIR' '$REMOTE_LOG_DIR' '$REMOTE_RUN_DIR'
kill_matching '$REMOTE_ROOT/bin/helios-node'
kill_matching '$REMOTE_ROOT/bin/helios-engine'
kill_matching '$REMOTE_ROOT/bin/helios-peripherals'
rm -f '$REMOTE_RUN_DIR'/*.pid
chmod +x '$REMOTE_BIN_DIR'/helios-node '$REMOTE_BIN_DIR'/helios-engine '$REMOTE_BIN_DIR'/helios-peripherals
nohup env \
  HELIOS_NODE_ID='$NODE_ID' \
  HELIOS_IPC_DIR='$REMOTE_IPC_DIR' \
  HELIOS_PERIPHERAL_STREAM_DIR='$REMOTE_STREAM_DIR' \
  HELIOS_SENSOR_CONFIG_PATHS='$SENSOR_CONFIG_PATHS' \
  RUST_LOG=info \
  '$REMOTE_BIN_DIR'/helios-peripherals \
  >'$REMOTE_LOG_DIR'/helios-peripherals-$PROFILE_LABEL.log 2>&1 &
echo \$! > '$REMOTE_RUN_DIR'/helios-peripherals.pid
nohup env \
  HELIOS_NODE_ID='$NODE_ID' \
  HELIOS_IPC_DIR='$REMOTE_IPC_DIR' \
  HELIOS_ENGINE_STREAM_DIR='$REMOTE_STREAM_DIR' \
  HELIOS_ENGINE_GPU=cpu \
  HELIOS_ENGINE_METRICS_LEVEL=basic \
  RUST_LOG=info \
  '$REMOTE_BIN_DIR'/helios-engine \
  >'$REMOTE_LOG_DIR'/helios-engine-$PROFILE_LABEL.log 2>&1 &
echo \$! > '$REMOTE_RUN_DIR'/helios-engine.pid
nohup env \
  HELIOS_NODE_ID='$NODE_ID' \
  HELIOS_NODE_DISPLAY_NAME='$NODE_DISPLAY_NAME' \
  HELIOS_CLUSTER_ID='$CLUSTER_ID' \
  HELIOS_IPC_DIR='$REMOTE_IPC_DIR' \
  HELIOS_API_BIND_ADDR='0.0.0.0:$API_PORT' \
  HELIOS_NODE_TICK_MS='$NODE_TICK_MS' \
  HELIOS_NODE_API_TIMEOUT_MS='$NODE_API_TIMEOUT_MS' \
  RUST_LOG=info \
  '$REMOTE_BIN_DIR'/helios-node \
  >'$REMOTE_LOG_DIR'/helios-node-$PROFILE_LABEL.log 2>&1 &
echo \$! > '$REMOTE_RUN_DIR'/helios-node.pid
sleep 2
echo "api_base=http://$SSH_HOST:$API_PORT"
echo "logs=$REMOTE_LOG_DIR"
echo "ipc_dir=$REMOTE_IPC_DIR"
echo "stream_dir=$REMOTE_STREAM_DIR"
EOF
}

remote_stop_script() {
  cat <<EOF
set -euo pipefail
kill_matching() {
  pattern="\$1"
  pids=\$(ps -ef | grep "\$pattern" | grep -v grep | awk '{print \$1}' || true)
  for pid in \$pids; do
    kill -9 "\$pid" 2>/dev/null || true
  done
}
for service in helios-node helios-engine helios-peripherals; do
  pid_file='$REMOTE_RUN_DIR'/"\$service".pid
  if [ -f "\$pid_file" ]; then
    pid=\$(cat "\$pid_file" || true)
    if [ -n "\${pid:-}" ]; then
      kill "\$pid" 2>/dev/null || true
    fi
    rm -f "\$pid_file"
  fi
done
kill_matching '$REMOTE_ROOT/bin/helios-node'
kill_matching '$REMOTE_ROOT/bin/helios-engine'
kill_matching '$REMOTE_ROOT/bin/helios-peripherals'
EOF
}

remote_status_script() {
  cat <<EOF
set -euo pipefail
for service in helios-node helios-engine helios-peripherals; do
  pid_file='$REMOTE_RUN_DIR'/"\$service".pid
  printf '%s: ' "\$service"
  if [ -f "\$pid_file" ]; then
    pid=\$(cat "\$pid_file" || true)
    if [ -n "\${pid:-}" ] && kill -0 "\$pid" 2>/dev/null; then
      printf 'running pid=%s\\n' "\$pid"
    else
      printf 'stale-pid-file\\n'
    fi
  else
    printf 'stopped\\n'
  fi
done
printf 'api_base=http://%s:%s\\n' '$SSH_HOST' '$API_PORT'
printf 'logs=%s\\n' '$REMOTE_LOG_DIR'
EOF
}

if [[ "$ACTION" == "start" ]]; then
  [[ -x "$BINS_DIR/helios-node" ]] || die "missing executable: $BINS_DIR/helios-node"
  [[ -x "$BINS_DIR/helios-engine" ]] || die "missing executable: $BINS_DIR/helios-engine"
  [[ -x "$BINS_DIR/helios-peripherals" ]] || die "missing executable: $BINS_DIR/helios-peripherals"
  ssh_cmd "mkdir -p '$REMOTE_BIN_DIR' '$REMOTE_IPC_DIR' '$REMOTE_STREAM_DIR' '$REMOTE_LOG_DIR' '$REMOTE_RUN_DIR'"
  upload_bins "$REMOTE_BIN_DIR"
  ssh_cmd "$(remote_start_script)"
elif [[ "$ACTION" == "stop" ]]; then
  ssh_cmd "$(remote_stop_script)"
else
  ssh_cmd "$(remote_status_script)"
fi
