#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GAIA_BIN_DEFAULT="$ROOT_DIR/../../Gaia-Image-Builder/target/debug/gaia"
GAIA_BIN="${GAIA_BIN:-$GAIA_BIN_DEFAULT}"
DOCKER_IMAGE="${GAIA_LAYER_DOCKER_IMAGE:-gaia-buildroot-smoke:latest}"

run_build() {
  local build_file="$1"
  "$GAIA_BIN" run "$build_file" \
    --set execution.docker.enabled=true \
    --set execution.docker.image="$DOCKER_IMAGE"
}

seed_buildroot_output() {
  local from_build="$1"
  local to_build="$2"
  local from_dir="$ROOT_DIR/output/$from_build/images/buildroot-output"
  local to_dir="$ROOT_DIR/output/$to_build/images/buildroot-output"

  rm -rf "$to_dir"
  mkdir -p "$(dirname "$to_dir")"
  cp --reflink=auto -a "$from_dir" "$to_dir"
}

echo "==> proving base-os-cm5"
run_build "$ROOT_DIR/configs/builds/base-os-cm5.toml"

echo "==> seeding appliance-core-cm5 from helios-base-cm5"
seed_buildroot_output "helios-base-cm5" "helios-appliance-core-cm5"
run_build "$ROOT_DIR/configs/builds/appliance-core-cm5.toml"

echo "==> seeding backend-core-cm5 from helios-appliance-core-cm5"
seed_buildroot_output "helios-appliance-core-cm5" "helios-backend-core-cm5"
run_build "$ROOT_DIR/configs/builds/backend-core-cm5.toml"

echo "==> done"
