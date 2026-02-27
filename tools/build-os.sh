#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
GAIA_ROOT="${GAIA_ROOT:-$ROOT_DIR/gaia}"

if ! command -v gaia >/dev/null 2>&1; then
  echo "error: gaia CLI not found in PATH"
  echo "Install it with:"
  echo "  cargo install --locked --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --package gaia-image-builder --bin gaia --force"
  exit 1
fi

if [[ ! -d "$GAIA_ROOT" ]]; then
  echo "error: Gaia buildchain directory not found at: $GAIA_ROOT"
  echo "Set GAIA_ROOT or ensure HeliOS/gaia exists."
  exit 1
fi

if [[ $# -gt 0 && "${1:-}" != -* ]]; then
  case "${1:-}" in
    cm5|HeliOS-cm5|helios-cm5)
      shift
      ;;
    *)
      echo "error: unsupported target '${1:-}' (supported: cm5)"
      exit 1
      ;;
  esac
fi

cd "$GAIA_ROOT"
exec gaia tui --builds-dir configs/builds "$@"
