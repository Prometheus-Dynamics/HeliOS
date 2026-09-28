#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
BUILDS_DIR="gaia/configs/builds"
CROSS_IMAGE="${CROSS_IMAGE:-helios-cross-rust194}"
CROSS_DOCKERFILE="${CROSS_DOCKERFILE:-gaia/docker/aarch64/Dockerfile.aarch64-rpi4}"
CROSS_CONTEXT="${CROSS_CONTEXT:-gaia}"

usage() {
  cat <<EOF
Build HeliOS OS images with Gaia.

Usage:
  tools/build-os.sh                          Open the Gaia TUI with all HeliOS builds
  tools/build-os.sh <target> [profile] [gaia run args...]
                                             Run one build non-interactively

Targets:  $(ls "$ROOT_DIR/$BUILDS_DIR" | sed 's/\.toml$//' | tr '\n' ' ')
Profiles: base-os | full (default: full)

Env:
  CROSS_IMAGE       Docker image used for Rust artifact builds (default: $CROSS_IMAGE)
  REBUILD_CROSS=1   Rebuild the cross image even if it already exists
  FORCE_FRONTEND_BUILD=1
                    Rebuild the frontend bundle even if its inputs are unchanged
EOF
}

if ! command -v gaia >/dev/null 2>&1; then
  echo "error: gaia CLI not found in PATH" >&2
  echo "Install it with:" >&2
  echo "  cargo install --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder gaia" >&2
  exit 1
fi

# Gaia builds the Rust artifacts inside $CROSS_IMAGE but cannot build that image itself.
ensure_cross_image() {
  command -v docker >/dev/null 2>&1 || { echo "error: docker is required for Rust artifact builds" >&2; exit 1; }
  if [[ "${REBUILD_CROSS:-0}" != "1" ]] && docker image inspect "$CROSS_IMAGE" >/dev/null 2>&1; then
    return
  fi
  echo "Building cross image '$CROSS_IMAGE' from $CROSS_DOCKERFILE..."
  DOCKER_BUILDKIT=1 docker build -f "$ROOT_DIR/$CROSS_DOCKERFILE" -t "$CROSS_IMAGE" "$ROOT_DIR/$CROSS_CONTEXT"
}

# The full profile stages frontend/build as-is; rebuild it first when its inputs changed.
ensure_frontend_bundle() {
  command -v bun >/dev/null 2>&1 || { echo "error: bun is required to build the frontend bundle" >&2; exit 1; }
  bun frontend/scripts/build-if-changed.mjs
}

cd "$ROOT_DIR"

case "${1:-}" in
  -h|--help)
    usage
    ;;
  "")
    ensure_cross_image
    ensure_frontend_bundle
    exec gaia tui --builds-dir "$BUILDS_DIR"
    ;;
  *)
    target="$1"
    shift
    build_file="$BUILDS_DIR/$target.toml"
    [[ -f "$build_file" ]] || { echo "error: unknown target '$target'" >&2; usage >&2; exit 1; }
    profile="full"
    if [[ $# -gt 0 && "$1" != -* ]]; then
      profile="$1"
      shift
    fi
    ensure_cross_image
    if [[ "$profile" == "full" ]]; then
      ensure_frontend_bundle
    fi
    exec gaia run "$build_file" --set "input.profile=$profile" "$@"
    ;;
esac
