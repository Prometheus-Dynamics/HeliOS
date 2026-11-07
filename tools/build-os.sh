#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
BUILDS_DIR="gaia/configs/builds"
# Dev images are signed with Atlas's dev key when it is installed (Atlas keys/README.md); Gaia
# itself has no home-path fallback, so the path is set here.
if [[ -z "${ATLAS_DEV_SIGNING_KEY:-}" && -f "$HOME/.config/atlas/dev-signing/minisign.key" ]]; then
  export ATLAS_DEV_SIGNING_KEY="$HOME/.config/atlas/dev-signing/minisign.key"
fi
if [[ -n "${ATLAS_DEV_SIGNING_KEY:-}" ]]; then
  echo "build-os: signing images with the key at $ATLAS_DEV_SIGNING_KEY (Atlas dev/example key unless you set another)" >&2
fi

usage() {
  cat <<EOF
Build HeliOS OS images with Gaia.

Usage:
  tools/build-os.sh                          Open the Gaia TUI with all HeliOS builds
  tools/build-os.sh <target> [profile] [gaia run args...]
                                             Run one build non-interactively

Targets:  $(cd "$ROOT_DIR/$BUILDS_DIR" && ls *.toml | sed 's/\.toml$//' | tr '\n' ' ')(HeliOS supports the Raze only)
Profiles: base-os | full (default: full)
Machine:  --machine board (default) | qemu: the simulated board (-M virt) for Gaia's QEMU runner.
          Its image is never a board image: the slot id carries -styx-qemu. Full profile only.
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

# --machine qemu: the simulated board's scene and camera spec, built for the host here (Gaia has no
# build-time command step). gaia/build/qemu-sim/{scene.glb,sim-cameras} are staged by the qemu layer.
# CARGO_TARGET_DIR (default backend/target) takes a target dir on fast storage. HELIOS_QEMU_PREPARE_ONLY=1
# stops after writing them (validate and plan need the files, not a build).
prepare_qemu_sim() {
  local out=gaia/build/qemu-sim target_dir="${CARGO_TARGET_DIR:-backend/target}" lens
  command -v cargo >/dev/null || { echo "error: cargo not found (helios-sim-scene is built for the host)" >&2; exit 1; }
  CARGO_TARGET_DIR="$target_dir" cargo build --manifest-path backend/Cargo.toml -p helios-sim-scene
  mkdir -p "$out"
  # The camera and lens are tools/sim's default scenario (up.sh), at its calibration; the spec's
  # lens keys come from helios-sim-scene's stdout. lockstep: emulation renders slowly.
  lens="$("$target_dir/debug/helios-sim-scene" --layout tools/sim/scenarios/default/layout.json \
    --camera tools/sim/scenarios/default/camera.json --calibration tools/sim/ov9782-calibration.json \
    --out "$out/scene.glb")" || { echo "error: helios-sim-scene failed" >&2; exit 1; }
  {
    echo "# Simulated cameras of the qemu image (Styx reads /etc/styx/sim-cameras): written by tools/build-os.sh --machine qemu."
    echo "ov9782,scene=gltf,path=/usr/share/helios/sim/scene.glb,$lens,clock=lockstep,truth=/run/helios-sim/truth.jsonl"
  } > "$out/sim-cameras"
  echo "build-os: qemu scene and camera spec in $out" >&2
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
    args=()
    # --machine NAME (or --set input.machine=NAME) selects the machine; anything else goes to gaia as before.
    machine=board
    rest=()
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --machine)
          [[ $# -ge 2 ]] || { echo "error: --machine needs board or qemu" >&2; exit 1; }
          rest+=(--set "input.machine=$2"); shift 2 ;;
        --machine=*) rest+=(--set "input.machine=${1#--machine=}"); shift ;;
        *) rest+=("$1"); shift ;;
      esac
    done
    for a in "${rest[@]}"; do
      case "$a" in input.machine=*) machine="${a#input.machine=}" ;; esac
    done
    case "$machine" in
      board|qemu) ;;
      *) echo "error: machine must be board or qemu, not $machine" >&2; exit 1 ;;
    esac
    # Image builds have a profile input; a package build such as systemcore has none. The slot's
    # version and commit are the build file's own defaults (project tokens).
    if grep -q '^\[inputs.profile\]' "$build_file"; then
      args+=(--set "input.profile=$profile")
    fi
    if [[ "$machine" == qemu ]]; then
      [[ "$profile" == full ]] || { echo "error: --machine qemu needs the full profile" >&2; exit 1; }
      prepare_qemu_sim
      # A slot id the board's Atlas never installs: the variant says it is the simulated board.
      args+=(--set "input.slot_variant=styx-qemu")
      [[ -z "${HELIOS_QEMU_PREPARE_ONLY:-}" ]] || exit 0
    fi
    exec gaia run "$build_file" "${args[@]}" "${rest[@]}"
    ;;
esac
