#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '[api-codegen:%s] %s\n' "$1" "$2"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(realpath "$SCRIPT_DIR/../..")"

require_cmd cargo

log run "Generating bindings via xtask"
(cd "$REPO_ROOT" && cargo run --manifest-path backend/Cargo.toml -p xtask -- generate generated-contracts)
