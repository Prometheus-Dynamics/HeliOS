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
  FORCE_UI_BUILD=1  Rebuild the UI (ui/build) even if its inputs are unchanged
EOF
}

if ! command -v gaia >/dev/null 2>&1; then
  echo "error: gaia CLI not found in PATH" >&2
  echo "Install it with:" >&2
  echo "  cargo install --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --branch main gaia" >&2
  exit 1
fi

# The full profile stages ui/build (served by helios-api) as-is; rebuild it first
# when it is missing or older than any of its inputs.
ensure_ui_build() {
  command -v bun >/dev/null 2>&1 || { echo "error: bun is required to build the UI (ui/)" >&2; exit 1; }
  local stamp=ui/build/index.html
  if [[ "${FORCE_UI_BUILD:-0}" != 1 && -f "$stamp" ]] &&
    [[ -z "$(find ui/src ui/static ui/package.json ui/bun.lock ui/vite.config.ts ui/tsconfig.json -newer "$stamp" -print -quit 2>/dev/null)" ]]; then
    return
  fi
  (cd ui && bun install --frozen-lockfile && bun run build)
}

# What the image ships of the UI: ui/build without mock/ (the offline demo's replay frames,
# ~12 MB of JPEGs a device never serves). Staged from gaia/build/ui (gaia/configs/payloads/ui.toml).
stage_ui_build() {
  rm -rf gaia/build/ui
  mkdir -p gaia/build/ui
  (cd ui/build && tar --exclude=./mock -cf - .) | (cd gaia/build/ui && tar -xf -)
}

cd "$ROOT_DIR"

case "${1:-}" in
  -h|--help)
    usage
    ;;
  "")
    ensure_ui_build
    stage_ui_build
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
      ensure_ui_build
      stage_ui_build
    fi
    # The slot id carries this checkout's commit (input defaults can't use project tokens; the
    # version takes it from Gaia's ${project.commit_short}).
    commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)
    args=()
    # Image builds (profile and slot inputs); a package build such as systemcore has neither.
    if grep -q '^\[inputs.profile\]' "$build_file"; then
      # Slot ids sort by slot.json's version and allow no "+": the version without its build
      # metadata (`2026.2.0-dev.<count>`), the commit in its own field.
      slot_version=$(grep -m1 '^version = ' "$build_file" | sed -E 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)-dev.*/\1/')
      args+=(--set "input.profile=$profile" --set "input.slot_version=${slot_version}-dev.$(git rev-list --count HEAD)"
        --set "input.slot_commit=${commit:0:8}" --set "input.slot_source=$commit")
    fi
    exec gaia run "$build_file" "${args[@]}" "$@"
    ;;
esac
