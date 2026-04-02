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
BACKEND_DIR="$REPO_ROOT/backend"
FRONTEND_DIR="$REPO_ROOT/frontend"
CLIENT_TEMPLATE_DIR="$REPO_ROOT/tools/api-client"
TS_BINDINGS_ROOT="$FRONTEND_DIR/src/lib/ts-bindings"
HTTP_BINDINGS_DIR="$TS_BINDINGS_ROOT/http"
WS_BINDINGS_DIR="$TS_BINDINGS_ROOT/ws"
HTTP_CLIENT_DIR="$HTTP_BINDINGS_DIR/client"

require_cmd cargo
require_cmd bun

SPEC_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'helios-spec')"
trap 'rm -rf "$SPEC_DIR"' EXIT
HTTP_SPEC="$SPEC_DIR/http.json"
WS_SPEC="$SPEC_DIR/ws.json"

log spec "Generating OpenAPI + AsyncAPI descriptions via helios-api CLI"
(cd "$BACKEND_DIR" && cargo run -p helios-api -- apispec --http "$HTTP_SPEC" --ws "$WS_SPEC")

log deps "Installing JS dependencies with bun"
(cd "$CLIENT_TEMPLATE_DIR" && bun install)

log clean "Removing previous bindings output"
rm -rf "$TS_BINDINGS_ROOT"

mkdir -p "$HTTP_BINDINGS_DIR" "$WS_BINDINGS_DIR"

log types "Generating shared TypeScript types"
(cd "$CLIENT_TEMPLATE_DIR" && bun x openapi-typescript "$HTTP_SPEC" -o "$TS_BINDINGS_ROOT/types.ts")

log http-client "Generating HTTP client bindings"
rm -rf "$HTTP_CLIENT_DIR"
(cd "$CLIENT_TEMPLATE_DIR" && bun x openapi-typescript-codegen \
  --input "$HTTP_SPEC" \
  --output "$HTTP_CLIENT_DIR" \
  --useOptions \
  --useUnionTypes)

log sync "Copying emitted specs into frontend bindings"
cp "$HTTP_SPEC" "$HTTP_BINDINGS_DIR/openapi.json"
cp "$WS_SPEC" "$WS_BINDINGS_DIR/asyncapi.json"

log codec-families "Generating shared codec family bindings"
(cd "$REPO_ROOT" && node "$SCRIPT_DIR/generate-codec-families.mjs")

log runtime-contracts "Generating shared runtime contract bindings"
(cd "$REPO_ROOT" && node "$SCRIPT_DIR/generate-runtime-contracts.mjs")

log done "Artifacts written to $(realpath --relative-to="$REPO_ROOT" "$TS_BINDINGS_ROOT")"
