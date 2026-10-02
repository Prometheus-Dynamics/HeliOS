#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
BUILDS_DIR="gaia/configs/builds"

usage() {
  cat <<EOF
Build HeliOS OS images with Gaia.

Usage:
  tools/build-os.sh                          Open the Gaia TUI with all HeliOS builds
  tools/build-os.sh <target> [profile] [gaia run args...]
                                             Run one build non-interactively

Targets:  $(cd "$ROOT_DIR/$BUILDS_DIR" && ls *.toml | sed 's/\.toml$//' | tr '\n' ' ')(HeliOS supports the Raze only)
Profiles: base-os | full (default: full)
Output:   gaia/output/helios-<profile>-<target>/images/ (e.g. helios-full-raze)

Raze device support comes from the Atlas device package (devices/raze), fetched
at the rev pinned in $BUILDS_DIR/raze.toml. To use a local Atlas checkout:
  tools/build-os.sh raze full --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager

Env:
  FORCE_FRONTEND_BUILD=1
                    Rebuild the frontend bundle even if its inputs are unchanged
EOF
}

if ! command -v gaia >/dev/null 2>&1; then
  echo "error: gaia CLI not found in PATH" >&2
  echo "Install it with:" >&2
  echo "  cargo install --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --branch dev gaia" >&2
  exit 1
fi

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
    if [[ "$profile" == "full" ]]; then
      ensure_frontend_bundle
    fi
    exec gaia run "$build_file" --set "input.profile=$profile" "$@"
    ;;
esac
